//! Held Windows process trees. Assignment is part of CreateProcess, before execution.
//! No PID, job name, or caller-supplied locator is a destructive capability.
use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io;
use std::mem::{size_of, zeroed};
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::path::Path;
use std::ptr::{null, null_mut};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Security::{
    DuplicateTokenEx, SECURITY_ATTRIBUTES, SecurityImpersonation, TOKEN_ADJUST_DEFAULT,
    TOKEN_ASSIGN_PRIMARY, TOKEN_DUPLICATE, TOKEN_QUERY, TokenPrimary,
};
use windows_sys::Win32::Storage::FileSystem::*;
use windows_sys::Win32::System::Console::{
    GetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE,
};
use windows_sys::Win32::System::JobObjects::*;
use windows_sys::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, CreatePipe, PIPE_NOWAIT, PIPE_READMODE_BYTE,
    PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, SetNamedPipeHandleState,
};
use windows_sys::Win32::System::Threading::*;

pub enum Stream<'a> {
    Null,
    Piped,
    File(&'a File),
    Inherit,
}

/// The environment is complete and explicit, never merged with ambient credentials.
pub struct Launch<'a> {
    pub program: &'a Path,
    pub args: &'a [OsString],
    pub env: &'a [(OsString, OsString)],
    pub cwd: Option<&'a Path>,
    pub stdin: Stream<'a>,
    pub stdout: Stream<'a>,
    pub stderr: Stream<'a>,
}

pub struct Child {
    pid: u32,
    process: OwnedHandle,
    job: OwnedHandle,
    pub stdin: Option<File>,
    pub stdout: Option<File>,
    pub stderr: Option<File>,
}

