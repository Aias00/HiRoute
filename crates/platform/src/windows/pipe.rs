//! Owner-only local byte streams with bounded, cancellable overlapped I/O.
//!
//! Each outstanding operation owns a stable OVERLAPPED allocation and event.
//! Cancellation is completed before releasing its buffer. Pipe names are derived
//! from an existing private parent; no socket-shaped filesystem entry is created.

use std::fs::File;
use std::io::{self, Read, Write};
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle};
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use sha2::{Digest, Sha256};
use windows_sys::Win32::Foundation::{
    ERROR_BROKEN_PIPE, ERROR_IO_INCOMPLETE, ERROR_IO_PENDING, ERROR_OPERATION_ABORTED,
    ERROR_PIPE_CONNECTED, GENERIC_READ, GENERIC_WRITE, HANDLE, INVALID_HANDLE_VALUE, WAIT_OBJECT_0,
    WAIT_TIMEOUT,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAG_FIRST_PIPE_INSTANCE, FILE_FLAG_OVERLAPPED, OPEN_EXISTING,
    PIPE_ACCESS_DUPLEX, ReadFile, SECURITY_IDENTIFICATION, SECURITY_SQOS_PRESENT, WriteFile,
};
use windows_sys::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};
use windows_sys::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, GetNamedPipeClientProcessId, GetNamedPipeServerProcessId,
    PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_UNLIMITED_INSTANCES,
};
use windows_sys::Win32::System::Threading::{CreateEventW, INFINITE, WaitForSingleObject};

use super::security::{
    OwnerSecurity, current_user_sid, open_identity, verify_owner_only_handle, verify_process_user,
};

/// The same logical locator is retained by CLI and daemon on every platform.
/// Canonicalizing only after checking the held parent prevents endpoint aliases
/// from silently following a junction in the private endpoint directory.
pub fn name(endpoint: &Path) -> io::Result<PathBuf> {
    if !endpoint.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "endpoint is not absolute",
        ));
    }
    let parent = endpoint
        .parent()
        .ok_or_else(|| io::Error::other("endpoint has no parent"))?;
    let _held_parents = parent
        .ancestors()
        .map(|path| open_identity(path, true))
        .collect::<io::Result<Vec<_>>>()?;
    let parent = parent.canonicalize()?;
    let leaf = endpoint
        .file_name()
        .ok_or_else(|| io::Error::other("endpoint has no name"))?;
    let mut hash = Sha256::new();
    hash.update(b"hiroute.local-pipe/v1\0");
    hash.update(current_user_sid()?);
    for word in parent.join(leaf).as_os_str().encode_wide() {
        hash.update(word.to_le_bytes());
    }
    Ok(PathBuf::from(format!(
        r"\\.\pipe\HiRoute-{:x}",
        hash.finalize()
    )))
}

/// Verify a kernel-provided peer PID, including the launcher-owned Desktop PID.
/// The pipe DACL supplies the account boundary; callers must never substitute a
/// client-provided PID for this query.
pub fn peer_pid(pipe: &impl AsRawHandle, server_end: bool) -> io::Result<u32> {
    let mut pid = 0;
    // SAFETY: AsRawHandle borrows a live pipe handle and `pid` is a valid output.
    let result = unsafe {
        if server_end {
            GetNamedPipeClientProcessId(pipe.as_raw_handle().cast(), &mut pid)
        } else {
            GetNamedPipeServerProcessId(pipe.as_raw_handle().cast(), &mut pid)
        }
    };
    if result == 0 || pid == 0 {
        return Err(io::Error::last_os_error());
    }
    // A name containing a SID is not authentication: a different account can
    // pre-create it with a permissive DACL. Check the actual peer token before
    // sending protected material or dispatching a request.
    verify_process_user(pid)?;
    Ok(pid)
}

