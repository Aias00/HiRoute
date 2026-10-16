//! Windows native fixtures for consumers which forbid unsafe code. This crate
//! is a Windows-only dev dependency and supplies no production authorization API.
#![cfg(windows)]

use std::io;
use std::path::Path;
use std::ptr::null_mut;

use windows_sys::Win32::Foundation::LocalFree;
use windows_sys::Win32::Security::Authorization::{
    ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1, SE_FILE_OBJECT,
    SetNamedSecurityInfoW,
};
use windows_sys::Win32::Security::{
    DACL_SECURITY_INFORMATION, GetSecurityDescriptorDacl, PROTECTED_DACL_SECURITY_INFORMATION,
};

#[path = "../../tests/support/windows_junction.rs"]
pub mod junction;

struct Descriptor(*mut std::ffi::c_void);
impl Drop for Descriptor {
    fn drop(&mut self) {
        // SAFETY: conversion transfers one LocalAlloc allocation to this guard.
        unsafe {
            LocalFree(self.0);
        }
    }
}

/// Make a previously private fixture nonprivate by adding an actual Everyone
/// reader ACE. Callers must own the fixture lifetime and prevent path mutation;
/// unrelated or already foreign objects are rejected before changing anything.
pub fn grant_everyone_read(path: &Path, directory: bool) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    let private = hiroute_platform::windows::security::open_owner_only(path, directory)?;
    let owner = hiroute_platform::windows::background::current_sid_string()?;
    let inherit = if directory { "OICI" } else { "" };
    let text = format!("D:P(A;{inherit};FA;;;{owner})(A;{inherit};FR;;;WD)");
    let text: Vec<_> = text.encode_utf16().chain(Some(0)).collect();
    let mut descriptor = null_mut();
    // SAFETY: bounded trusted fixture SDDL and output remain live for conversion.
    if unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            text.as_ptr(),
            SDDL_REVISION_1,
            &mut descriptor,
            null_mut(),
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    let _descriptor = Descriptor(descriptor);
    let mut present = 0;
    let mut defaulted = 0;
    let mut acl = null_mut();
    // SAFETY: the converted descriptor remains held until ACL mutation returns.
    if unsafe { GetSecurityDescriptorDacl(descriptor, &mut present, &mut acl, &mut defaulted) } == 0
    {
        return Err(io::Error::last_os_error());
    }
    if present == 0 || acl.is_null() {
        return Err(io::ErrorKind::InvalidData.into());
    }
    let mut name: Vec<_> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    // SAFETY: only the already verified owned test fixture is changed. The path
    // is retained by the caller, which cannot race another fixture mutation.
    let status = unsafe {
        SetNamedSecurityInfoW(
            name.as_mut_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            null_mut(),
            null_mut(),
            acl,
            null_mut(),
        )
    };
    drop(private);
    if status != 0 {
        return Err(io::Error::from_raw_os_error(status as i32));
    }
    Ok(())
}
