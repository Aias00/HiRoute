//! Test-only alternate-account launch. SeImpersonate is available to elevated CI;
//! SeAssignPrimaryToken is not assumed. Jobs/handle allowlists precede execution.
use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io;
use std::mem::{size_of, zeroed};
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle};
use std::path::Path;
use std::ptr::{null, null_mut};
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;
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
fn pipe(input: bool) -> io::Result<(File, File)> {
    let attributes = SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: null_mut(),
        bInheritHandle: 1,
    };
    let (mut read, mut write) = (null_mut(), null_mut());
    if unsafe { CreatePipe(&mut read, &mut write, &attributes, 0) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let (read, write) = (owned(read)?, owned(write)?);
    let (child, parent) = if input { (read, write) } else { (write, read) };
    if unsafe { SetHandleInformation(parent.as_raw_handle().cast(), HANDLE_FLAG_INHERIT, 0) } == 0 {
        return Err(io::Error::last_os_error());
    }
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
struct Attributes(Vec<usize>);
impl Drop for Attributes {
    fn drop(&mut self) {
        unsafe {
            DeleteProcThreadAttributeList(self.0.as_mut_ptr().cast());
        }
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
            environment.extend(key.encode_wide());
            environment.push(b'=' as u16);
            environment.extend(value.encode_wide());
            environment.push(0);
        }
        environment.push(0);
        let (input, stdin) = pipe(true)?;
        let (output, stdout) = pipe(false)?;
        let (error, stderr) = pipe(false)?;
        let job = owned(unsafe { CreateJobObjectW(null(), null()) })?;
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
            return Err(io::Error::last_os_error());
        }
        let mut bytes = 0;
        unsafe {
            InitializeProcThreadAttributeList(null_mut(), 2, 0, &mut bytes);
        }
        assert!(bytes > 0);
        let mut memory = vec![0; bytes.div_ceil(size_of::<usize>())];
        if unsafe {
            InitializeProcThreadAttributeList(memory.as_mut_ptr().cast(), 2, 0, &mut bytes)
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        let mut attributes = Attributes(memory);
        let mut jobs = [job.as_raw_handle()];
        let mut handles = [
            input.as_raw_handle(),
            output.as_raw_handle(),
            error.as_raw_handle(),
        ];
        for (kind, values) in [
            (PROC_THREAD_ATTRIBUTE_JOB_LIST, &mut jobs[..]),
            (PROC_THREAD_ATTRIBUTE_HANDLE_LIST, &mut handles[..]),
        ] {
            if unsafe {
                UpdateProcThreadAttribute(
                    attributes.0.as_mut_ptr().cast(),
                    0,
                    kind as usize,
                    values.as_mut_ptr().cast(),
                    std::mem::size_of_val(values),
                    null_mut(),
                    null_mut(),
                )
            } == 0
            {
                return Err(io::Error::last_os_error());
            }
        }
        let mut desktop = [0_u16];
        let mut startup: STARTUPINFOEXW = unsafe { zeroed() };
        startup.StartupInfo.cb = size_of::<STARTUPINFOEXW>() as u32;
        startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
        startup.StartupInfo.lpDesktop = desktop.as_mut_ptr();
        startup.StartupInfo.hStdInput = input.as_raw_handle().cast();
        startup.StartupInfo.hStdOutput = output.as_raw_handle().cast();
        startup.StartupInfo.hStdError = error.as_raw_handle().cast();
        startup.lpAttributeList = attributes.0.as_mut_ptr().cast();
        let mut information: PROCESS_INFORMATION = unsafe { zeroed() };
        if unsafe {
            CreateProcessWithTokenW(
                token.as_raw_handle().cast(),
                0,
                application.as_ptr(),
                command.as_mut_ptr(),
                EXTENDED_STARTUPINFO_PRESENT | CREATE_UNICODE_ENVIRONMENT | CREATE_NO_WINDOW,
                environment.as_ptr().cast(),
                cwd.as_ptr(),
                &startup.StartupInfo,
                &mut information,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        let process = owned(information.hProcess)?;
        let _thread = owned(information.hThread)?;
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
        let mut value: JOBOBJECT_BASIC_ACCOUNTING_INFORMATION = unsafe { zeroed() };
        if unsafe {
            QueryInformationJobObject(
                self.job.as_raw_handle().cast(),
                JobObjectBasicAccountingInformation,
                (&mut value as *mut JOBOBJECT_BASIC_ACCOUNTING_INFORMATION).cast(),
                size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>() as u32,
                null_mut(),
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(value.ActiveProcesses == 0)
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
        use windows_sys::Win32::Storage::FileSystem::{CreateDirectoryW, FILE_SHARE_READ};
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
        let mut target = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .share_mode(FILE_SHARE_READ)
            .open(&path)
            .unwrap();
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
