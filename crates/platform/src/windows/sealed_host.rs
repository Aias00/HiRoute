//! Administrator-protected SCM executable and descriptor. User-writable code is
//! executed only after the service host has created a restricted primary token.
use std::fs::File;
use std::io::{self, Read, Write};
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle};
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::{INVALID_HANDLE_VALUE, LocalFree};
use windows_sys::Win32::Security::Authorization::{ConvertStringSecurityDescriptorToSecurityDescriptorW, GetSecurityInfo, SDDL_REVISION_1, SE_FILE_OBJECT};
use windows_sys::Win32::Security::{ACCESS_ALLOWED_ACE, ACL, ACL_SIZE_INFORMATION, AclSizeInformation, CreateWellKnownSid, DACL_SECURITY_INFORMATION, EqualSid, GetAce, GetAclInformation, GetSecurityDescriptorControl, OWNER_SECURITY_INFORMATION, SE_DACL_PROTECTED, SECURITY_ATTRIBUTES, WinAuthenticatedUserSid, WinBuiltinAdministratorsSid, WinLocalSystemSid};
use windows_sys::Win32::Storage::FileSystem::{CreateDirectoryW, CreateFileW, CREATE_NEW, FILE_ALL_ACCESS, FILE_GENERIC_EXECUTE, FILE_GENERIC_READ, FILE_SHARE_READ, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW};
use windows_sys::Win32::Foundation::GENERIC_WRITE;

use super::background::{Definition, Manager, digest};

struct Descriptor(*mut std::ffi::c_void);
impl Descriptor {
    fn new(directory: bool) -> io::Result<Self> {
        let inheritance = if directory { "OICI" } else { "" };
        // Owner is the administrator GROUP, so a medium token for an admin
        // account cannot use its individual user SID to rewrite this executable.
        let text = format!("O:BAG:SYD:P(A;{inheritance};FA;;;SY)(A;{inheritance};FA;;;BA)(A;{inheritance};FRFX;;;AU)");
        let text = wide(Path::new(&text)); let mut descriptor = null_mut();
        if unsafe { ConvertStringSecurityDescriptorToSecurityDescriptorW(text.as_ptr(), SDDL_REVISION_1, &mut descriptor, null_mut()) } == 0 { return Err(io::Error::last_os_error()); }
        Ok(Self(descriptor))
    }
    fn attributes(&self) -> SECURITY_ATTRIBUTES {
        SECURITY_ATTRIBUTES { nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32, lpSecurityDescriptor: self.0, bInheritHandle: 0 }
    }
}
impl Drop for Descriptor { fn drop(&mut self) { unsafe { LocalFree(self.0); } } }
fn wide(path: &Path) -> Vec<u16> { path.as_os_str().encode_wide().chain(Some(0)).collect() }

fn sid(kind: i32) -> io::Result<Vec<usize>> {
    let mut value = vec![0_usize; 68_usize.div_ceil(std::mem::size_of::<usize>())];
    let mut length = (value.len() * std::mem::size_of::<usize>()) as u32;
    if unsafe { CreateWellKnownSid(kind, null_mut(), value.as_mut_ptr().cast(), &mut length) } == 0 { return Err(io::Error::last_os_error()); }
    Ok(value)
}