/// The daemon binds itself before any business child can start. This static owner is
/// deliberately never dropped during Rust shutdown: closing a job containing ourselves
/// would interrupt durable shutdown. Kernel process teardown closes it and kills children.
pub fn initialize_resident() -> io::Result<()> {
    static RESIDENT: OnceLock<Mutex<Option<OwnedHandle>>> = OnceLock::new();
    let mut resident = RESIDENT
        .get_or_init(|| Mutex::new(None))
        .lock()
        .map_err(|_| io::Error::other("resident job initialization"))?;
    if resident.is_some() {
        return Ok(());
    }
    let job = owned(unsafe { CreateJobObjectW(null(), null()) })?;
    let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { zeroed() };
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    if unsafe {
        SetInformationJobObject(
            job.as_raw_handle(),
            JobObjectExtendedLimitInformation,
            (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
            size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    // Nested jobs are supported on the Windows 11 headless target. Never enable breakaway.
    if unsafe { AssignProcessToJobObject(job.as_raw_handle(), GetCurrentProcess()) } == 0 {
        return Err(io::Error::last_os_error());
    }
    *resident = Some(job);
    Ok(())
}

/// Read-only observation for an authenticated orphan locator. This never authorizes a kill.
pub fn process_is_running(pid: u32) -> io::Result<bool> {
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE, 0, pid) };
    if handle.is_null() {
        let error = io::Error::last_os_error();
        if error.raw_os_error() == Some(ERROR_INVALID_PARAMETER as i32) {
            return Ok(false);
        }
        return Err(error);
    }
    let process = owned(handle)?;
    match unsafe { WaitForSingleObject(process.as_raw_handle(), 0) } {
        WAIT_TIMEOUT => Ok(true),
        WAIT_OBJECT_0 => Ok(false),
        _ => Err(io::Error::last_os_error()),
    }
}

/// An inherited credential transport is a non-stdio pipe, never a regular file.
pub fn prepare_inherited_pipe(file: &File) -> io::Result<()> {
    let handle = file.as_raw_handle();
    if [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE]
        .into_iter()
        .any(|standard| unsafe { GetStdHandle(standard) } == handle)
        || unsafe { GetFileType(handle) } != FILE_TYPE_PIPE
    {
        return Err(io::Error::other("inherited pipe required"));
    }
    let mode = PIPE_NOWAIT;
    if unsafe { SetNamedPipeHandleState(handle, &mode, null(), null()) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// A private bounded bootstrap channel. Both endpoints have the attributes access
/// required to set nonblocking mode; a CreatePipe read endpoint need not have it.
/// These handles start non-inheritable. The launcher owns any explicit duplication.
pub fn credential_pipe_pair() -> io::Result<(File, File)> {
    let mut nonce = [0u8; 32];
    getrandom::fill(&mut nonce).map_err(|_| io::Error::other("credential pipe nonce"))?;
    let name = format!(
        "\\\\.\\pipe\\hiroute-credential-{}",
        nonce
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    );
    let name = wide(OsStr::new(&name))?;
    let mut security = super::security::OwnerSecurity::new(false)?;
    let attributes = security.attributes()?;
    // The name is random and first-instance exclusive. Creation attaches the
    // caller SID's protected ACL before another process can observe the pipe.
    let server = owned(unsafe {
        CreateNamedPipeW(
            name.as_ptr(),
            PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_NOWAIT | PIPE_REJECT_REMOTE_CLIENTS,
            1,
            4096,
            4096,
            0,
            &attributes,
        )
    })?;
    let client = owned(unsafe {
        CreateFileW(
            name.as_ptr(),
            GENERIC_READ | GENERIC_WRITE,
            0,
            &attributes,
            OPEN_EXISTING,
            0,
            null_mut(),
        )
    })?;
    if unsafe { ConnectNamedPipe(server.as_raw_handle(), null_mut()) } == 0
        && io::Error::last_os_error().raw_os_error() != Some(ERROR_PIPE_CONNECTED as i32)
    {
        return Err(io::Error::last_os_error());
    }
    let (server, client) = (File::from(server), File::from(client));
    super::security::verify_owner_only_handle(&server, false)?;
    super::security::verify_owner_only_handle(&client, false)?;
    prepare_inherited_pipe(&server)?;
    prepare_inherited_pipe(&client)?;
    Ok((server, client))
}

/// Make one explicit non-stdio pipe capability inheritable. The duplicate's number
/// is the locator to put in the child arguments, and its lifetime remains caller-owned.
pub fn duplicate_inheritable_pipe(pipe: &File) -> io::Result<File> {
    prepare_inherited_pipe(pipe)?;
    super::security::verify_owner_only_handle(pipe, false)?;
    Ok(File::from(duplicate(pipe.as_raw_handle())?))
}

/// Read the caller's explicitly inherited, private pipe through a non-inheritable
/// duplicate. The raw numeric locator never becomes ownership of the original handle.
/// Callers retain their text/framing rules; the transport enforces EOF, time and size.
pub fn read_inherited_credential(
    handle: u64,
    limit: usize,
    timeout: Duration,
) -> io::Result<zeroize::Zeroizing<Vec<u8>>> {
    let invalid = || io::Error::new(io::ErrorKind::InvalidInput, "protected pipe input");
    if limit == 0 || limit > 1024 * 1024 || timeout.is_zero() || timeout > Duration::from_secs(30) {
        return Err(invalid());
    }
    let handle = usize::try_from(handle).map_err(|_| invalid())? as HANDLE;
    if handle.is_null()
        || handle == INVALID_HANDLE_VALUE
        || [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE]
            .into_iter()
            .any(|standard| unsafe { GetStdHandle(standard) } == handle)
    {
        return Err(invalid());
    }
    let mut flags = 0;
    if unsafe { GetHandleInformation(handle, &mut flags) } == 0 {
        return Err(io::Error::last_os_error());
    }
    if flags & HANDLE_FLAG_INHERIT == 0 {
        return Err(invalid());
    }
    let file = File::from(duplicate_with_inheritance(handle, false)?);
    // Validate the duplicated object, not a locator that another thread might close.
    if unsafe { GetFileType(file.as_raw_handle()) } != FILE_TYPE_PIPE {
        return Err(invalid());
    }
    super::security::verify_owner_only_handle(&file, false)?;
    prepare_inherited_pipe(&file)?;
    let deadline = Instant::now() + timeout;
    let mut bytes = zeroize::Zeroizing::new(Vec::new());
    let mut block = zeroize::Zeroizing::new(vec![0u8; (limit + 1).min(8192)]);
    loop {
        if Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "protected pipe deadline",
            ));
        }
        let remaining = (limit + 1 - bytes.len()).min(block.len());
        // std::fs::File::read folds ERROR_NO_DATA into Ok(0) on Windows, which
        // loses the distinction between an empty connected NOWAIT pipe and EOF.
        // Preserve the native error and finish only after the writer disconnects.
        let mut count = 0;
        // SAFETY: the held pipe is in NOWAIT mode; the live zeroizing allocation
        // has `remaining` writable bytes and the byte-count out pointer is valid.
        let read = unsafe {
            ReadFile(
                file.as_raw_handle(),
                block.as_mut_ptr(),
                remaining as u32,
                &mut count,
                null_mut(),
            )
        };
        let result = if read == 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(count as usize)
        };
        match result {
            Ok(0) => std::thread::sleep(Duration::from_millis(2)),
            Ok(count) => {
                bytes.extend_from_slice(&block[..count]);
                if bytes.len() > limit {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "protected pipe bound",
                    ));
                }
            }
            Err(error) if error.raw_os_error() == Some(ERROR_BROKEN_PIPE as i32) => {
                return Ok(bytes);
            }
            Err(error)
                if error.kind() == io::ErrorKind::WouldBlock
                    || error.raw_os_error() == Some(ERROR_NO_DATA as i32) =>
            {
                std::thread::sleep(Duration::from_millis(2));
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
}

