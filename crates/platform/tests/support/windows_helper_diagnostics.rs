//! Failure-only observations. Numeric PIDs are never used to stop a process.
use std::ffi::OsString;
use std::mem::{size_of, zeroed};
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::{CloseHandle, FILETIME, INVALID_HANDLE_VALUE};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::System::Threading::{
    GetExitCodeProcess, GetProcessTimes, IsWow64Process2, OpenProcess,
    PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};

pub fn descendants(parent: u32) -> serde_json::Value {
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return serde_json::json!({"snapshot_error":std::io::Error::last_os_error().raw_os_error()});
    }
    let mut entry: PROCESSENTRY32W = unsafe { zeroed() };
    entry.dwSize = size_of::<PROCESSENTRY32W>() as u32;
    let mut processes = Vec::new();
    let mut present = unsafe { Process32FirstW(snapshot, &mut entry) } != 0;
    while present && processes.len() < 8192 {
        processes.push((entry.th32ProcessID, entry.th32ParentProcessID));
        present = unsafe { Process32NextW(snapshot, &mut entry) } != 0;
    }
    unsafe {
        CloseHandle(snapshot);
    }
    let mut selected = vec![parent];
    for _ in 0..16 {
        let before = selected.len();
        for (pid, ppid) in &processes {
            if selected.contains(ppid) && !selected.contains(pid) && selected.len() < 64 {
                selected.push(*pid);
            }
        }
        if selected.len() == before {
            break;
        }
    }
    let observations = selected.into_iter().map(|pid| {
        let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        if handle.is_null() {
            return serde_json::json!({"pid":pid,"open_error":std::io::Error::last_os_error().raw_os_error()});
        }
        let handle = unsafe { OwnedHandle::from_raw_handle(handle) };
        let mut image = vec![0u16; 32768];
        let mut length = image.len() as u32;
        let image = if unsafe { QueryFullProcessImageNameW(handle.as_raw_handle(), 0, image.as_mut_ptr(), &mut length) } != 0 {
            String::from_utf16_lossy(&image[..length as usize])
        } else { String::from("image-query-failed") };
        let mut machine = 0;
        let mut native = 0;
        let architecture = unsafe { IsWow64Process2(handle.as_raw_handle(), &mut machine, &mut native) } != 0;
        let mut creation = FILETIME::default();
        let mut exit = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut user = FILETIME::default();
        let times = unsafe { GetProcessTimes(handle.as_raw_handle(), &mut creation, &mut exit, &mut kernel, &mut user) } != 0;
        let mut code = 0;
        let exit_known = unsafe { GetExitCodeProcess(handle.as_raw_handle(), &mut code) } != 0;
        let value = |time: FILETIME| u64::from(time.dwLowDateTime) | (u64::from(time.dwHighDateTime) << 32);
        serde_json::json!({"pid":pid,"image":image,"architecture_known":architecture,
            "process_machine":machine,"native_machine":native,"times_known":times,
            "creation_100ns":value(creation),"kernel_100ns":value(kernel),"user_100ns":value(user),
            "exit_known":exit_known,"exit_code":code})
    }).collect::<Vec<_>>();
    serde_json::json!(observations)
}

/// This follows a failed cold gate; it never warms or replaces the acceptance
/// command. It may distinguish a system PowerShell startup failure, but a pass
/// does not establish the cause of the earlier failure or repair that evidence.
pub fn post_failure_powershell(environment: &[(OsString, OsString)]) -> serde_json::Value {
    use base64::Engine;
    let powershell = match hiroute_platform::windows::launcher::system_executable(Path::new(
        "WindowsPowerShell/v1.0/powershell.exe",
    )) {
        Ok(path) => path,
        Err(error) => return serde_json::json!({"system_path_error":error.raw_os_error()}),
    };
    let payload: Vec<_> = "exit 0".encode_utf16().flat_map(u16::to_le_bytes).collect();
    let encoded = base64::engine::general_purpose::STANDARD.encode(payload);
    let started = Instant::now();
    let mut child = match Command::new(powershell)
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-EncodedCommand",
            &encoded,
        ])
        .env_clear()
        .envs(environment.iter().cloned())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(child) => child,
        Err(error) => return serde_json::json!({"spawn_error":error.raw_os_error()}),
    };
    let deadline = started + Duration::from_secs(5);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                return serde_json::json!({"exit_code":status.code(),"elapsed_ms":started.elapsed().as_millis()});
            }
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
            observed => {
                let process = descendants(child.id());
                let kill = child.kill().err().and_then(|error| error.raw_os_error());
                let reap_deadline = Instant::now() + Duration::from_secs(2);
                let waited = loop {
                    match child.try_wait() {
                        Ok(Some(_)) => break true,
                        Ok(None) if Instant::now() < reap_deadline => {
                            std::thread::sleep(Duration::from_millis(10))
                        }
                        _ => break false,
                    }
                };
                return serde_json::json!({"timeout_or_wait_error":true,"wait_error":observed.err().and_then(|error| error.raw_os_error()),
                    "elapsed_ms":started.elapsed().as_millis(),"processes":process,"held_child_stop_error":kill,"reaped":waited});
            }
        }
    }
}
