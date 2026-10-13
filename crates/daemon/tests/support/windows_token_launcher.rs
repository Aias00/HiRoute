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
                null(),
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
