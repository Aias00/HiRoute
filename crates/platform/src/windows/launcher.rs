//! Token-free descriptor for the Codex adapter's early app-server startup.
//! The selected native object is verified before the exact managed argv is applied.
use super::{
    job::{Child, Launch, Stream},
    security,
};
use std::{
    ffi::OsString,
    io::{self, Read},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

pub const DESCRIPTOR_ENV: &str = "HIROUTE_NATIVE_LAUNCH_DESCRIPTOR";
const LIMIT: u64 = 64 * 1024;

pub fn descriptor(binary: &Path, prefix: &[PathBuf], overrides: &[String]) -> io::Result<Vec<u8>> {
    let file = security::open_identity(binary, false)?;
    let (volume, index) = security::file_identity(&file)?;
    let mut entries = Vec::new();
    for path in prefix {
        if !path.is_absolute() { return Err(io::Error::other("launcher entry")); }
        let file = security::open_identity(path, false)?;
        let (volume, index) = security::file_identity(&file)?;
        entries.push(serde_json::json!({"path":path,"volume":volume,"index":index}));
    }
    let value = serde_json::json!({
        "schema": "hiroute.native-launch/v1", "binary": binary,
        "volume": volume, "index": index, "prefix": entries, "overrides": overrides,
    });
    let bytes = serde_json::to_vec(&value).map_err(io::Error::other)?;
    if bytes.len() as u64 > LIMIT {
        return Err(io::Error::other("descriptor bound"));
    }
    Ok(bytes)
}

pub fn run_from_environment() -> io::Result<i32> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args != [OsString::from("app-server")] {
        return Err(io::Error::other("launcher arguments"));
    }
    let path = PathBuf::from(
        std::env::var_os(DESCRIPTOR_ENV).ok_or_else(|| io::Error::other("descriptor absent"))?,
    );
    let mut descriptor = security::open_owner_only(&path, false)?;
    if descriptor.metadata()?.len() > LIMIT {
        return Err(io::Error::other("descriptor bound"));
    }
    let mut bytes = Vec::new();
    (&mut descriptor).take(LIMIT + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > LIMIT {
        return Err(io::Error::other("descriptor bound"));
    }
    let value: serde_json::Value = serde_json::from_slice(&bytes).map_err(io::Error::other)?;
    let invalid = || io::Error::other("invalid launch descriptor");
    let object = value.as_object().ok_or_else(invalid)?;
    if object.len() != 6 || value["schema"] != "hiroute.native-launch/v1" {
        return Err(invalid());
    }
    let binary = PathBuf::from(value["binary"].as_str().ok_or_else(invalid)?);
    let identity = (
        value["volume"]
            .as_u64()
            .and_then(|value| u32::try_from(value).ok())
            .ok_or_else(invalid)?,
        value["index"].as_u64().ok_or_else(invalid)?,
    );
    // Keep the native file object open through CreateProcess to block replacement.
    let native = security::open_identity(&binary, false)?;
    if security::file_identity(&native)? != identity {
        return Err(invalid());
    }
    let mut argv = Vec::new();
    let mut entries = Vec::new();
    for entry in value["prefix"].as_array().ok_or_else(invalid)? {
        if entry.as_object().is_none_or(|entry| entry.len() != 3) { return Err(invalid()); }
        let path = PathBuf::from(entry["path"].as_str().ok_or_else(invalid)?);
        let identity = (entry["volume"].as_u64().and_then(|value| u32::try_from(value).ok()).ok_or_else(invalid)?, entry["index"].as_u64().ok_or_else(invalid)?);
        let file = security::open_identity(&path, false)?;
        if security::file_identity(&file)? != identity { return Err(invalid()); }
        entries.push(file);
        argv.push(path.into_os_string());
    }
    for override_value in value["overrides"].as_array().ok_or_else(invalid)? {
        let argument = override_value
            .as_str()
            .filter(|value| !value.contains('\0'))
            .ok_or_else(invalid)?;
        argv.extend([OsString::from("-c"), OsString::from(argument)]);
    }
    argv.push(OsString::from("app-server"));
    // Only the controlled parent supplied this environment. Do not re-read secrets from files.
    let environment: Vec<_> = std::env::vars_os()
        .filter(|(key, _)| {
            !key.as_encoded_bytes()
                .eq_ignore_ascii_case(DESCRIPTOR_ENV.as_bytes())
        })
        .collect();
    let cwd = std::env::current_dir()?;
    let child = Child::spawn(Launch {
        program: &binary,
        args: &argv,
        env: &environment,
        cwd: Some(&cwd),
        stdin: Stream::Inherit,
        stdout: Stream::Inherit,
        stderr: Stream::Inherit,
    })?;
    let code = loop {
        if let Some(code) = child.exit_code()? {
            break code;
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    child.terminate()?;
    let deadline = Instant::now() + Duration::from_secs(3);
    while !child.empty()? {
        if Instant::now() >= deadline {
            return Err(io::Error::other("launcher scope remains"));
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    Ok(code)
}