pub fn verify_protected(file: &File) -> io::Result<()> {
    let administrator = sid(WinBuiltinAdministratorsSid)?;
    let system = sid(WinLocalSystemSid)?;
    let readers = sid(WinAuthenticatedUserSid)?;
    let mut owner = null_mut(); let mut acl: *mut ACL = null_mut(); let mut descriptor = null_mut();
    let status = unsafe { GetSecurityInfo(file.as_raw_handle().cast(), SE_FILE_OBJECT,
        OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION, &mut owner, null_mut(), &mut acl, null_mut(), &mut descriptor) };
    if status != 0 { return Err(io::Error::from_raw_os_error(status as i32)); }
    let _descriptor = Descriptor(descriptor);
    let mut control = 0; let mut revision = 0;
    if owner.is_null() || acl.is_null() || unsafe { EqualSid(owner, administrator.as_ptr().cast_mut().cast()) } == 0
        || unsafe { GetSecurityDescriptorControl(descriptor, &mut control, &mut revision) } == 0
        || control & SE_DACL_PROTECTED == 0 { return Err(unsafe_host()); }
    let mut information = ACL_SIZE_INFORMATION::default();
    if unsafe { GetAclInformation(acl, (&mut information as *mut ACL_SIZE_INFORMATION).cast(), std::mem::size_of::<ACL_SIZE_INFORMATION>() as u32, AclSizeInformation) } == 0 || information.AceCount != 3 { return Err(unsafe_host()); }
    let mut seen = [false; 3];
    for index in 0..3 {
        let mut pointer = null_mut();
        if unsafe { GetAce(acl, index, &mut pointer) } == 0 || pointer.is_null() { return Err(unsafe_host()); }
        let ace = unsafe { &*pointer.cast::<ACCESS_ALLOWED_ACE>() };
        if ace.Header.AceType != 0 { return Err(unsafe_host()); }
        let ace_sid = std::ptr::addr_of!(ace.SidStart).cast_mut().cast();
        let matched = if unsafe { EqualSid(ace_sid, system.as_ptr().cast_mut().cast()) } != 0 { Some((0, FILE_ALL_ACCESS)) }
            else if unsafe { EqualSid(ace_sid, administrator.as_ptr().cast_mut().cast()) } != 0 { Some((1, FILE_ALL_ACCESS)) }
            else if unsafe { EqualSid(ace_sid, readers.as_ptr().cast_mut().cast()) } != 0 { Some((2, FILE_GENERIC_READ | FILE_GENERIC_EXECUTE)) }
            else { None };
        let Some((kind, mask)) = matched else { return Err(unsafe_host()); };
        if seen[kind] || ace.Mask != mask { return Err(unsafe_host()); }
        seen[kind] = true;
    }
    if !seen.into_iter().all(|value| value) { return Err(unsafe_host()); }
    Ok(())
}

fn directory(path: &Path) -> io::Result<File> {
    match super::security::open_identity(path, true) {
        Ok(file) => { verify_protected(&file)?; Ok(file) }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            let descriptor = Descriptor::new(true)?;
            let attributes = descriptor.attributes(); let name = wide(path);
            if unsafe { CreateDirectoryW(name.as_ptr(), &attributes) } == 0 { return Err(io::Error::last_os_error()); }
            let file = super::security::open_identity(path, true)?; verify_protected(&file)?; Ok(file)
        }
        Err(error) => Err(error),
    }
}

fn create_file(path: &Path) -> io::Result<File> {
    let descriptor = Descriptor::new(false)?; let attributes = descriptor.attributes(); let name = wide(path);
    let handle = unsafe { CreateFileW(name.as_ptr(), GENERIC_WRITE | windows_sys::Win32::Foundation::GENERIC_READ,
        FILE_SHARE_READ, &attributes, CREATE_NEW, 0, null_mut()) };
    if handle == INVALID_HANDLE_VALUE || handle.is_null() { return Err(io::Error::last_os_error()); }
    let file = unsafe { File::from_raw_handle(handle.cast()) }; verify_protected(&file)?; Ok(file)
}

fn descriptor_path(definition: &Definition) -> io::Result<PathBuf> {
    Ok(definition.service_host.as_ref().and_then(|p| p.parent()).ok_or_else(unsafe_host)?.join(format!("{}.json", definition.name)))
}

