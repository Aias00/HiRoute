//! Typed Task Scheduler COM boundary. No XML or PowerShell is generated.
use std::io;
use windows::Win32::Foundation::{VARIANT_BOOL, VARIANT_FALSE, VARIANT_TRUE};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
};
use windows::Win32::System::TaskScheduler::*;
use windows::Win32::System::Variant::VARIANT;
use windows::core::{BSTR, Interface};

use super::background::{Definition, Manager, com_error};

struct Apartment;
impl Apartment {
    fn new() -> io::Result<Self> {
        // SAFETY: every public operation runs in its own apartment/thread and
        // balances successful initialization after all COM interfaces drop.
        unsafe {
            CoInitializeEx(None, COINIT_MULTITHREADED)
                .ok()
                .map_err(com_error)?;
        }
        Ok(Self)
    }
}
impl Drop for Apartment {
    fn drop(&mut self) {
        unsafe {
            CoUninitialize();
        }
    }
}

fn with_folder<T: Send + 'static>(
    definition: &Definition,
    operation: impl FnOnce(&ITaskFolder, &Definition) -> io::Result<T> + Send + 'static,
) -> io::Result<T> {
    let definition = definition.clone();
    definition.validate()?;
    if definition.manager != Manager::Task {
        return Err(io::Error::from(io::ErrorKind::InvalidInput));
    }
    std::thread::spawn(move || {
        let _apartment = Apartment::new()?;
        // SAFETY: live interfaces stay on this initialized apartment, never
        // shared with Tokio threads. Empty credential variants select self.
        let folder = unsafe {
            let service: ITaskService =
                CoCreateInstance(&TaskScheduler, None, CLSCTX_INPROC_SERVER).map_err(com_error)?;
            let empty = VARIANT::default();
            service
                .Connect(&empty, &empty, &empty, &empty)
                .map_err(com_error)?;
            service.GetFolder(&BSTR::from("\\")).map_err(com_error)?
        };
        operation(&folder, &definition)
    })
    .join()
    .map_err(|_| io::Error::other("Task Scheduler apartment panicked"))?
}

/// An existing registration can be updated only after matching the previous
/// owned definition. The installer retains that definition until commit.
pub fn register(definition: &Definition, previous: Option<&Definition>) -> io::Result<()> {
    definition.verify_image()?;
    let previous = previous.cloned();
    with_folder(definition, move |folder, definition| {
        // SAFETY: all out-pointers below point to initialized Rust values; all
        // BSTR/VARIANT buffers and interfaces outlive their synchronous calls.
        unsafe {
            let current = folder.GetTask(&BSTR::from(&definition.name));
            let mut autostart = false;
            let flags = match current {
                Ok(task) => {
                    if verify_task(&task, previous.as_ref().unwrap_or(definition)).is_err() {
                        verify_task(&task, definition)?;
                    }
                    autostart = trigger_enabled(&task)?;
                    TASK_UPDATE.0 | TASK_IGNORE_REGISTRATION_TRIGGERS.0
                }
                Err(error) if missing(&error) => TASK_CREATE.0,
                Err(error) => return Err(com_error(error)),
            };
            // Obtain the connected service through a fresh in-apartment proxy.
            let service: ITaskService =
                CoCreateInstance(&TaskScheduler, None, CLSCTX_INPROC_SERVER).map_err(com_error)?;
            let empty = VARIANT::default();
            service
                .Connect(&empty, &empty, &empty, &empty)
                .map_err(com_error)?;
            let task = service.NewTask(0).map_err(com_error)?;
            let principal = task.Principal().map_err(com_error)?;
            principal
                .SetUserId(&BSTR::from(&definition.owner_sid))
                .map_err(com_error)?;
            principal
                .SetLogonType(TASK_LOGON_INTERACTIVE_TOKEN)
                .map_err(com_error)?;
            principal
                .SetRunLevel(TASK_RUNLEVEL_LUA)
                .map_err(com_error)?;
            let settings = task.Settings().map_err(com_error)?;
            settings.SetEnabled(VARIANT_TRUE).map_err(com_error)?;
            settings
                .SetAllowDemandStart(VARIANT_TRUE)
                .map_err(com_error)?;
            settings
                .SetMultipleInstances(TASK_INSTANCES_IGNORE_NEW)
                .map_err(com_error)?;
            settings
                .SetExecutionTimeLimit(&BSTR::from("PT0S"))
                .map_err(com_error)?;
            settings
                .SetDisallowStartIfOnBatteries(VARIANT_FALSE)
                .map_err(com_error)?;
            settings
                .SetStopIfGoingOnBatteries(VARIANT_FALSE)
                .map_err(com_error)?;
            settings
                .SetRunOnlyIfIdle(VARIANT_FALSE)
                .map_err(com_error)?;
            settings
                .SetRunOnlyIfNetworkAvailable(VARIANT_FALSE)
                .map_err(com_error)?;
            settings
                .SetStartWhenAvailable(VARIANT_TRUE)
                .map_err(com_error)?;
            settings
                .SetRestartInterval(&BSTR::from("PT1M"))
                .map_err(com_error)?;
            settings.SetRestartCount(3).map_err(com_error)?;
            let trigger = task
                .Triggers()
                .map_err(com_error)?
                .Create(TASK_TRIGGER_LOGON)
                .map_err(com_error)?;
            let logon: ILogonTrigger = trigger.cast().map_err(com_error)?;
            logon
                .SetUserId(&BSTR::from(&definition.owner_sid))
                .map_err(com_error)?;
            // First install is demand-only. Updating preserves login policy.
            trigger.SetEnabled(boolean(autostart)).map_err(com_error)?;
            let action: IExecAction = task
                .Actions()
                .map_err(com_error)?
                .Create(TASK_ACTION_EXEC)
                .map_err(com_error)?
                .cast()
                .map_err(com_error)?;
            let path = definition
                .daemon
                .to_str()
                .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidInput))?;
            let directory = definition
                .daemon
                .parent()
                .and_then(|p| p.to_str())
                .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidInput))?;
            action.SetPath(&BSTR::from(path)).map_err(com_error)?;
            action
                .SetArguments(&BSTR::from(definition.arguments()))
                .map_err(com_error)?;
            action
                .SetWorkingDirectory(&BSTR::from(directory))
                .map_err(com_error)?;
            let sid = VARIANT::from(definition.owner_sid.as_str());
            let registered = folder
                .RegisterTaskDefinition(
                    &BSTR::from(&definition.name),
                    &task,
                    flags,
                    &sid,
                    &empty,
                    TASK_LOGON_INTERACTIVE_TOKEN,
                    &empty,
                )
                .map_err(com_error)?;
            verify_task(&registered, definition)
        }
    })
}

