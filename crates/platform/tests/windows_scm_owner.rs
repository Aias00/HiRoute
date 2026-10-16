#![cfg(windows)]
//! Native authentication/profile/reversion gate. The daemon background target
//! separately drives actual installer, administrator helper, SCM and business APIs.

#[allow(dead_code)]
#[path = "../../daemon/tests/support/windows_account.rs"]
mod windows_account;

use std::fs::File;
use std::io;
use std::os::windows::io::FromRawHandle;
use std::ptr::null_mut;
use std::sync::atomic::{AtomicBool, Ordering};

use hiroute_platform::windows::{background, scm_owner};
use windows_sys::Win32::Foundation::{ERROR_NO_TOKEN, GetLastError};
use windows_sys::Win32::Security::TOKEN_QUERY;
use windows_sys::Win32::System::Threading::{GetCurrentThread, OpenThreadToken};
use zeroize::Zeroizing;

fn assert_reverted(original: &str) {
    let mut thread = null_mut();
    assert_eq!(
        unsafe { OpenThreadToken(GetCurrentThread(), TOKEN_QUERY, 1, &mut thread) },
        0
    );
    assert_eq!(unsafe { GetLastError() }, ERROR_NO_TOKEN);
    assert_eq!(background::current_sid_string().unwrap(), original);
}

#[test]
fn native_standard_owner_credentials_select_profile_and_reader_failure_reverts_identity() {
    let _privileges = windows_account::Privileges::enable();
    let original = background::current_sid_string().unwrap();
    let account = windows_account::Account::create();
    let visited = AtomicBool::new(false);
    let result = scm_owner::execute(
        &account.account,
        &account.password,
        scm_owner::Operation::Install,
        None,
        |owner| {
            visited.store(true, Ordering::SeqCst);
            assert_eq!(owner.sid, account.sid);
            assert_eq!(owner.profile, account.profile);
            assert_eq!(owner.account, account.account);
            assert!(owner.local_app_data.starts_with(&account.profile));
            // Inspect the actual thread token inside the production impersonation
            // scope. A caller supplied SID/path cannot satisfy this assertion.
            let mut raw = null_mut();
            assert_ne!(
                unsafe { OpenThreadToken(GetCurrentThread(), TOKEN_QUERY, 1, &mut raw) },
                0
            );
            let token = unsafe { File::from_raw_handle(raw.cast()) };
            assert_eq!(
                background::sid_string(&windows_account::token_sid(&token)).unwrap(),
                account.sid
            );
            assert!(
                owner
                    .open_private_file(
                        &account.profile.join(".local/share/hiroute/standalone.json")
                    )
                    .is_err(),
                "missing owner installation cannot manufacture machine authority"
            );
            Err(io::ErrorKind::PermissionDenied.into())
        },
    );
    assert!(
        visited.load(Ordering::SeqCst),
        "valid native credentials must reach their own mounted profile"
    );
    assert_eq!(
        result.err().unwrap().kind(),
        io::ErrorKind::PermissionDenied
    );
    assert_reverted(&original);

    let wrong = Zeroizing::new(format!("{}wrong", account.password.as_str()));
    visited.store(false, Ordering::SeqCst);
    let denied = scm_owner::execute(
        &account.account,
        &wrong,
        scm_owner::Operation::Install,
        None,
        |_| {
            visited.store(true, Ordering::SeqCst);
            Err(io::ErrorKind::PermissionDenied.into())
        },
    );
    assert!(denied.is_err());
    assert!(
        !visited.load(Ordering::SeqCst),
        "invalid owner password must not inspect files or modify SCM"
    );
    assert_reverted(&original);
    account.finish();
}