fn wide(path: &Path) -> io::Result<Vec<u16>> {
    let mut value: Vec<_> = path.as_os_str().encode_wide().collect();
    if value.contains(&0) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "embedded NUL"));
    }
    value.push(0);
    Ok(value)
}

fn owned(handle: HANDLE) -> io::Result<File> {
    if handle.is_null() || handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: every caller passes a newly created, uniquely owned kernel handle.
    Ok(unsafe { File::from_raw_handle(handle.cast()) })
}

struct Operation {
    overlapped: Box<OVERLAPPED>,
    _event: File,
}

// SAFETY: the allocation and event stay at fixed addresses across a move. Only
// the owner (or the listener's mutex holder) touches the OVERLAPPED; Windows
// completes it through its event before the allocation is released.
unsafe impl Send for Operation {}

impl Operation {
    fn new() -> io::Result<Self> {
        // SAFETY: no security/name pointers are supplied; this is a private,
        // manual-reset, initially nonsignalled event owned by this operation.
        let event = owned(unsafe { CreateEventW(null(), 1, 0, null()) })?;
        let mut overlapped = Box::new(OVERLAPPED::default());
        overlapped.hEvent = event.as_raw_handle().cast();
        Ok(Self {
            overlapped,
            _event: event,
        })
    }

    fn result(&mut self, handle: HANDLE, wait: bool) -> io::Result<u32> {
        let mut bytes = 0;
        // SAFETY: the handle, allocation, and operation buffer are still live.
        if unsafe { GetOverlappedResult(handle, &*self.overlapped, &mut bytes, i32::from(wait)) }
            == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(bytes)
    }

    fn cancel(&mut self, handle: HANDLE) -> io::Result<u32> {
        // SAFETY: this cancels only our still-owned operation, never another
        // reader/writer. Completion is observed before OVERLAPPED/buffer drop.
        unsafe { CancelIoEx(handle, &*self.overlapped) };
        self.result(handle, true)
    }

    fn finish(&mut self, handle: HANDLE, timeout: Option<Duration>) -> io::Result<u32> {
        let milliseconds = timeout.map_or(INFINITE, |duration| {
            u32::try_from(duration.as_millis().max(1))
                .unwrap_or(INFINITE - 1)
                .min(INFINITE - 1)
        });
        // SAFETY: our live event belongs to this single outstanding operation.
        match unsafe { WaitForSingleObject(self.overlapped.hEvent, milliseconds) } {
            WAIT_OBJECT_0 => self.result(handle, false),
            WAIT_TIMEOUT => match self.cancel(handle) {
                // CancelIoEx races completion. Preserve successfully consumed
                // or written bytes rather than dropping or duplicating a frame.
                Ok(bytes) => Ok(bytes),
                Err(error) if error.raw_os_error() == Some(ERROR_OPERATION_ABORTED as i32) => Err(
                    io::Error::new(io::ErrorKind::TimedOut, "pipe deadline exceeded"),
                ),
                Err(error) => Err(error),
            },
            _ => {
                let error = io::Error::last_os_error();
                let _ = self.cancel(handle);
                Err(error)
            }
        }
    }
}

struct Pending {
    handle: Arc<File>,
    operation: Operation,
    connected: bool,
}