pub fn install(definition: &Definition) -> io::Result<()> {
    definition.verify_image()?;
    if definition.manager != Manager::Scm { return Err(unsafe_host()); }
    let base = super::background::program_files()?;
    // Verify traversal by native held identity, then create only administrator
    // protected descendants. Never adopt a user-writable preexisting HiRoute dir.
    let _base = super::security::open_identity(&base, true)?;
    let _app = directory(&base.join("HiRoute"))?;
    let _hosts = directory(&base.join("HiRoute/service-host"))?;
    let _version = directory(&base.join("HiRoute/service-host").join(&definition.daemon_sha256))?;
    let host = definition.service_host.as_ref().ok_or_else(unsafe_host)?;
    match super::security::open_identity(host, false) {
        Ok(file) => { verify_protected(&file)?; if digest(file)? != definition.daemon_sha256 { return Err(unsafe_host()); } }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            let temp = host.with_extension(format!("{}.stage", std::process::id()));
            let mut source = super::security::open_identity(&definition.daemon, false)?;
            let mut destination = create_file(&temp)?;
            io::copy(&mut source, &mut destination)?; destination.sync_all()?; drop(destination);
            let check = super::security::open_identity(&temp, false)?;
            if digest(check)? != definition.daemon_sha256 { return Err(unsafe_host()); }
            let from = wide(&temp); let to = wide(host);
            if unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), MOVEFILE_WRITE_THROUGH) } == 0 { return Err(io::Error::last_os_error()); }
        }
        Err(error) => return Err(error),
    }
    let path = descriptor_path(definition)?;
    if path.try_exists()? { let existing = super::security::open_identity(&path, false)?; verify_protected(&existing)?; }
    let temp = path.with_extension(format!("{}.stage", std::process::id()));
    let mut file = create_file(&temp)?;
    file.write_all(&serde_json::to_vec(definition).map_err(io::Error::other)?)?;
    file.sync_all()?; drop(file);
    let from = wide(&temp); let to = wide(&path);
    if unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH) } == 0 { return Err(io::Error::last_os_error()); }
    verify_definition(definition)
}

pub fn verify_definition(definition: &Definition) -> io::Result<()> {
    definition.validate()?;
    let file = super::security::open_identity(definition.service_host.as_ref().ok_or_else(unsafe_host)?, false)?;
    verify_protected(&file)?;
    if digest(file)? != definition.service_host_sha256.as_deref().ok_or_else(unsafe_host)? { return Err(unsafe_host()); }
    let mut descriptor = super::security::open_identity(&descriptor_path(definition)?, false)?;
    verify_protected(&descriptor)?;
    let mut bytes = Vec::new(); Read::by_ref(&mut descriptor).take(16 * 1024 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > 16 * 1024 { return Err(unsafe_host()); }
    let current: Definition = serde_json::from_slice(&bytes).map_err(io::Error::other)?;
    if &current != definition { return Err(unsafe_host()); }
    Ok(())
}

pub fn load_for_dispatch(name: &str) -> io::Result<Definition> {
    // ServiceMain is never dispatched from an unsealed user image. Retain native
    // file identities while reading this machine-owned descriptor and executable.
    let executable = std::env::current_exe()?;
    let parent = executable.parent().ok_or_else(unsafe_host)?;
    let path = parent.join(format!("{name}.json"));
    let mut descriptor = super::security::open_identity(&path, false)?;
    verify_protected(&descriptor)?;
    let mut bytes = Vec::new(); Read::by_ref(&mut descriptor).take(16 * 1024 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > 16 * 1024 { return Err(unsafe_host()); }
    let value: Definition = serde_json::from_slice(&bytes).map_err(io::Error::other)?;
    value.validate()?;
    if value.manager != Manager::Scm || value.name != name { return Err(unsafe_host()); }
    let configured = super::security::open_identity(value.service_host.as_ref().ok_or_else(unsafe_host)?, false)?;
    let running = super::security::open_identity(&executable, false)?;
    verify_protected(&configured)?; verify_protected(&running)?;
    if super::security::file_identity(&configured)? != super::security::file_identity(&running)?
        || digest(configured)? != value.service_host_sha256.as_deref().ok_or_else(unsafe_host)? { return Err(unsafe_host()); }
    value.verify_image()?;
    Ok(value)
}

fn unsafe_host() -> io::Error { io::Error::new(io::ErrorKind::PermissionDenied, "SCM host is not administrator-protected") }
