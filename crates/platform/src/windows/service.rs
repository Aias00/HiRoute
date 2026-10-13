//! SCM boundary: owned service handles, same-account credentials and dispatcher.
use std::ffi::OsString;
use std::ffi::c_void;
use std::fs::File;
use std::io;
use std::os::windows::io::{AsRawHandle, FromRawHandle};
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{
    ERROR_SERVICE_ALREADY_RUNNING, ERROR_SERVICE_DOES_NOT_EXIST, ERROR_SERVICE_MARKED_FOR_DELETE,
    ERROR_SERVICE_NOT_ACTIVE, LocalFree,
};
use windows_sys::Win32::Security::Authorization::{
    ConvertStringSecurityDescriptorToSecurityDescriptorW, ConvertStringSidToSidW, SDDL_REVISION_1,
};
use windows_sys::Win32::Security::{
    CreateRestrictedToken, CreateWellKnownSid, DACL_SECURITY_INFORMATION, DISABLE_MAX_PRIVILEGE,
    EqualSid, GetLengthSid, GetTokenInformation, LOGON32_LOGON_INTERACTIVE,
    LOGON32_PROVIDER_DEFAULT, LogonUserW, LookupAccountSidW, SID_AND_ATTRIBUTES, SID_NAME_USE,
    SetTokenInformation, TOKEN_ADJUST_DEFAULT, TOKEN_ASSIGN_PRIMARY, TOKEN_DUPLICATE,
    TOKEN_MANDATORY_LABEL, TOKEN_QUERY, TOKEN_USER, TokenIntegrityLevel, TokenUser,
    WinBuiltinAdministratorsSid,
};
use windows_sys::Win32::System::Services::*;
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
use zeroize::Zeroizing;

use super::background::{Definition, Manager, quote_argument};

// WinNT.h: SE_GROUP_INTEGRITY. windows-sys places this SDK bit behind the
// unrelated SystemServices feature; no additional API is needed for this flag.
const SE_GROUP_INTEGRITY: u32 = 0x0000_0020;

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
    if definition.manager != Manager::Scm {
        return Err(io::Error::from(io::ErrorKind::InvalidInput));
    }
    let scm = manager(SC_MANAGER_CONNECT)?;
    let name = wide(&definition.name)?;
    // SAFETY: SCM and NUL-terminated service name outlive this call.
    let handle =
        Handle::new(unsafe { OpenServiceW(scm.0, name.as_ptr(), access | SERVICE_QUERY_CONFIG) })?;
    verify(&handle, definition)?;
    Ok(handle)
}

pub fn account_name() -> io::Result<String> {
    let mut sid = super::security::current_user_sid()?;
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
    let name = String::from_utf16(&name[..names as usize]).map_err(io::Error::other)?;
    let domain = String::from_utf16(&domain[..domains as usize]).map_err(io::Error::other)?;
    if name.is_empty() || domain.is_empty() {
        return Err(io::Error::from(io::ErrorKind::InvalidData));
    }
    Ok(format!("{domain}\\{name}"))
}

