//! Test-only alternate-account launch. SeImpersonate is available to elevated CI;
//! SeAssignPrimaryToken is not assumed. The initial thread stays suspended until
//! its held process has joined the cleanup Job and its actual token is checked.
use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io;
use std::mem::{size_of, zeroed};
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle};
use std::path::Path;
use std::ptr::{null, null_mut};
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Security::{SECURITY_ATTRIBUTES, TOKEN_QUERY};
use windows_sys::Win32::System::JobObjects::*;
use windows_sys::Win32::System::Threading::*;

// This declaration avoids introducing a production Pipes dependency solely for
// a fixture. The remaining APIs use the daemon's native-test feature inventory.
#[link(name = "kernel32")]
unsafe extern "system" {
    fn CreatePipe(
        read: *mut HANDLE,
        write: *mut HANDLE,
        attributes: *const SECURITY_ATTRIBUTES,
        size: u32,
    ) -> i32;
}

fn owned(handle: HANDLE) -> io::Result<File> {
    if handle.is_null() || handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    Ok(unsafe { File::from_raw_handle(handle.cast()) })
}
fn native_error(stage: &'static str) -> io::Error {
    let error = io::Error::last_os_error();
    io::Error::new(
        error.kind(),
        format!("native owner launcher {stage}: {error}"),
    )
}

fn pipe(input: bool) -> io::Result<(File, File)> {
    let attributes = SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: null_mut(),
        bInheritHandle: 1,
    };
    let (mut read, mut write) = (null_mut(), null_mut());
    if unsafe { CreatePipe(&mut read, &mut write, &attributes, 0) } == 0 {
        return Err(native_error("CreatePipe"));
    }
    let (read, write) = (owned(read)?, owned(write)?);
    let (child, parent) = if input { (read, write) } else { (write, read) };
    if unsafe { SetHandleInformation(parent.as_raw_handle().cast(), HANDLE_FLAG_INHERIT, 0) } == 0 {
        return Err(native_error("SetHandleInformation(parent pipe)"));
    }
    let mut flags = 0;
    if unsafe { GetHandleInformation(child.as_raw_handle().cast(), &mut flags) } == 0 {
        return Err(native_error("GetHandleInformation(child pipe)"));
    }
    assert_ne!(
        flags & HANDLE_FLAG_INHERIT,
        0,
        "child stdio must be inheritable"
    );
    Ok((child, parent))
}
fn quote(value: &OsStr) -> Vec<u16> {
    let mut output = vec![b'"' as u16];
    let mut slashes = 0;
    for unit in value.encode_wide() {
        assert_ne!(unit, 0);
        if unit == b'\\' as u16 {
            slashes += 1;
            continue;
        }
        output.extend(std::iter::repeat_n(
            b'\\' as u16,
            if unit == b'"' as u16 {
                slashes * 2 + 1
            } else {
                slashes
            },
        ));
        slashes = 0;
        output.push(unit);
    }
    output.extend(std::iter::repeat_n(b'\\' as u16, slashes * 2));
    output.push(b'"' as u16);
    output
}
/// Owns both returned handles immediately after suspended creation. Every error
/// or panic before transfer terminates and waits for this exact process, even
/// when assigning it to the Job failed. No numeric PID is used for cleanup.
struct PendingProcess<'a> {
    process: HANDLE,
    thread: HANDLE,
    job: &'a File,
    assigned: bool,
}
impl PendingProcess<'_> {
    fn take_process(&mut self) -> File {
        let handle = std::mem::replace(&mut self.process, null_mut());
        // SAFETY: successful creation provided this unique owned handle.
        unsafe { File::from_raw_handle(handle.cast()) }
    }
}
impl Drop for PendingProcess<'_> {
    fn drop(&mut self) {
        let mut cleanup_wait = None;
        let mut cleanup_empty = true;
        if !self.process.is_null() {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            // Termination is asynchronous: retaining and waiting for the native
            // process is required, including the pre-Job failure path.
            unsafe {
                if self.assigned {
                    TerminateJobObject(self.job.as_raw_handle().cast(), 1);
                }
                TerminateProcess(self.process, 1);
            }
            let wait = unsafe { WaitForSingleObject(self.process, 5000) };
            if self.assigned {
                loop {
                    match active_processes(self.job) {
                        Ok(0) => break,
                        Ok(_) if std::time::Instant::now() < deadline => {
                            std::thread::sleep(std::time::Duration::from_millis(10));
                        }
                        _ => {
                            cleanup_empty = false;
                            break;
                        }
                    }
                }
            }
            unsafe { CloseHandle(self.process) };
            cleanup_wait = Some(wait);
        }
        if !self.thread.is_null() {
            unsafe { CloseHandle(self.thread) };
        }
        if let Some(wait) = cleanup_wait {
            assert_eq!(
                wait, WAIT_OBJECT_0,
                "native pending-owner cleanup did not finish"
            );
            assert!(
                cleanup_empty,
                "native pending-owner Job cleanup did not finish"
            );
        }
    }
}

