//! SCM boundary: owned service handles, same-account credentials and dispatcher.
use std::fs::File;
use std::io;
use std::ptr::{null, null_mut};
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{
    ERROR_SERVICE_ALREADY_RUNNING, ERROR_SERVICE_DOES_NOT_EXIST, ERROR_SERVICE_MARKED_FOR_DELETE,
    ERROR_SERVICE_NOT_ACTIVE, LocalFree,
};
use windows_sys::Win32::Security::Authentication::Identity::{
    LSA_OBJECT_ATTRIBUTES, LSA_UNICODE_STRING, LsaAddAccountRights, LsaClose,
    LsaNtStatusToWinError, LsaOpenPolicy, POLICY_CREATE_ACCOUNT, POLICY_LOOKUP_NAMES,
};
use windows_sys::Win32::Security::Authorization::{
    ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows_sys::Win32::Security::{DACL_SECURITY_INFORMATION, LookupAccountSidW, SID_NAME_USE};
use windows_sys::Win32::System::Services::*;
use zeroize::Zeroizing;

use super::background::{Definition, Manager, quote_argument};

mod host;
mod token;
pub use host::{ServiceShutdown, run_service};

struct Handle(SC_HANDLE);
impl Handle {
    fn new(value: SC_HANDLE) -> io::Result<Self> {
        if value.is_null() {
            Err(io::Error::last_os_error())
        } else {
            Ok(Self(value))
        }
    }
}
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            CloseServiceHandle(self.0);
        }
    }
}

fn wide(value: &str) -> io::Result<Vec<u16>> {
    if value.contains('\0') {
        return Err(io::Error::from(io::ErrorKind::InvalidInput));
    }
    Ok(value.encode_utf16().chain(Some(0)).collect())
}

fn manager(access: u32) -> io::Result<Handle> {
    // SAFETY: local active SCM, no remote machine and minimum requested rights.
    Handle::new(unsafe { OpenSCManagerW(null(), null(), access) })
}
fn open(definition: &Definition, access: u32) -> io::Result<Handle> {
    definition.validate()?;
    open_for_owner(definition, &definition.owner_sid, &account_name()?, access)
}

fn open_for_owner(
    definition: &Definition,
    owner_sid: &str,
    account: &str,
    access: u32,
) -> io::Result<Handle> {
    definition.validate_for_owner_sid(owner_sid)?;
    if definition.manager != Manager::Scm {
        return Err(io::Error::from(io::ErrorKind::InvalidInput));
    }
    let scm = manager(SC_MANAGER_CONNECT)?;
    let name = wide(&definition.name)?;
    // SAFETY: SCM and NUL-terminated service name outlive this call.
    let handle =
        Handle::new(unsafe { OpenServiceW(scm.0, name.as_ptr(), access | SERVICE_QUERY_CONFIG) })?;
    verify_for_owner(&handle, definition, owner_sid, account)?;
    Ok(handle)
}

pub fn account_name() -> io::Result<String> {
    account_name_for_sid(&super::security::current_user_sid()?)
}

pub fn account_name_for_sid(sid: &[u8]) -> io::Result<String> {
    super::background::sid_string(sid)?;
    let mut sid = sid.to_vec();
    let mut name = vec![0_u16; 1024];
    let mut domain = vec![0_u16; 1024];
    let mut names = name.len() as u32;
    let mut domains = domain.len() as u32;
    let mut usage: SID_NAME_USE = 0;
    // SAFETY: SID and two bounded output buffers remain live.
    if unsafe {
        LookupAccountSidW(
            null(),
            sid.as_mut_ptr().cast(),
            name.as_mut_ptr(),
            &mut names,
            domain.as_mut_ptr(),
            &mut domains,
            &mut usage,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    // The length parameters are capacities on input and sizing outputs only on
    // an insufficient-buffer failure. Success returns NUL-terminated strings.
    let name_length = name
        .iter()
        .position(|value| *value == 0)
        .ok_or(io::ErrorKind::InvalidData)?;
    let domain_length = domain
        .iter()
        .position(|value| *value == 0)
        .ok_or(io::ErrorKind::InvalidData)?;
    let name = String::from_utf16(&name[..name_length]).map_err(io::Error::other)?;
    let domain = String::from_utf16(&domain[..domain_length]).map_err(io::Error::other)?;
    if name.is_empty() || domain.is_empty() {
        return Err(io::Error::from(io::ErrorKind::InvalidData));
    }
    Ok(format!("{domain}\\{name}"))
}

fn command_line(definition: &Definition) -> io::Result<String> {
    Ok(format!(
        "{} {}",
        quote_argument(
            definition
                .service_host
                .as_ref()
                .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidInput))?
                .to_str()
                .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidInput))?
        )?,
        definition.arguments()
    ))
}

