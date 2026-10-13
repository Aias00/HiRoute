use std::ffi::c_void;
use std::fs::File;
use std::io;
use std::os::windows::io::AsRawHandle;
use std::path::Path;
use std::ptr::{addr_of, null, null_mut};

use windows_sys::Win32::Foundation::{
    CloseHandle, GENERIC_ALL, GENERIC_READ, GENERIC_WRITE, HANDLE, LocalFree,
};
use windows_sys::Win32::Security::Authorization::{
    EXPLICIT_ACCESS_W, GetSecurityInfo, NO_MULTIPLE_TRUSTEE, SE_FILE_OBJECT, SET_ACCESS,
    SetEntriesInAclW, SetSecurityInfo, TRUSTEE_IS_SID, TRUSTEE_IS_USER, TRUSTEE_W,
};
use windows_sys::Win32::Security::{
    ACCESS_ALLOWED_ACE, ACL, ACL_SIZE_INFORMATION, AclSizeInformation, DACL_SECURITY_INFORMATION,
    EqualSid, GetAce, GetAclInformation, GetLengthSid, GetSecurityDescriptorControl,
    GetTokenInformation, InitializeSecurityDescriptor, NO_INHERITANCE, OWNER_SECURITY_INFORMATION,
    PROTECTED_DACL_SECURITY_INFORMATION, PSID, SE_DACL_PROTECTED, SECURITY_ATTRIBUTES,
    SECURITY_DESCRIPTOR, SUB_CONTAINERS_AND_OBJECTS_INHERIT, SetSecurityDescriptorControl,
    SetSecurityDescriptorDacl, SetSecurityDescriptorOwner, TOKEN_QUERY, TOKEN_USER, TokenUser,
};
use windows_sys::Win32::Storage::FileSystem::{
    BY_HANDLE_FILE_INFORMATION, DELETE, FILE_ALL_ACCESS, FILE_DISPOSITION_INFO, FILE_SHARE_NONE,
    FILE_SHARE_READ, FILE_SHARE_WRITE, FileDispositionInfo, GetFileInformationByHandle,
    SetFileInformationByHandle, WRITE_DAC,
};
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION,
};

// WinNT.h defines ACCESS_ALLOWED_ACE_TYPE as zero. windows-sys exposes the
// constant behind an unrelated SystemServices feature, while the ACE itself
// correctly lives in Security.
const ACCESS_ALLOWED_ACE_KIND: u8 = 0;

/// Stable account identity for a namespace shared across this user's logons.
/// A service logon and an interactive CLI use the same user SID, not logon SID.
pub fn current_user_sid() -> io::Result<Vec<u8>> {
    let user = CurrentUser::load()?;
    // SAFETY: TOKEN_USER supplies a valid SID kept live by `user`.
    let length = unsafe { GetLengthSid(user.sid()) } as usize;
    if !(8..=68).contains(&length) {
        return Err(permission_error());
    }
    // SAFETY: the validated Windows SID contains exactly `length` readable bytes.
    Ok(unsafe { std::slice::from_raw_parts(user.sid().cast::<u8>(), length) }.to_vec())
}

