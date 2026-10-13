//! Trusted SCM dispatcher: only a restricted child runs the user business image.
use super::wide;
use std::ffi::{OsString, c_void};
use std::io;
use std::ptr::null;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use windows_sys::Win32::System::Services::*;

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
        if self.is_requested() {
            return Ok(());
        }
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

/// Dispatch before touching user business state. Only administrator-protected
/// code launches the sole business runtime with a restricted primary token.
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
    use crate::windows::job::{Child, Launch, Stream};
    let run = || -> io::Result<()> {
        let name = String::from_utf16(
            &HOST
                .get()
                .ok_or_else(|| io::Error::other("service host absent"))?
                .name[..HOST.get().unwrap().name.len() - 1],
        )
        .map_err(io::Error::other)?;
        let definition = crate::windows::sealed_host::load_for_dispatch(&name)?;
        let binary = definition.daemon;
        let token = super::token::restricted_token()?;
        let args = [
            OsString::from("--role"),
            OsString::from("all"),
            OsString::from("--standalone"),
        ];
        let environment: Vec<_> = std::env::vars_os()
            .filter(|(key, _)| {
                !key.to_string_lossy()
                    .eq_ignore_ascii_case(crate::windows::launcher::DESCRIPTOR_ENV)
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
        let endpoint = crate::windows::background::local_app_data()?
            .join("HiRoute/cli/run/hiroute/lifecycle-v1.sock");
        let startup_deadline = Instant::now() + Duration::from_secs(30);
        while !shutdown.is_requested() {
            if let Some(code) = child.exit_code()? {
                return Err(io::Error::other(format!(
                    "service business child exited {code}"
                )));
            }
            if crate::windows::lifecycle::query(&endpoint, &binary, Duration::from_millis(250))
                .is_ok()
            {
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
        let _ = crate::windows::lifecycle::stop(&endpoint, &binary, Duration::from_secs(15));
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
            } else if inner
                .status
                .lock()
                .is_ok_and(|status| status.state == SERVICE_START_PENDING)
            {
                let _ = inner.report(SERVICE_START_PENDING, 0);
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
        .report(SERVICE_STOPPED, if outcome.is_ok() { 0 } else { 1067 });
    if let Ok(mut value) = host.outcome.lock() {
        *value = Some(outcome);
    }
}
