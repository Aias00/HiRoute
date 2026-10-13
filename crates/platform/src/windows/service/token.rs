//! The SCM business child retains owner identity while dropping administrator authority.
use super::wide;
use std::fs::File;
use std::io;
use std::os::windows::io::{AsRawHandle, FromRawHandle};
use std::ptr::{null, null_mut};
use windows_sys::Win32::Foundation::LocalFree;
use windows_sys::Win32::Security::Authorization::ConvertStringSidToSidW;
use windows_sys::Win32::Security::{
    CreateRestrictedToken, CreateWellKnownSid, DISABLE_MAX_PRIVILEGE, EqualSid, GetLengthSid,
    GetTokenInformation, LOGON32_LOGON_INTERACTIVE, LOGON32_PROVIDER_DEFAULT, LogonUserW,
    SID_AND_ATTRIBUTES, SetTokenInformation, TOKEN_ADJUST_DEFAULT, TOKEN_ASSIGN_PRIMARY,
    TOKEN_DUPLICATE, TOKEN_MANDATORY_LABEL, TOKEN_QUERY, TOKEN_USER, TokenIntegrityLevel,
    TokenUser, WinBuiltinAdministratorsSid,
};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
// WinNT.h SE_GROUP_INTEGRITY, independent of the SystemServices API feature.
const SE_GROUP_INTEGRITY: u32 = 0x0000_0020;

pub(super) fn verify_credentials(
    account: &[u16],
    password: &[u16],
    owner_sid: &[u8],
) -> io::Result<()> {
    let mut token = null_mut();
    // SAFETY: a qualified DOMAIN\\name and live zeroizing password select only
    // the recorded owner; an account mismatch never falls back to SYSTEM.
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
mod tests {
    use super::*;
    use crate::windows::{
        background,
        job::{Child, Launch, Stream},
    };
    use std::ffi::OsString;
    use std::io::Read;
    use std::time::{Duration, Instant};
    use windows_sys::Win32::Security::{
        LookupPrivilegeNameW, TOKEN_GROUPS, TOKEN_PRIVILEGES, TokenGroups, TokenPrivileges,
    };

    const PROBE: &str = "HIROUTE_TEST_RESTRICTED_OWNER";
    const TEST: &str =
        "windows::service::token::tests::restricted_business_child_runs_with_owner_medium_token";

    fn information(token: &File, class: i32) -> Vec<usize> {
        let mut length = 0;
        unsafe {
            GetTokenInformation(
                token.as_raw_handle().cast(),
                class,
                null_mut(),
                0,
                &mut length,
            );
        }
        assert!((1..=65536).contains(&length));
        let mut words = vec![0; (length as usize).div_ceil(std::mem::size_of::<usize>())];
        assert_ne!(
            unsafe {
                GetTokenInformation(
                    token.as_raw_handle().cast(),
                    class,
                    words.as_mut_ptr().cast(),
                    length,
                    &mut length,
                )
            },
            0
        );
        words
    }

    fn assert_actual_child_identity(expected: &str) {
        let mut raw = null_mut();
        assert_ne!(
            unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut raw) },
            0
        );
        let token = owned_token(raw).unwrap();
        assert_eq!(background::current_sid_string().unwrap(), expected);
        let label = information(&token, TokenIntegrityLevel);
        let label = unsafe { &*label.as_ptr().cast::<TOKEN_MANDATORY_LABEL>() };
        let length = unsafe { GetLengthSid(label.Label.Sid) } as usize;
        let bytes = unsafe { std::slice::from_raw_parts(label.Label.Sid.cast::<u8>(), length) };
        assert_eq!(background::sid_string(bytes).unwrap(), "S-1-16-8192");
        let mut admin = vec![0_usize; 68_usize.div_ceil(std::mem::size_of::<usize>())];
        let mut length = 68;
        assert_ne!(
            unsafe {
                CreateWellKnownSid(
                    WinBuiltinAdministratorsSid,
                    null_mut(),
                    admin.as_mut_ptr().cast(),
                    &mut length,
                )
            },
            0
        );
        let groups = information(&token, TokenGroups);
        let groups = unsafe { &*groups.as_ptr().cast::<TOKEN_GROUPS>() };
        let entries = unsafe {
            std::slice::from_raw_parts(groups.Groups.as_ptr(), groups.GroupCount as usize)
        };
        for group in entries {
            if unsafe { EqualSid(group.Sid, admin.as_mut_ptr().cast()) } != 0 {
                // SDK: SE_GROUP_USE_FOR_DENY_ONLY=0x10, SE_GROUP_ENABLED=0x4.
                assert_eq!(group.Attributes & 0x10, 0x10);
                assert_eq!(group.Attributes & 0x4, 0);
            }
        }
        let privileges = information(&token, TokenPrivileges);
        let privileges = unsafe { &*privileges.as_ptr().cast::<TOKEN_PRIVILEGES>() };
        let entries = unsafe {
            std::slice::from_raw_parts(
                privileges.Privileges.as_ptr(),
                privileges.PrivilegeCount as usize,
            )
        };
        for privilege in entries.iter().filter(|value| value.Attributes & 0x2 != 0) {
            let mut name = [0_u16; 128];
            let mut count = name.len() as u32;
            assert_ne!(
                unsafe {
                    LookupPrivilegeNameW(null(), &privilege.Luid, name.as_mut_ptr(), &mut count)
                },
                0
            );
            assert_eq!(
                String::from_utf16(&name[..count as usize]).unwrap(),
                "SeChangeNotifyPrivilege"
            );
        }
        println!("HIRoute restricted child identity verified");
    }

    /// Executes the production restriction and atomic Job launch path. Run this
    /// gate both as CI's administrator and its fresh standard service owner;
    /// compile-only or administrator-only evidence cannot prove the latter.
    #[test]
    fn restricted_business_child_runs_with_owner_medium_token() {
        if let Ok(expected) = std::env::var(PROBE) {
            assert_actual_child_identity(&expected);
            return;
        }
        let token = restricted_token().unwrap();
        let binary = std::env::current_exe().unwrap();
        let args = [
            OsString::from("--exact"),
            OsString::from(TEST),
            OsString::from("--nocapture"),
        ];
        let mut env: Vec<_> = std::env::vars_os()
            .filter(|(key, _)| key != PROBE)
            .collect();
        env.push((
            PROBE.into(),
            background::current_sid_string().unwrap().into(),
        ));
        let mut child = Child::spawn_as_token(
            Launch {
                program: &binary,
                args: &args,
                env: &env,
                cwd: binary.parent(),
                stdin: Stream::Null,
                stdout: Stream::Piped,
                stderr: Stream::Piped,
            },
            &token,
        )
        .unwrap();
        let deadline = Instant::now() + Duration::from_secs(15);
        let code = loop {
            if let Some(code) = child.exit_code().unwrap() {
                break code;
            }
            assert!(Instant::now() < deadline, "restricted child did not exit");
            std::thread::sleep(Duration::from_millis(10));
        };
        let mut output = String::new();
        child
            .stdout
            .take()
            .unwrap()
            .take(65536)
            .read_to_string(&mut output)
            .unwrap();
        assert_eq!(code, 0, "restricted child failed: {output}");
        assert!(
            output.contains("HIRoute restricted child identity verified"),
            "child test was not selected: {output}"
        );
        assert!(child.empty().unwrap());
    }
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
