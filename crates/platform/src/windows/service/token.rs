//! The SCM business child retains owner identity while dropping administrator authority.
use super::wide;
use std::fs::File;
use std::io;
use std::os::windows::io::{AsRawHandle, FromRawHandle};
use std::ptr::{null, null_mut};
use windows_sys::Win32::Foundation::LocalFree;
use windows_sys::Win32::Security::Authorization::{
    ConvertStringSecurityDescriptorToSecurityDescriptorW, ConvertStringSidToSidW, SDDL_REVISION_1,
};
use windows_sys::Win32::Security::{
    CreateRestrictedToken, CreateWellKnownSid, DISABLE_MAX_PRIVILEGE, EqualSid, GetLengthSid,
    GetSecurityDescriptorDacl, GetTokenInformation, LOGON32_LOGON_INTERACTIVE,
    LOGON32_PROVIDER_DEFAULT, LogonUserW, SID_AND_ATTRIBUTES, SetTokenInformation,
    TOKEN_ADJUST_DEFAULT, TOKEN_ASSIGN_PRIMARY, TOKEN_DEFAULT_DACL, TOKEN_DUPLICATE,
    TOKEN_MANDATORY_LABEL, TOKEN_OWNER, TOKEN_QUERY, TOKEN_USER, TokenDefaultDacl,
    TokenIntegrityLevel, TokenOwner, TokenUser, WinBuiltinAdministratorsSid,
};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
// WinNT.h SE_GROUP_INTEGRITY, independent of the SystemServices API feature.
const SE_GROUP_INTEGRITY: u32 = 0x0000_0020;

