//! Handle-relative diagnostic and backup files. Existing native ACLs are preserved;
//! new objects are owner-only. Namespace mutation uses write-through handles.

use std::ffi::OsStr;
use std::fs::File;
use std::io;
use std::mem::{offset_of, size_of};
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::AsRawHandle;
use std::path::Path;
use std::ptr::{null, null_mut};

use windows_sys::Wdk::Storage::FileSystem::{
    FILE_NAMES_INFORMATION, FILE_RENAME_INFORMATION, FILE_RENAME_POSIX_SEMANTICS,
    FILE_RENAME_REPLACE_IF_EXISTS, FileNamesInformation, FileRenameInformationEx,
    NtQueryDirectoryFile, NtSetInformationFile,
};
use windows_sys::Win32::Foundation::{
    GENERIC_READ, GENERIC_WRITE, RtlNtStatusToDosError, STATUS_NO_MORE_FILES,
};
use windows_sys::Win32::Security::SECURITY_DESCRIPTOR;
use windows_sys::Win32::Storage::FileSystem::{
    BY_HANDLE_FILE_INFORMATION, DELETE, FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT,
    FILE_DISPOSITION_FLAG_DELETE, FILE_DISPOSITION_FLAG_POSIX_SEMANTICS, FILE_DISPOSITION_INFO_EX,
    FileDispositionInfoEx, GetFileInformationByHandle, SetFileInformationByHandle,
};
use windows_sys::Win32::System::IO::IO_STATUS_BLOCK;

use super::{path, security};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Identity(pub u32, pub u64);

pub struct Stat {
    pub identity: Identity,
    pub length: u64,
}

pub fn stat(file: &File, directory: bool) -> io::Result<Stat> {
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: a live owned file handle and exact output structure are passed.
    if unsafe { GetFileInformationByHandle(file.as_raw_handle().cast(), &mut info) } == 0 {
        return Err(io::Error::last_os_error());
    }
    if info.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
        || (info.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY != 0) != directory
        || (!directory && info.nNumberOfLinks != 1)
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "unsafe native object",
        ));
    }
    Ok(Stat {
        identity: Identity(
            info.dwVolumeSerialNumber,
            u64::from(info.nFileIndexHigh) << 32 | u64::from(info.nFileIndexLow),
        ),
        length: u64::from(info.nFileSizeHigh) << 32 | u64::from(info.nFileSizeLow),
    })
}

pub fn open_directory(location: &Path, create: bool) -> io::Result<File> {
    let absolute = std::path::absolute(location)?;
    let file = match path::open(&absolute, true, GENERIC_READ, path::SHARE_ALL, false, None) {
        Err(error) if create && error.kind() == io::ErrorKind::NotFound => {
            let parent = absolute.parent().ok_or(io::ErrorKind::InvalidInput)?;
            let name = absolute.file_name().ok_or(io::ErrorKind::InvalidInput)?;
            let parent = open_directory(parent, true)?;
            match create_at(&parent, name, true, GENERIC_READ) {
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => path::relative(
                    &parent,
                    name,
                    true,
                    GENERIC_READ,
                    path::SHARE_ALL,
                    false,
                    None,
                ),
                result => result,
            }?
        }
        result => result?,
    };
    stat(&file, true)?;
    Ok(file)
}

#[derive(Clone, Copy)]
pub enum OpenMode {
    Read,
    Write,
    New,
    Delete,
}

pub fn open_file(parent: &File, name: &str, mode: OpenMode) -> io::Result<File> {
    let access = match mode {
        OpenMode::Read => GENERIC_READ,
        OpenMode::Write | OpenMode::New => GENERIC_READ | GENERIC_WRITE,
        OpenMode::Delete => GENERIC_READ | DELETE,
    };
    let open = || {
        path::relative(
            parent,
            OsStr::new(name),
            false,
            access,
            path::SHARE_ALL,
            false,
            None,
        )
    };
    let file = match mode {
        OpenMode::New => create(parent, name, false, access),
        OpenMode::Write => match open() {
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                match create(parent, name, false, access) {
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => open(),
                    result => result,
                }
            }
            result => result,
        },
        _ => open(),
    }?;
    stat(&file, false)?;
    Ok(file)
}

pub fn child_directory(parent: &File, name: &str) -> io::Result<File> {
    let open = || {
        path::relative(
            parent,
            OsStr::new(name),
            true,
            GENERIC_READ,
            path::SHARE_ALL,
            false,
            None,
        )
    };
    let file = match open() {
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            match create(parent, name, true, GENERIC_READ) {
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => open(),
                result => result,
            }
        }
        result => result,
    }?;
    stat(&file, true)?;
    Ok(file)
}

fn create(parent: &File, name: &str, directory: bool, access: u32) -> io::Result<File> {
    create_at(parent, OsStr::new(name), directory, access)
}

fn create_at(parent: &File, name: &OsStr, directory: bool, access: u32) -> io::Result<File> {
    let owner = security::OwnerSecurity::new(directory)?;
    // SAFETY: OwnerSecurity owns the correctly aligned descriptor through this call.
    let descriptor = unsafe { &*owner.descriptor().cast::<SECURITY_DESCRIPTOR>() };
    let file = path::relative(
        parent,
        name,
        directory,
        access,
        path::SHARE_ALL,
        true,
        Some(descriptor),
    )?;
    security::verify_owner_only_handle(&file, directory)?;
    Ok(file)
}