fn command_line(definition: &Definition) -> io::Result<String> {
    Ok(format!(
        "{} {}",
        quote_argument(
            definition.service_host.as_ref().ok_or_else(|| io::Error::from(io::ErrorKind::InvalidInput))?
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
    super::sealed_host::install(definition)?;
    if password.is_empty() {
        return Err(io::Error::from(io::ErrorKind::InvalidInput));
    }
    let scm = manager(SC_MANAGER_CONNECT | SC_MANAGER_CREATE_SERVICE)?;
    let name = wide(&definition.name)?;
    let account = wide(&account_name()?)?;
    let command = wide(&command_line(definition)?)?;
    let password = Zeroizing::new(wide(password)?);
    verify_credentials(&account, &password)?;
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
        if verify(&service, previous.unwrap_or(definition)).is_err() { verify(&service, definition)?; }
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
    verify(&service, definition)
}

fn verify_credentials(account: &[u16], password: &[u16]) -> io::Result<()> {
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
    let mut owner = super::security::current_user_sid()?;
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

fn restricted_token() -> io::Result<File> {
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

fn verify(service: &Handle, definition: &Definition) -> io::Result<()> {
    definition.validate()?;
    let config = Config::load(service)?;
    let value = config.value();
    if value.dwServiceType != SERVICE_WIN32_OWN_PROCESS
        || config.text(value.lpBinaryPathName)? != command_line(definition)?
        || !config
            .text(value.lpServiceStartName)?
            .eq_ignore_ascii_case(&account_name()?)
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
    Ok(state(&open(definition, SERVICE_QUERY_STATUS)?)?.dwCurrentState != SERVICE_STOPPED)
}
pub fn autostart(definition: &Definition) -> io::Result<bool> {
    let service = open(definition, SERVICE_QUERY_CONFIG)?;
    Ok(Config::load(&service)?.value().dwStartType == SERVICE_AUTO_START)
}
pub fn set_autostart(definition: &Definition, enabled: bool) -> io::Result<()> {
    let service = open(definition, SERVICE_CHANGE_CONFIG)?;
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
    let service = match open(definition, SERVICE_QUERY_STATUS | 0x10000) {
        // DELETE
        Ok(service) => service,
        Err(error) if error.raw_os_error() == Some(ERROR_SERVICE_DOES_NOT_EXIST as i32) => {
            return Ok(());
        }
        Err(error) => return Err(error),
    };
    if state(&service)?.dwCurrentState != SERVICE_STOPPED {
        return Err(io::Error::from(io::ErrorKind::WouldBlock));
    }
    // SAFETY: deletion is requested only for the verified stopped service.
    if unsafe { DeleteService(service.0) } == 0 {
        return Err(io::Error::last_os_error());
    }
    // DeleteService marks deletion; our handle would itself keep it pending.
    drop(service);
    let deadline = Instant::now() + timeout;
    loop {
        let scm = manager(SC_MANAGER_CONNECT)?;
        let name = wide(&definition.name)?;
        let raw = unsafe { OpenServiceW(scm.0, name.as_ptr(), SERVICE_QUERY_STATUS) };
        if raw.is_null() {
            let error = io::Error::last_os_error();
            if error.raw_os_error() == Some(ERROR_SERVICE_DOES_NOT_EXIST as i32) {
                return Ok(());
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

#[derive(Clone)]
pub struct ServiceShutdown {
    inner: Arc<Inner>,
}
struct Inner {
    requested: AtomicBool,
    finished: AtomicBool,
    status: Mutex<Status>,
}
struct Status {
    handle: usize,
    state: u32,
    checkpoint: u32,
}
type Run = Box<dyn FnOnce(ServiceShutdown) -> Result<(), String> + Send>;
struct Host {
    name: Vec<u16>,
    inner: Arc<Inner>,
    run: Mutex<Option<Run>>,
    outcome: Mutex<Option<Result<(), String>>>,
}
static HOST: OnceLock<Host> = OnceLock::new();

impl ServiceShutdown {
    pub fn is_requested(&self) -> bool {
        self.inner.requested.load(Ordering::Acquire)
    }
    pub fn mark_running(&self) -> io::Result<()> {
        self.inner.report(SERVICE_RUNNING, 0)
    }
    pub fn wait_timeout(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        while !self.is_requested() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        self.is_requested()
    }
}
impl Inner {
    fn report(&self, state: u32, exit: u32) -> io::Result<()> {
        let mut value = self
            .status
            .lock()
            .map_err(|_| io::Error::other("service status poisoned"))?;
        if value.handle == 0 {
            return Err(io::Error::other("service handler unavailable"));
        }
        if matches!(state, SERVICE_START_PENDING | SERVICE_STOP_PENDING) {
            value.checkpoint = value.checkpoint.saturating_add(1);
        } else {
            value.checkpoint = 0;
        }
        value.state = state;
        let status = SERVICE_STATUS {
            dwServiceType: SERVICE_WIN32_OWN_PROCESS,
            dwCurrentState: state,
            dwControlsAccepted: if state == SERVICE_RUNNING {
                SERVICE_ACCEPT_STOP | SERVICE_ACCEPT_SHUTDOWN
            } else {
                0
            },
            dwWin32ExitCode: exit,
            dwServiceSpecificExitCode: 0,
            dwCheckPoint: value.checkpoint,
            dwWaitHint: if matches!(state, SERVICE_START_PENDING | SERVICE_STOP_PENDING) {
                20_000
            } else {
                0
            },
        };
        // SAFETY: registered status handle remains valid through dispatcher return.
        if unsafe { SetServiceStatus(value.handle as SERVICE_STATUS_HANDLE, &status) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
}

/// Dispatch before role-all startup. The callback owns the sole business runtime
/// and marks RUNNING only after Local Control/Gateway have actually become ready.
pub fn run_service(
    name: &str,
    _run: impl FnOnce(ServiceShutdown) -> Result<(), String> + Send + 'static,
) -> Result<(), String> {
    if !name.starts_with("HiRoute-")
        || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
    {
        return Err("invalid service name".into());
    }
    let host = Host {
        name: wide(name).map_err(|e| e.to_string())?,
        inner: Arc::new(Inner {
            requested: AtomicBool::new(false),
            finished: AtomicBool::new(false),
            status: Mutex::new(Status {
                handle: 0,
                state: SERVICE_START_PENDING,
                checkpoint: 0,
            }),
        }),
        run: Mutex::new(Some(Box::new(run_restricted_child))),
        outcome: Mutex::new(None),
    };
    HOST.set(host)
        .map_err(|_| "service dispatcher already initialized".to_owned())?;
    let host = HOST
        .get()
        .ok_or_else(|| "service dispatcher missing".to_owned())?;
    let table = [
        SERVICE_TABLE_ENTRYW {
            lpServiceName: host.name.as_ptr().cast_mut(),
            lpServiceProc: Some(service_main),
        },
        SERVICE_TABLE_ENTRYW::default(),
    ];
    // SAFETY: static host/name live through dispatcher; table has a null terminator.
    if unsafe { StartServiceCtrlDispatcherW(table.as_ptr()) } == 0 {
        return Err(io::Error::last_os_error().to_string());
    }
    host.outcome
        .lock()
        .map_err(|_| "service outcome poisoned".to_owned())?
        .take()
        .unwrap_or_else(|| Err("service callback was not executed".into()))
}

fn run_restricted_child(shutdown: ServiceShutdown) -> Result<(), String> {
    use super::job::{Child, Launch, Stream};
    let run = || -> io::Result<()> {
        let name = String::from_utf16(&HOST.get().ok_or_else(|| io::Error::other("service host absent"))?.name[..HOST.get().unwrap().name.len() - 1]).map_err(io::Error::other)?;
        let definition = super::sealed_host::load_for_dispatch(&name)?;
        let binary = definition.daemon;
        let token = restricted_token()?;
        let args = [
            OsString::from("--role"),
            OsString::from("all"),
            OsString::from("--standalone"),
        ];
        let environment: Vec<_> = std::env::vars_os()
            .filter(|(key, _)| {
                !key.to_string_lossy()
                    .eq_ignore_ascii_case(super::launcher::DESCRIPTOR_ENV)
            })
            .collect();
        let child = Child::spawn_as_token(
            Launch {
                program: &binary,
                args: &args,
                env: &environment,
                cwd: binary.parent(),
                stdin: Stream::Null,
                stdout: Stream::Null,
                stderr: Stream::Null,
            },
            &token,
        )?;
        let endpoint =
            super::background::local_app_data()?.join("HiRoute/cli/run/hiroute/lifecycle-v1.sock");
        let startup_deadline = Instant::now() + Duration::from_secs(30);
        while !shutdown.is_requested() {
            if let Some(code) = child.exit_code()? {
                return Err(io::Error::other(format!(
                    "service business child exited {code}"
                )));
            }
            if super::lifecycle::query(&endpoint, &binary, Duration::from_millis(250)).is_ok() {
                break;
            }
            if Instant::now() >= startup_deadline {
                child.terminate()?;
                return Err(io::Error::from(io::ErrorKind::TimedOut));
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        if !shutdown.is_requested() {
            shutdown.mark_running()?;
        }
        while !shutdown.is_requested() {
            if let Some(code) = child.exit_code()? {
                return if code == 0 {
                    Ok(())
                } else {
                    Err(io::Error::other(format!(
                        "service business child exited {code}"
                    )))
                };
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let _ = super::lifecycle::stop(&endpoint, &binary, Duration::from_secs(15));
        if child.exit_code()?.is_none() {
            child.terminate()?;
        }
        let deadline = Instant::now() + Duration::from_secs(3);
        while !child.empty()? {
            if Instant::now() >= deadline {
                return Err(io::Error::from(io::ErrorKind::TimedOut));
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        Ok(())
    };
    run().map_err(|error| error.to_string())
}

unsafe extern "system" fn handler(control: u32, _: u32, _: *mut c_void, _: *mut c_void) -> u32 {
    let Some(host) = HOST.get() else {
        return 120;
    };
    match control {
        SERVICE_CONTROL_STOP | SERVICE_CONTROL_SHUTDOWN => {
            host.inner.requested.store(true, Ordering::Release);
            let _ = host.inner.report(SERVICE_STOP_PENDING, 0);
            0
        }
        SERVICE_CONTROL_INTERROGATE => 0,
        _ => 120,
    }
}

unsafe extern "system" fn service_main(_: u32, _: *mut *mut u16) {
    let Some(host) = HOST.get() else {
        return;
    };
    let handle =
        unsafe { RegisterServiceCtrlHandlerExW(host.name.as_ptr(), Some(handler), null()) };
    if handle.is_null() {
        if let Ok(mut outcome) = host.outcome.lock() {
            *outcome = Some(Err(io::Error::last_os_error().to_string()));
        }
        return;
    }
    if let Ok(mut status) = host.inner.status.lock() {
        status.handle = handle as usize;
    }
    let _ = host.inner.report(SERVICE_START_PENDING, 0);
    let inner = Arc::clone(&host.inner);
    let monitor = std::thread::spawn(move || {
        while !inner.finished.load(Ordering::Acquire) {
            if inner.requested.load(Ordering::Acquire) {
                let _ = inner.report(SERVICE_STOP_PENDING, 0);
            }
            std::thread::sleep(Duration::from_millis(250));
        }
    });
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let run = host
            .run
            .lock()
            .map_err(|_| "service callback poisoned".to_owned())?
            .take()
            .ok_or_else(|| "service callback missing".to_owned())?;
        run(ServiceShutdown {
            inner: Arc::clone(&host.inner),
        })
    }))
    .unwrap_or_else(|_| Err("service callback panicked".into()));
    host.inner.finished.store(true, Ordering::Release);
    let _ = monitor.join();
    let _ = host
        .inner
        .report(SERVICE_STOPPED, if outcome.is_ok() { 0 } else { 1066 });
    if let Ok(mut value) = host.outcome.lock() {
        *value = Some(outcome);
    }
}