fn active_processes(job: &File) -> io::Result<u32> {
    let mut value: JOBOBJECT_BASIC_ACCOUNTING_INFORMATION = unsafe { zeroed() };
    if unsafe {
        QueryInformationJobObject(
            job.as_raw_handle().cast(),
            JobObjectBasicAccountingInformation,
            (&mut value as *mut JOBOBJECT_BASIC_ACCOUNTING_INFORMATION).cast(),
            size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>() as u32,
            null_mut(),
        )
    } == 0
    {
        return Err(native_error("QueryInformationJobObject(active processes)"));
    }
    Ok(value.ActiveProcesses)
}

/// Executed by the actual owner fixture before consuming its request. Real
/// request/response I/O additionally proves the transferred stdin/stdout work.
pub fn assert_fixture_stdio() {
    use windows_sys::Win32::Storage::FileSystem::{FILE_TYPE_PIPE, GetFileType};
    use windows_sys::Win32::System::Console::{
        GetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE,
    };
    for kind in [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
        let handle = unsafe { GetStdHandle(kind) };
        assert!(
            !handle.is_null() && handle != INVALID_HANDLE_VALUE,
            "owner stdio handle missing"
        );
        let mut flags = 0;
        assert_ne!(
            unsafe { GetHandleInformation(handle, &mut flags) },
            0,
            "owner stdio handle invalid"
        );
        assert_eq!(
            unsafe { GetFileType(handle) },
            FILE_TYPE_PIPE,
            "owner stdio must be an actual pipe"
        );
    }
}