pub fn start(definition: &Definition) -> io::Result<()> {
    definition.verify_image()?;
    with_folder(definition, |folder, definition| unsafe {
        let task = owned_task(folder, definition)?;
        task.Run(&VARIANT::default()).map_err(com_error)?;
        Ok(())
    })
}

/// Only used after graceful stop's deadline. Task Stop is not proof of child
/// cleanup; the actual daemon holds the kill-on-close Job for that guarantee.
pub fn force_stop(definition: &Definition) -> io::Result<()> {
    with_folder(definition, |folder, definition| unsafe {
        let task = owned_task(folder, definition)?;
        task.Stop(0).map_err(com_error)
    })
}

pub fn active(definition: &Definition) -> io::Result<bool> {
    with_folder(definition, |folder, definition| unsafe {
        let task = match folder.GetTask(&BSTR::from(&definition.name)) {
            Ok(task) => { verify_task(&task, definition)?; task },
            Err(error) if missing(&error) => return Ok(false),
            Err(error) => return Err(com_error(error)),
        };
        // Do not use IRunningTask::EnginePID: it identifies the scheduler host.
        Ok(task
            .GetInstances(0)
            .map_err(com_error)?
            .Count()
            .map_err(com_error)?
            > 0)
    })
}

pub fn autostart(definition: &Definition) -> io::Result<bool> {
    with_folder(definition, |folder, definition| unsafe {
        trigger_enabled(&owned_task(folder, definition)?)
    })
}

pub fn set_autostart(definition: &Definition, enabled: bool) -> io::Result<()> {
    with_folder(definition, move |folder, definition| unsafe {
        let registered = owned_task(folder, definition)?;
        let task = registered.Definition().map_err(com_error)?;
        let trigger = task
            .Triggers()
            .map_err(com_error)?
            .get_Item(1)
            .map_err(com_error)?;
        trigger.SetEnabled(boolean(enabled)).map_err(com_error)?;
        let empty = VARIANT::default();
        folder
            .RegisterTaskDefinition(
                &BSTR::from(&definition.name),
                &task,
                TASK_UPDATE.0 | TASK_IGNORE_REGISTRATION_TRIGGERS.0,
                &VARIANT::from(definition.owner_sid.as_str()),
                &empty,
                TASK_LOGON_INTERACTIVE_TOKEN,
                &empty,
            )
            .map_err(com_error)?;
        // The task stays enabled, so demand start still works without logon autostart.
        Ok(())
    })
}

pub fn unregister(definition: &Definition) -> io::Result<()> {
    with_folder(definition, |folder, definition| unsafe {
        match folder.GetTask(&BSTR::from(&definition.name)) {
            Ok(task) => {
                verify_task(&task, definition)?;
                folder
                    .DeleteTask(&BSTR::from(&definition.name), 0)
                    .map_err(com_error)
            }
            Err(error) if missing(&error) => Ok(()),
            Err(error) => Err(com_error(error)),
        }
    })
}