pub(crate) fn verify_process_user(pid: u32) -> io::Result<()> {
    // SAFETY: pid comes from a kernel pipe-peer query. The returned process
    // handle is owned and requires only identity-query permission.
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if process.is_null() {
        return Err(io::Error::last_os_error());
    }
    let process = OwnedHandle(process);
    let mut token = null_mut();
    // SAFETY: this process handle and output pointer are live through the call.
    if unsafe { OpenProcessToken(process.0, TOKEN_QUERY, &mut token) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let token = OwnedHandle(token);
    let peer = CurrentUser::from_token(token.0)?;
    let current = CurrentUser::load()?;
    // SAFETY: both buffers own valid TOKEN_USER SID views for this comparison.
    if unsafe { EqualSid(peer.sid(), current.sid()) } == 0 {
        return Err(permission_error());
    }
    Ok(())
}

/// Open the actual object by handle-relative, reparse-free traversal.
/// Files deny deletion; directories retain object identity across renames.
pub fn open_identity(path: &Path, directory: bool) -> io::Result<File> {
    super::path::open(
        path,
        directory,
        GENERIC_READ,
        if directory {
            super::path::SHARE_ALL
        } else {
            FILE_SHARE_READ | FILE_SHARE_WRITE
        },
        false,
        None,
    )
}

/// Open an existing private object and check security on the held handle.
pub fn open_owner_only(path: &Path, directory: bool) -> io::Result<File> {
    let file = open_identity(path, directory)?;
    verify_owner_only_handle(&file, directory)?;
    Ok(file)
}

/// Create missing directories with private defaults; retain the native ACL of
/// an existing accessible directory. Every object is reparse-free and held.
pub fn ensure_private_directory(path: &Path) -> io::Result<File> {
    match open_identity(path, true) {
        Ok(file) => return Ok(file),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("directory has no parent"))?;
    let parent = ensure_private_directory(parent)?;
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::other("directory has no name"))?;
    match create_owner_only_directory_at(&parent, name) {
        Ok(file) => Ok(file),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => open_identity(path, true),
        Err(error) => Err(error),
    }
}

pub fn create_owner_only_directory(path: &Path) -> io::Result<File> {
    create_owner_only(path, true)
}

pub fn create_owner_only_file(path: &Path) -> io::Result<File> {
    create_owner_only(path, false)
}

fn create_owner_only(path: &Path, directory: bool) -> io::Result<File> {
    let security = OwnerSecurity::new(directory)?;
    // The descriptor is attached atomically to creation. Traversal handles
    // share deletion so an already held DELETE-capable parent remains usable;
    // resolving children by RootDirectory prevents path retargeting.
    let file = super::path::open(
        path,
        directory,
        GENERIC_READ | GENERIC_WRITE | DELETE,
        if directory {
            FILE_SHARE_READ | FILE_SHARE_WRITE
        } else {
            FILE_SHARE_NONE
        },
        true,
        Some(&security.descriptor),
    )?;
    verify_owner_only_handle(&file, directory)?;
    Ok(file)
}

/// Create one private child relative to a caller-owned directory identity.
/// No parent pathname is reopened, including when the parent is renamed.
pub fn create_owner_only_directory_at(parent: &File, name: &std::ffi::OsStr) -> io::Result<File> {
    create_owner_only_at(parent, name, true)
}

pub fn create_owner_only_file_at(parent: &File, name: &std::ffi::OsStr) -> io::Result<File> {
    create_owner_only_at(parent, name, false)
}

fn create_owner_only_at(
    parent: &File,
    name: &std::ffi::OsStr,
    directory: bool,
) -> io::Result<File> {
    let security = OwnerSecurity::new(directory)?;
    let file = super::path::relative(
        parent,
        name,
        directory,
        GENERIC_READ | GENERIC_WRITE | DELETE,
        if directory {
            FILE_SHARE_READ | FILE_SHARE_WRITE
        } else {
            FILE_SHARE_NONE
        },
        true,
        Some(&security.descriptor),
    )?;
    verify_owner_only_handle(&file, directory)?;
    Ok(file)
}

/// Open one existing child for identity-bound deletion; no path is re-resolved.
pub fn open_delete_at(parent: &File, name: &std::ffi::OsStr, directory: bool) -> io::Result<File> {
    super::path::relative(parent, name, directory, GENERIC_READ | DELETE,
        FILE_SHARE_READ | FILE_SHARE_WRITE, false, None)
}

pub fn open_delete(path: &Path, directory: bool) -> io::Result<File> {
    super::path::open(path, directory, GENERIC_READ | DELETE,
        FILE_SHARE_READ | FILE_SHARE_WRITE, false, None)
}

/// Writable identity for payload flush and handle-relative namespace changes.
pub fn open_write_through(path: &Path, directory: bool) -> io::Result<File> {
    super::path::open(path, directory,
        GENERIC_READ | DELETE | if directory { 0 } else { GENERIC_WRITE },
        super::path::SHARE_ALL, false, None)
}