impl Pending {
    fn new(name: &Path, first: bool) -> io::Result<Self> {
        let name = wide(name)?;
        let mut security = OwnerSecurity::new(false)?;
        let attributes = security.attributes()?;
        // SAFETY: the descriptor and name outlive creation. Owner-only access,
        // remote rejection and first-instance exclusivity are attached before
        // this endpoint can be observed by another process.
        let handle = Arc::new(owned(unsafe {
            CreateNamedPipeW(
                name.as_ptr(),
                PIPE_ACCESS_DUPLEX
                    | FILE_FLAG_OVERLAPPED
                    | if first {
                        FILE_FLAG_FIRST_PIPE_INSTANCE
                    } else {
                        0
                    },
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_REJECT_REMOTE_CLIENTS,
                PIPE_UNLIMITED_INSTANCES,
                65536,
                65536,
                0,
                &attributes,
            )
        })?);
        let mut operation = Operation::new()?;
        // SAFETY: the stable allocation/event are owned until completion/drop.
        let result =
            unsafe { ConnectNamedPipe(handle.as_raw_handle().cast(), &mut *operation.overlapped) };
        let connected = if result != 0 {
            true
        } else {
            match io::Error::last_os_error()
                .raw_os_error()
                .map(|code| code as u32)
            {
                Some(ERROR_PIPE_CONNECTED) => true,
                Some(ERROR_IO_PENDING) => false,
                _ => return Err(io::Error::last_os_error()),
            }
        };
        Ok(Self {
            handle,
            operation,
            connected,
        })
    }
}

impl Drop for Pending {
    fn drop(&mut self) {
        if !self.connected {
            let _ = self.operation.cancel(self.handle.as_raw_handle().cast());
        }
    }
}

pub struct Listener {
    name: PathBuf,
    pending: Mutex<Pending>,
    nonblocking: AtomicBool,
}

impl Listener {
    pub fn bind(endpoint: &Path) -> io::Result<Self> {
        let name = name(endpoint)?;
        let pending = Mutex::new(Pending::new(&name, true)?);
        Ok(Self {
            name,
            pending,
            nonblocking: AtomicBool::new(false),
        })
    }

    pub fn set_nonblocking(&self, nonblocking: bool) -> io::Result<()> {
        self.nonblocking.store(nonblocking, Ordering::Release);
        Ok(())
    }

    pub fn accept(&self) -> io::Result<(Stream, ())> {
        let mut pending = self
            .pending
            .lock()
            .map_err(|_| io::Error::other("pipe listener poisoned"))?;
        if !pending.connected {
            let handle = pending.handle.as_raw_handle().cast();
            let result = pending
                .operation
                .result(handle, !self.nonblocking.load(Ordering::Acquire));
            match result {
                Ok(_) => pending.connected = true,
                Err(error) if error.raw_os_error() == Some(ERROR_IO_INCOMPLETE as i32) => {
                    return Err(io::Error::from(io::ErrorKind::WouldBlock));
                }
                Err(error) => return Err(error),
            }
        }
        // Preserve the exclusive namespace while creating its next instance.
        let next = Pending::new(&self.name, false)?;
        let accepted = std::mem::replace(&mut *pending, next);
        let stream = Stream::new(Arc::clone(&accepted.handle));
        peer_pid(&stream, true)?;
        Ok((stream, ()))
    }
}

pub struct Stream {
    handle: Arc<File>,
    read_timeout: Mutex<Option<Duration>>,
    write_timeout: Mutex<Option<Duration>>,
}

impl Stream {
    fn new(handle: Arc<File>) -> Self {
        Self {
            handle,
            read_timeout: Mutex::new(None),
            write_timeout: Mutex::new(None),
        }
    }

    pub fn connect(endpoint: &Path) -> io::Result<Self> {
        let name = wide(&name(endpoint)?)?;
        // SAFETY: the NUL-terminated name lives through creation. Identification
        // QOS prevents the server from impersonating this client for effects.
        let handle = owned(unsafe {
            CreateFileW(
                name.as_ptr(),
                GENERIC_READ | GENERIC_WRITE,
                0,
                null(),
                OPEN_EXISTING,
                FILE_FLAG_OVERLAPPED | SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION,
                null_mut(),
            )
        })?;
        verify_owner_only_handle(&handle, false)?;
        let stream = Self::new(Arc::new(handle));
        peer_pid(&stream, false)?;
        Ok(stream)
    }

    pub fn try_clone(&self) -> io::Result<Self> {
        Ok(Self {
            handle: Arc::clone(&self.handle),
            read_timeout: Mutex::new(timeout(&self.read_timeout)?),
            write_timeout: Mutex::new(timeout(&self.write_timeout)?),
        })
    }