pub(super) fn verify_credentials(
    account: &str,
    password: &[u16],
    owner_sid: &[u8],
) -> io::Result<()> {
    let (domain, username) = account
        .split_once('\\')
        .filter(|(domain, username)| {
            !domain.is_empty() && !username.is_empty() && !username.contains('\\')
        })
        .ok_or(io::ErrorKind::InvalidInput)?;
    let domain = wide(domain)?;
    let username = wide(username)?;
    let mut token = null_mut();
    // SAFETY: a qualified DOMAIN\\name and live zeroizing password select only
    // the recorded owner; an account mismatch never falls back to SYSTEM.
    if unsafe {
        LogonUserW(
            username.as_ptr(),
            domain.as_ptr(),
            password.as_ptr(),
            LOGON32_LOGON_INTERACTIVE,
            LOGON32_PROVIDER_DEFAULT,
            &mut token,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    let token = owned_token(token)?;
    let mut needed = 0;
    unsafe {
        GetTokenInformation(
            token.as_raw_handle().cast(),
            TokenUser,
            null_mut(),
            0,
            &mut needed,
        );
    }
    if needed == 0 || needed > 64 * 1024 {
        return Err(io::Error::from(io::ErrorKind::InvalidData));
    }
    let mut words = vec![0_usize; (needed as usize).div_ceil(std::mem::size_of::<usize>())];
    if unsafe {
        GetTokenInformation(
            token.as_raw_handle().cast(),
            TokenUser,
            words.as_mut_ptr().cast(),
            needed,
            &mut needed,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    let mut owner = owner_sid.to_vec();
    // SAFETY: TOKEN_USER populated by LogonUser/GetTokenInformation owns its SID.
    if unsafe {
        EqualSid(
            (*words.as_ptr().cast::<TOKEN_USER>()).User.Sid,
            owner.as_mut_ptr().cast(),
        )
    } == 0
    {
        return Err(io::Error::from(io::ErrorKind::PermissionDenied));
    }
    Ok(())
}

fn owned_token(handle: windows_sys::Win32::Foundation::HANDLE) -> io::Result<File> {
    if handle.is_null() {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: native token API transferred this newly owned kernel handle.
    Ok(unsafe { File::from_raw_handle(handle.cast()) })
}

#[cfg(test)]
mod tests;

/// The parent's default DACL may grant only the administrator group that this
/// token disables. New child process/thread/kernel objects must remain usable by
/// their actual owner. Limit the replacement to that SID and SYSTEM; no group or
/// logon SID obtains new authority over the privileged service host.
fn owner_default_dacl(token: &File) -> io::Result<()> {
    let mut needed = 0;
    unsafe {
        GetTokenInformation(
            token.as_raw_handle().cast(),
            TokenUser,
            null_mut(),
            0,
            &mut needed,
        );
    }
    if !(1..=65536).contains(&needed) {
        return Err(io::Error::from(io::ErrorKind::InvalidData));
    }
    let mut user = vec![0_usize; (needed as usize).div_ceil(std::mem::size_of::<usize>())];
    if unsafe {
        GetTokenInformation(
            token.as_raw_handle().cast(),
            TokenUser,
            user.as_mut_ptr().cast(),
            needed,
            &mut needed,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    let sid = unsafe { (*user.as_ptr().cast::<TOKEN_USER>()).User.Sid };
    let length = unsafe { GetLengthSid(sid) } as usize;
    if !(8..=68).contains(&length) {
        return Err(io::Error::from(io::ErrorKind::InvalidData));
    }
    let sid_text = super::super::background::sid_string(unsafe {
        std::slice::from_raw_parts(sid.cast::<u8>(), length)
    })?;
    let text = wide(&format!("D:P(A;;GA;;;SY)(A;;GA;;;{sid_text})"))?;
    let mut descriptor = null_mut();
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
    let result = (|| {
        let mut present = 0;
        let mut defaulted = 0;
        let mut acl = null_mut();
        if unsafe { GetSecurityDescriptorDacl(descriptor, &mut present, &mut acl, &mut defaulted) }
            == 0
            || present == 0
            || acl.is_null()
        {
            return Err(io::Error::from(io::ErrorKind::InvalidData));
        }
        let dacl = TOKEN_DEFAULT_DACL { DefaultDacl: acl };
        let owner = TOKEN_OWNER { Owner: sid };
        for (class, value, size) in [
            (
                TokenDefaultDacl,
                (&dacl as *const TOKEN_DEFAULT_DACL).cast(),
                std::mem::size_of::<TOKEN_DEFAULT_DACL>() as u32,
            ),
            (
                TokenOwner,
                (&owner as *const TOKEN_OWNER).cast(),
                std::mem::size_of::<TOKEN_OWNER>() as u32,
            ),
        ] {
            // SAFETY: ACL/SID storage remains live; the API copies their bytes.
            if unsafe { SetTokenInformation(token.as_raw_handle().cast(), class, value, size) } == 0
            {
                return Err(io::Error::last_os_error());
            }
        }
        Ok(())
    })();
    unsafe {
        LocalFree(descriptor);
    }
    result
}

pub(super) fn restricted_token() -> io::Result<File> {
    let mut current = null_mut();
    // SAFETY: current process token; exact rights needed for restriction/spawn.
    if unsafe {
        OpenProcessToken(
            GetCurrentProcess(),
            TOKEN_QUERY | TOKEN_DUPLICATE | TOKEN_ASSIGN_PRIMARY | TOKEN_ADJUST_DEFAULT,
            &mut current,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    let current = owned_token(current)?;
    let mut administrators = vec![0_usize; 68_usize.div_ceil(std::mem::size_of::<usize>())];
    let mut length = (administrators.len() * std::mem::size_of::<usize>()) as u32;
    // SAFETY: well-known SID output is aligned and has SECURITY_MAX_SID_SIZE bytes.
    if unsafe {
        CreateWellKnownSid(
            WinBuiltinAdministratorsSid,
            null_mut(),
            administrators.as_mut_ptr().cast(),
            &mut length,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    let disabled = SID_AND_ATTRIBUTES {
        Sid: administrators.as_mut_ptr().cast(),
        Attributes: 0,
    };
    let mut token = null_mut();
    // SAFETY: primary token creation disables administrator authority and every
    // privilege except traversal. It retains the account SID/private-file access.
    if unsafe {
        CreateRestrictedToken(
            current.as_raw_handle().cast(),
            DISABLE_MAX_PRIVILEGE,
            1,
            &disabled,
            0,
            null(),
            0,
            null(),
            &mut token,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    let token = owned_token(token)?;
    #[cfg(test)]
    tests::diagnose_token(&token, "before owner object policy");
    owner_default_dacl(&token)?;
    #[cfg(test)]
    tests::diagnose_token(&token, "after owner object policy");
    let medium = wide("S-1-16-8192")?;
    let mut sid = null_mut();
    if unsafe { ConvertStringSidToSidW(medium.as_ptr(), &mut sid) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let label = TOKEN_MANDATORY_LABEL {
        Label: SID_AND_ATTRIBUTES {
            Sid: sid,
            Attributes: SE_GROUP_INTEGRITY,
        },
    };
    // SAFETY: label SID remains live until SetTokenInformation copies it.
    let success = unsafe {
        SetTokenInformation(
            token.as_raw_handle().cast(),
            TokenIntegrityLevel,
            (&label as *const TOKEN_MANDATORY_LABEL).cast(),
            std::mem::size_of::<TOKEN_MANDATORY_LABEL>() as u32 + GetLengthSid(sid),
        )
    };
    let error = io::Error::last_os_error();
    unsafe {
        LocalFree(sid);
    }
    if success == 0 {
        return Err(error);
    }
    Ok(token)
}
