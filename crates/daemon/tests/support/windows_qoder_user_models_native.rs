//! Ordinary fixed Qoder 1.1.65 from the officially prepared account. No auth bytes,
//! Worker route, provider settings override, credential variable or synthetic login.
use super::super::{InstalledProduct, windows_execution_host};
use hiroute_platform::windows::{
    job::{Child, Launch, Stream},
    security,
};
use serde_json::Value;
use std::ffi::OsString;
use std::fs::File;
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};
use zeroize::Zeroizing;

const BOUND: u64 = 4 * 1024 * 1024;
const NATIVE_WAIT: Duration = Duration::from_secs(120);
const NATURAL_TREE_WAIT: Duration = Duration::from_secs(10);

pub struct Contexts {
    pub home: PathBuf,
    pub config: PathBuf,
    pub workspace: PathBuf,
    qoder: PathBuf,
    directories: Vec<(PathBuf, File)>,
    image: File,
    variables: Vec<(OsString, OsString)>,
}

impl Contexts {
    pub fn borrow(product: &InstalledProduct) -> Self {
        assert!(
            std::env::var_os("HIROUTE_WINDOWS_QODER_LOGIN_PAT").is_none(),
            "official PAT must be absent from the business process"
        );
        let selected = |name| {
            let path = std::env::var_os(name)
                .map(PathBuf::from)
                .expect("official private account selectors are required");
            assert!(
                path.is_absolute() && !path.components().any(|part| part == Component::ParentDir),
                "account selectors must be absolute native paths"
            );
            path
        };
        let home = selected("HIROUTE_WINDOWS_QODER_ACCOUNT_HOME");
        let config = selected("HIROUTE_WINDOWS_QODER_ACCOUNT_CONFIG");
        let mut directories = Vec::new();
        for directory in [&home, &config] {
            let held = security::open_owner_only(directory, true)
                .expect("official account root must remain private");
            directories.push((directory.clone(), held));
        }
        let native = security::open_identity(&home.join(".qoder"), true)
            .expect("official Qoder config must be HOME/.qoder");
        assert!(
            security::file_identity(&native).unwrap()
                == security::file_identity(&directories[1].1).unwrap(),
            "account config must be the same actual HOME/.qoder directory"
        );
        let auth = config.join(".auth");
        let held = security::open_identity(&auth, true)
            .expect("official login auth directory is required");
        directories.push((auth.clone(), held));
        for (name, bound) in [("user", 1024 * 1024), ("machine_id", 4096)] {
            let file = security::open_identity(&auth.join(name), false)
                .expect("official auth file must be natively readable");
            let facts = file.metadata().unwrap();
            assert!(
                facts.is_file()
                    && facts.len() > 0
                    && facts.len() <= bound
                    && security::file_link_count(&file).unwrap() == 1,
                "official auth file shape is invalid"
            );
            // No read/hash/clone: release this deny-delete lease before official refresh.
        }
        let qoder = selected("HIROUTE_WORKER_QODER_BINARY");
        let image = security::open_identity(&qoder, false)
            .expect("fixed Qoder must be a held native image");
        windows_execution_host::image(&qoder);
        let discovery_path =
            std::env::var_os("PATH").expect("fixed native discovery PATH is required");
        let discovered = std::env::split_paths(&discovery_path)
            .flat_map(|path| [path.join("qoder.exe"), path.join("qodercli.exe")])
            .find_map(|path| security::open_identity(&path, false).ok())
            .expect("ordinary public scan must find the fixed native image");
        assert!(
            security::file_identity(&discovered).unwrap()
                == security::file_identity(&image).unwrap(),
            "public discovery must resolve the same fixed Qoder object"
        );
        let workspace = product.root.join("ordinary Qoder 用户 models");
        drop(security::create_owner_only_directory(&workspace).unwrap());
        // Daemon's OS HOME remains KnownProfile. Its published Qoder model target is
        // the explicit config selector, so only that selector and fixed PATH are needed.
        let variables = vec![
            ("QODER_CONFIG_DIR".into(), config.as_os_str().to_owned()),
            ("PATH".into(), discovery_path),
        ];
        let context = Self {
            home,
            config,
            workspace,
            qoder,
            directories,
            image,
            variables,
        };
        context.verify();
        context
    }
    pub fn variables(&self) -> &[(OsString, OsString)] {
        &self.variables
    }
    pub fn verify(&self) {
        for (path, held) in &self.directories {
            let current = security::open_identity(path, true)
                .expect("borrowed account directory must retain native identity");
            assert!(
                security::file_identity(&current).unwrap()
                    == security::file_identity(held).unwrap(),
                "borrowed account directory was replaced"
            );
        }
        let current = security::open_identity(&self.qoder, false).unwrap();
        assert!(
            security::file_identity(&current).unwrap()
                == security::file_identity(&self.image).unwrap(),
            "selected native image was replaced"
        );
    }
    fn environment(&self) -> Vec<(OsString, OsString)> {
        let mut result: Vec<_> = std::env::vars_os()
            .filter(|(key, _)| {
                matches!(
                    key.to_string_lossy().to_ascii_uppercase().as_str(),
                    "SYSTEMROOT"
                        | "WINDIR"
                        | "COMSPEC"
                        | "PATHEXT"
                        | "SYSTEMDRIVE"
                        | "APPDATA"
                        | "LOCALAPPDATA"
                        | "PROGRAMDATA"
                        | "PROGRAMFILES"
                        | "PROGRAMFILES(X86)"
                        | "COMMONPROGRAMFILES"
                        | "COMMONPROGRAMFILES(X86)"
                        | "TEMP"
                        | "TMP"
                        | "USERNAME"
                        | "USERDOMAIN"
                        | "COMPUTERNAME"
                )
            })
            .collect();
        result.extend(self.variables.iter().cloned());
        // Exactly the same borrowed selectors used by official preparation. These
        // affect this native process only; they do not redirect daemon KnownFolders.
        result.extend([
            ("HOME".into(), self.home.as_os_str().to_owned()),
            ("USERPROFILE".into(), self.home.as_os_str().to_owned()),
        ]);
        result
    }
    pub fn invoke(&self, selector: &str, prompt: &str, receipt: &str) {
        self.verify();
        let args: Vec<OsString> = [
            OsString::from("--cwd"),
            self.workspace.as_os_str().to_owned(),
            OsString::from("--config-dir"),
            self.config.as_os_str().to_owned(),
        ]
        .into_iter()
        .chain(
            [
                "--model",
                selector,
                "--print",
                "--no-session-persistence",
                "--output-format",
                "stream-json",
                "--tools",
                "Skill,Read,Bash",
                "--allowed-tools",
                "Skill,Bash",
                "--permission-mode",
                "dont_ask",
                "--max-model-request-retries",
                "0",
                "--max-turns",
                "4",
                "-p",
                prompt,
            ]
            .map(OsString::from),
        )
        .collect();
        let output = run(
            "ordinary-model",
            &self.qoder,
            &args,
            &self.environment(),
            &self.workspace,
        );
        terminal(&output, receipt)
            .expect("ordinary fixed Qoder must return one actual successful terminal receipt");
        self.verify();
    }
    pub fn check_collaboration(&self, product: &InstalledProduct) {
        self.verify();
        let args: Vec<_> = [
            "agents",
            "check",
            "agent_qoder_default",
            "--scope",
            "collaboration",
            "--timeout",
            "90s",
            "--output",
            "json",
        ]
        .map(OsString::from)
        .into();
        let output = run(
            "public-collaboration-check",
            &product.stable,
            &args,
            // The public CLI belongs to the installed user's host context.
            // Borrowed Qoder USERPROFILE can redirect KnownFolder LocalAppData
            // in a fresh Windows process and hide the real daemon endpoint.
            &std::env::vars_os().collect::<Vec<_>>(),
            product.stable.parent().unwrap(),
        );
        let envelope: Value = serde_json::from_slice(&output)
            .expect("public collaboration check must return its machine envelope");
        assert!(
            envelope["status"] == "succeeded"
                && envelope["data"]["skill_loading"] == "proven"
                && envelope["data"]["trusted_cli_execution"] == "proven",
            "public Qoder collaboration check requires actual Skill and trusted CLI evidence"
        );
        self.verify();
    }
}

