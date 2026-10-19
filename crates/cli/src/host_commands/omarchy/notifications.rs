use super::*;
use hiroute_host_runtime::desktop_integration::omarchy::notifications::Notice;
use serde::{Deserialize, Serialize};

#[derive(Serialize)]
pub(super) struct Delivery {
    started: usize,
    not_started: usize,
}
pub(super) fn notify(context: &Context, globals: &Globals) -> Result<Delivery, Failure> {
    let query = reads::Query::new(globals.timeout.unwrap_or(Duration::from_secs(4)))?;
    let ready = reads::service_ready(&query);
    let suggestions = if ready {
        reads::connections(&query)
            .and_then(|c| reads::suggestions(&query, &c))
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    let due = context
        .take_notifications(ready, &suggestions)
        .map_err(host_error)?;
    let mut delivery = Delivery {
        started: 0,
        not_started: 0,
    };
    for notice in due {
        if detach(&context.stable_cli(), notice).is_ok() {
            delivery.started += 1;
        } else {
            delivery.not_started += 1;
        }
    }
    Ok(delivery)
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    notice: Notice,
    zh: bool,
}
fn detach(program: &Path, notice: Notice) -> std::io::Result<()> {
    use std::os::unix::process::CommandExt;
    let locale = std::env::var("LC_ALL")
        .ok()
        .filter(|v| !v.is_empty())
        .or_else(|| std::env::var("LC_MESSAGES").ok())
        .or_else(|| std::env::var("LANG").ok())
        .unwrap_or_default();
    let bytes = serde_json::to_vec(&Input {
        notice,
        zh: locale.starts_with("zh"),
    })
    .map_err(std::io::Error::other)?;
    if bytes.len() > 4096 {
        return Err(std::io::Error::other("notification bound"));
    }
    let mut child = Command::new(program)
        .arg("__omarchy_notification")
        .process_group(0)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    child
        .stdin
        .take()
        .ok_or_else(|| std::io::Error::other("notification input"))?
        .write_all(&bytes)?;
    // The worker owns its bounded notify-send child and lives in a separate group.
    // Reap asynchronously without making the caller wait for a notification action.
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}
fn read_input() -> Result<Vec<u8>, Failure> {
    use nix::fcntl::{FcntlArg, OFlag, fcntl};
    let stdin = std::io::stdin();
    let mut input = stdin.lock();
    let flags = fcntl(&input, FcntlArg::F_GETFL).map_err(|_| invalid())?;
    fcntl(
        &input,
        FcntlArg::F_SETFL(OFlag::from_bits_retain(flags) | OFlag::O_NONBLOCK),
    )
    .map_err(|_| invalid())?;
    let start = Instant::now();
    let mut bytes = Vec::new();
    let mut part = [0; 1024];
    loop {
        match input.read(&mut part) {
            Ok(0) => return Ok(bytes),
            Ok(n) if bytes.len() + n <= 4096 => bytes.extend_from_slice(&part[..n]),
            Ok(_) => return Err(invalid()),
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(5))
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(_) => return Err(invalid()),
        }
        if start.elapsed() > Duration::from_secs(1) {
            return Err(invalid());
        }
    }
}

pub(crate) fn worker() -> CliExecution {
    let result = (|| {
        let bytes = read_input()?;
        if bytes.len() > 4096 {
            return Err(invalid());
        }
        let input: Input = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
        if !input.notice.validate() {
            return Err(invalid());
        }
        let program = host::find_executable("notify-send").ok_or_else(invalid)?;
        let action = if input.zh { "打开" } else { "Open" };
        let arguments: Vec<OsString> = vec![
            "--app-name".into(),
            "HiRoute".into(),
            format!("--action=default={action}").into(),
            format!("--action=open={action}").into(),
            "--wait".into(),
            "--expire-time=30000".into(),
            "--".into(),
            "HiRoute".into(),
            input.notice.body(input.zh).into(),
        ];
        let out = host::process::run(&program, &arguments, Duration::from_secs(35))
            .map_err(|_| invalid())?;
        if out.success
            && matches!(
                std::str::from_utf8(&out.stdout).map(str::trim),
                Ok("default" | "open")
            )
        {
            let context = Context::from_environment(false).map_err(host_error)?;
            open_console(&context, input.notice.landing(), &Globals::default())?;
        }
        Ok::<(), Failure>(())
    })();
    CliExecution {
        exit_code: if result.is_ok() { 0 } else { 1 },
        stdout: String::new(),
        stderr: String::new(),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn omarchy_notify_detaches_without_waiting_for_the_action_process() {
        use std::os::unix::fs::PermissionsExt;
        let temp = tempfile::tempdir().unwrap();
        let program = temp.path().join("waiter");
        std::fs::write(
            &program,
            "#!/bin/sh\ncat >/dev/null\ntouch \"$0.started\"\nsleep 1\ntouch \"$0.finished\"\n",
        )
        .unwrap();
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o700)).unwrap();
        let start = Instant::now();
        detach(&program, Notice::ServiceUnavailable).unwrap();
        let started = program.with_file_name("waiter.started");
        let finished = program.with_file_name("waiter.finished");
        while !started.exists() && start.elapsed() < Duration::from_millis(700) {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(started.exists(), "The fake action waiter never started");
        assert!(
            !finished.exists(),
            "The caller waited for the action process"
        );
        assert!(start.elapsed() < Duration::from_millis(700));
        while !finished.exists() && start.elapsed() < Duration::from_secs(3) {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(
            finished.exists(),
            "The owned fake waiter did not finish before cleanup"
        );
    }
}
