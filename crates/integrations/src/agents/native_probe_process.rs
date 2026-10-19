//! Narrow ownership for a synchronous native check and its read-only CLI baseline.
//! Exit observation retains the group leader until all mutating group signals are finished.
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;
#[cfg(all(test, unix))]
use std::time::{Duration, Instant};

pub use hiroute_platform::owned_child::NativeChildScope as NativeProbeProcess;

/// Rechecks the limit during the final read, including children that write and exit between polls.
pub(super) fn read_bounded(path: &Path, limit: usize) -> io::Result<zeroize::Zeroizing<Vec<u8>>> {
    let mut bytes = zeroize::Zeroizing::new(Vec::new());
    std::fs::File::open(path)?
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        return Err(io::Error::other("native output bound"));
    }
    Ok(bytes)
}

/// Read an owned capture only after every writer in its process scope has stopped.
/// The duplicate retains the original file object, including exclusive Windows
/// sharing. Its seek pointer is shared, so this must never inspect a live writer.
pub(super) fn read_bounded_file(
    file: &File,
    limit: usize,
) -> io::Result<zeroize::Zeroizing<Vec<u8>>> {
    let cap = u64::try_from(limit)
        .ok()
        .and_then(|limit| limit.checked_add(1))
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "native output limit"))?;
    let mut file = file.try_clone()?;
    file.seek(SeekFrom::Start(0))?;
    let mut bytes = zeroize::Zeroizing::new(Vec::new());
    file.take(cap).read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        return Err(io::Error::other("native output bound"));
    }
    Ok(bytes)
}

#[cfg(test)]
mod output_tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn retained_capture_starts_at_zero_and_rechecks_the_exact_final_bound() {
        let root = super::super::probe_files::temporary("hiroute-held-output-").unwrap();
        let mut capture = super::super::probe_files::file(&root.path().join("capture")).unwrap();
        assert!(read_bounded_file(&capture, 0).unwrap().is_empty());
        capture.write_all(b"fixed capture").unwrap();
        capture.flush().unwrap();
        let length = b"fixed capture".len();
        assert_eq!(
            read_bounded_file(&capture, length).unwrap().as_slice(),
            b"fixed capture"
        );
        assert_eq!(
            read_bounded_file(&capture, length - 1).unwrap_err().kind(),
            io::ErrorKind::Other
        );
        assert_eq!(
            read_bounded_file(&capture, 0).unwrap_err().kind(),
            io::ErrorKind::Other
        );
        assert_eq!(
            read_bounded_file(&capture, length).unwrap().as_slice(),
            b"fixed capture"
        );
        if usize::BITS == 64 {
            assert_eq!(
                read_bounded_file(&capture, usize::MAX).unwrap_err().kind(),
                io::ErrorKind::InvalidInput
            );
        }
    }

    #[test]
    fn retained_capture_preserves_actual_non_limit_read_errors() {
        let root = super::super::probe_files::temporary("hiroute-write-only-output-").unwrap();
        let path = root.path().join("write-only");
        let mut capture = File::options()
            .create_new(true)
            .write(true)
            .open(path)
            .unwrap();
        capture.write_all(b"fixed").unwrap();
        capture.flush().unwrap();
        let mut direct = capture.try_clone().unwrap();
        direct.seek(SeekFrom::Start(0)).unwrap();
        let expected = direct.read(&mut [0; 1]).unwrap_err();
        let error = read_bounded_file(&capture, 128).unwrap_err();
        assert_eq!(error.kind(), expected.kind());
        assert_eq!(error.raw_os_error(), expected.raw_os_error());
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::process::CommandExt;
    use std::process::{Command, Stdio};

    fn await_exit(child: &NativeProbeProcess) {
        let start = Instant::now();
        loop {
            if child.observe().unwrap().is_some() {
                return;
            }
            assert!(start.elapsed() < Duration::from_secs(2));
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn successful_probe_exit_remains_waitable_until_group_stop() {
        let child = Command::new("/bin/sh")
            .args(["-c", "exit 0"])
            .process_group(0)
            .spawn()
            .unwrap();
        let mut child = NativeProbeProcess::new(child);
        await_exit(&child);
        assert_eq!(child.observe().unwrap(), Some(true));
        // A second WNOWAIT observation proves the first poll did not reap the leader.
        assert_eq!(child.observe().unwrap(), Some(true));
        child.stop().unwrap();
        assert!(child.observe().is_err());
    }

    #[test]
    fn leader_exit_does_not_abandon_term_ignoring_tool() {
        let root = tempfile::tempdir().unwrap();
        let pidfile = root.path().join("tool.pid");
        let child = Command::new("/bin/sh")
            .args([
                "-c",
                "trap '' TERM; sleep 90 & echo $! > \"$1\"; exit 0",
                "probe",
            ])
            .arg(&pidfile)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(0)
            .spawn()
            .unwrap();
        let mut child = NativeProbeProcess::new(child);
        await_exit(&child);
        let tool = std::fs::read_to_string(pidfile).unwrap();
        let _ = child.stop(); // An orphan zombie may conservatively leave scope unknown.
        let state = Command::new("/bin/ps")
            .args(["-p", tool.trim(), "-o", "stat="])
            .output()
            .unwrap();
        let state = String::from_utf8_lossy(&state.stdout);
        assert!(
            state.trim().is_empty() || state.trim().starts_with('Z'),
            "owned tool remains active: {state}"
        );
    }

    #[test]
    fn fast_exit_output_cannot_bypass_final_read_bound() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("out");
        std::fs::write(&path, vec![b'x'; 1025]).unwrap();
        assert!(read_bounded(&path, 1024).is_err());
        assert_eq!(read_bounded(&path, 1025).unwrap().len(), 1025);
    }
}