fn run(
    action: &'static str,
    program: &Path,
    args: &[OsString],
    env: &[(OsString, OsString)],
    cwd: &Path,
) -> Zeroizing<Vec<u8>> {
    let mut child = Child::spawn(Launch {
        program,
        args,
        env,
        cwd: Some(cwd),
        stdin: Stream::Null,
        stdout: Stream::Piped,
        stderr: Stream::Piped,
    })
    .expect("fixed native launch failed");
    let capture = |file: File| {
        thread::spawn(move || {
            let mut bytes = Zeroizing::new(Vec::new());
            file.take(BOUND + 1)
                .read_to_end(&mut bytes)
                .map_err(|_| "native-output-read")?;
            if bytes.len() as u64 > BOUND {
                return Err("native-output-bound");
            }
            Ok(bytes)
        })
    };
    let output = capture(child.stdout.take().unwrap());
    let error = capture(child.stderr.take().unwrap());
    let deadline = Instant::now() + NATIVE_WAIT;
    let mut failure = None;
    let exit = loop {
        match child.exit_code() {
            Ok(Some(code)) => break Some(code),
            Ok(None) => {}
            Err(_) => {
                failure = Some("root-query");
                break None;
            }
        }
        if Instant::now() >= deadline {
            failure = Some("root-deadline");
            break None;
        }
        thread::sleep(Duration::from_millis(10));
    };
    let natural_deadline = Instant::now() + NATURAL_TREE_WAIT;
    while failure.is_none() {
        match child.empty() {
            Ok(true) => break,
            Ok(false) => {}
            Err(_) => {
                failure = Some("job-query");
                break;
            }
        }
        if Instant::now() >= natural_deadline {
            failure = Some("descendant-deadline");
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }
    // Cleanup after a failed natural gate is separate evidence. Capture threads
    // are joined only after every owned writer actually exits; a forced stop can
    // never satisfy the successful natural-root/ten-second-descendant contract.
    if failure.is_some() {
        child
            .terminate()
            .expect("owned native failure cleanup failed");
        let cleanup_deadline = Instant::now() + NATURAL_TREE_WAIT;
        while !child.empty().expect("owned native cleanup query failed") {
            assert!(
                Instant::now() < cleanup_deadline,
                "owned native failure cleanup did not finish"
            );
            thread::sleep(Duration::from_millis(10));
        }
    }
    let output = output
        .join()
        .expect("native output collector failed")
        .expect("native output is unreadable or over bound");
    let error = error
        .join()
        .expect("native error collector failed")
        .expect("native error is unreadable or over bound");
    // Native stdout/stderr, account metadata, arguments and paths never appear
    // in diagnostics. Even the error class is selected from a closed vocabulary.
    if failure.is_some() || exit != Some(0) {
        if action == "public-collaboration-check" {
            // Only typed public failure enums leave this capture, never the
            // envelope, request IDs, native stderr or account/provider data.
            let value = serde_json::from_slice::<Value>(&output).unwrap_or(Value::Null);
            let code = serde_json::from_value::<hiroute_application_api::ErrorCode>(
                value["error"]["code"].clone(),
            )
            .ok();
            let reason = serde_json::from_value::<
                hiroute_application_api::AgentCollaborationCheckFailureReasonV1,
            >(value["data"]["reason"].clone())
            .ok();
            eprintln!("Windows ordinary Qoder public failure: code={code:?}, reason={reason:?}");
        }
        eprintln!(
            "Windows ordinary Qoder checkpoint: action={action}, failure={}, exit_code={}, natural_tree={}, stdout_bytes={}, stderr_bytes={}, output_class={}",
            failure.unwrap_or("nonzero-exit"),
            exit.unwrap_or(-1),
            failure.is_none(),
            output.len(),
            error.len(),
            output_class(&output)
        );
        panic!("fixed native/public CLI did not satisfy its bounded natural success contract");
    }
    output
}

fn output_class(bytes: &[u8]) -> &'static str {
    let Ok(text) = std::str::from_utf8(bytes) else {
        return "non-utf8";
    };
    let mut json = false;
    for line in text.lines() {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        json = true;
        if value["type"] == "result" {
            return if value["subtype"] == "success" && value["is_error"] == false {
                "native-success"
            } else {
                "native-error"
            };
        }
        if value["schema_version"].is_object() && value["status"].is_string() {
            return if value["status"] == "succeeded" {
                "machine-success"
            } else {
                "machine-error"
            };
        }
    }
    if json {
        "json-without-terminal"
    } else {
        "no-json"
    }
}