fn owned(handle: HANDLE) -> io::Result<OwnedHandle> {
    if handle.is_null() || handle == INVALID_HANDLE_VALUE {
        Err(io::Error::last_os_error())
    } else {
        // SAFETY: the successful API returned an independently owned, valid handle.
        Ok(unsafe { OwnedHandle::from_raw_handle(handle) })
    }
}

fn wide(value: &OsStr) -> io::Result<Vec<u16>> {
    let mut value: Vec<_> = value.encode_wide().collect();
    if value.contains(&0) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "embedded NUL"));
    }
    value.push(0);
    Ok(value)
}

/// Microsoft CRT argv quoting, without cmd.exe/PowerShell interpretation.
fn argument(value: &OsStr, output: &mut Vec<u16>) -> io::Result<()> {
    let value: Vec<_> = value.encode_wide().collect();
    if value.contains(&0) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "embedded NUL"));
    }
    output.push(b'"' as u16);
    let mut slashes = 0;
    for ch in value {
        if ch == b'\\' as u16 {
            slashes += 1;
            continue;
        }
        output.extend(std::iter::repeat_n(
            b'\\' as u16,
            slashes * if ch == b'"' as u16 { 2 } else { 1 },
        ));
        if ch == b'"' as u16 {
            output.push(b'\\' as u16);
        }
        output.push(ch);
        slashes = 0;
    }
    output.extend(std::iter::repeat_n(b'\\' as u16, slashes * 2));
    output.push(b'"' as u16);
    Ok(())
}

fn duplicate(handle: HANDLE) -> io::Result<OwnedHandle> {
    duplicate_with_inheritance(handle, true)
}

