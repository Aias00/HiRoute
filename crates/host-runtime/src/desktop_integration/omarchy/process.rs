//! Bounded argv execution; never evaluate desktop text as shell commands.
use std::ffi::OsString;
use std::io::{self, Read};
use std::os::{fd::AsFd, unix::process::CommandExt};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub struct Output {
    pub success: bool,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}
const LIMIT: usize = 64 * 1024;

pub fn run(program: &Path, arguments: &[OsString], timeout: Duration) -> io::Result<Output> {
    run_in(program, arguments, timeout, None)
}
pub fn run_in(
    program: &Path,
    arguments: &[OsString],
    timeout: Duration,
    directory: Option<&Path>,
) -> io::Result<Output> {
    use nix::fcntl::{FcntlArg, OFlag, fcntl};
    fn nonblocking(fd: impl AsFd) -> io::Result<()> {
        let flags = fcntl(&fd, FcntlArg::F_GETFL)?;
        fcntl(
            &fd,
            FcntlArg::F_SETFL(OFlag::from_bits_retain(flags) | OFlag::O_NONBLOCK),
        )?;
        Ok(())
    }
    fn drain(reader: &mut impl Read, target: &mut Vec<u8>, total: &mut usize) -> io::Result<()> {
        let mut part = [0; 4096];
        loop {
            match reader.read(&mut part) {
                Ok(0) => return Ok(()),
                Ok(n) => {
                    *total = total
                        .checked_add(n)
                        .ok_or_else(|| io::Error::other("desktop output bound"))?;
                    if *total > LIMIT {
                        return Err(io::Error::other("desktop output bound"));
                    }
                    target.extend_from_slice(&part[..n]);
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => return Ok(()),
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e),
            }
        }
    }
    let start = Instant::now();
    let mut command = Command::new(program);
    if let Some(directory) = directory {
        command.current_dir(directory);
    }
    let mut child = command
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .spawn()?;
    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| io::Error::other("desktop stdout missing"))?;
    let mut stderr = child
        .stderr
        .take()
        .ok_or_else(|| io::Error::other("desktop stderr missing"))?;
    let mut child = hiroute_platform::owned_child::NativeChildScope::new(child);
    let result = (|| {
        nonblocking(&stdout)?;
        nonblocking(&stderr)?;
        let (mut out, mut err, mut total) = (Vec::new(), Vec::new(), 0);
        loop {
            drain(&mut stdout, &mut out, &mut total)?;
            drain(&mut stderr, &mut err, &mut total)?;
            if let Some(success) = child.observe()? {
                drain(&mut stdout, &mut out, &mut total)?;
                drain(&mut stderr, &mut err, &mut total)?;
                return Ok(Output {
                    success,
                    stdout: out,
                    stderr: err,
                });
            }
            if start.elapsed() >= timeout {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "desktop command deadline",
                ));
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    })();
    child.stop()?;
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn omarchy_process_preserves_argv_and_bounds_output_and_lifetime() {
        let args: Vec<OsString> = [
            "-c",
            "printf '%s' \"$1\"",
            "test",
            "literal; $(no-evaluation)",
        ]
        .map(OsString::from)
        .into();
        let result = run(Path::new("/bin/sh"), &args, Duration::from_secs(2)).unwrap();
        assert!(result.success);
        assert_eq!(result.stdout, b"literal; $(no-evaluation)");
        assert!(
            run(
                Path::new("/bin/sh"),
                &["-c".into(), "sleep 30".into()],
                Duration::from_millis(30)
            )
            .is_err()
        );
        assert!(
            run(
                Path::new("/bin/sh"),
                &["-c".into(), "head -c 65537 /dev/zero".into()],
                Duration::from_secs(2)
            )
            .is_err()
        );
    }
}
