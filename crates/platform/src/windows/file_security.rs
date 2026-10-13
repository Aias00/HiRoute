use crate::file_security::WindowsPermissions;
use std::ffi::c_void;
use std::fs::File;
use std::io;
use std::os::windows::fs::MetadataExt;
use std::os::windows::io::AsRawHandle;
use std::path::Path;
use std::ptr::null_mut;
use windows_sys::Win32::Foundation::{GENERIC_READ, GENERIC_WRITE, LocalFree};
use windows_sys::Win32::Security::Authorization::{
    ConvertSecurityDescriptorToStringSecurityDescriptorW,
    ConvertStringSecurityDescriptorToSecurityDescriptorW, GetSecurityInfo, SE_FILE_OBJECT,
};
use windows_sys::Win32::Security::{
    DACL_SECURITY_INFORMATION, OWNER_SECURITY_INFORMATION, SECURITY_DESCRIPTOR,
};
use windows_sys::Win32::Storage::FileSystem::{
    DELETE, FILE_ATTRIBUTE_NORMAL, FILE_ATTRIBUTE_READONLY, FILE_BASIC_INFO, FILE_SHARE_NONE,
    FileBasicInfo, SetFileInformationByHandle,
};

const SECURITY_FIELDS: u32 = OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION;
const MAX_DESCRIPTOR_WORDS: usize = 32768;

pub fn capture(file: &File) -> io::Result<WindowsPermissions> {
    let mut descriptor = null_mut();
    // SAFETY: file owns a live handle; the allocated descriptor is held below.
    let status = unsafe {
        GetSecurityInfo(
            file.as_raw_handle().cast(),
            SE_FILE_OBJECT,
            SECURITY_FIELDS,
            null_mut(),
            null_mut(),
            null_mut(),
            null_mut(),
            &mut descriptor,
        )
    };
    if status != 0 {
        return Err(io::Error::from_raw_os_error(status as i32));
    }
    let descriptor = Allocation(descriptor);
    Ok(WindowsPermissions {
        descriptor: encode(descriptor.0)?,
        read_only: file.metadata()?.file_attributes() & FILE_ATTRIBUTE_READONLY != 0,
    })
}

pub fn private_defaults(read_only: bool) -> io::Result<WindowsPermissions> {
    let security = super::security::OwnerSecurity::new(false)?;
    Ok(WindowsPermissions {
        descriptor: encode(security.descriptor())?,
        read_only,
    })
}

pub(crate) fn create(path: &Path, permissions: &WindowsPermissions) -> io::Result<File> {
    let descriptor = decode(&permissions.descriptor)?;
    // Windows checks whether the caller may assign the captured native owner.
    // A failure leaves the existing target untouched, rather than losing owner/ACL.
    // SAFETY: Windows returned a validated security descriptor in this allocation.
    let descriptor_ref = unsafe { &*descriptor.0.cast::<SECURITY_DESCRIPTOR>() };
    super::path::open(
        path,
        false,
        GENERIC_READ | GENERIC_WRITE | DELETE,
        FILE_SHARE_NONE,
        true,
        Some(descriptor_ref),
    )
}

pub(crate) fn apply_read_only(file: &File, read_only: bool) -> io::Result<()> {
    let information = FILE_BASIC_INFO {
        FileAttributes: if read_only {
            FILE_ATTRIBUTE_READONLY
        } else {
            FILE_ATTRIBUTE_NORMAL
        },
        ..Default::default()
    };
    // SAFETY: this exact buffer is valid throughout the held-handle update.
    if unsafe {
        SetFileInformationByHandle(
            file.as_raw_handle().cast(),
            FileBasicInfo,
            (&information as *const FILE_BASIC_INFO).cast(),
            std::mem::size_of::<FILE_BASIC_INFO>() as u32,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

fn encode(descriptor: *const c_void) -> io::Result<String> {
    let mut text = null_mut();
    let mut length = 0;
    // SAFETY: the caller retains a valid Windows security descriptor.
    if unsafe {
        ConvertSecurityDescriptorToStringSecurityDescriptorW(
            descriptor.cast_mut(),
            1,
            SECURITY_FIELDS,
            &mut text,
            &mut length,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    let allocation = Allocation(text.cast());
    let length = length as usize;
    if !(1..=MAX_DESCRIPTOR_WORDS).contains(&length) {
        return Err(invalid_descriptor());
    }
    // SAFETY: Windows reports its UTF-16 allocation size, which can exceed the
    // actual string length. Read only this allocation, then locate its first
    // terminator instead of persisting padding as embedded NULs in SDDL.
    let words = unsafe { std::slice::from_raw_parts(text, length) };
    let end = words
        .iter()
        .position(|word| *word == 0)
        .filter(|end| *end > 0)
        .ok_or_else(invalid_descriptor)?;
    let value = String::from_utf16(&words[..end]).map_err(|_| invalid_descriptor())?;
    drop(allocation);
    Ok(value)
}

fn decode(value: &str) -> io::Result<Allocation> {
    let mut text: Vec<u16> = value.encode_utf16().collect();
    if text.is_empty() || text.len() >= MAX_DESCRIPTOR_WORDS || text.contains(&0) {
        return Err(invalid_descriptor());
    }
    text.push(0);
    let mut descriptor = null_mut();
    // SAFETY: input is bounded NUL-terminated UTF-16; Windows validates SDDL.
    if unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            text.as_ptr(),
            1,
            &mut descriptor,
            null_mut(),
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(Allocation(descriptor))
}

fn invalid_descriptor() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "invalid native permission descriptor",
    )
}
struct Allocation(*mut c_void);
impl Drop for Allocation {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: each descriptor/string above is allocated by LocalAlloc.
            unsafe {
                LocalFree(self.0);
            }
        }
    }
}
