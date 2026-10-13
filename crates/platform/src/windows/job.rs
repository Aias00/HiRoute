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
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;
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
    let mut output = null_mut();
    // SAFETY: process pseudo handles are valid; only the new handle is inheritable.
    if unsafe {
        DuplicateHandle(
            GetCurrentProcess(),
            handle,
            GetCurrentProcess(),
            &mut output,
            0,
            1,
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
        Self::spawn_with_token(launch, None)
    }

    /// Borrow a caller-owned primary token. Job assignment and the explicit handle list
    /// use the same atomic CreateProcess contract as the ordinary current-user launch.
    pub fn spawn_as_token(launch: Launch<'_>, token: &impl AsRawHandle) -> io::Result<Self> {
        Self::spawn_with_token(launch, Some(token.as_raw_handle()))
    }

    fn spawn_with_token(launch: Launch<'_>, token: Option<HANDLE>) -> io::Result<Self> {
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
        let mut inherited = [
            input.as_raw_handle(),
            output.as_raw_handle(),
            error.as_raw_handle(),
        ];
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
        startup.lpAttributeList = attributes.pointer();
        let mut information: PROCESS_INFORMATION = unsafe { zeroed() };
        // The kernel assigns the job before allowing the initial thread to execute.
        // HANDLE_LIST prevents unrelated concurrent launches inheriting credentials/stdio.
        let flags = EXTENDED_STARTUPINFO_PRESENT | CREATE_UNICODE_ENVIRONMENT | CREATE_NO_WINDOW;
        let created = unsafe {
            if let Some(token) = token {
                CreateProcessAsUserW(
                    token,
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
        let _thread = owned(information.hThread)?;
        Ok(Self {
            pid: information.dwProcessId,
            process,
            job,
            stdin,
            stdout,
            stderr,
        })
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