pub fn delete_by_handle(file: &File) -> io::Result<()> {
    let disposition = FILE_DISPOSITION_INFO { DeleteFile: true };
    // SAFETY: `file` owns a DELETE-capable handle and the buffer exactly
    // matches FILE_DISPOSITION_INFO for FileDispositionInfo.
    if unsafe {
        SetFileInformationByHandle(
            file.as_raw_handle().cast(),
            FileDispositionInfo,
            (&disposition as *const FILE_DISPOSITION_INFO).cast(),
            size_of_u32::<FILE_DISPOSITION_INFO>()?,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

pub fn file_identity(file: &File) -> io::Result<(u32, u64)> {
    let mut information = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: `file` owns a live handle and `information` is the exact output
    // structure required by GetFileInformationByHandle.
    if unsafe { GetFileInformationByHandle(file.as_raw_handle().cast(), &mut information) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok((
        information.dwVolumeSerialNumber,
        (u64::from(information.nFileIndexHigh) << 32) | u64::from(information.nFileIndexLow),
    ))
}

pub fn set_owner_only(path: &Path, directory: bool) -> io::Result<()> {
    let object = super::path::open(
        path,
        directory,
        GENERIC_READ | WRITE_DAC,
        if directory {
            super::path::SHARE_ALL
        } else {
            FILE_SHARE_READ | FILE_SHARE_WRITE
        },
        false,
        None,
    )?;
    let user = CurrentUser::load()?;
    let mut acl: *mut ACL = null_mut();
    let access = EXPLICIT_ACCESS_W {
        grfAccessPermissions: FILE_ALL_ACCESS,
        grfAccessMode: SET_ACCESS,
        grfInheritance: if directory {
            SUB_CONTAINERS_AND_OBJECTS_INHERIT
        } else {
            NO_INHERITANCE
        },
        Trustee: TRUSTEE_W {
            pMultipleTrustee: null_mut(),
            MultipleTrusteeOperation: NO_MULTIPLE_TRUSTEE,
            TrusteeForm: TRUSTEE_IS_SID,
            TrusteeType: TRUSTEE_IS_USER,
            ptstrName: user.sid().cast(),
        },
    };
    // SAFETY: `access` and `acl` are valid for this call; the returned ACL is
    // released with LocalFree after SetSecurityInfo consumes it.
    let status = unsafe { SetEntriesInAclW(1, &access, null(), &mut acl) };
    if status != 0 {
        return Err(win32_error(status));
    }
    // SAFETY: the held object, `user` and `acl` remain live, and all
    // other optional security components are intentionally null.
    let status = unsafe {
        SetSecurityInfo(
            object.as_raw_handle().cast(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            null_mut(),
            null_mut(),
            acl,
            null(),
        )
    };
    // SAFETY: SetEntriesInAclW allocated `acl` with LocalAlloc.
    unsafe { LocalFree(acl.cast()) };
    if status != 0 {
        return Err(win32_error(status));
    }
    verify_owner_only_handle(&object, directory)
}

pub(crate) struct OwnerSecurity {
    user: CurrentUser,
    acl: *mut ACL,
    descriptor: SECURITY_DESCRIPTOR,
}

impl OwnerSecurity {
    pub(crate) fn descriptor(&self) -> *const c_void {
        (&self.descriptor as *const SECURITY_DESCRIPTOR).cast()
    }
    pub(crate) fn new(directory: bool) -> io::Result<Self> {
        let user = CurrentUser::load()?;
        let mut acl: *mut ACL = null_mut();
        let access = EXPLICIT_ACCESS_W {
            grfAccessPermissions: FILE_ALL_ACCESS,
            grfAccessMode: SET_ACCESS,
            grfInheritance: if directory {
                SUB_CONTAINERS_AND_OBJECTS_INHERIT
            } else {
                NO_INHERITANCE
            },
            Trustee: TRUSTEE_W {
                pMultipleTrustee: null_mut(),
                MultipleTrusteeOperation: NO_MULTIPLE_TRUSTEE,
                TrusteeForm: TRUSTEE_IS_SID,
                TrusteeType: TRUSTEE_IS_USER,
                ptstrName: user.sid().cast(),
            },
        };
        // SAFETY: `access` is fully initialized and `acl` is a live out ptr.
        let status = unsafe { SetEntriesInAclW(1, &access, null(), &mut acl) };
        if status != 0 {
            return Err(win32_error(status));
        }
        let mut descriptor = SECURITY_DESCRIPTOR::default();
        // SAFETY: the descriptor, current SID, and ACL all outlive creation.
        let initialized = unsafe {
            InitializeSecurityDescriptor((&mut descriptor as *mut SECURITY_DESCRIPTOR).cast(), 1)
                != 0
                && SetSecurityDescriptorOwner(
                    (&mut descriptor as *mut SECURITY_DESCRIPTOR).cast(),
                    user.sid(),
                    0,
                ) != 0
                && SetSecurityDescriptorDacl(
                    (&mut descriptor as *mut SECURITY_DESCRIPTOR).cast(),
                    1,
                    acl,
                    0,
                ) != 0
                && SetSecurityDescriptorControl(
                    (&mut descriptor as *mut SECURITY_DESCRIPTOR).cast(),
                    SE_DACL_PROTECTED,
                    SE_DACL_PROTECTED,
                ) != 0
        };
        if !initialized {
            // SAFETY: SetEntriesInAclW allocated this ACL with LocalAlloc.
            unsafe { LocalFree(acl.cast()) };
            return Err(io::Error::last_os_error());
        }
        Ok(Self {
            user,
            acl,
            descriptor,
        })
    }

    pub(crate) fn attributes(&mut self) -> io::Result<SECURITY_ATTRIBUTES> {
        Ok(SECURITY_ATTRIBUTES {
            nLength: size_of_u32::<SECURITY_ATTRIBUTES>()?,
            lpSecurityDescriptor: (&mut self.descriptor as *mut SECURITY_DESCRIPTOR).cast(),
            bInheritHandle: 0,
        })
    }
}

impl Drop for OwnerSecurity {
    fn drop(&mut self) {
        // Keep the SID owner alive until after the descriptor is no longer in
        // use, then release the ACL allocated by SetEntriesInAclW.
        let _ = self.user.words.len();
        if !self.acl.is_null() {
            // SAFETY: this allocation is uniquely owned by `self`.
            unsafe { LocalFree(self.acl.cast()) };
        }
    }
}

pub fn verify_owner_only(path: &Path, directory: bool) -> io::Result<()> {
    let file = open_identity(path, directory)?;
    verify_owner_only_handle(&file, directory)
}

pub fn verify_owner_only_handle(file: &impl AsRawHandle, directory: bool) -> io::Result<()> {
    let user = CurrentUser::load()?;
    let mut owner: PSID = null_mut();
    let mut dacl: *mut ACL = null_mut();
    let mut descriptor = null_mut();
    // SAFETY: the file owns a live kernel handle and every out-pointer remains
    // valid until the returned descriptor is released below.
    let status = unsafe {
        GetSecurityInfo(
            file.as_raw_handle().cast(),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            &mut owner,
            null_mut(),
            &mut dacl,
            null_mut(),
            &mut descriptor,
        )
    };
    if status != 0 {
        return Err(win32_error(status));
    }
    validate_security_descriptor(&user, owner, dacl, descriptor, directory)
}

fn validate_security_descriptor(
    user: &CurrentUser,
    owner: PSID,
    dacl: *mut ACL,
    descriptor: *mut c_void,
    directory: bool,
) -> io::Result<()> {
    let descriptor = LocalAllocation(descriptor);
    if owner.is_null() || dacl.is_null() {
        return Err(permission_error());
    }
    let mut control = 0_u16;
    let mut revision = 0_u32;
    // SAFETY: `descriptor` owns the live security descriptor returned above.
    if unsafe { GetSecurityDescriptorControl(descriptor.0, &mut control, &mut revision) } == 0
        || control & SE_DACL_PROTECTED == 0
    {
        return Err(permission_error());
    }
    // SAFETY: both SIDs are live for the duration of this function.
    if unsafe { EqualSid(owner, user.sid()) } == 0 {
        return Err(permission_error());
    }
    let mut information = ACL_SIZE_INFORMATION::default();
    // SAFETY: `dacl` points into the live security descriptor and the output
    // buffer has exactly the declared ACL_SIZE_INFORMATION size.
    if unsafe {
        GetAclInformation(
            dacl,
            (&mut information as *mut ACL_SIZE_INFORMATION).cast(),
            size_of_u32::<ACL_SIZE_INFORMATION>()?,
            AclSizeInformation,
        )
    } == 0
        || information.AceCount != 1
    {
        return Err(permission_error());
    }
    let mut ace: *mut c_void = null_mut();
    // SAFETY: the DACL reports one ACE and index zero is therefore valid.
    if unsafe { GetAce(dacl, 0, &mut ace) } == 0 || ace.is_null() {
        return Err(permission_error());
    }
    // SAFETY: the ACE type is checked before fields specific to an
    // ACCESS_ALLOWED_ACE are interpreted.
    let ace = unsafe { &*(ace.cast::<ACCESS_ALLOWED_ACE>()) };
    let expected_flags = if directory {
        SUB_CONTAINERS_AND_OBJECTS_INHERIT as u8
    } else {
        NO_INHERITANCE as u8
    };
    if ace.Header.AceType != ACCESS_ALLOWED_ACE_KIND
        || ace.Header.AceFlags != expected_flags
        || (ace.Mask != GENERIC_ALL && ace.Mask != FILE_ALL_ACCESS)
    {
        return Err(permission_error());
    }
    let ace_sid = addr_of!(ace.SidStart).cast_mut().cast::<c_void>();
    // SAFETY: ACCESS_ALLOWED_ACE stores its variable-length SID at SidStart.
    if unsafe { EqualSid(ace_sid, user.sid()) } == 0 {
        return Err(permission_error());
    }
    drop(descriptor);
    Ok(())
}

struct CurrentUser {
    words: Vec<usize>,
}

impl CurrentUser {
    fn load() -> io::Result<Self> {
        let mut token: HANDLE = null_mut();
        // SAFETY: `token` is a valid output pointer and the pseudo process
        // handle is valid in the current process.
        if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
            return Err(io::Error::last_os_error());
        }
        let token = OwnedHandle(token);
        Self::from_token(token.0)
    }

    fn from_token(token: HANDLE) -> io::Result<Self> {
        let mut bytes = 0_u32;
        // SAFETY: the documented sizing call accepts a null buffer and writes
        // the required byte count.
        unsafe {
            GetTokenInformation(token, TokenUser, null_mut(), 0, &mut bytes);
        }
        if bytes == 0 {
            return Err(io::Error::last_os_error());
        }
        let word = std::mem::size_of::<usize>();
        let words = (bytes as usize)
            .checked_add(word - 1)
            .ok_or_else(permission_error)?
            / word;
        let mut buffer = vec![0_usize; words];
        // SAFETY: the word-aligned buffer has at least the byte size returned
        // by the sizing call.
        if unsafe {
            GetTokenInformation(
                token,
                TokenUser,
                buffer.as_mut_ptr().cast(),
                bytes,
                &mut bytes,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(Self { words: buffer })
    }

    fn sid(&self) -> PSID {
        // SAFETY: `words` contains a TOKEN_USER populated by Windows and stays
        // immovable while its embedded SID pointer is used.
        unsafe { (*(self.words.as_ptr().cast::<TOKEN_USER>())).User.Sid }
    }
}

struct OwnedHandle(HANDLE);

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        // SAFETY: this handle was returned by OpenProcessToken and is owned.
        unsafe { CloseHandle(self.0) };
    }
}

struct LocalAllocation(*mut c_void);

impl Drop for LocalAllocation {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: GetNamedSecurityInfoW allocated this descriptor.
            unsafe { LocalFree(self.0) };
        }
    }
}

fn size_of_u32<T>() -> io::Result<u32> {
    u32::try_from(std::mem::size_of::<T>()).map_err(|_| permission_error())
}

fn win32_error(code: u32) -> io::Error {
    io::Error::from_raw_os_error(code as i32)
}

fn permission_error() -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        "private DACL is not owner-only",
    )
}