pub fn rename(source: &File, parent: &File, name: &str) -> io::Result<()> {
    stat(source, false)?;
    stat(parent, true)?;
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.encode_utf16().any(|ch| matches!(ch, 0 | 47 | 58 | 92))
    {
        return Err(io::Error::from(io::ErrorKind::InvalidInput));
    }
    let wide: Vec<_> = OsStr::new(name).encode_wide().collect();
    let prefix = offset_of!(FILE_RENAME_INFORMATION, FileName);
    let length = prefix
        .checked_add(wide.len() * 2)
        .map(|bytes| bytes.max(size_of::<FILE_RENAME_INFORMATION>()))
        .and_then(|bytes| u32::try_from(bytes).ok())
        .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidInput))?;
    let mut storage = vec![0u64; (length as usize).div_ceil(size_of::<u64>())];
    let information = storage.as_mut_ptr().cast::<FILE_RENAME_INFORMATION>();
    // SAFETY: aligned zeroed allocation holds the complete variable-sized structure;
    // its tail has exactly `wide.len()` UTF-16 code units, and both handles are live.
    unsafe {
        (*information).Anonymous.Flags =
            FILE_RENAME_REPLACE_IF_EXISTS | FILE_RENAME_POSIX_SEMANTICS;
        (*information).RootDirectory = parent.as_raw_handle().cast();
        (*information).FileNameLength = (wide.len() * 2) as u32;
        std::ptr::copy_nonoverlapping(
            wide.as_ptr(),
            storage.as_mut_ptr().cast::<u8>().add(prefix).cast(),
            wide.len(),
        );
    }
    let mut status = IO_STATUS_BLOCK::default();
    // SAFETY: all variable fields, input storage and handles remain valid synchronously.
    nt_result(unsafe {
        NtSetInformationFile(
            source.as_raw_handle().cast(),
            &mut status,
            information.cast(),
            length,
            FileRenameInformationEx,
        )
    })
}

pub fn remove(file: &File) -> io::Result<()> {
    stat(file, false)?;
    let information = FILE_DISPOSITION_INFO_EX {
        Flags: FILE_DISPOSITION_FLAG_DELETE | FILE_DISPOSITION_FLAG_POSIX_SEMANTICS,
    };
    // SAFETY: DELETE-capable handle identifies the exact verified object. POSIX unlink
    // releases its name even while a verified reader/writer still holds that object.
    if unsafe {
        SetFileInformationByHandle(
            file.as_raw_handle().cast(),
            FileDispositionInfoEx,
            (&information as *const FILE_DISPOSITION_INFO_EX).cast(),
            size_of::<FILE_DISPOSITION_INFO_EX>() as u32,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// Enumerate the held directory object, including after its pathname has been renamed.
/// Caller serializes scans because the cursor belongs to this kernel directory handle.
pub fn list_names(directory: &File) -> io::Result<Vec<String>> {
    stat(directory, true)?;
    let mut buffer = vec![0u64; 8192];
    let mut names = Vec::new();
    let mut restart = true;
    loop {
        let mut status = IO_STATUS_BLOCK::default();
        // SAFETY: a live synchronous directory handle and aligned writable 64KiB buffer
        // are passed. No pathname, completion APC or borrowed event is used.
        let result = unsafe {
            NtQueryDirectoryFile(
                directory.as_raw_handle().cast(),
                null_mut(),
                None,
                null(),
                &mut status,
                buffer.as_mut_ptr().cast(),
                (buffer.len() * 8) as u32,
                FileNamesInformation,
                false,
                null(),
                restart,
            )
        };
        if result == STATUS_NO_MORE_FILES {
            return Ok(names);
        }
        nt_result(result)?;
        restart = false;
        let length = status.Information;
        if length == 0 || length > buffer.len() * 8 {
            return Err(invalid_directory_data());
        }
        let mut offset = 0usize;
        loop {
            let prefix = offset_of!(FILE_NAMES_INFORMATION, FileName);
            if offset.checked_add(prefix).is_none_or(|end| end > length) {
                return Err(invalid_directory_data());
            }
            // SAFETY: bounds above cover the fixed header; unaligned reads support each
            // OS-provided entry offset without manufacturing an aligned reference.
            let next_entry = unsafe {
                std::ptr::read_unaligned(buffer.as_ptr().cast::<u8>().add(offset).cast::<u32>())
            };
            // SAFETY: the verified 12-byte fixed header includes FileNameLength at +8.
            let bytes = unsafe {
                std::ptr::read_unaligned(buffer.as_ptr().cast::<u8>().add(offset + 8).cast::<u32>())
            } as usize;
            if !bytes.is_multiple_of(2)
                || offset
                    .checked_add(prefix)
                    .and_then(|start| start.checked_add(bytes))
                    .is_none_or(|end| end > length)
            {
                return Err(invalid_directory_data());
            }
            // SAFETY: the validated UTF-16 byte range lies wholly in the owned buffer.
            let wide = unsafe {
                std::slice::from_raw_parts(
                    buffer
                        .as_ptr()
                        .cast::<u8>()
                        .add(offset + prefix)
                        .cast::<u16>(),
                    bytes / 2,
                )
            };
            let name = String::from_utf16_lossy(wide);
            if name != "." && name != ".." {
                names.push(name);
            }
            if names.len() > 100_000 {
                return Err(invalid_directory_data());
            }
            if next_entry == 0 {
                break;
            }
            let next = next_entry as usize;
            if next < prefix + bytes || !next.is_multiple_of(4) {
                return Err(invalid_directory_data());
            }
            offset = offset
                .checked_add(next)
                .ok_or_else(invalid_directory_data)?;
        }
    }
}

fn nt_result(status: i32) -> io::Result<()> {
    if status < 0 {
        // SAFETY: numeric error translation has no pointer or ownership preconditions.
        Err(io::Error::from_raw_os_error(
            unsafe { RtlNtStatusToDosError(status) } as i32,
        ))
    } else {
        Ok(())
    }
}

fn invalid_directory_data() -> io::Error {
    io::Error::from(io::ErrorKind::InvalidData)
}