fn terminal(bytes: &[u8], receipt: &str) -> Result<(), &'static str> {
    let text = std::str::from_utf8(bytes).map_err(|_| "native-output-encoding")?;
    let mut results = Vec::new();
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if value["type"] == "result" {
            results.push(value);
        }
    }
    if results.len() != 1 {
        return Err("native-terminal-count");
    }
    let result = &results[0];
    if result["subtype"] != "success"
        || result["is_error"] != false
        || result["result"].as_str().map(str::trim) != Some(receipt)
        || !result["session_id"]
            .as_str()
            .is_some_and(|id| !id.is_empty())
    {
        return Err("native-terminal-receipt");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn account_output_classification_never_reflects_native_error_text() {
        assert!(output_class(b"unclassified private account text") == "no-json");
        assert!(output_class(br#"{"type":"result","subtype":"error","is_error":true,"result":"private account metadata"}"#) == "native-error");
        assert!(output_class(br#"{"schema_version":{"major":1},"status":"failed","error":{"message":"private provider response"}}"#) == "machine-error");
        assert!(output_class(br#"{"account":"private metadata"}"#) == "json-without-terminal");
        assert!(output_class(&[255]) == "non-utf8");
    }
    #[test]
    fn native_terminal_requires_exact_success_receipt_and_one_session() {
        let valid = br#"{"type":"result","subtype":"success","is_error":false,"result":"independent","session_id":"native-session"}"#;
        assert!(terminal(valid, "independent").is_ok());
        assert!(terminal(valid, "other").is_err());
        let duplicate = [valid.as_slice(), b"\n", valid.as_slice()].concat();
        assert!(terminal(&duplicate, "independent").is_err());
        assert!(terminal(br#"{"type":"result","subtype":"error","is_error":true,"result":"independent","session_id":"native-session"}"#, "independent").is_err());
        assert!(terminal(br#"{"type":"result","subtype":"success","is_error":false,"result":"independent","session_id":""}"#, "independent").is_err());
        assert!(terminal(b"unclassified native output", "independent").is_err());
    }
}
