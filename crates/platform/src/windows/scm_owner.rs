//! Privileged SCM maintenance for a freshly authenticated owner account.
//!
//! Only a credential token supplies the owner SID and KnownFolders. Reads happen
//! while impersonating that owner; native image handles survive RevertToSelf.

use std::fs::File;
use std::io;
use std::marker::PhantomData;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};
use std::rc::Rc;
use std::time::Duration;

use windows::Win32::Foundation::HANDLE as ShellHandle;
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::UI::Shell::{
    FOLDERID_LocalAppData, FOLDERID_Profile, LoadUserProfileW, PROFILEINFOW, SHGetKnownFolderPath,
    UnloadUserProfile,
};
use windows::core::{GUID, PWSTR};
use windows_sys::Win32::Foundation::{ERROR_NOT_ALL_ASSIGNED, GetLastError, LUID};
use windows_sys::Win32::Security::{
    AdjustTokenPrivileges, GetLengthSid, GetTokenInformation, ImpersonateLoggedOnUser,
    LOGON32_LOGON_INTERACTIVE, LOGON32_PROVIDER_DEFAULT, LUID_AND_ATTRIBUTES, LogonUserW,
    LookupPrivilegeValueW, RevertToSelf, SE_PRIVILEGE_ENABLED, TOKEN_ADJUST_PRIVILEGES,
    TOKEN_ELEVATION, TOKEN_PRIVILEGES, TOKEN_QUERY, TOKEN_USER, TokenElevation, TokenUser,
};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
use zeroize::Zeroizing;

use super::background::{self, Definition, Manager};
use super::{security, service};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Operation {
    Install,
    Uninstall,
    SetAutostart(bool),
}

/// Paths and private-read authority are valid only in this synchronous owner scope.
/// They cannot be moved to another thread or manufactured from a numeric SID.
pub struct Owner {
    pub profile: PathBuf,
    pub local_app_data: PathBuf,
    pub sid: String,
    pub account: String,
    sid_bytes: Vec<u8>,
    _thread_scope: PhantomData<Rc<()>>,
}

impl Owner {
    pub fn open_private_file(&self, path: &Path) -> io::Result<File> {
        if !path.is_absolute()
            || (!path.starts_with(&self.profile) && !path.starts_with(&self.local_app_data))
        {
            return Err(io::ErrorKind::PermissionDenied.into());
        }
        let file = security::open_identity(path, false)?;
        security::verify_owner_only_handle_for_sid(&file, false, &self.sid_bytes)?;
        if security::file_link_count(&file)? != 1 {
            return Err(io::ErrorKind::PermissionDenied.into());
        }
        Ok(file)
    }

    /// An accessible native executable need not have a private-record ACL. Its
    /// reparse-free identity and verified digest bind the held bytes. The installer
    /// deliberately creates a stable hirouted.exe hardlink to this versioned image.
    pub fn open_image(&self, path: &Path) -> io::Result<File> {
        if !path.is_absolute() || !path.starts_with(self.profile.join(".local/lib/hiroute")) {
            return Err(io::ErrorKind::PermissionDenied.into());
        }
        security::open_identity(path, false)
    }
}

pub struct Installation {
    pub definition: Definition,
    pub previous: Option<Definition>,
    pub image: Option<File>,
}

/// No user pathname is opened after reversion. The supplied callback decodes only
/// the authenticated owner's fixed installation marker and definition contracts.
pub fn execute(
    account: &str,
    password: &str,
    operation: Operation,
    mut trusted_host: Option<File>,
    inspect: impl FnOnce(&Owner) -> io::Result<Installation>,
) -> io::Result<Definition> {
    let privileges = Privileges::enable_profile_access()?;
    let token = logon(account, password)?;
    let sid_bytes = token_sid(&token)?;
    let sid = background::sid_string(&sid_bytes)?;
    let canonical_account = service::account_name_for_sid(&sid_bytes)?;
    if !account.eq_ignore_ascii_case(&canonical_account) {
        return Err(io::ErrorKind::PermissionDenied.into());
    }
    let profile = LoadedProfile::load(token, &canonical_account)?;
    let owner = Owner {
        profile: known_folder(&FOLDERID_Profile, &profile.token)?,
        local_app_data: known_folder(&FOLDERID_LocalAppData, &profile.token)?,
        sid,
        account: canonical_account,
        sid_bytes,
        _thread_scope: PhantomData,
    };
    let impersonation = Impersonation::begin(&profile.token)?;
    let installation = inspect(&owner);
    // Reversion runs even on a reader error; its failure is fatal, never a reason
    // to continue machine maintenance under an unexpected effective identity.
    impersonation.finish();
    let mut installation = installation?;
    installation.definition.validate_for_owner_sid(&owner.sid)?;
    if installation.definition.manager != Manager::Scm {
        return Err(io::ErrorKind::PermissionDenied.into());
    }
    if let Some(previous) = &installation.previous {
        previous.validate_for_owner_sid(&owner.sid)?;
        if previous.manager != Manager::Scm {
            return Err(io::ErrorKind::PermissionDenied.into());
        }
    }
    match operation {
        Operation::Install => service::register_for_owner(
            &installation.definition,
            installation.previous.as_ref(),
            &owner.sid_bytes,
            &owner.account,
            password,
            installation
                .image
                .as_mut()
                .ok_or(io::ErrorKind::InvalidInput)?,
            trusted_host
                .as_mut()
                .ok_or(io::ErrorKind::PermissionDenied)?,
        )?,
        Operation::Uninstall => service::unregister_for_owner(
            &installation.definition,
            &owner.sid,
            &owner.account,
            Duration::from_secs(30),
        )?,
        Operation::SetAutostart(enabled) => service::set_autostart_for_owner(
            &installation.definition,
            &owner.sid,
            &owner.account,
            enabled,
        )?,
    }
    // Release this load's hive reference before restoring the original privileges.
    profile.finish()?;
    drop(privileges);
    Ok(installation.definition)
}

