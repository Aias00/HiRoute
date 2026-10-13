use std::fs::File;
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::AsRawHandle;
use std::path::Path;
use windows_sys::Wdk::Storage::FileSystem::{
    FILE_RENAME_INFORMATION, FileRenameInformation, NtSetInformationFile,
};
use windows_sys::Win32::Foundation::RtlNtStatusToDosError;
use windows_sys::Win32::System::IO::IO_STATUS_BLOCK;

use super::security;

pub(crate) fn rename(source: &Path, target: &Path, replace: bool) -> io::Result<()> {
    let metadata = std::fs::symlink_metadata(source)?;
    let file = security::open_write_through(source, metadata.is_dir())?;
    rename_file(&file, target, replace)?;
    if metadata.is_file() {
        file.sync_all()?;
    }
    Ok(())
}

pub(crate) fn rename_file(file: &File, target: &Path, replace: bool) -> io::Result<()> {
    let parent_path = target
        .parent()
        .ok_or_else(|| io::Error::other("missing parent"))?;
    let parent = security::open_identity(parent_path, true)?;
    let leaf = target
        .file_name()
        .ok_or_else(|| io::Error::other("missing name"))?;
    rename_file_at(file, &parent, leaf, replace)
}

pub fn rename_file_at(
    file: &File,
    parent: &File,
    leaf: &std::ffi::OsStr,
    replace: bool,
) -> io::Result<()> {
    if security::file_identity(file)?.0 != security::file_identity(&parent)?.0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "publication must stay on one volume",
        ));
    }
    let name: Vec<u16> = leaf.encode_wide().collect();
    if name.is_empty() || name.iter().any(|word| matches!(*word, 0 | 58 | 47 | 92)) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid publication name",
        ));
    }
    let offset = std::mem::offset_of!(FILE_RENAME_INFORMATION, FileName);
    let length = offset
        .checked_add(
            name.len()
                .checked_mul(2)
                .ok_or_else(|| io::Error::other("name bound"))?,
        )
        .ok_or_else(|| io::Error::other("name bound"))?;
    let length = length.max(std::mem::size_of::<FILE_RENAME_INFORMATION>());
    let words = length.div_ceil(std::mem::size_of::<usize>());
    let mut buffer = vec![0_usize; words];
    // SAFETY: word-aligned storage contains the entire header plus UTF-16 tail.
    // RootDirectory borrows the inspected live parent; the source is the held
    // DELETE-capable write-through object, never a reopened pathname.
    unsafe {
        let information = buffer.as_mut_ptr().cast::<FILE_RENAME_INFORMATION>();
        (*information).Anonymous.ReplaceIfExists = replace;
        (*information).RootDirectory = parent.as_raw_handle().cast();
        (*information).FileNameLength =
            u32::try_from(name.len() * 2).map_err(|_| io::Error::other("name bound"))?;
        std::ptr::copy_nonoverlapping(
            name.as_ptr(),
            (*information).FileName.as_mut_ptr(),
            name.len(),
        );
        let mut status = IO_STATUS_BLOCK::default();
        let result = NtSetInformationFile(
            file.as_raw_handle().cast(),
            &mut status,
            information.cast(),
            u32::try_from(length).map_err(|_| io::Error::other("name bound"))?,
            FileRenameInformation,
        );
        if result < 0 {
            return Err(io::Error::from_raw_os_error(
                RtlNtStatusToDosError(result) as i32
            ));
        }
    }
    Ok(())
}
