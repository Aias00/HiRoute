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
    use std::path::Path;
    use std::time::{Duration, Instant};
    use windows_sys::Win32::Security::Authorization::{GetSecurityInfo, SE_FILE_OBJECT};
    use windows_sys::Win32::Security::{
        AccessCheck, DACL_SECURITY_INFORMATION, DuplicateTokenEx, GENERIC_MAPPING,
        GROUP_SECURITY_INFORMATION, GetUserObjectSecurity, LookupPrivilegeNameW,
        OWNER_SECURITY_INFORMATION, PRIVILEGE_SET, SecurityImpersonation, TOKEN_GROUPS,
        TOKEN_PRIVILEGES, TokenGroups, TokenImpersonation, TokenPrivileges,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ALL_ACCESS, FILE_GENERIC_EXECUTE, FILE_GENERIC_READ, FILE_GENERIC_WRITE,
    };
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, MODULEENTRY32W, Module32FirstW, Module32NextW, TH32CS_SNAPMODULE,
        TH32CS_SNAPMODULE32,
    };
    use windows_sys::Win32::System::StationsAndDesktops::{
        DESKTOP_CREATEMENU, DESKTOP_CREATEWINDOW, DESKTOP_ENUMERATE, DESKTOP_HOOKCONTROL,
        DESKTOP_JOURNALPLAYBACK, DESKTOP_JOURNALRECORD, DESKTOP_READOBJECTS, DESKTOP_SWITCHDESKTOP,
        DESKTOP_WRITEOBJECTS, GetProcessWindowStation, GetThreadDesktop,
    };
    use windows_sys::Win32::System::Threading::{GetCurrentProcessId, GetCurrentThreadId};

    const PROBE: &str = "HIROUTE_TEST_RESTRICTED_OWNER";
    const TEST: &str =
        "windows::service::token::tests::restricted_business_child_runs_with_owner_medium_token";

    fn impersonation_token(token: &File) -> io::Result<File> {
        let mut duplicate = null_mut();
        if unsafe {
            DuplicateTokenEx(
                token.as_raw_handle().cast(),
                TOKEN_QUERY,
                null(),
                SecurityImpersonation,
                TokenImpersonation,
                &mut duplicate,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        owned_token(duplicate)
    }

    fn check_descriptor(
        descriptor: *mut std::ffi::c_void,
        token: &File,
        mapping: &GENERIC_MAPPING,
        access: u32,
    ) -> io::Result<bool> {
        let mut privileges = [0_usize; 128];
        let mut length = std::mem::size_of_val(&privileges) as u32;
        let mut granted = 0;
        let mut allowed = 0;
        if unsafe {
            AccessCheck(
                descriptor,
                token.as_raw_handle().cast(),
                access,
                mapping,
                privileges.as_mut_ptr().cast::<PRIVILEGE_SET>(),
                &mut length,
                &mut granted,
                &mut allowed,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(allowed != 0)
    }

    fn file_access(path: &Path, directory: bool, token: &File) -> io::Result<bool> {
        let file = crate::windows::security::open_identity(path, directory)?;
        let mut descriptor = null_mut();
        let result = unsafe {
            GetSecurityInfo(
                file.as_raw_handle().cast(),
                SE_FILE_OBJECT,
                OWNER_SECURITY_INFORMATION | GROUP_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
                null_mut(),
                null_mut(),
                null_mut(),
                null_mut(),
                &mut descriptor,
            )
        };
        if result != 0 {
            return Err(io::Error::from_raw_os_error(result as i32));
        }
        let mapping = GENERIC_MAPPING {
            GenericRead: FILE_GENERIC_READ,
            GenericWrite: FILE_GENERIC_WRITE,
            GenericExecute: FILE_GENERIC_EXECUTE,
            GenericAll: FILE_ALL_ACCESS,
        };
        let result = check_descriptor(
            descriptor,
            token,
            &mapping,
            FILE_GENERIC_READ | FILE_GENERIC_EXECUTE,
        );
        unsafe { LocalFree(descriptor) };
        result
    }

    fn user_object_access(
        handle: windows_sys::Win32::Foundation::HANDLE,
        token: &File,
        mapping: &GENERIC_MAPPING,
    ) -> io::Result<bool> {
        let information =
            OWNER_SECURITY_INFORMATION | GROUP_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION;
        let mut needed = 0;
        unsafe { GetUserObjectSecurity(handle, &information, null_mut(), 0, &mut needed) };
        if !(1..=65536).contains(&needed) {
            return Err(io::Error::last_os_error());
        }
        let mut descriptor =
            vec![0_usize; (needed as usize).div_ceil(std::mem::size_of::<usize>())];
        if unsafe {
            GetUserObjectSecurity(
                handle,
                &information,
                descriptor.as_mut_ptr().cast(),
                needed,
                &mut needed,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        check_descriptor(
            descriptor.as_mut_ptr().cast(),
            token,
            mapping,
            mapping.GenericAll,
        )
    }

    fn diagnose_access(binary: &Path, token: &File) {
        let token = impersonation_token(token).unwrap();
        for (depth, path) in binary.ancestors().take(16).enumerate() {
            let result = file_access(path, depth != 0, &token);
            eprintln!("restricted source DACL read/execute: depth={depth}, result={result:?}");
        }
        let snapshot = unsafe {
            CreateToolhelp32Snapshot(
                TH32CS_SNAPMODULE | TH32CS_SNAPMODULE32,
                GetCurrentProcessId(),
            )
        };
        if snapshot == windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE {
            eprintln!(
                "restricted parent loaded-DLL inventory: {:?}",
                io::Error::last_os_error()
            );
        } else {
            let snapshot = owned_token(snapshot).unwrap();
            let mut module = MODULEENTRY32W {
                dwSize: std::mem::size_of::<MODULEENTRY32W>() as u32,
                ..Default::default()
            };
            let mut present =
                unsafe { Module32FirstW(snapshot.as_raw_handle().cast(), &mut module) } != 0;
            let mut count = 0;
            while present && count < 256 {
                let name_length = module
                    .szModule
                    .iter()
                    .position(|value| *value == 0)
                    .unwrap_or(module.szModule.len());
                let path_length = module
                    .szExePath
                    .iter()
                    .position(|value| *value == 0)
                    .unwrap_or(module.szExePath.len());
                let name = String::from_utf16_lossy(&module.szModule[..name_length]);
                if let Ok(path) = String::from_utf16(&module.szExePath[..path_length]) {
                    let result = file_access(Path::new(&path), false, &token);
                    eprintln!(
                        "restricted parent loaded image DACL read/execute: module={name:?}, result={result:?}"
                    );
                }
                count += 1;
                present =
                    unsafe { Module32NextW(snapshot.as_raw_handle().cast(), &mut module) } != 0;
            }
        }
        // This is read-only evidence about the caller's existing station and
        // desktop. It does not prove which object the child selected, and a
        // denied full mask alone does not establish the DLL-init failure cause.
        const READ_CONTROL: u32 = 0x0002_0000;
        // WinUser.h WINSTA_* object masks (windows-sys 0.61.2 exposes these
        // constants behind WindowsAndMessaging, unrelated to the probe APIs).
        let station = GENERIC_MAPPING {
            GenericRead: 0x0001 | 0x0100 | 0x0002 | READ_CONTROL,
            GenericWrite: 0x0004 | 0x0008 | 0x0010 | READ_CONTROL,
            GenericExecute: 0x0020 | 0x0040 | READ_CONTROL,
            GenericAll: 0x037f,
        };
        let desktop = GENERIC_MAPPING {
            GenericRead: DESKTOP_READOBJECTS | DESKTOP_ENUMERATE | READ_CONTROL,
            GenericWrite: DESKTOP_CREATEMENU
                | DESKTOP_CREATEWINDOW
                | DESKTOP_HOOKCONTROL
                | DESKTOP_JOURNALPLAYBACK
                | DESKTOP_JOURNALRECORD
                | DESKTOP_WRITEOBJECTS
                | READ_CONTROL,
            GenericExecute: DESKTOP_SWITCHDESKTOP | READ_CONTROL,
            GenericAll: 0x000f_01ff,
        };
        eprintln!(
            "restricted caller window station full DACL: {:?}",
            user_object_access(unsafe { GetProcessWindowStation() }, &token, &station)
        );
        eprintln!(
            "restricted caller desktop full DACL: {:?}",
            user_object_access(
                unsafe { GetThreadDesktop(GetCurrentThreadId()) },
                &token,
                &desktop
            )
        );
    }

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

    fn launch_probe(
        binary: &Path,
        token: &File,
        env: &[(OsString, OsString)],
    ) -> (i32, String, String) {
        let args = [
            OsString::from("--exact"),
            OsString::from(TEST),
            OsString::from("--nocapture"),
        ];
        let mut child = Child::spawn_as_token(
            Launch {
                program: binary,
                args: &args,
                env,
                cwd: binary.parent(),
                stdin: Stream::Null,
                stdout: Stream::Piped,
                stderr: Stream::Piped,
            },
            token,
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
        let mut errors = String::new();
        child
            .stderr
            .take()
            .unwrap()
            .take(65536)
            .read_to_string(&mut errors)
            .unwrap();
        assert!(child.empty().unwrap());
        (code, output, errors)
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
        let mut env: Vec<_> = std::env::vars_os()
            .filter(|(key, _)| key != PROBE)
            .collect();
        env.push((
            PROBE.into(),
            background::current_sid_string().unwrap().into(),
        ));
        diagnose_access(&binary, &token);
        let (original_code, original_output, original_errors) = launch_probe(&binary, &token, &env);
        eprintln!(
            "restricted original-source child: exit=0x{:08x}, stdout={original_output:?}, stderr={original_errors:?}",
            original_code as u32
        );

        // The production business image resides under its owner's profile.
        // Stage the same held bytes with that native private ACL, without
        // changing permissions on the CI checkout or using an unrestricted token.
        let epoch = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let stage = background::local_app_data().unwrap().join(format!(
            "HiRoute/restricted-probe-{}-{epoch}",
            std::process::id()
        ));
        let parent = crate::windows::security::ensure_private_directory(&stage).unwrap();
        struct Remove(std::path::PathBuf);
        impl Drop for Remove {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        let _remove = Remove(stage.clone());
        let staged_binary = stage.join("hirouted.exe");
        let mut source = crate::windows::security::open_identity(&binary, false).unwrap();
        let mut destination = crate::windows::security::create_owner_only_file_at(
            &parent,
            std::ffi::OsStr::new("hirouted.exe"),
        )
        .unwrap();
        io::copy(&mut source, &mut destination).unwrap();
        destination.sync_all().unwrap();
        drop(destination);
        let impersonation = impersonation_token(&token).unwrap();
        assert!(file_access(&staged_binary, false, &impersonation).unwrap());
        let (code, output, errors) = launch_probe(&staged_binary, &token, &env);
        assert_eq!(
            code, 0,
            "restricted owner-profile child failed (exit=0x{:08x}, original_exit=0x{original_code:08x}, primary token remained restricted): stdout={output:?}; stderr={errors:?}",
            code as u32
        );
        assert!(
            output.contains("HIRoute restricted child identity verified"),
            "child test was not selected: {output}"
        );
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
