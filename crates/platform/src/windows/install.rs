//! Installation admission uses the native architecture, including under emulation.
use std::io;
use windows_sys::Win32::System::Threading::{GetCurrentProcess, IsWow64Process2};

pub fn require_x64() -> io::Result<()> {
    let mut process = 0_u16;
    let mut native = 0_u16;
    // SAFETY: current process and two correctly sized IMAGE_FILE_MACHINE outputs.
    if unsafe { IsWow64Process2(GetCurrentProcess(), &mut process, &mut native) } == 0 {
        return Err(io::Error::last_os_error());
    }
    if native != 0x8664 || !cfg!(target_arch = "x86_64") {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "INSTALL_ARCHITECTURE_UNSUPPORTED",
        ));
    }
    Ok(())
}
