//! Evidence for x64 user-mode execution on a real Windows 11 ARM64 host.
//! An opted-in client gate never infers native architecture from shell variables.
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::os::windows::io::{AsRawHandle, FromRawHandle};
use std::path::Path;

use hiroute_platform::windows::{lifecycle::DaemonProcess, security};
use serde_json::{Value, json};
use windows_sys::Win32::Foundation::{FILETIME, HANDLE};
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, GetProcessInformation, GetProcessTimes, OpenProcess,
    PROCESS_QUERY_LIMITED_INFORMATION, ProcessMachineTypeInfo,
};

pub const VARIABLE: &str = "HIROUTE_WINDOWS_EXECUTION_HOST";
const MODE: &str = "windows11-arm-x64";
const AMD64: u16 = 0x8664;
const ARM64: u16 = 0xaa64;

#[repr(C)]
#[derive(Default)]
struct MachineInformation {
    process_machine: u16,
    reserved: u16,
    attributes: u32,
}

#[repr(C)]
struct VersionInformation {
    size: u32,
    major: u32,
    minor: u32,
    build: u32,
    platform: u32,
    service_pack: [u16; 128],
    service_pack_major: u16,
    service_pack_minor: u16,
    suite: u16,
    product_type: u8,
    reserved: u8,
}

// Win32 PROCESS_MACHINE_INFORMATION and RTL_OSVERSIONINFOEXW ABI boundaries.
const _: () = assert!(std::mem::size_of::<MachineInformation>() == 8);
const _: () = assert!(std::mem::size_of::<VersionInformation>() == 284);

#[link(name = "kernel32")]
unsafe extern "system" {
    fn IsWow64Process2(process: HANDLE, process_machine: *mut u16, native_machine: *mut u16)
    -> i32;
}
#[link(name = "ntdll")]
unsafe extern "system" {
    fn RtlGetVersion(version: *mut VersionInformation) -> i32;
}

pub fn selected() -> bool {
    match std::env::var(VARIABLE) {
        Ok(mode) => {
            assert_eq!(mode, MODE, "unknown execution-host evidence mode");
            true
        }
        Err(std::env::VarError::NotPresent) => false,
        Err(error) => panic!("execution-host evidence mode is invalid: {error}"),
    }
}

pub fn current() -> Option<Value> {
    if !selected() {
        return None;
    }
    assert_eq!(
        std::env::consts::ARCH,
        "x86_64",
        "consumer must execute the compiled x64 gate"
    );
    image(&std::env::current_exe().unwrap());
    // SAFETY: pseudo-handle is valid for the current test process, never closed.
    let evidence = process(unsafe { GetCurrentProcess() });
    println!("actual execution host: {evidence}");
    Some(evidence)
}

pub fn image(path: &Path) {
    if !selected() {
        return;
    }
    let mut held = security::open_identity(path, false).unwrap();
    let mut header = [0_u8; 64];
    held.read_exact(&mut header).unwrap();
    assert_eq!(
        &header[..2],
        b"MZ",
        "candidate executable must be a real PE image"
    );
    let offset = u32::from_le_bytes(header[60..64].try_into().unwrap());
    assert!((64..=16 * 1024 * 1024).contains(&offset));
    held.seek(SeekFrom::Start(u64::from(offset))).unwrap();
    let mut pe = [0_u8; 6];
    held.read_exact(&mut pe).unwrap();
    assert_eq!(&pe[..4], b"PE\0\0");
    assert_eq!(
        u16::from_le_bytes(pe[4..6].try_into().unwrap()),
        AMD64,
        "candidate executable target must be AMD64"
    );
}

pub fn daemon(daemon: &DaemonProcess) -> Option<Value> {
    if !selected() {
        return None;
    }
    image(&daemon.image);
    // The lifecycle client already holds its authenticated process. A second
    // read-only handle is accepted only if birth time matches that held object.
    // It supplies no stop/termination authority and cannot authorize a PID reuse.
    let raw = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, daemon.pid) };
    assert!(
        !raw.is_null(),
        "cannot inspect the retained actual daemon architecture"
    );
    let held = unsafe { File::from_raw_handle(raw.cast()) };
    let mut creation = FILETIME::default();
    let mut exit = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    assert_ne!(
        unsafe { GetProcessTimes(raw, &mut creation, &mut exit, &mut kernel, &mut user) },
        0
    );
    assert_eq!(
        u64::from(creation.dwHighDateTime) << 32 | u64::from(creation.dwLowDateTime),
        daemon.creation_time,
        "architecture query must observe the authenticated daemon birth"
    );
    let evidence = process(held.as_raw_handle().cast());
    assert!(
        !daemon.exited().unwrap(),
        "actual daemon exited during architecture verification"
    );
    Some(evidence)
}

fn process(handle: HANDLE) -> Value {
    // RtlGetVersion reports the actual OS without application-manifest version lies.
    let mut version: VersionInformation = unsafe { std::mem::zeroed() };
    version.size = std::mem::size_of::<VersionInformation>() as u32;
    assert_eq!(unsafe { RtlGetVersion(&mut version) }, 0);
    assert_eq!(
        version.product_type, 1,
        "client gate requires Windows workstation OS"
    );
    assert_eq!(version.major, 10);
    assert!(
        version.build >= 22000,
        "client gate requires actual Windows 11"
    );
    let mut machine = MachineInformation::default();
    assert_ne!(
        unsafe {
            GetProcessInformation(
                handle,
                ProcessMachineTypeInfo,
                (&mut machine as *mut MachineInformation).cast(),
                std::mem::size_of::<MachineInformation>() as u32,
            )
        },
        0,
        "actual process machine query failed: {}",
        std::io::Error::last_os_error()
    );
    assert_eq!(
        machine.process_machine, AMD64,
        "actual execution target must be x64"
    );
    let mut wow_machine = 0;
    let mut native_machine = 0;
    assert_ne!(
        unsafe { IsWow64Process2(handle, &mut wow_machine, &mut native_machine) },
        0
    );
    assert_eq!(native_machine, ARM64, "actual host must be ARM64");
    // x64 emulation does not require the x86 WOW64 layer. Keep the raw value,
    // while using ProcessMachineTypeInfo for the actual x64 process architecture.
    json!({
        "host_os":"Windows 11", "os_version":format!("{}.{}.{}", version.major, version.minor, version.build),
        "host_machine":"ARM64", "process_machine":"AMD64", "execution":"x64_emulation",
        "native_machine":native_machine, "wow_process_machine":wow_machine,
        "process_machine_attributes":machine.attributes, "arm64_native_evidence":false
    })
}
