//! A Task fixture must use a real logged-on user's limited token. Restricting an
//! elevated token's groups/IL does not prove TokenElevation is zero, and Scheduler
//! independently chooses its InteractiveToken rather than our installer token.
//!
//! Native contracts:
//! https://learn.microsoft.com/en-us/windows/win32/api/winnt/ne-winnt-token_elevation_type
//! https://learn.microsoft.com/en-us/windows/win32/api/winnt/ns-winnt-token_linked_token
//! https://learn.microsoft.com/en-us/windows/win32/taskschd/security-contexts-for-running-tasks
use std::fs::File;
use std::os::windows::io::{AsRawHandle, FromRawHandle};
use std::ptr::{null, null_mut};

use hiroute_platform::windows::{background, service};
use windows_sys::Win32::Foundation::{GetLastError, HANDLE};
use windows_sys::Win32::Security::*;
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

use super::windows_account as account;

pub struct Owner {
    pub token: File,
    pub session: u32,
}

fn owned(handle: HANDLE) -> File {
    assert!(!handle.is_null(), "Task owner token handle must be present");
    unsafe { File::from_raw_handle(handle.cast()) }
}

fn dword(token: &File, class: TOKEN_INFORMATION_CLASS) -> u32 {
    let information = account::information(token, class);
    unsafe { *information.as_ptr().cast::<u32>() }
}

pub fn owner() -> Owner {
    let mut original = null_mut();
    assert_ne!(
        unsafe {
            OpenProcessToken(
                GetCurrentProcess(),
                TOKEN_QUERY | TOKEN_DUPLICATE,
                &mut original,
            )
        },
        0,
        "Task gate cannot query its actual runner token"
    );
    let original = owned(original);
    let original_sid = account::token_sid(&original);
    let sid = background::sid_string(&original_sid).unwrap();
    let elevation = dword(&original, TokenElevation);
    let kind = dword(&original, TokenElevationType);
    let runner_session = dword(&original, TokenSessionId);
    eprintln!(
        "Task owner prerequisite: runner_elevation={elevation}, elevation_type={kind}, session={runner_session}"
    );
    let selected = if elevation == 0 {
        original
    } else {
        // Default means no linked token. A real standard interactive logon or a
        // UAC split-token session is a runner prerequisite, not a product failure.
        // Do not synthesize a LUA token or remove the standard-owner assertion.
        assert_eq!(
            kind, TokenElevationTypeFull as u32,
            "Task gate needs a real standard interactive owner or a UAC linked limited token; an elevated default token has no linked owner"
        );
        let mut linked = TOKEN_LINKED_TOKEN::default();
        let mut length = 0;
        let received = unsafe {
            GetTokenInformation(
                original.as_raw_handle().cast(),
                TokenLinkedToken,
                (&mut linked as *mut TOKEN_LINKED_TOKEN).cast(),
                std::mem::size_of::<TOKEN_LINKED_TOKEN>() as u32,
                &mut length,
            )
        };
        let error = unsafe { GetLastError() };
        assert_ne!(
            received, 0,
            "Task gate cannot obtain its actual linked limited token; native_error={error}"
        );
        let linked = owned(linked.LinkedToken);
        assert_eq!(length as usize, std::mem::size_of::<TOKEN_LINKED_TOKEN>());
        assert_eq!(
            dword(&linked, TokenElevationType),
            TokenElevationTypeLimited as u32,
            "Only the actual UAC limited token can be selected"
        );
        linked
    };
    // The duplicate grants only what launching the existing owner fixture needs;
    // no SID/group/IL/default owner is edited to imitate a standard user.
    let mut primary = null_mut();
    let duplicated = unsafe {
        DuplicateTokenEx(
            selected.as_raw_handle().cast(),
            TOKEN_QUERY | TOKEN_DUPLICATE | TOKEN_ASSIGN_PRIMARY | TOKEN_ADJUST_DEFAULT,
            null(),
            SecurityImpersonation,
            TokenPrimary,
            &mut primary,
        )
    };
    let error = unsafe { GetLastError() };
    assert_ne!(
        duplicated, 0,
        "Task gate cannot duplicate its actual limited primary token; native_error={error}"
    );
    let token = owned(primary);
    assert_eq!(account::token_sid(&token), original_sid);
    let session = account::assert_standard(&token, &sid);
    assert_eq!(
        session, runner_session,
        "Task token must retain the real logon session"
    );
    let name = service::account_name_for_sid(&original_sid).unwrap();
    assert_eq!(
        session,
        account::interactive_session(&name),
        "Task owner must actually be logged on in its WTS session"
    );
    let owner = account::information(&token, TokenOwner);
    let owner = unsafe { &*owner.as_ptr().cast::<TOKEN_OWNER>() };
    let owner: &[u8] = unsafe {
        std::slice::from_raw_parts(owner.Owner.cast::<u8>(), GetLengthSid(owner.Owner) as usize)
    };
    eprintln!(
        "Task owner selected: elevation=0, medium_integrity=true, sid_matches=true, session={session}, default_owner_is_user={}",
        owner == original_sid
    );
    // Managed files/directories continue to be created with explicit owner SDs;
    // their security never relies on this diagnostic default-owner observation.
    Owner { token, session }
}