unsafe fn owned_task(folder: &ITaskFolder, definition: &Definition) -> io::Result<IRegisteredTask> {
    // SAFETY: caller holds initialized in-apartment interfaces.
    let task = unsafe { folder.GetTask(&BSTR::from(&definition.name)) }.map_err(com_error)?;
    unsafe {
        verify_task(&task, definition)?;
    }
    Ok(task)
}

unsafe fn trigger_enabled(task: &IRegisteredTask) -> io::Result<bool> {
    let mut enabled = VARIANT_FALSE;
    // SAFETY: caller validated this single-trigger owned task, and output is live.
    unsafe {
        task.Definition()
            .map_err(com_error)?
            .Triggers()
            .map_err(com_error)?
            .get_Item(1)
            .map_err(com_error)?
            .Enabled(&mut enabled)
            .map_err(com_error)?;
    }
    Ok(enabled == VARIANT_TRUE)
}

unsafe fn verify_task(registered: &IRegisteredTask, expected: &Definition) -> io::Result<()> {
    expected.validate()?;
    // SAFETY: caller owns this apartment and all output values below.
    unsafe {
        let task = registered.Definition().map_err(com_error)?;
        let principal = task.Principal().map_err(com_error)?;
        let mut sid = BSTR::new();
        let mut logon_type = TASK_LOGON_NONE;
        let mut level = TASK_RUNLEVEL_HIGHEST;
        principal.UserId(&mut sid).map_err(com_error)?;
        principal.LogonType(&mut logon_type).map_err(com_error)?;
        principal.RunLevel(&mut level).map_err(com_error)?;
        let actions = task.Actions().map_err(com_error)?;
        let mut count = 0;
        actions.Count(&mut count).map_err(com_error)?;
        if sid.to_string() != expected.owner_sid
            || logon_type != TASK_LOGON_INTERACTIVE_TOKEN
            || level != TASK_RUNLEVEL_LUA
            || count != 1
            || registered.Enabled().map_err(com_error)? != VARIANT_TRUE
        {
            return Err(foreign());
        }
        let action: IExecAction = actions
            .get_Item(1)
            .map_err(com_error)?
            .cast()
            .map_err(com_error)?;
        let mut path = BSTR::new();
        let mut arguments = BSTR::new();
        let mut working = BSTR::new();
        action.Path(&mut path).map_err(com_error)?;
        action.Arguments(&mut arguments).map_err(com_error)?;
        action.WorkingDirectory(&mut working).map_err(com_error)?;
        if PathCompare::new(&path.to_string())
            != PathCompare::new(expected.daemon.to_str().ok_or_else(foreign)?)
            || arguments.to_string() != expected.arguments()
            || PathCompare::new(&working.to_string())
                != PathCompare::new(
                    expected
                        .daemon
                        .parent()
                        .and_then(|p| p.to_str())
                        .ok_or_else(foreign)?,
                )
        {
            return Err(foreign());
        }
        let triggers = task.Triggers().map_err(com_error)?;
        triggers.Count(&mut count).map_err(com_error)?;
        if count != 1 {
            return Err(foreign());
        }
        let logon: ILogonTrigger = triggers
            .get_Item(1)
            .map_err(com_error)?
            .cast()
            .map_err(com_error)?;
        let mut trigger_sid = BSTR::new();
        logon.UserId(&mut trigger_sid).map_err(com_error)?;
        if trigger_sid.to_string() != expected.owner_sid {
            return Err(foreign());
        }
        let settings = task.Settings().map_err(com_error)?;
        let mut limit = BSTR::new();
        let mut policy = TASK_INSTANCES_PARALLEL;
        let mut demand = VARIANT_FALSE;
        settings.ExecutionTimeLimit(&mut limit).map_err(com_error)?;
        settings.MultipleInstances(&mut policy).map_err(com_error)?;
        settings.AllowDemandStart(&mut demand).map_err(com_error)?;
        if limit.to_string() != "PT0S"
            || policy != TASK_INSTANCES_IGNORE_NEW
            || demand != VARIANT_TRUE
        {
            return Err(foreign());
        }
        Ok(())
    }
}

// Task Scheduler may canonicalize casing, but it must not retarget an image.
#[derive(Eq, PartialEq)]
struct PathCompare(String);
impl PathCompare {
    fn new(value: &str) -> Self {
        Self(value.replace('/', "\\").to_lowercase())
    }
}
fn boolean(value: bool) -> VARIANT_BOOL {
    if value { VARIANT_TRUE } else { VARIANT_FALSE }
}
fn missing(error: &windows::core::Error) -> bool {
    error.code().0 as u32 == 0x80070002
}
fn foreign() -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        "background task differs from owned definition",
    )
}
