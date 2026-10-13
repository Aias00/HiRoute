//! Windows native execution uses argv and held Jobs, never evaluates npm .cmd text.
use hiroute_platform::windows::job::{Child, Launch, Stream};
use std::{
    ffi::OsString,
    io,
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant},
};

pub fn windows_agent_command(
    program: &Path,
    args: &[PathBuf],
    search: &str,
) -> io::Result<(PathBuf, Vec<PathBuf>)> {
    if program
        .extension()
        .is_some_and(|ext| ext.as_encoded_bytes().eq_ignore_ascii_case(b"exe"))
    {
        return Ok((program.to_owned(), args.to_vec()));
    }
    let entry = if program
        .extension()
        .is_some_and(|ext| ext.as_encoded_bytes().eq_ignore_ascii_case(b"cmd"))
    {
        windows_npm_entry(program)?
    } else {
        program.to_owned()
    };
    let node = node_path(Some(std::ffi::OsStr::new(search)), &entry)?;
    let mut args = args.to_vec();
    args.insert(0, entry);
    Ok((node, args))
}

pub fn windows_claude_version(
    binary: &Path,
    search_path: &str,
    timeout: Duration,
) -> io::Result<Vec<u8>> {
    let home = tempfile::Builder::new()
        .prefix("hiroute-claude-version-")
        .tempdir()?;
    hiroute_platform::windows::security::set_owner_only(home.path(), true)?;
    let mut command = Command::new(binary);
    command
        .arg("--version")
        .env_clear()
        .env("HOME", home.path())
        .env("USERPROFILE", home.path())
        .env("PATH", search_path)
        .env("CLAUDE_CONFIG_DIR", home.path().join(".claude"))
        .current_dir(home.path());
    std::fs::create_dir(home.path().join(".claude"))?;
    let result = bounded(&command, false, timeout, 4096);
    if result.as_ref().is_err_and(cleanup_unknown) {
        let _ = home.keep();
    }
    result.map(|bytes| bytes.to_vec())
}

/// Resolve only a declared bin from a known npm producer next to its generated shim.
pub fn windows_npm_entry(shim: &Path) -> io::Result<PathBuf> {
    let name = shim
        .file_stem()
        .and_then(|name| name.to_str())
        .ok_or_else(|| io::Error::other("npm entry"))?;
    let package = match name.to_ascii_lowercase().as_str() {
        "codex" => "@openai/codex",
        "claude" => "@anthropic-ai/claude-code",
        "codex-acp" => "@agentclientprotocol/codex-acp",
        "claude-agent-acp" => "@agentclientprotocol/claude-agent-acp",
        "pi" => "@earendil-works/pi-coding-agent",
        "dsh" => "@deepseek-ai/dsh",
        "qoder" | "qodercli" => "@qoder-ai/qodercli",
        _ => return Err(io::Error::other("unknown npm producer")),
    };
    let root = shim
        .parent()
        .ok_or_else(|| io::Error::other("npm root"))?
        .join("node_modules")
        .join(package);
    let manifest = root.join("package.json");
    if std::fs::metadata(&manifest)?.len() > 64 * 1024 {
        return Err(io::Error::other("npm manifest bound"));
    }
    let value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(manifest)?).map_err(io::Error::other)?;
    if value["name"] != package {
        return Err(io::Error::other("npm producer mismatch"));
    }
    let bin = value["bin"]
        .as_str()
        .or_else(|| value["bin"].get(name).and_then(serde_json::Value::as_str))
        .ok_or_else(|| io::Error::other("npm bin"))?;
    let root = std::fs::canonicalize(root)?;
    let entry = std::fs::canonicalize(root.join(bin))?;
    if !entry.starts_with(&root) || !entry.is_file() {
        return Err(io::Error::other("npm bin identity"));
    }
    Ok(entry)
}

pub(super) fn node_path(search: Option<&std::ffi::OsStr>, entry: &Path) -> io::Result<PathBuf> {
    let mut directories = vec![entry.parent().unwrap_or(entry).to_owned()];
    if let Some(search) = search {
        directories.extend(std::env::split_paths(search).filter(|path| path.is_absolute()));
    }
    for directory in directories {
        let node = directory.join("node.exe");
        if node.is_file() {
            return std::fs::canonicalize(node);
        }
    }
    Err(io::Error::other("selected command Node unavailable"))
}