fn duplicate_with_inheritance(handle: HANDLE, inheritable: bool) -> io::Result<OwnedHandle> {
    let mut output = null_mut();
    // SAFETY: process pseudo handles are valid; only the new handle is inheritable.
    if unsafe {
        DuplicateHandle(
            GetCurrentProcess(),
            handle,
            GetCurrentProcess(),
            &mut output,
            0,
            i32::from(inheritable),
            DUPLICATE_SAME_ACCESS,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    owned(output)
}

fn stream(mode: Stream<'_>, input: bool, standard: u32) -> io::Result<(OwnedHandle, Option<File>)> {
    match mode {
        Stream::File(file) => Ok((duplicate(file.as_raw_handle())?, None)),
        Stream::Inherit => {
            // SAFETY: querying a standard handle does not transfer its ownership.
            let handle = unsafe { GetStdHandle(standard) };
            if handle.is_null() || handle == INVALID_HANDLE_VALUE {
                stream(Stream::Null, input, standard)
            } else {
                Ok((duplicate(handle)?, None))
            }
        }
        Stream::Null => {
            let name = wide(OsStr::new("NUL"))?;
            let attributes = SECURITY_ATTRIBUTES {
                nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
                lpSecurityDescriptor: null_mut(),
                bInheritHandle: 1,
            };
            // SAFETY: pointers remain live for the synchronous open.
            let handle = unsafe {
                CreateFileW(
                    name.as_ptr(),
                    if input { GENERIC_READ } else { GENERIC_WRITE },
                    FILE_SHARE_READ | FILE_SHARE_WRITE,
                    &attributes,
                    OPEN_EXISTING,
                    FILE_ATTRIBUTE_NORMAL,
                    null_mut(),
                )
            };
            Ok((owned(handle)?, None))
        }
        Stream::Piped => {
            let attributes = SECURITY_ATTRIBUTES {
                nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
                lpSecurityDescriptor: null_mut(),
                bInheritHandle: 1,
            };
            let (mut read, mut write) = (null_mut(), null_mut());
            // SAFETY: output slots and attributes are valid for the call.
            if unsafe { CreatePipe(&mut read, &mut write, &attributes, 0) } == 0 {
                return Err(io::Error::last_os_error());
            }
            let (read, write) = (owned(read)?, owned(write)?);
            let (child, parent) = if input { (read, write) } else { (write, read) };
            // SAFETY: parent is owned and valid; prevent inheritance of the wrong pipe end.
            if unsafe { SetHandleInformation(parent.as_raw_handle(), HANDLE_FLAG_INHERIT, 0) } == 0
            {
                return Err(io::Error::last_os_error());
            }
            Ok((child, Some(File::from(parent))))
        }
    }
}

struct Attributes {
    memory: Vec<usize>,
}
impl Attributes {
    fn new() -> io::Result<Self> {
        let mut bytes = 0;
        // SAFETY: first call obtains the required allocation size.
        unsafe {
            InitializeProcThreadAttributeList(null_mut(), 2, 0, &mut bytes);
        }
        if bytes == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut memory = vec![0; bytes.div_ceil(size_of::<usize>())];
        // SAFETY: the pointer has native-word alignment and at least the requested bytes.
        if unsafe {
            InitializeProcThreadAttributeList(memory.as_mut_ptr().cast(), 2, 0, &mut bytes)
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(Self { memory })
    }
    fn pointer(&mut self) -> LPPROC_THREAD_ATTRIBUTE_LIST {
        self.memory.as_mut_ptr().cast()
    }
}
impl Drop for Attributes {
    fn drop(&mut self) {
        // SAFETY: only initialized lists construct Attributes; storage is still live.
        unsafe {
            DeleteProcThreadAttributeList(self.pointer());
        }
    }
}

impl Child {
    pub fn spawn(launch: Launch<'_>) -> io::Result<Self> {
        Self::spawn_with_token(launch, None, &[], false)
    }

    /// An isolated native-test console host. Never use this for a product daemon
    /// or a Worker: ordinary production launches remain CREATE_NO_WINDOW.
    /// The new console contains only this held Job's fixture, so its Ctrl-C/Break
    /// tests cannot broadcast into Cargo, the runner, or the user's parent console.
    pub fn spawn_console_host(launch: Launch<'_>) -> io::Result<Self> {
        Self::spawn_with_token(launch, None, &[], true)
    }

    /// Include only these caller-held private pipe capabilities in the same atomic
    /// handle list as stdio and Job assignment. No ambient inheritable handle is added.
    pub fn spawn_with_inherited_pipes(launch: Launch<'_>, pipes: &[&File]) -> io::Result<Self> {
        Self::spawn_with_token(launch, None, pipes, false)
    }

    /// Borrow a caller-owned primary token. Job assignment and the explicit handle list
    /// use the same atomic CreateProcess contract as the ordinary current-user launch.
    pub fn spawn_as_token(launch: Launch<'_>, token: &impl AsRawHandle) -> io::Result<Self> {
        Self::spawn_with_token(launch, Some(token.as_raw_handle()), &[], false)
    }

    fn spawn_with_token(
        launch: Launch<'_>,
        token: Option<HANDLE>,
        pipes: &[&File],
        console_host: bool,
    ) -> io::Result<Self> {
        if pipes.len() > 8 {
            return Err(io::Error::other("inherited pipe count"));
        }
        for (index, pipe) in pipes.iter().enumerate() {
            prepare_inherited_pipe(pipe)?;
            super::security::verify_owner_only_handle(*pipe, false)?;
            let mut flags = 0;
            if unsafe { GetHandleInformation(pipe.as_raw_handle(), &mut flags) } == 0 {
                return Err(io::Error::last_os_error());
            }
            if flags & HANDLE_FLAG_INHERIT == 0
                || pipes[..index]
                    .iter()
                    .any(|old| old.as_raw_handle() == pipe.as_raw_handle())
            {
                return Err(io::Error::other("explicit inherited pipe required"));
            }
        }
        if !launch.program.is_absolute()
            || launch
                .program
                .extension()
                .is_none_or(|ext| !ext.as_encoded_bytes().eq_ignore_ascii_case(b"exe"))
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "native executable required",
            ));
        }
        let program = wide(launch.program.as_os_str())?;
        let mut command = Vec::new();
        argument(launch.program.as_os_str(), &mut command)?;
        for arg in launch.args {
            command.push(b' ' as u16);
            argument(arg, &mut command)?;
        }
        command.push(0);
        if command.len() > 32767 {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "command bound"));
        }
        let mut environment = zeroize::Zeroizing::new(Vec::new());
        let mut entries: Vec<_> = launch.env.iter().collect();
        entries.sort_by_key(|(key, _)| key.to_string_lossy().to_ascii_uppercase());
        let mut previous = None;
        for (key, value) in entries {
            let folded = key.to_string_lossy().to_ascii_uppercase();
            if folded.is_empty() || folded.contains('=') || previous.as_ref() == Some(&folded) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "environment key",
                ));
            }
            previous = Some(folded);
            let mut key = wide(key)?;
            key.pop();
            environment.extend(key);
            environment.push(b'=' as u16);
            environment.extend(wide(value)?);
        }
        // An empty environment also requires the complete double-NUL terminator.
        if environment.is_empty() {
            environment.push(0);
        }
        environment.push(0);
        let cwd = launch.cwd.map(|path| wide(path.as_os_str())).transpose()?;
        let (input, stdin) = stream(launch.stdin, true, STD_INPUT_HANDLE)?;
        let (output, stdout) = stream(launch.stdout, false, STD_OUTPUT_HANDLE)?;
        let (error, stderr) = stream(launch.stderr, false, STD_ERROR_HANDLE)?;
        // SAFETY: unnamed job is private and cannot collide with another run's object.
        let job = owned(unsafe { CreateJobObjectW(null(), null()) })?;
        let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { zeroed() };
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        // No BREAKAWAY_OK/SILENT_BREAKAWAY_OK: ordinary descendants remain contained.
        if unsafe {
            SetInformationJobObject(
                job.as_raw_handle(),
                JobObjectExtendedLimitInformation,
                (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        let mut attributes = Attributes::new()?;
        let mut job_handles = [job.as_raw_handle()];
        let mut inherited = vec![
            input.as_raw_handle(),
            output.as_raw_handle(),
            error.as_raw_handle(),
        ];
        inherited.extend(pipes.iter().map(|file| file.as_raw_handle()));
        // SAFETY: handle arrays, jobs and attribute storage outlive CreateProcessW.
        for (attribute, handles) in [
            (PROC_THREAD_ATTRIBUTE_JOB_LIST, &mut job_handles[..]),
            (PROC_THREAD_ATTRIBUTE_HANDLE_LIST, &mut inherited[..]),
        ] {
            if unsafe {
                UpdateProcThreadAttribute(
                    attributes.pointer(),
                    0,
                    attribute as usize,
                    handles.as_mut_ptr().cast(),
                    std::mem::size_of_val(handles),
                    null_mut(),
                    null_mut(),
                )
            } == 0
            {
                return Err(io::Error::last_os_error());
            }
        }
        let mut startup: STARTUPINFOEXW = unsafe { zeroed() };
        startup.StartupInfo.cb = size_of::<STARTUPINFOEXW>() as u32;
        startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
        startup.StartupInfo.hStdInput = input.as_raw_handle();
        startup.StartupInfo.hStdOutput = output.as_raw_handle();
        startup.StartupInfo.hStdError = error.as_raw_handle();
        // A restricted business token must choose its own noninteractive logon
        // station. NULL would inherit a parent station whose ACL/IL may require
        // the elevated groups we deliberately removed from the child token.
        // The empty, live UTF-16 string requests the documented logon selection;
        // it neither modifies desktop ACLs nor selects an interactive desktop.
        let mut desktop = [0u16];
        if token.is_some() {
            startup.StartupInfo.lpDesktop = desktop.as_mut_ptr();
        }
        startup.lpAttributeList = attributes.pointer();
        let mut information: PROCESS_INFORMATION = unsafe { zeroed() };
        // A caller-owned primary token may otherwise become the child's actual
        // token object. Duplicate it before assignment so changing child defaults
        // cannot change the service owner's retained authentication token.
        let launch_token = token
            .map(|token| {
                let mut copy = null_mut();
                if unsafe {
                    DuplicateTokenEx(
                        token,
                        TOKEN_QUERY | TOKEN_ADJUST_DEFAULT | TOKEN_ASSIGN_PRIMARY | TOKEN_DUPLICATE,
                        null(),
                        SecurityImpersonation,
                        TokenPrimary,
                        &mut copy,
                    )
                } == 0
                {
                    return Err(io::Error::last_os_error());
                }
                owned(copy)
            })
            .transpose()?;
        // The kernel assigns the job before allowing the initial thread to execute.
        // HANDLE_LIST prevents unrelated concurrent launches inheriting credentials/stdio.
        let flags = EXTENDED_STARTUPINFO_PRESENT
            | CREATE_UNICODE_ENVIRONMENT
            | if console_host {
                CREATE_NEW_CONSOLE
            } else {
                CREATE_NO_WINDOW
            }
            | CREATE_SUSPENDED;
        let created = unsafe {
            if let Some(token) = &launch_token {
                CreateProcessAsUserW(
                    token.as_raw_handle(),
                    program.as_ptr(),
                    command.as_mut_ptr(),
                    null(),
                    null(),
                    1,
                    flags,
                    environment.as_ptr().cast(),
                    cwd.as_ref().map_or(null(), |cwd| cwd.as_ptr()),
                    &startup.StartupInfo,
                    &mut information,
                )
            } else {
                CreateProcessW(
                    program.as_ptr(),
                    command.as_mut_ptr(),
                    null(),
                    null(),
                    1,
                    flags,
                    environment.as_ptr().cast(),
                    cwd.as_ref().map_or(null(), |cwd| cwd.as_ptr()),
                    &startup.StartupInfo,
                    &mut information,
                )
            }
        };
        if created == 0 {
            return Err(io::Error::last_os_error());
        }
        let process = owned(information.hProcess)?;
        let thread = owned(information.hThread)?;
        let child = Self {
            pid: information.dwProcessId,
            process,
            job,
            stdin,
            stdout,
            stderr,
        };
        let initialized = (|| {
            // Normalize only the new child's token, not the borrowed SCM token
            // or the parent's identity. No user-mode loader/Go/C code has run.
            super::security::initialize_child_process_private_defaults(&child.process, token)?;
            let suspended = unsafe { ResumeThread(thread.as_raw_handle()) };
            if suspended != 1 {
                return Err(if suspended == u32::MAX {
                    io::Error::last_os_error()
                } else {
                    io::Error::other("initial thread suspension changed")
                });
            }
            Ok(())
        })();
        if let Err(error) = initialized {
            child.terminate()?;
            let deadline = Instant::now() + Duration::from_secs(5);
            while !child.empty()? {
                if Instant::now() >= deadline {
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "failed child cleanup deadline",
                    ));
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            return Err(error);
        }
        Ok(child)
    }

    /// The held process object remains authoritative even after its numeric PID is reused.
    pub fn id(&self) -> u32 {
        self.pid
    }

    pub fn exit_code(&self) -> io::Result<Option<i32>> {
        match unsafe { WaitForSingleObject(self.process.as_raw_handle(), 0) } {
            WAIT_TIMEOUT => Ok(None),
            WAIT_OBJECT_0 => {
                let mut code = 0;
                if unsafe { GetExitCodeProcess(self.process.as_raw_handle(), &mut code) } == 0 {
                    return Err(io::Error::last_os_error());
                }
                Ok(Some(code as i32))
            }
            _ => Err(io::Error::last_os_error()),
        }
    }

    pub fn terminate(&self) -> io::Result<()> {
        if unsafe { TerminateJobObject(self.job.as_raw_handle(), 1) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    pub fn empty(&self) -> io::Result<bool> {
        let mut accounting: JOBOBJECT_BASIC_ACCOUNTING_INFORMATION = unsafe { zeroed() };
        if unsafe {
            QueryInformationJobObject(
                self.job.as_raw_handle(),
                JobObjectBasicAccountingInformation,
                (&mut accounting as *mut JOBOBJECT_BASIC_ACCOUNTING_INFORMATION).cast(),
                size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>() as u32,
                null_mut(),
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(accounting.ActiveProcesses == 0)
    }
}

impl Drop for Child {
    fn drop(&mut self) {
        // Last resort; callers must observe empty() before reporting successful cleanup.
        let _ = self.terminate();
    }
}