pub struct Child {
    process: File,
    job: File,
    pub stdin: Option<File>,
    pub stdout: Option<File>,
    pub stderr: Option<File>,
}
impl Child {
    pub fn spawn(
        token: &File,
        program: &Path,
        args: &[OsString],
        env: &[(OsString, OsString)],
    ) -> io::Result<Self> {
        assert!(program.is_absolute());
        let application: Vec<_> = program.as_os_str().encode_wide().chain([0]).collect();
        let cwd: Vec<_> = program
            .parent()
            .unwrap()
            .as_os_str()
            .encode_wide()
            .chain([0])
            .collect();
        let mut command = quote(program.as_os_str());
        for arg in args {
            command.push(b' ' as u16);
            command.extend(quote(arg));
        }
        command.push(0);
        assert!(
            command.len() <= 1024,
            "CreateProcessWithTokenW command exceeds documented fixture bound"
        );
        let mut environment = Vec::new();
        let mut entries = env.to_vec();
        entries.sort_by_key(|(key, _)| key.to_string_lossy().to_lowercase());
        for (key, value) in entries {
            assert!(
                !key.is_empty() && !key.encode_wide().any(|unit| matches!(unit, 0 | 61)),
                "invalid native environment key"
            );
            assert!(
                !value.encode_wide().any(|unit| unit == 0),
                "invalid native environment value"
            );
            environment.extend(key.encode_wide());
            environment.push(b'=' as u16);
            environment.extend(value.encode_wide());
            environment.push(0);
        }
        environment.push(0);
        if environment.len() == 1 {
            environment.push(0);
        }
        let (input, stdin) = pipe(true)?;
        let (output, stdout) = pipe(false)?;
        let (error, stderr) = pipe(false)?;
        let job_handle = unsafe { CreateJobObjectW(null(), null()) };
        if job_handle.is_null() {
            return Err(native_error("CreateJobObjectW"));
        }
        let job = owned(job_handle)?;
        let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { zeroed() };
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        if unsafe {
            SetInformationJobObject(
                job.as_raw_handle().cast(),
                JobObjectExtendedLimitInformation,
                (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        } == 0
        {
            return Err(native_error("SetInformationJobObject(KILL_ON_JOB_CLOSE)"));
        }
        let mut desktop = [0_u16];
        // WithToken uses Secondary Logon and does not provide CreateProcessW's
        // bInheritHandles control. Pass only the three documented std handles,
        // and use ordinary STARTUPINFO rather than relying on extended attrs.
        let startup = STARTUPINFOW {
            cb: size_of::<STARTUPINFOW>() as u32,
            dwFlags: STARTF_USESTDHANDLES,
            lpDesktop: desktop.as_mut_ptr(),
            hStdInput: input.as_raw_handle().cast(),
            hStdOutput: output.as_raw_handle().cast(),
            hStdError: error.as_raw_handle().cast(),
            ..unsafe { zeroed() }
        };
        let mut information: PROCESS_INFORMATION = unsafe { zeroed() };
        if unsafe {
            CreateProcessWithTokenW(
                token.as_raw_handle().cast(),
                0,
                application.as_ptr(),
                command.as_mut_ptr(),
                CREATE_SUSPENDED | CREATE_UNICODE_ENVIRONMENT | CREATE_NO_WINDOW,
                environment.as_ptr().cast(),
                cwd.as_ptr(),
                &startup,
                &mut information,
            )
        } == 0
        {
            return Err(native_error(
                "CreateProcessWithTokenW(suspended STARTUPINFO)",
            ));
        }
        let mut pending = PendingProcess {
            process: information.hProcess,
            thread: information.hThread,
            job: &job,
            assigned: false,
        };
        assert!(!pending.process.is_null() && !pending.thread.is_null());
        if unsafe { AssignProcessToJobObject(job.as_raw_handle().cast(), pending.process) } == 0 {
            return Err(native_error("AssignProcessToJobObject(before resume)"));
        }
        pending.assigned = true;
        let mut actual_token = null_mut();
        if unsafe { OpenProcessToken(pending.process, TOKEN_QUERY, &mut actual_token) } == 0 {
            return Err(native_error("OpenProcessToken(suspended owner)"));
        }
        let actual_token = owned(actual_token)?;
        let expected_sid = super::windows_account::token_sid(token);
        assert_eq!(
            super::windows_account::token_sid(&actual_token),
            expected_sid,
            "actual owner child SID must equal authenticated token SID"
        );
        let expected_sid =
            hiroute_platform::windows::background::sid_string(&expected_sid).unwrap();
        super::windows_account::assert_standard(&actual_token, &expected_sid);
        let mut in_job = 0;
        if unsafe { IsProcessInJob(pending.process, job.as_raw_handle().cast(), &mut in_job) } == 0
        {
            return Err(native_error("IsProcessInJob(before resume)"));
        }
        assert_ne!(in_job, 0, "suspended owner must already be in cleanup Job");
        let previous = unsafe { ResumeThread(pending.thread) };
        if previous == u32::MAX {
            return Err(native_error("ResumeThread(initial owner thread)"));
        }
        assert_eq!(
            previous, 1,
            "owner initial thread must resume from exactly one suspension"
        );
        let process = pending.take_process();
        drop(pending);
        Ok(Self {
            process,
            job,
            stdin: Some(stdin),
            stdout: Some(stdout),
            stderr: Some(stderr),
        })
    }
    pub fn exit_code(&self) -> io::Result<Option<i32>> {
        match unsafe { WaitForSingleObject(self.process.as_raw_handle().cast(), 0) } {
            WAIT_TIMEOUT => Ok(None),
            WAIT_OBJECT_0 => {
                let mut code = 0;
                if unsafe { GetExitCodeProcess(self.process.as_raw_handle().cast(), &mut code) }
                    == 0
                {
                    Err(io::Error::last_os_error())
                } else {
                    Ok(Some(code as i32))
                }
            }
            _ => Err(io::Error::last_os_error()),
        }
    }
    pub fn empty(&self) -> io::Result<bool> {
        Ok(active_processes(&self.job)? == 0)
    }
    pub fn terminate(&self) -> io::Result<()> {
        if unsafe { TerminateJobObject(self.job.as_raw_handle().cast(), 1) } == 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }
}

/// Stage the verified currently running fixture under its real owner. Retained
/// native image access denies replacement until all owner subprocesses finish.
pub struct StagedFixture {
    pub path: std::path::PathBuf,
    token: File,
    image: Option<File>,
}
impl StagedFixture {
    pub fn create(token: &File) -> Self {
        use hiroute_platform::windows::{background, security};
        use sha2::{Digest, Sha256};
        use std::io::{Read, Seek, SeekFrom, Write};
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Security::Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW;
        use windows_sys::Win32::Security::{ImpersonateLoggedOnUser, RevertToSelf};
        use windows_sys::Win32::Storage::FileSystem::{
            CREATE_NEW, CreateDirectoryW, CreateFileW, FILE_ATTRIBUTE_NORMAL,
            FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ,
        };
        let sid_bytes = super::windows_account::token_sid(token);
        let sid = background::sid_string(&sid_bytes).unwrap();
        let directory = super::windows_account::profile_for_token(token).join(format!(
            ".hiroute-native-background-test-{}",
            std::process::id()
        ));
        let path = directory.join("windows-background-fixture.exe");
        let mut source = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ)
            .open(std::env::current_exe().unwrap())
            .unwrap();
        let expected_len = source.metadata().unwrap().len();
        let source_hash = {
            let mut hash = Sha256::new();
            let mut buffer = [0; 65536];
            loop {
                let n = source.read(&mut buffer).unwrap();
                if n == 0 {
                    break;
                }
                hash.update(&buffer[..n]);
            }
            source.seek(SeekFrom::Start(0)).unwrap();
            hash.finalize()
        };
        let sddl = super::windows_account::wide(&format!("O:{sid}G:{sid}D:P(A;OICI;FA;;;{sid})"));
        let mut descriptor = null_mut();
        assert_ne!(
            unsafe {
                ConvertStringSecurityDescriptorToSecurityDescriptorW(
                    sddl.as_ptr(),
                    1,
                    &mut descriptor,
                    null_mut(),
                )
            },
            0
        );
        let attributes = SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor,
            bInheritHandle: 0,
        };
        assert_ne!(
            unsafe { ImpersonateLoggedOnUser(token.as_raw_handle().cast()) },
            0
        );
        struct Revert;
        impl Drop for Revert {
            fn drop(&mut self) {
                if unsafe { RevertToSelf() } == 0 {
                    std::process::abort();
                }
            }
        }
        let _revert = Revert;
        let created = unsafe {
            CreateDirectoryW(
                super::windows_account::wide(directory.to_str().unwrap()).as_ptr(),
                &attributes,
            )
        };
        unsafe {
            LocalFree(descriptor);
        }
        assert_ne!(created, 0, "owned fixture staging directory must be fresh");
        let mut staged = Self {
            path: path.clone(),
            token: token.try_clone().unwrap(),
            image: None,
        };
        // This is an authenticated alternate account, while OwnerSecurity
        // deliberately reads the process token. Supply the held account SID
        // explicitly, and protect the file DACL from the directory's OICI ACE.
        let file_sddl = super::windows_account::wide(&format!("O:{sid}G:{sid}D:P(A;;FA;;;{sid})"));
        let mut file_descriptor = null_mut();
        assert_ne!(
            unsafe {
                ConvertStringSecurityDescriptorToSecurityDescriptorW(
                    file_sddl.as_ptr(),
                    1,
                    &mut file_descriptor,
                    null_mut(),
                )
            },
            0
        );
        struct Descriptor(*mut std::ffi::c_void);
        impl Drop for Descriptor {
            fn drop(&mut self) {
                unsafe {
                    LocalFree(self.0);
                }
            }
        }
        let _descriptor = Descriptor(file_descriptor);
        let file_attributes = SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: file_descriptor,
            bInheritHandle: 0,
        };
        let file_path = super::windows_account::wide(path.to_str().unwrap());
        let mut target = owned(unsafe {
            CreateFileW(
                file_path.as_ptr(),
                GENERIC_READ | GENERIC_WRITE,
                FILE_SHARE_READ,
                &file_attributes,
                CREATE_NEW,
                FILE_ATTRIBUTE_NORMAL | FILE_FLAG_OPEN_REPARSE_POINT,
                null_mut(),
            )
        })
        .expect("owned fixture executable must be created with explicit native security");
        assert_eq!(
            std::io::copy(&mut source, &mut target).unwrap(),
            expected_len
        );
        target.flush().unwrap();
        target.sync_all().unwrap();
        target.seek(SeekFrom::Start(0)).unwrap();
        let mut hash = Sha256::new();
        let mut buffer = [0; 65536];
        loop {
            let n = target.read(&mut buffer).unwrap();
            if n == 0 {
                break;
            }
            hash.update(&buffer[..n]);
        }
        assert_eq!(
            source_hash,
            hash.finalize(),
            "owner fixture must preserve verified executable bytes"
        );
        security::verify_owner_only_handle_for_sid(&target, false, &sid_bytes).unwrap();
        let identity = security::file_identity(&target).unwrap();
        drop(target);
        let mut held = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ)
            .open(&path)
            .unwrap();
        assert_eq!(security::file_identity(&held).unwrap(), identity);
        let mut hash = Sha256::new();
        loop {
            let n = held.read(&mut buffer).unwrap();
            if n == 0 {
                break;
            }
            hash.update(&buffer[..n]);
        }
        assert_eq!(
            source_hash,
            hash.finalize(),
            "held fixture executable bytes must remain the verified source"
        );
        staged.image = Some(held);
        staged
    }
}
impl Drop for StagedFixture {
    fn drop(&mut self) {
        use windows_sys::Win32::Security::{ImpersonateLoggedOnUser, RevertToSelf};
        if unsafe { ImpersonateLoggedOnUser(self.token.as_raw_handle().cast()) } == 0 {
            eprintln!("owned native fixture cleanup cannot impersonate");
            return;
        }
        self.image.take();
        let result = std::fs::remove_file(&self.path)
            .and_then(|_| std::fs::remove_dir(self.path.parent().unwrap()));
        if unsafe { RevertToSelf() } == 0 {
            std::process::abort();
        }
        if result.is_err() {
            eprintln!("owned native fixture cleanup failed");
        }
    }
}