    pub fn set_nonblocking(&self, nonblocking: bool) -> io::Result<()> {
        if nonblocking {
            return Err(io::Error::from(io::ErrorKind::Unsupported));
        }
        Ok(())
    }

    pub fn set_read_timeout(&self, value: Option<Duration>) -> io::Result<()> {
        set_timeout(&self.read_timeout, value)
    }

    pub fn set_write_timeout(&self, value: Option<Duration>) -> io::Result<()> {
        set_timeout(&self.write_timeout, value)
    }
}

impl AsRawHandle for Stream {
    fn as_raw_handle(&self) -> std::os::windows::io::RawHandle {
        self.handle.as_raw_handle()
    }
}

impl Read for Stream {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if buffer.is_empty() {
            return Ok(0);
        }
        let timeout = timeout(&self.read_timeout)?;
        let mut operation = Operation::new()?;
        let handle = self.as_raw_handle().cast();
        let length = u32::try_from(buffer.len()).unwrap_or(u32::MAX);
        let mut bytes = 0;
        // SAFETY: the unique caller buffer, event and stable allocation stay
        // live until success or completed cancellation; count is bounded.
        let result = unsafe {
            ReadFile(
                handle,
                buffer.as_mut_ptr(),
                length,
                &mut bytes,
                &mut *operation.overlapped,
            )
        };
        if result != 0 {
            return Ok(bytes as usize);
        }
        match io::Error::last_os_error()
            .raw_os_error()
            .map(|code| code as u32)
        {
            Some(ERROR_IO_PENDING) => match operation.finish(handle, timeout) {
                Ok(bytes) => Ok(bytes as usize),
                Err(error) if error.raw_os_error() == Some(ERROR_BROKEN_PIPE as i32) => Ok(0),
                Err(error) => Err(error),
            },
            Some(ERROR_BROKEN_PIPE) => Ok(0),
            _ => Err(io::Error::last_os_error()),
        }
    }
}

impl Write for Stream {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        if buffer.is_empty() {
            return Ok(0);
        }
        let timeout = timeout(&self.write_timeout)?;
        let mut operation = Operation::new()?;
        let handle = self.as_raw_handle().cast();
        let length = u32::try_from(buffer.len()).unwrap_or(u32::MAX);
        let mut bytes = 0;
        // SAFETY: the immutable caller buffer and operation are retained through
        // completion or completed cancellation, as in the read implementation.
        let result = unsafe {
            WriteFile(
                handle,
                buffer.as_ptr(),
                length,
                &mut bytes,
                &mut *operation.overlapped,
            )
        };
        if result != 0 {
            return Ok(bytes as usize);
        }
        if io::Error::last_os_error().raw_os_error() == Some(ERROR_IO_PENDING as i32) {
            return operation
                .finish(handle, timeout)
                .map(|bytes| bytes as usize);
        }
        Err(io::Error::last_os_error())
    }

    fn flush(&mut self) -> io::Result<()> {
        // Like a socket this stream has no userspace buffer. Protocol owners
        // supply response framing/acknowledgement; FlushFileBuffers would wait
        // indefinitely for a peer and cannot enforce the connection deadline.
        Ok(())
    }
}

fn timeout(value: &Mutex<Option<Duration>>) -> io::Result<Option<Duration>> {
    value
        .lock()
        .map(|value| *value)
        .map_err(|_| io::Error::other("pipe timeout poisoned"))
}

fn set_timeout(timeout: &Mutex<Option<Duration>>, value: Option<Duration>) -> io::Result<()> {
    if value.is_some_and(|value| value.is_zero()) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "zero pipe timeout",
        ));
    }
    *timeout
        .lock()
        .map_err(|_| io::Error::other("pipe timeout poisoned"))? = value;
    Ok(())
}
