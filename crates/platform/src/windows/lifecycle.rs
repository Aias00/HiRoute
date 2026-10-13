//! Owner-authenticated lifecycle client with an actual, retained daemon handle.
use std::fs::File;
use std::io;
use std::os::windows::io::{AsRawHandle, FromRawHandle};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use windows_sys::Win32::Foundation::{FILETIME, WAIT_OBJECT_0, WAIT_TIMEOUT};
use windows_sys::Win32::System::Threading::{
    GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
    PROCESS_TERMINATE, QueryFullProcessImageNameW, TerminateProcess, WaitForSingleObject,
};

use super::{pipe, security};
use crate::local_record::{self, Channel, Direction};

pub const SCHEMA: &str = "hiroute.windows-lifecycle/v1";
pub const MAXIMUM: usize = 4096;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub schema: String,
    pub action: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Response {
    pub schema: String,
    pub action: String,
    pub pid: u32,
    pub accepted: bool,
}

pub struct DaemonProcess {
    file: File,
    pub pid: u32,
    pub creation_time: u64,
    pub image: PathBuf,
}

impl DaemonProcess {
    fn from_pipe(
        stream: &pipe::Stream,
        expected_image: &Path,
        allow_terminate: bool,
    ) -> io::Result<Self> {
        let pid = pipe::peer_pid(stream, false)?;
        let access = PROCESS_QUERY_LIMITED_INFORMATION
            | PROCESS_SYNCHRONIZE
            | if allow_terminate {
                PROCESS_TERMINATE
            } else {
                0
            };
        // SAFETY: kernel pipe peer PID is used once to retain this exact process.
        let handle = unsafe { OpenProcess(access, 0, pid) };
        if handle.is_null() {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: OpenProcess transferred a unique kernel handle to us.
        let file = unsafe { File::from_raw_handle(handle.cast()) };
        let mut creation = FILETIME::default();
        let mut exit = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut user = FILETIME::default();
        // SAFETY: process handle and exact FILETIME outputs stay live.
        if unsafe { GetProcessTimes(handle, &mut creation, &mut exit, &mut kernel, &mut user) } == 0
        {
            return Err(io::Error::last_os_error());
        }
        let mut buffer = vec![0_u16; 32768];
        let mut count = buffer.len() as u32;
        // SAFETY: caller-owned UTF-16 buffer has the declared capacity.
        if unsafe { QueryFullProcessImageNameW(handle, 0, buffer.as_mut_ptr(), &mut count) } == 0 {
            return Err(io::Error::last_os_error());
        }
        let image =
            PathBuf::from(String::from_utf16(&buffer[..count as usize]).map_err(io::Error::other)?);
        // Hold both reparse-free objects until matching volume/file IDs. A path
        // or basename alone must never authorize terminating another process.
        let actual = security::open_identity(&image, false)?;
        let expected = security::open_identity(expected_image, false)?;
        if security::file_identity(&actual)? != security::file_identity(&expected)? {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "lifecycle daemon image differs",
            ));
        }
        Ok(Self {
            file,
            pid,
            image,
            creation_time: u64::from(creation.dwHighDateTime) << 32
                | u64::from(creation.dwLowDateTime),
        })
    }

    pub fn exited(&self) -> io::Result<bool> {
        self.wait(Duration::ZERO)
    }

    pub fn wait(&self, timeout: Duration) -> io::Result<bool> {
        let millis = timeout.as_millis().min(u128::from(u32::MAX - 1)) as u32;
        // SAFETY: only this retained process handle is observed, no PID lookup.
        match unsafe { WaitForSingleObject(self.file.as_raw_handle().cast(), millis) } {
            WAIT_OBJECT_0 => Ok(true),
            WAIT_TIMEOUT => Ok(false),
            _ => Err(io::Error::last_os_error()),
        }
    }

    pub fn terminate(&self) -> io::Result<()> {
        // SAFETY: handle is the authenticated daemon retained before stop; its
        // resident kill-on-close Job cleans children when this process exits.
        if unsafe { TerminateProcess(self.file.as_raw_handle().cast(), 1) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
}

fn exchange(
    endpoint: &Path,
    image: &Path,
    action: &str,
    timeout: Duration,
    allow_terminate: bool,
) -> io::Result<DaemonProcess> {
    let deadline = Instant::now() + timeout;
    let mut stream = pipe::Stream::connect_timeout(endpoint, timeout)?;
    stream.set_deadline(Some(deadline))?;
    let process = DaemonProcess::from_pipe(&stream, image, allow_terminate)?;
    let request = serde_json::to_vec(&Request {
        schema: SCHEMA.into(),
        action: action.into(),
    })
    .map_err(io::Error::other)?;
    local_record::write(
        &mut stream,
        Channel::Lifecycle,
        Direction::Request,
        &request,
        MAXIMUM,
    )?;
    let bytes = local_record::read(
        &mut stream,
        Channel::Lifecycle,
        Direction::Response,
        MAXIMUM,
    )?;
    let response: Response = serde_json::from_slice(&bytes).map_err(io::Error::other)?;
    if response.schema != SCHEMA
        || response.action != action
        || response.pid != process.pid
        || !response.accepted
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid lifecycle response",
        ));
    }
    local_record::acknowledge(&mut stream)?;
    Ok(process)
}

pub fn query(endpoint: &Path, image: &Path, timeout: Duration) -> io::Result<DaemonProcess> {
    exchange(endpoint, image, "status", timeout, false)
}

/// Retain the authenticated daemon before asking SCM to stop its trusted host.
/// Unlike `stop`, this query does not race the host's own graceful request.
pub fn retain_for_stop(
    endpoint: &Path,
    image: &Path,
    timeout: Duration,
) -> io::Result<DaemonProcess> {
    exchange(endpoint, image, "status", timeout, true)
}

/// One deadline covers graceful acknowledgement and actual daemon exit. An
/// unresponsive accepted daemon is terminated through its retained handle.
pub fn stop(endpoint: &Path, image: &Path, timeout: Duration) -> io::Result<()> {
    let deadline = Instant::now() + timeout;
    let process = exchange(
        endpoint,
        image,
        "stop",
        timeout.min(Duration::from_secs(2)),
        true,
    )?;
    if process.wait(deadline.saturating_duration_since(Instant::now()))? {
        return Ok(());
    }
    process.terminate()?;
    if !process.wait(Duration::from_secs(2))? {
        return Err(io::Error::from(io::ErrorKind::TimedOut));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lifecycle_payload_is_closed_and_does_not_accept_extra_authority() {
        assert!(
            serde_json::from_str::<Request>(
                r#"{"schema":"hiroute.windows-lifecycle/v1","action":"stop","password":"x"}"#
            )
            .is_err()
        );
        assert!(
            serde_json::from_str::<Response>(
                r#"{"schema":"x","action":"stop","pid":1,"accepted":true,"secret":null}"#
            )
            .is_err()
        );
    }
}
