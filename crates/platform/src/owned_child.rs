//! Owned native process group; observe without reaping before bounded group cleanup.
#[cfg(windows)]
use crate::windows::job::Child;
#[cfg(unix)]
use rustix::process::{
    Pid, Signal, WaitId, WaitIdOptions, kill_process_group, test_kill_process_group, waitid,
};
use std::io;
#[cfg(unix)]
use std::process::Child;
use std::time::{Duration, Instant};

pub struct NativeChildScope {
    child: Child,
    #[cfg(unix)]
    group: Pid,
    signals_closed: bool,
    stopped: bool,
}

impl NativeChildScope {
    /// Unix callers spawn with process_group(0); Windows callers retain an atomic Job child.
    pub fn new(child: Child) -> Self {
        Self {
            #[cfg(unix)]
            group: Pid::from_child(&child),
            child,
            signals_closed: false,
            stopped: false,
        }
    }

    pub fn observe(&self) -> io::Result<Option<bool>> {
        if self.signals_closed {
            return Err(io::Error::other("native process ownership closed"));
        }
        #[cfg(windows)]
        return self
            .child
            .exit_code()
            .map(|code| code.map(|code| code == 0));
        #[cfg(unix)]
        waitid(
            WaitId::Pid(self.group),
            WaitIdOptions::EXITED | WaitIdOptions::NOHANG | WaitIdOptions::NOWAIT,
        )
        .map(|value| value.map(|status| status.exit_status() == Some(0)))
        .map_err(Into::into)
    }

    #[cfg(unix)]
    fn signal(&self, signal: Signal) -> io::Result<()> {
        self.observe()?;
        match kill_process_group(self.group, signal) {
            Ok(()) | Err(rustix::io::Errno::SRCH) => Ok(()),
            // Darwin may refuse a group that exits between observe and signal.
            // This is provisional: stop still requires reap and proven group absence.
            Err(rustix::io::Errno::PERM) => Ok(()),
            Err(error) => Err(error.into()),
        }
    }

    #[cfg(unix)]
    pub fn stop(&mut self) -> io::Result<()> {
        if self.stopped {
            return Ok(());
        }
        if self.signals_closed {
            return Err(io::Error::other("native process stop incomplete"));
        }
        self.signal(Signal::TERM)?;
        std::thread::sleep(Duration::from_millis(100));
        // The leader is still waitable even when it exited before a surviving tool.
        self.signal(Signal::KILL)?;
        self.signals_closed = true;
        let start = Instant::now();
        loop {
            if self.child.try_wait()?.is_some() {
                break;
            }
            if start.elapsed() > Duration::from_secs(1) {
                return Err(io::Error::other("native process reap deadline"));
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        // Never mutate this numeric PID/PGID again. A reused group yields conservative unknown.
        loop {
            if matches!(
                test_kill_process_group(self.group),
                Err(rustix::io::Errno::SRCH)
            ) {
                self.stopped = true;
                return Ok(());
            }
            if start.elapsed() > Duration::from_secs(1) {
                return Err(io::Error::other("native process scope remains"));
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[cfg(windows)]
    pub fn stop(&mut self) -> io::Result<()> {
        if self.stopped {
            return Ok(());
        }
        self.child.terminate()?;
        let start = Instant::now();
        while !self.child.empty()? || self.child.exit_code()?.is_none() {
            if start.elapsed() >= Duration::from_secs(1) {
                return Err(io::Error::other("native process scope remains"));
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        self.signals_closed = true;
        self.stopped = true;
        Ok(())
    }
}

impl Drop for NativeChildScope {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}
