//! Test-only mount-point fixture; no cmd.exe parsing or symlink privilege needed.
use std::fs::OpenOptions;
use std::io;
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::io::AsRawHandle;
use std::path::Path;
use windows_sys::Win32::Storage::FileSystem::{FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT};
use windows_sys::Win32::System::IO::DeviceIoControl;

pub fn create(alias: &Path, target: &Path) -> io::Result<()> {
    if !alias.is_absolute() || !target.is_absolute() { return Err(io::ErrorKind::InvalidInput.into()); }
    std::fs::create_dir(alias)?;
    let target = target.to_str().ok_or(io::ErrorKind::InvalidInput)?.replace('/', "\\");
    let target = target.strip_prefix("\\\\?\\").unwrap_or(&target);
    let substitute: Vec<u16> = format!("\\??\\{target}").encode_utf16().collect();
    let print: Vec<u16> = target.encode_utf16().collect();
    let length = 16 + (substitute.len() + print.len() + 2) * 2;
    if length > 16384 { return Err(io::ErrorKind::InvalidInput.into()); }
    let mut aligned = vec![0u32; length.div_ceil(4)];
    // SAFETY: the aligned allocation owns exactly the writable byte range.
    let bytes = unsafe { std::slice::from_raw_parts_mut(aligned.as_mut_ptr().cast::<u8>(), length) };
    // WinNT.h mount point tag and documented REPARSE_DATA_BUFFER offsets.
    bytes[0..4].copy_from_slice(&0xa0000003u32.to_le_bytes());
    bytes[4..6].copy_from_slice(&((length - 8) as u16).to_le_bytes());
    bytes[10..12].copy_from_slice(&((substitute.len() * 2) as u16).to_le_bytes());
    bytes[12..14].copy_from_slice(&(((substitute.len() + 1) * 2) as u16).to_le_bytes());
    bytes[14..16].copy_from_slice(&((print.len() * 2) as u16).to_le_bytes());
    for (index, word) in substitute.iter().copied().chain(Some(0)).chain(print).chain(Some(0)).enumerate() {
        bytes[16 + index * 2..18 + index * 2].copy_from_slice(&word.to_le_bytes());
    }
    let file = OpenOptions::new().write(true)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT).open(alias)?;
    let mut returned = 0;
    // SAFETY: FSCTL_SET_REPARSE_POINT consumes a complete, bounded mount-point
    // buffer on the newly created owned fixture directory; output is unused.
    if unsafe { DeviceIoControl(file.as_raw_handle().cast(), 0x000900a4,
        aligned.as_ptr().cast(), length as u32, std::ptr::null_mut(), 0,
        &mut returned, std::ptr::null_mut()) } == 0 { return Err(io::Error::last_os_error()); }
    Ok(())
}