/// Administrator operation. Credentials are consumed synchronously by SCM;
/// neither argv, XML, environment, markers nor diagnostics carry the password.
pub fn register(
    definition: &Definition,
    previous: Option<&Definition>,
    password: &str,
) -> io::Result<()> {
    definition.verify_image()?;
    let mut image = super::security::open_identity(&definition.daemon, false)?;
    let executable = std::env::current_exe()?;
    let trusted_host_path = executable
        .parent()
        .ok_or(io::ErrorKind::InvalidData)?
        .join("hirouted.exe");
    let mut trusted_host = super::security::open_identity(&trusted_host_path, false)?;
    register_for_owner(
        definition,
        previous,
        &super::security::current_user_sid()?,
        &account_name()?,
        password,
        &mut image,
        &mut trusted_host,
    )
}

/// The privileged helper authenticates owner credentials and holds the actual
/// owner image across impersonation/reversion. This entry verifies those same
/// credentials/SID again. The trusted host must be retained from the fixed daemon
/// beside the administrator's independently verified, executing CLI package. A
/// credential SID or a digest of owner-controlled bytes cannot establish code trust.
pub fn register_for_owner(
    definition: &Definition,
    previous: Option<&Definition>,
    owner_sid: &[u8],
    account_name: &str,
    password: &str,
    image: &mut File,
    trusted_host: &mut File,
) -> io::Result<()> {
    let sid = super::background::sid_string(owner_sid)?;
    definition.validate_for_owner_sid(&sid)?;
    let expected_account = account_name_for_sid(owner_sid)?;
    if !account_name.eq_ignore_ascii_case(&expected_account) {
        return Err(io::Error::from(io::ErrorKind::PermissionDenied));
    }
    if password.is_empty() {
        return Err(io::Error::from(io::ErrorKind::InvalidInput));
    }
    let scm = manager(SC_MANAGER_CONNECT | SC_MANAGER_CREATE_SERVICE)?;
    let name = wide(&definition.name)?;
    let account = wide(&expected_account)?;
    let command = wide(&command_line(definition)?)?;
    let password = Zeroizing::new(wide(password)?);
    token::verify_credentials(&expected_account, &password, owner_sid)?;
    super::sealed_host::install_for_owner(definition, &sid, image, trusted_host)?;
    grant_service_logon(owner_sid)?;
    let access = SERVICE_QUERY_CONFIG | SERVICE_QUERY_STATUS | SERVICE_CHANGE_CONFIG | 0x40000; // WRITE_DAC
    // SAFETY: no credentials are formatted into this process command line.
    let current = unsafe { OpenServiceW(scm.0, name.as_ptr(), access) };
    let service = if current.is_null() {
        if io::Error::last_os_error().raw_os_error() != Some(ERROR_SERVICE_DOES_NOT_EXIST as i32) {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: same-account name and zeroizing password remain live until SCM
        // stores its LSA secret. No system/builtin account fallback is permitted.
        Handle::new(unsafe {
            CreateServiceW(
                scm.0,
                name.as_ptr(),
                name.as_ptr(),
                access,
                SERVICE_WIN32_OWN_PROCESS,
                SERVICE_DEMAND_START,
                SERVICE_ERROR_NORMAL,
                command.as_ptr(),
                null(),
                null_mut(),
                null(),
                account.as_ptr(),
                password.as_ptr(),
            )
        })?
    } else {
        let service = Handle(current);
        if verify_for_owner(
            &service,
            previous.unwrap_or(definition),
            &sid,
            &expected_account,
        )
        .is_err()
        {
            verify_for_owner(&service, definition, &sid, &expected_account)?;
        }
        // A stopped registration is required: an old executable cannot be
        // overwritten or reinterpreted while its service still runs.
        if state(&service)?.dwCurrentState != SERVICE_STOPPED {
            return Err(io::Error::from(io::ErrorKind::WouldBlock));
        }
        // SAFETY: update keeps the same verified service account and start policy.
        if unsafe {
            ChangeServiceConfigW(
                service.0,
                SERVICE_NO_CHANGE,
                SERVICE_NO_CHANGE,
                SERVICE_NO_CHANGE,
                command.as_ptr(),
                null(),
                null_mut(),
                null(),
                account.as_ptr(),
                password.as_ptr(),
                null(),
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        service
    };
    owner_control_dacl(&service, &definition.owner_sid)?;
    failure_restart(&service)?;
    verify_for_owner(&service, definition, &sid, &expected_account)
}

fn grant_service_logon(owner_sid: &[u8]) -> io::Result<()> {
    let attributes = LSA_OBJECT_ATTRIBUTES {
        Length: std::mem::size_of::<LSA_OBJECT_ATTRIBUTES>() as u32,
        ..Default::default()
    };
    let mut policy = 0_isize;
    let status = unsafe {
        LsaOpenPolicy(
            null(),
            &attributes,
            (POLICY_CREATE_ACCOUNT | POLICY_LOOKUP_NAMES) as u32,
            &mut policy,
        )
    };
    if status != 0 {
        return Err(io::Error::from_raw_os_error(
            unsafe { LsaNtStatusToWinError(status) } as i32,
        ));
    }
    let mut sid = owner_sid.to_vec();
    let mut right: Vec<_> = "SeServiceLogonRight".encode_utf16().collect();
    let text = LSA_UNICODE_STRING {
        Length: (right.len() * 2) as u16,
        MaximumLength: (right.len() * 2) as u16,
        Buffer: right.as_mut_ptr(),
    };
    let status = unsafe { LsaAddAccountRights(policy, sid.as_mut_ptr().cast(), &text, 1) };
    unsafe {
        LsaClose(policy);
    }
    if status != 0 {
        return Err(io::Error::from_raw_os_error(
            unsafe { LsaNtStatusToWinError(status) } as i32,
        ));
    }
    // This account policy may support other services; never revoke their shared
    // service-logon right or alter account password expiration on uninstall.
    Ok(())
}

fn failure_restart(service: &Handle) -> io::Result<()> {
    let mut actions = [
        SC_ACTION {
            Type: SC_ACTION_RESTART,
            Delay: 60_000,
        },
        SC_ACTION {
            Type: SC_ACTION_RESTART,
            Delay: 60_000,
        },
        SC_ACTION {
            Type: SC_ACTION_NONE,
            Delay: 0,
        },
    ];
    let policy = SERVICE_FAILURE_ACTIONSW {
        dwResetPeriod: 86_400,
        lpRebootMsg: null_mut(),
        lpCommand: null_mut(),
        cActions: actions.len() as u32,
        lpsaActions: actions.as_mut_ptr(),
    };
    if unsafe {
        ChangeServiceConfig2W(
            service.0,
            SERVICE_CONFIG_FAILURE_ACTIONS,
            (&policy as *const SERVICE_FAILURE_ACTIONSW).cast(),
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    let flag = SERVICE_FAILURE_ACTIONS_FLAG {
        fFailureActionsOnNonCrashFailures: 1,
    };
    if unsafe {
        ChangeServiceConfig2W(
            service.0,
            SERVICE_CONFIG_FAILURE_ACTIONS_FLAG,
            (&flag as *const SERVICE_FAILURE_ACTIONS_FLAG).cast(),
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

fn owner_control_dacl(service: &Handle, sid: &str) -> io::Result<()> {
    // Owner can query/start/stop. SERVICE_CHANGE_CONFIG is deliberately absent;
    // autostart, account/image changes and deletion remain administrator actions.
    let sddl = wide(&format!(
        "D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;CCLCSWRPWPLOCRRC;;;{sid})"
    ))?;
    let mut descriptor = null_mut();
    // SAFETY: fixed SDDL with validated current SID, live output descriptor.
    if unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            SDDL_REVISION_1,
            &mut descriptor,
            null_mut(),
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: SetServiceObjectSecurity copies the DACL before LocalFree.
    let success =
        unsafe { SetServiceObjectSecurity(service.0, DACL_SECURITY_INFORMATION, descriptor) };
    let error = io::Error::last_os_error();
    unsafe {
        LocalFree(descriptor);
    }
    if success == 0 { Err(error) } else { Ok(()) }
}

struct Config {
    words: Vec<usize>,
}
impl Config {
    fn load(service: &Handle) -> io::Result<Self> {
        let mut needed = 0;
        // SAFETY: documented sizing call; cap the returned allocation.
        unsafe {
            QueryServiceConfigW(service.0, null_mut(), 0, &mut needed);
        }
        if needed < std::mem::size_of::<QUERY_SERVICE_CONFIGW>() as u32 || needed > 64 * 1024 {
            return Err(io::Error::from(io::ErrorKind::InvalidData));
        }
        let mut words = vec![0_usize; (needed as usize).div_ceil(std::mem::size_of::<usize>())];
        // SAFETY: word-aligned allocation has at least the requested bytes.
        if unsafe { QueryServiceConfigW(service.0, words.as_mut_ptr().cast(), needed, &mut needed) }
            == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(Self { words })
    }
    fn value(&self) -> &QUERY_SERVICE_CONFIGW {
        // SAFETY: load populated a correctly aligned, live native structure.
        unsafe { &*self.words.as_ptr().cast() }
    }
    fn text(&self, value: *const u16) -> io::Result<String> {
        let begin = self.words.as_ptr() as usize;
        let end = begin + self.words.len() * std::mem::size_of::<usize>();
        let address = value as usize;
        if address < begin || address >= end || address % 2 != 0 {
            return Err(io::Error::from(io::ErrorKind::InvalidData));
        }
        let maximum = (end - address) / 2;
        // SAFETY: pointer is checked inside the retained service-config allocation.
        let text = unsafe { std::slice::from_raw_parts(value, maximum) };
        let count = text
            .iter()
            .position(|word| *word == 0)
            .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidData))?;
        String::from_utf16(&text[..count]).map_err(io::Error::other)
    }
}

fn verify_for_owner(
    service: &Handle,
    definition: &Definition,
    sid: &str,
    account: &str,
) -> io::Result<()> {
    definition.validate_for_owner_sid(sid)?;
    let config = Config::load(service)?;
    let value = config.value();
    if value.dwServiceType != SERVICE_WIN32_OWN_PROCESS
        || config.text(value.lpBinaryPathName)? != command_line(definition)?
        || !config
            .text(value.lpServiceStartName)?
            .eq_ignore_ascii_case(account)
        || !matches!(value.dwStartType, SERVICE_DEMAND_START | SERVICE_AUTO_START)
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "service differs from owned account or image",
        ));
    }
    Ok(())
}

fn state(service: &Handle) -> io::Result<SERVICE_STATUS_PROCESS> {
    let mut status = SERVICE_STATUS_PROCESS::default();
    let mut count = 0;
    // SAFETY: exact live output buffer, QueryServiceStatusEx's process-info level.
    if unsafe {
        QueryServiceStatusEx(
            service.0,
            SC_STATUS_PROCESS_INFO,
            (&mut status as *mut SERVICE_STATUS_PROCESS).cast(),
            std::mem::size_of::<SERVICE_STATUS_PROCESS>() as u32,
            &mut count,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(status)
}

pub fn start(definition: &Definition) -> io::Result<()> {
    definition.verify_image()?;
    super::sealed_host::verify_definition(definition)?;
    let service = open(definition, SERVICE_START | SERVICE_QUERY_STATUS)?;
    if state(&service)?.dwCurrentState == SERVICE_STOP_PENDING {
        return Err(io::ErrorKind::WouldBlock.into());
    }
    // SAFETY: no service arguments, service configuration is already verified.
    if unsafe { StartServiceW(service.0, 0, null()) } == 0
        && io::Error::last_os_error().raw_os_error() != Some(ERROR_SERVICE_ALREADY_RUNNING as i32)
    {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

pub fn request_stop(definition: &Definition) -> io::Result<()> {
    let service = open(definition, SERVICE_STOP | SERVICE_QUERY_STATUS)?;
    if matches!(
        state(&service)?.dwCurrentState,
        SERVICE_STOPPED | SERVICE_STOP_PENDING
    ) {
        return Ok(());
    }
    let mut status = SERVICE_STATUS::default();
    // SAFETY: service owns its handler; ControlService only requests bounded stop.
    if unsafe { ControlService(service.0, SERVICE_CONTROL_STOP, &mut status) } == 0
        && io::Error::last_os_error().raw_os_error() != Some(ERROR_SERVICE_NOT_ACTIVE as i32)
    {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

pub fn active(definition: &Definition) -> io::Result<bool> {
    match open(definition, SERVICE_QUERY_STATUS) {
        Ok(service) => Ok(state(&service)?.dwCurrentState != SERVICE_STOPPED),
        Err(error) if error.raw_os_error() == Some(ERROR_SERVICE_DOES_NOT_EXIST as i32) => {
            Ok(false)
        }
        Err(error) => Err(error),
    }
}

/// Unprivileged owner finalization verifies the real machine registration and
/// protected deployment before publishing its private manager record.
pub fn verify_registration(definition: &Definition) -> io::Result<()> {
    definition.verify_image()?;
    super::sealed_host::verify_definition(definition)?;
    let service = open(definition, SERVICE_QUERY_STATUS)?;
    if state(&service)?.dwCurrentState != SERVICE_STOPPED {
        return Err(io::ErrorKind::WouldBlock.into());
    }
    Ok(())
}

pub fn registration_absent(definition: &Definition) -> io::Result<bool> {
    match open(definition, SERVICE_QUERY_STATUS) {
        Ok(_) => Ok(false),
        Err(error) if error.raw_os_error() == Some(ERROR_SERVICE_DOES_NOT_EXIST as i32) => {
            super::sealed_host::descriptor_absent(definition)
        }
        Err(error) => Err(error),
    }
}
pub fn autostart(definition: &Definition) -> io::Result<bool> {
    let service = match open(definition, SERVICE_QUERY_CONFIG) {
        Ok(service) => service,
        Err(error) if error.raw_os_error() == Some(ERROR_SERVICE_DOES_NOT_EXIST as i32) => {
            return Ok(false);
        }
        Err(error) => return Err(error),
    };
    Ok(Config::load(&service)?.value().dwStartType == SERVICE_AUTO_START)
}
pub fn set_autostart(definition: &Definition, enabled: bool) -> io::Result<()> {
    definition.validate()?;
    set_autostart_for_owner(definition, &definition.owner_sid, &account_name()?, enabled)
}

/// Administrator helper has authenticated `owner_sid` and resolved this account
/// from the token SID; the ordinary same-account entry remains strict above.
pub fn set_autostart_for_owner(
    definition: &Definition,
    owner_sid: &str,
    account: &str,
    enabled: bool,
) -> io::Result<()> {
    let service = open_for_owner(definition, owner_sid, account, SERVICE_CHANGE_CONFIG)?;
    // SAFETY: only start policy changes; caller needs administrator-held change rights.
    if unsafe {
        ChangeServiceConfigW(
            service.0,
            SERVICE_NO_CHANGE,
            if enabled {
                SERVICE_AUTO_START
            } else {
                SERVICE_DEMAND_START
            },
            SERVICE_NO_CHANGE,
            null(),
            null(),
            null_mut(),
            null(),
            null(),
            null(),
            null(),
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

pub fn unregister(definition: &Definition, timeout: Duration) -> io::Result<()> {
    definition.validate()?;
    unregister_for_owner(definition, &definition.owner_sid, &account_name()?, timeout)
}

pub fn unregister_for_owner(
    definition: &Definition,
    owner_sid: &str,
    account: &str,
    timeout: Duration,
) -> io::Result<()> {
    let service = match open_for_owner(
        definition,
        owner_sid,
        account,
        SERVICE_QUERY_STATUS | 0x10000,
    ) {
        // DELETE
        Ok(service) => Some(service),
        Err(error) if error.raw_os_error() == Some(ERROR_SERVICE_DOES_NOT_EXIST as i32) => {
            return super::sealed_host::remove_descriptor_for_owner(definition, owner_sid);
        }
        Err(error) if error.raw_os_error() == Some(ERROR_SERVICE_MARKED_FOR_DELETE as i32) => None,
        Err(error) => return Err(error),
    };
    if let Some(service) = service {
        if state(&service)?.dwCurrentState != SERVICE_STOPPED {
            return Err(io::Error::from(io::ErrorKind::WouldBlock));
        }
        // SAFETY: deletion is requested only for the verified stopped service.
        if unsafe { DeleteService(service.0) } == 0 {
            return Err(io::Error::last_os_error());
        }
        // Our handle would itself keep DeleteService's deletion pending.
        drop(service);
    }
    let deadline = Instant::now() + timeout;
    loop {
        let scm = manager(SC_MANAGER_CONNECT)?;
        let name = wide(&definition.name)?;
        let raw = unsafe { OpenServiceW(scm.0, name.as_ptr(), SERVICE_QUERY_STATUS) };
        if raw.is_null() {
            let error = io::Error::last_os_error();
            if error.raw_os_error() == Some(ERROR_SERVICE_DOES_NOT_EXIST as i32) {
                return super::sealed_host::remove_descriptor_for_owner(definition, owner_sid);
            }
            if error.raw_os_error() != Some(ERROR_SERVICE_MARKED_FOR_DELETE as i32) {
                return Err(error);
            }
        } else {
            drop(Handle(raw));
        }
        if Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "service deletion remains pending",
            ));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}