pub(super) fn spawn(
    command: &Command,
    inherit: bool,
    input: Stream<'_>,
    output: Stream<'_>,
    error: Stream<'_>,
) -> io::Result<Child> {
    let selected = Path::new(command.get_program());
    let mut args: Vec<_> = command.get_args().map(ToOwned::to_owned).collect();
    let mut env: std::collections::BTreeMap<OsString, OsString> = if inherit {
        std::env::vars_os().collect()
    } else {
        Default::default()
    };
    for (key, value) in command.get_envs() {
        // Windows logical keys are case-insensitive, including absent protected keys.
        env.retain(|name, _| {
            !name
                .as_encoded_bytes()
                .eq_ignore_ascii_case(key.as_encoded_bytes())
        });
        if let Some(value) = value {
            env.insert(key.to_owned(), value.to_owned());
        }
    }
    for key in ["SystemRoot", "WINDIR", "COMSPEC", "PATHEXT"] {
        if !env
            .keys()
            .any(|name| name.as_encoded_bytes().eq_ignore_ascii_case(key.as_bytes()))
        {
            if let Some(value) = std::env::var_os(key) {
                env.insert(key.into(), value);
            }
        }
    }
    let value = |name: &str, env: &std::collections::BTreeMap<OsString, OsString>| {
        env.iter()
            .find(|(key, _)| key.as_encoded_bytes().eq_ignore_ascii_case(name.as_bytes()))
            .map(|(_, value)| value.clone())
    };
    if let Some(home) = value("HOME", &env) {
        env.retain(|key, _| !key.as_encoded_bytes().eq_ignore_ascii_case(b"USERPROFILE"));
        env.insert("USERPROFILE".into(), home.clone());
        for (key, suffix) in [
            ("APPDATA", "AppData/Roaming"),
            ("LOCALAPPDATA", "AppData/Local"),
        ] {
            if value(key, &env).is_none() {
                env.insert(
                    key.into(),
                    PathBuf::from(&home).join(suffix).into_os_string(),
                );
            }
        }
    }
    let program = if selected
        .extension()
        .is_some_and(|ext| ext.as_encoded_bytes().eq_ignore_ascii_case(b"exe"))
    {
        selected.to_owned()
    } else {
        let entry = if selected
            .extension()
            .is_some_and(|ext| ext.as_encoded_bytes().eq_ignore_ascii_case(b"cmd"))
        {
            windows_npm_entry(selected)?
        } else {
            selected.to_owned()
        };
        let search = value("PATH", &env);
        let node = node_path(search.as_deref(), &entry)?;
        args.insert(0, entry.into_os_string());
        node
    };
    let env = env.into_iter().collect::<Vec<_>>();
    Child::spawn(Launch {
        program: &program,
        args: &args,
        env: &env,
        cwd: command.get_current_dir(),
        stdin: input,
        stdout: output,
        stderr: error,
    })
}

/// Capture to a private regular file so no synchronous pipe read can defeat the deadline.
pub(super) fn bounded(
    command: &Command,
    inherit: bool,
    timeout: Duration,
    limit: usize,
) -> io::Result<zeroize::Zeroizing<Vec<u8>>> {
    let capture = tempfile::NamedTempFile::new()?;
    hiroute_platform::windows::security::set_owner_only(capture.path(), false)?;
    let writer = capture.reopen()?;
    let child = spawn(
        command,
        inherit,
        Stream::Null,
        Stream::File(&writer),
        Stream::Null,
    )
    .map_err(|error| io::Error::other(LaunchFailure(error.raw_os_error())))?;
    let start = Instant::now();
    let result = (|| {
        loop {
            if writer.metadata()?.len() > limit as u64 {
                break Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "native output bound",
                ));
            }
            if let Some(code) = child.exit_code()? {
                break if code == 0 {
                    super::native_probe_process::read_bounded(capture.path(), limit)
                } else {
                    Err(io::Error::other("native check failed"))
                };
            }
            if start.elapsed() >= timeout {
                break Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "native check timeout",
                ));
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    })();
    let cleanup = (|| {
        child.terminate()?;
        let stop = Instant::now();
        while !child.empty()? {
            if stop.elapsed() >= Duration::from_secs(1) {
                return Err(io::Error::other("native cleanup failed"));
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        Ok(())
    })();
    if cleanup.is_err() {
        let _ = capture.keep();
        return Err(io::Error::other(CleanupUnknown));
    }
    result
}

#[derive(Debug, thiserror::Error)]
#[error("native process did not start")]
struct LaunchFailure(Option<i32>);

pub(super) fn launch_failure_code(error: &io::Error) -> Option<Option<i32>> {
    error
        .get_ref()
        .and_then(|source| source.downcast_ref::<LaunchFailure>())
        .map(|failure| failure.0)
}

#[derive(Debug)]
struct CleanupUnknown;
impl std::fmt::Display for CleanupUnknown {
    fn fmt(&self, output: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        output.write_str("native cleanup failed")
    }
}
impl std::error::Error for CleanupUnknown {}
pub(super) fn cleanup_unknown(error: &io::Error) -> bool {
    error
        .get_ref()
        .is_some_and(|error| error.is::<CleanupUnknown>())
}
