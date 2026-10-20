//! Read-only export of this product's fixed current-user NSIS registration.
use std::{
    io,
    os::windows::process::CommandExt,
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

pub fn export(destination: &Path) -> io::Result<()> {
    super::security::verify_owner_only(
        destination.parent().ok_or(io::ErrorKind::InvalidInput)?,
        true,
    )?;
    if destination.try_exists()? {
        return Err(io::ErrorKind::AlreadyExists.into());
    }
    let mut directory = [0_u16; 32768];
    // SAFETY: native system-directory API and an actual writable UTF-16 buffer.
    let count = unsafe {
        windows_sys::Win32::System::SystemInformation::GetSystemDirectoryW(
            directory.as_mut_ptr(),
            directory.len() as u32,
        )
    } as usize;
    if count == 0 || count >= directory.len() {
        return Err(io::Error::last_os_error());
    }
    let directory = String::from_utf16(&directory[..count]).map_err(io::Error::other)?;
    let mut child = Command::new(Path::new(&directory).join("reg.exe"))
        .args([
            "export",
            r"HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\HiRoute",
        ])
        .arg(destination)
        .arg("/y")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(0x08000000)
        .spawn()?;
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(status) = child.try_wait()? {
            if !status.success() {
                return Err(io::Error::other("SHELL_REGISTRATION_UNAVAILABLE"));
            }
            let file = super::security::open_owner_only(destination, false)?;
            if file.metadata()?.len() > 64 * 1024 {
                return Err(io::ErrorKind::InvalidData.into());
            }
            return Ok(());
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(io::ErrorKind::TimedOut.into());
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}
