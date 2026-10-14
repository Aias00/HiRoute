//! A GUI daemon only joins the existing parent console for explicit foreground use.
//! Console attachment is independent of redirected standard I/O and never creates a window.
use std::io;
use std::mem::zeroed;
use std::sync::atomic::{AtomicBool, Ordering};
use windows_sys::Win32::Foundation::{ERROR_INVALID_HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::Storage::FileSystem::{FILE_TYPE_DISK, FILE_TYPE_PIPE, GetFileType};
use windows_sys::Win32::System::Console::{
    ATTACH_PARENT_PROCESS, AttachConsole, CTRL_BREAK_EVENT, CTRL_C_EVENT, GetConsoleProcessList,
    GetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, SetConsoleCtrlHandler,
    SetStdHandle,
};
use windows_sys::Win32::System::Threading::{GetStartupInfoW, STARTF_USESTDHANDLES, STARTUPINFOW};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ParentConsoleAttachment {
    Attached,
    AlreadyAttached,
    ParentHasNoConsole,
}

/// Query actual attachment, including a pseudoconsole without a visible window.
pub fn is_attached() -> io::Result<bool> {
    let mut process = [0u32];
    // SAFETY: GetConsoleProcessList writes at most the live array's capacity.
    if unsafe { GetConsoleProcessList(process.as_mut_ptr(), process.len() as u32) } != 0 {
        return Ok(true);
    }
    let error = io::Error::last_os_error();
    if error.raw_os_error() == Some(ERROR_INVALID_HANDLE as i32) {
        Ok(false)
    } else {
        Err(error)
    }
}

/// Call once, before registering signal handlers or starting business threads.
/// AttachConsole resets the handler table. It may also replace standard handles;
/// preserve explicitly inherited I/O rather than redirecting a ready frame or pipe.
/// https://learn.microsoft.com/en-us/windows/console/getstdhandle#attachdetach-behavior
pub fn attach_parent_preserving_stdio() -> io::Result<ParentConsoleAttachment> {
    if is_attached()? {
        return Ok(ParentConsoleAttachment::AlreadyAttached);
    }
    // SAFETY: GetStartupInfoW fills this process's ordinary startup information.
    let mut startup: STARTUPINFOW = unsafe { zeroed() };
    unsafe { GetStartupInfoW(&mut startup) };
    let explicit = startup.dwFlags & STARTF_USESTDHANDLES != 0;
    let mut originals = Vec::with_capacity(3);
    for standard in [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
        // SAFETY: standard selectors refer to the current process table. Values
        // are borrowed; neither this function nor SetStdHandle closes them.
        let handle = unsafe { GetStdHandle(standard) };
        if handle == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        let redirected = !handle.is_null()
            && matches!(
                unsafe { GetFileType(handle) },
                FILE_TYPE_PIPE | FILE_TYPE_DISK
            );
        if explicit || redirected {
            originals.push((standard, handle));
        }
    }
    // SAFETY: the fixed selector only attaches this process to its actual parent.
    let attachment = if unsafe { AttachConsole(ATTACH_PARENT_PROCESS) } != 0 {
        ParentConsoleAttachment::Attached
    } else {
        let error = io::Error::last_os_error();
        if error.raw_os_error() != Some(ERROR_INVALID_HANDLE as i32) {
            return Err(error);
        }
        ParentConsoleAttachment::ParentHasNoConsole
    };
    let mut restore_error = None;
    for (standard, handle) in originals {
        // SAFETY: restore the borrowed original, including an explicit null.
        if unsafe { SetStdHandle(standard, handle) } == 0 {
            restore_error.get_or_insert_with(io::Error::last_os_error);
        }
    }
    if let Some(error) = restore_error {
        return Err(error);
    }
    Ok(attachment)
}

static WAIT_HANDLER: AtomicBool = AtomicBool::new(false);

/// While the console CLI waits, its daemon receives the same signal and drains.
/// This consumes only the CLI's notification, without inheriting a Ctrl-C-ignore bit.
pub struct ForegroundWaitGuard {
    registered: bool,
}

unsafe extern "system" fn foreground_wait_handler(event: u32) -> windows_sys::core::BOOL {
    i32::from(matches!(event, CTRL_C_EVENT | CTRL_BREAK_EVENT))
}

impl ForegroundWaitGuard {
    pub fn install() -> io::Result<Self> {
        if WAIT_HANDLER
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "foreground console wait",
            ));
        }
        // SAFETY: the handler has static lifetime and performs no blocking work.
        if unsafe { SetConsoleCtrlHandler(Some(foreground_wait_handler), 1) } == 0 {
            let error = io::Error::last_os_error();
            WAIT_HANDLER.store(false, Ordering::Release);
            return Err(error);
        }
        Ok(Self { registered: true })
    }

    pub fn finish(mut self) -> io::Result<()> {
        self.remove()
    }

    fn remove(&mut self) -> io::Result<()> {
        if !self.registered {
            return Ok(());
        }
        // SAFETY: this removes only the same static callback installed by this guard.
        if unsafe { SetConsoleCtrlHandler(Some(foreground_wait_handler), 0) } == 0 {
            return Err(io::Error::last_os_error());
        }
        self.registered = false;
        WAIT_HANDLER.store(false, Ordering::Release);
        Ok(())
    }
}

impl Drop for ForegroundWaitGuard {
    fn drop(&mut self) {
        let _ = self.remove();
    }
}