fn wide(value: &str) -> io::Result<Vec<u16>> {
    if value.is_empty() || value.contains('\0') {
        return Err(io::ErrorKind::InvalidInput.into());
    }
    Ok(value.encode_utf16().chain(Some(0)).collect())
}

fn logon(account: &str, password: &str) -> io::Result<OwnedHandle> {
    if account.len() > 512 || password.len() > 4096 || !account.contains('\\') {
        return Err(io::ErrorKind::InvalidInput.into());
    }
    let account = wide(account)?;
    let password = Zeroizing::new(wide(password)?);
    let mut token = null_mut();
    // SAFETY: qualified account and bounded zeroizing password buffers are live;
    // the returned primary token is newly owned and never supplied by the request.
    if unsafe {
        LogonUserW(
            account.as_ptr(),
            null(),
            password.as_ptr(),
            LOGON32_LOGON_INTERACTIVE,
            LOGON32_PROVIDER_DEFAULT,
            &mut token,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    if token.is_null() {
        return Err(io::ErrorKind::InvalidData.into());
    }
    // SAFETY: LogonUserW transfers ownership of a valid token handle.
    Ok(unsafe { OwnedHandle::from_raw_handle(token.cast()) })
}

fn token_sid(token: &OwnedHandle) -> io::Result<Vec<u8>> {
    let mut needed = 0;
    // SAFETY: documented bounded sizing call against a live credential token.
    unsafe {
        GetTokenInformation(
            token.as_raw_handle().cast(),
            TokenUser,
            null_mut(),
            0,
            &mut needed,
        )
    };
    if needed < std::mem::size_of::<TOKEN_USER>() as u32 || needed > 64 * 1024 {
        return Err(io::ErrorKind::InvalidData.into());
    }
    let mut buffer = vec![0usize; (needed as usize).div_ceil(std::mem::size_of::<usize>())];
    // SAFETY: aligned owned storage has the API-requested TOKEN_USER capacity.
    if unsafe {
        GetTokenInformation(
            token.as_raw_handle().cast(),
            TokenUser,
            buffer.as_mut_ptr().cast(),
            needed,
            &mut needed,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: a successful TOKEN_USER query owns its valid embedded SID view.
    let sid = unsafe { (*buffer.as_ptr().cast::<TOKEN_USER>()).User.Sid };
    let length = unsafe { GetLengthSid(sid) } as usize;
    if !(8..=68).contains(&length) {
        return Err(io::ErrorKind::InvalidData.into());
    }
    // SAFETY: the kernel-populated SID remains live for this exact length.
    let bytes = unsafe { std::slice::from_raw_parts(sid.cast::<u8>(), length) }.to_vec();
    background::sid_string(&bytes)?;
    Ok(bytes)
}

struct LoadedProfile {
    token: OwnedHandle,
    hive: Option<ShellHandle>,
}

impl LoadedProfile {
    fn load(token: OwnedHandle, account: &str) -> io::Result<Self> {
        let mut name = wide(
            account
                .rsplit('\\')
                .next()
                .ok_or(io::ErrorKind::InvalidInput)?,
        )?;
        let mut info = PROFILEINFOW {
            dwSize: std::mem::size_of::<PROFILEINFOW>() as u32,
            dwFlags: 1, // PI_NOUI: maintenance never prompts from the profile loader.
            lpUserName: PWSTR(name.as_mut_ptr()),
            ..Default::default()
        };
        // SAFETY: authenticated token and complete PROFILEINFO remain live. LogonUser
        // itself does not load a registry hive; KnownFolder queries require this load.
        unsafe { LoadUserProfileW(ShellHandle(token.as_raw_handle()), &mut info) }
            .map_err(background::com_error)?;
        if info.hProfile.is_invalid() {
            return Err(io::ErrorKind::InvalidData.into());
        }
        Ok(Self {
            token,
            hive: Some(info.hProfile),
        })
    }

    fn finish(mut self) -> io::Result<()> {
        self.unload()
    }

    fn unload(&mut self) -> io::Result<()> {
        let Some(hive) = self.hive else { return Ok(()) };
        // SAFETY: release precisely this successful load while its token remains live.
        unsafe { UnloadUserProfile(ShellHandle(self.token.as_raw_handle()), hive) }
            .map_err(background::com_error)?;
        self.hive = None;
        Ok(())
    }
}

impl Drop for LoadedProfile {
    fn drop(&mut self) {
        // SAFETY: this releases precisely the successful load's hive reference;
        // the credential token is kept alive until after this call.
        let _ = self.unload();
    }
}

fn known_folder(id: &GUID, token: &OwnedHandle) -> io::Result<PathBuf> {
    // SAFETY: the explicit credential token has QUERY/IMPERSONATE access and a
    // loaded hive. Ambient HOME/USERPROFILE and the admin's profile are irrelevant.
    let value = unsafe {
        SHGetKnownFolderPath(
            id,
            Default::default(),
            Some(ShellHandle(token.as_raw_handle())),
        )
    }
    .map_err(background::com_error)?;
    // SAFETY: Shell owns a NUL-terminated UTF-16 CoTaskMem result.
    let text = unsafe { value.to_string() }.map_err(io::Error::other);
    unsafe { CoTaskMemFree(Some(value.0.cast())) };
    let path = PathBuf::from(text?);
    if !path.is_absolute() {
        return Err(io::ErrorKind::InvalidData.into());
    }
    Ok(path)
}

struct Impersonation {
    active: bool,
    _thread_scope: PhantomData<Rc<()>>,
}

impl Impersonation {
    fn begin(token: &OwnedHandle) -> io::Result<Self> {
        // SAFETY: authenticated owner token, synchronous same-thread scope.
        if unsafe { ImpersonateLoggedOnUser(token.as_raw_handle().cast()) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self {
            active: true,
            _thread_scope: PhantomData,
        })
    }
    fn finish(mut self) {
        self.revert();
    }
    fn revert(&mut self) {
        if self.active {
            // SAFETY: only this guard established the current thread impersonation.
            if unsafe { RevertToSelf() } == 0 {
                std::process::abort();
            }
            self.active = false;
        }
    }
}
impl Drop for Impersonation {
    fn drop(&mut self) {
        self.revert();
    }
}

struct Privileges {
    token: OwnedHandle,
    previous: Vec<TOKEN_PRIVILEGES>,
}

impl Privileges {
    fn enable_profile_access() -> io::Result<Self> {
        let mut token = null_mut();
        // SAFETY: current process token, minimum rights for temporary profile loading.
        if unsafe {
            OpenProcessToken(
                GetCurrentProcess(),
                TOKEN_QUERY | TOKEN_ADJUST_PRIVILEGES,
                &mut token,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: OpenProcessToken transferred the newly owned token handle.
        let token = unsafe { OwnedHandle::from_raw_handle(token.cast()) };
        let mut elevation = TOKEN_ELEVATION::default();
        let mut length = 0;
        // SAFETY: exact fixed elevation structure and live current-process token.
        if unsafe {
            GetTokenInformation(
                token.as_raw_handle().cast(),
                TokenElevation,
                (&mut elevation as *mut TOKEN_ELEVATION).cast(),
                std::mem::size_of::<TOKEN_ELEVATION>() as u32,
                &mut length,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        if elevation.TokenIsElevated == 0 {
            return Err(io::ErrorKind::PermissionDenied.into());
        }
        let mut guard = Self {
            token,
            previous: Vec::new(),
        };
        for name in ["SeBackupPrivilege", "SeRestorePrivilege"] {
            let name = wide(name)?;
            let mut luid = LUID::default();
            // SAFETY: local privilege name and exact output LUID.
            if unsafe { LookupPrivilegeValueW(null(), name.as_ptr(), &mut luid) } == 0 {
                return Err(io::Error::last_os_error());
            }
            let requested = TOKEN_PRIVILEGES {
                PrivilegeCount: 1,
                Privileges: [LUID_AND_ATTRIBUTES {
                    Luid: luid,
                    Attributes: SE_PRIVILEGE_ENABLED,
                }],
            };
            let mut previous = TOKEN_PRIVILEGES::default();
            // SAFETY: one-privilege inputs and same-sized previous-state allocation.
            if unsafe {
                AdjustTokenPrivileges(
                    guard.token.as_raw_handle().cast(),
                    0,
                    &requested,
                    std::mem::size_of::<TOKEN_PRIVILEGES>() as u32,
                    &mut previous,
                    &mut length,
                )
            } == 0
            {
                return Err(io::Error::last_os_error());
            }
            // GetLastError reports partial success even when the API returned true.
            let error = unsafe { GetLastError() };
            guard.previous.push(previous);
            if error == ERROR_NOT_ALL_ASSIGNED {
                return Err(io::Error::from_raw_os_error(error as i32));
            }
        }
        Ok(guard)
    }
}
impl Drop for Privileges {
    fn drop(&mut self) {
        for previous in self.previous.iter().rev() {
            // SAFETY: restore only the exact state returned by our two adjustments.
            if unsafe {
                AdjustTokenPrivileges(
                    self.token.as_raw_handle().cast(),
                    0,
                    previous,
                    0,
                    null_mut(),
                    null_mut(),
                )
            } == 0
            {
                std::process::abort();
            }
        }
    }
}
