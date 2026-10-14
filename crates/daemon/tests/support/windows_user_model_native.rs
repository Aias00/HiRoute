//! Ordinary pinned Pi / DSH invocations. No Worker profile, provider settings or
//! credential override enters argv/environment; selectors refer to saved models.
use super::{InstalledProduct, assert_no_secret, security};
use hiroute_platform::windows::job::{Child, Launch, Stream};
use serde_json::{Value, json};
use std::ffi::OsString;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const BOUND: u64 = 4 * 1024 * 1024;
const NATIVE_BOUND: Duration = Duration::from_secs(120);

pub struct Contexts {
    pub pi: PathBuf,
    pub dsh: PathBuf,
    pub workspace: PathBuf,
    node: PathBuf,
    pi_cli: PathBuf,
    dsh_cli: PathBuf,
    variables: Vec<(OsString, OsString)>,
}
impl Contexts {
    pub fn prepare(product: &InstalledProduct) -> Self {
        let node = selected("HIROUTE_WORKER_NODE");
        super::super::windows_execution_host::image(&node);
        let pi_cli = selected("HIROUTE_WORKER_PI_BINARY");
        let dsh_cli = selected("HIROUTE_WORKER_DSH_BINARY");
        let mut directories = vec![node.parent().unwrap().to_owned()];
        for (entry, package, name) in [
            (&pi_cli, "@earendil-works/pi-coding-agent", "pi"),
            (&dsh_cli, "@deepseek-ai/dsh", "dsh"),
        ] {
            directories.push(declared_shim_directory(entry, package, name));
        }
        if let Some(path) = std::env::var_os("PATH") {
            directories.extend(std::env::split_paths(&path));
        }
        let root = product.root.join("ordinary native 用户 models");
        let pi = root.join("pi");
        let dsh = root.join("dsh");
        let workspace = root.join("workspace");
        for path in [&root, &pi, &dsh, &workspace] {
            security::create_owner_only_directory(path).unwrap();
        }
        // These are empty test-owned directories; only the production Agent
        // settings transaction may create or edit native configuration files.
        let profiles = dsh.join("profiles");
        let web = profiles.join("web");
        for path in [&profiles, &web] {
            security::create_owner_only_directory(path).unwrap();
        }
        let variables = vec![
            ("PI_CODING_AGENT_DIR".into(), pi.as_os_str().to_owned()),
            ("DSH_HOME".into(), dsh.as_os_str().to_owned()),
            ("PATH".into(), std::env::join_paths(directories).unwrap()),
        ];
        Self {
            pi,
            dsh,
            workspace,
            node,
            pi_cli,
            dsh_cli,
            variables,
        }
    }
    pub fn variables(&self) -> &[(OsString, OsString)] {
        &self.variables
    }
    fn environment(&self) -> Vec<(OsString, OsString)> {
        let mut environment: Vec<_> = std::env::vars_os()
            .filter(|(name, _)| {
                let name = name.to_string_lossy().to_ascii_uppercase();
                // Native clients and their user tools receive only OS/profile
                // selectors. In particular, no CI token, provider credentials,
                // proxy override or arbitrary inherited SDK setting is admitted.
                matches!(
                    name.as_str(),
                    "SYSTEMROOT"
                        | "WINDIR"
                        | "COMSPEC"
                        | "PATHEXT"
                        | "SYSTEMDRIVE"
                        | "USERPROFILE"
                        | "HOME"
                        | "HOMEDRIVE"
                        | "HOMEPATH"
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
        for (name, value) in &self.variables {
            environment.retain(|(key, _)| {
                !key.as_encoded_bytes()
                    .eq_ignore_ascii_case(name.as_encoded_bytes())
            });
            environment.push((name.clone(), value.clone()));
        }
        environment
    }
    pub fn invoke(&self, pi: bool, provider: &str, alias: &str, prompt: &str, receipt: &str) {
        eprintln!(
            "Windows ordinary native checkpoint: harness={}, stage=invoke",
            if pi { "pi" } else { "dsh" }
        );
        if pi {
            let args: Vec<OsString> = [self.pi_cli.as_os_str().to_owned()]
                .into_iter()
                .chain(
                    [
                        "--provider",
                        provider,
                        "--model",
                        alias,
                        "--mode",
                        "json",
                        "--print",
                        "--no-session",
                        "--no-extensions",
                        "--offline",
                        "--tools",
                        "read,bash",
                        prompt,
                    ]
                    .map(OsString::from),
                )
                .collect();
            let output = run(
                &self.node,
                &args,
                &self.environment(),
                &self.workspace,
                None,
                NATIVE_BOUND,
            )
            .1;
            let rows: Vec<Value> = std::str::from_utf8(&output)
                .unwrap()
                .lines()
                .filter(|line| line.starts_with('{'))
                .map(|line| serde_json::from_str(line).unwrap())
                .collect();
            assert_eq!(
                rows.iter().filter(|row| row["type"] == "session").count(),
                1
            );
            let endings: Vec<_> = rows
                .iter()
                .filter(|row| row["type"] == "agent_end")
                .collect();
            assert_eq!(
                endings.len(),
                1,
                "ordinary Pi must publish one actual terminal"
            );
            let last = endings[0]["messages"].as_array().unwrap().last().unwrap();
            assert_eq!(last["role"], "assistant");
            assert_eq!(last["stopReason"], "stop");
            assert!(
                text(&last["content"]).trim() == receipt,
                "ordinary Pi lost the independent upstream receipt"
            );
        } else {
            let args = vec![
                self.dsh_cli.as_os_str().to_owned(),
                "--profile".into(),
                "acp".into(),
                "--patch".into(),
                self.dsh
                    .join("profiles/web/cordis.patch.yml")
                    .into_os_string(),
            ];
            let mut acp = Acp::spawn(&self.node, &args, &self.environment(), &self.workspace);
            let initialized = acp.request(
                "initialize",
                json!({"protocolVersion":1,"clientCapabilities":{},
                "clientInfo":{"name":"hiroute-native-acceptance","version":"1"}}),
            );
            assert_eq!(initialized["protocolVersion"], 1);
            let session = acp.request("session/new", json!({"cwd":self.workspace,"mcpServers":[]}));
            let id = session["sessionId"].as_str().unwrap().to_owned();
            let option = session["configOptions"]
                .as_array()
                .unwrap()
                .iter()
                .find(|option| option["category"] == "model" || option["id"] == "model")
                .unwrap();
            let selected = serde_json::to_string(&[provider, alias]).unwrap();
            let present = option["options"].as_array().unwrap().iter().any(|option| {
                option["value"] == selected
                    || option["options"]
                        .as_array()
                        .is_some_and(|group| group.iter().any(|option| option["value"] == selected))
            });
            assert!(
                present,
                "ordinary DSH must advertise the exact saved provider/alias"
            );
            acp.request(
                "session/set_config_option",
                json!({"sessionId":id,"configId":option["id"],"value":selected}),
            );
            acp.active = Some(id.clone());
            let result = acp.request(
                "session/prompt",
                json!({"sessionId":id,"prompt":[{"type":"text","text":prompt}]}),
            );
            assert_eq!(result["stopReason"], "end_turn");
            assert!(
                acp.text.trim() == receipt,
                "ordinary DSH lost the independent upstream receipt"
            );
            acp.stop();
        }
    }
}
fn selected(name: &str) -> PathBuf {
    let path = PathBuf::from(std::env::var_os(name).expect("required fixed native artifact input"));
    assert!(path.is_absolute());
    security::open_identity(&path, false).expect("selected native artifact must be reparse-free");
    path
}
fn declared_shim_directory(entry: &Path, package: &str, name: &str) -> PathBuf {
    let package_root = entry
        .ancestors()
        .take(6)
        .find(|root| {
            let path = root.join("package.json");
            path.is_file()
                && serde_json::from_slice::<Value>(&std::fs::read(path).unwrap())
                    .is_ok_and(|value| value["name"] == package)
        })
        .expect("fixed entry must belong to its actual npm producer");
    let manifest: Value =
        serde_json::from_slice(&std::fs::read(package_root.join("package.json")).unwrap()).unwrap();
    let declared = manifest["bin"]
        .as_str()
        .or_else(|| manifest["bin"][name].as_str())
        .unwrap();
    let actual = security::open_identity(entry, false).unwrap();
    let declared = security::open_identity(&package_root.join(declared), false).unwrap();
    assert_eq!(
        security::file_identity(&actual).unwrap(),
        security::file_identity(&declared).unwrap()
    );
    let modules = package_root
        .ancestors()
        .find(|path| path.file_name().is_some_and(|name| name == "node_modules"))
        .unwrap();
    let directory = modules.join(".bin");
    security::open_identity(&directory.join(format!("{name}.cmd")), false).expect(
        "npm ci must provide the actual known-producer discovery shim; it is never executed",
    );
    directory
}
pub fn run(
    program: &Path,
    args: &[OsString],
    env: &[(OsString, OsString)],
    cwd: &Path,
    input: Option<&[u8]>,
    bound: Duration,
) -> (i32, Vec<u8>) {
    let mut child = Child::spawn(Launch {
        program,
        args,
        env,
        cwd: Some(cwd),
        stdin: if input.is_some() {
            Stream::Piped
        } else {
            Stream::Null
        },
        stdout: Stream::Piped,
        stderr: Stream::Piped,
    })
    .unwrap();
    let capture = |file: std::fs::File| {
        thread::spawn(move || {
            let mut bytes = Vec::new();
            file.take(BOUND + 1).read_to_end(&mut bytes).unwrap();
            assert!(bytes.len() as u64 <= BOUND);
            assert_no_secret(&bytes);
            bytes
        })
    };
    let output = capture(child.stdout.take().unwrap());
    let error = capture(child.stderr.take().unwrap());
    if let Some(input) = input {
        child.stdin.take().unwrap().write_all(input).unwrap();
    }
    let deadline = Instant::now() + bound;
    let exit = wait(&mut child, deadline);
    let output = output.join().unwrap();
    let _error = error.join().unwrap();
    assert_eq!(exit, 0, "actual native/public CLI process must succeed");
    (exit, output)
}
fn wait(child: &mut Child, deadline: Instant) -> i32 {
    let exit = loop {
        if let Some(exit) = child.exit_code().unwrap() {
            break exit;
        }
        if Instant::now() >= deadline {
            child.terminate().unwrap();
            panic!("owned native process exceeded its fixed deadline");
        }
        thread::sleep(Duration::from_millis(10));
    };
    while !child.empty().unwrap() {
        assert!(
            Instant::now() < deadline,
            "actual native descendant cleanup must complete"
        );
        thread::sleep(Duration::from_millis(10));
    }
    exit
}
struct Acp {
    child: Child,
    messages: Receiver<Value>,
    reader: Option<JoinHandle<()>>,
    error: Option<JoinHandle<()>>,
    deadline: Instant,
    id: u64,
    active: Option<String>,
    text: String,
}
impl Acp {
    fn spawn(program: &Path, args: &[OsString], env: &[(OsString, OsString)], cwd: &Path) -> Self {
        let mut child = Child::spawn(Launch {
            program,
            args,
            env,
            cwd: Some(cwd),
            stdin: Stream::Piped,
            stdout: Stream::Piped,
            stderr: Stream::Piped,
        })
        .unwrap();
        let (send, messages) = mpsc::channel();
        let output = child.stdout.take().unwrap();
        let reader = thread::spawn(move || {
            let mut reader = BufReader::new(output);
            let mut total = 0usize;
            loop {
                let mut line = Vec::new();
                if reader
                    .by_ref()
                    .take(BOUND + 1)
                    .read_until(b'\n', &mut line)
                    .unwrap()
                    == 0
                {
                    break;
                }
                total += line.len();
                assert!(total as u64 <= BOUND, "ordinary ACP output exceeded bound");
                assert_no_secret(&line);
                let value = serde_json::from_slice(&line)
                    .expect("ordinary native ACP must emit actual JSON-RPC");
                if send.send(value).is_err() {
                    break;
                }
            }
        });
        let stderr = child.stderr.take().unwrap();
        let error = thread::spawn(move || {
            let mut bytes = Vec::new();
            stderr.take(BOUND + 1).read_to_end(&mut bytes).unwrap();
            assert!(bytes.len() as u64 <= BOUND);
            assert_no_secret(&bytes);
        });
        Self {
            child,
            messages,
            reader: Some(reader),
            error: Some(error),
            deadline: Instant::now() + NATIVE_BOUND,
            id: 0,
            active: None,
            text: String::new(),
        }
    }
    fn request(&mut self, method: &str, params: Value) -> Value {
        self.id += 1;
        eprintln!("Windows ordinary native checkpoint: harness=dsh, method={method}");
        let input = json!({"jsonrpc":"2.0","id":self.id,"method":method,"params":params});
        let stdin = self.child.stdin.as_mut().unwrap();
        serde_json::to_writer(&mut *stdin, &input).unwrap();
        stdin.write_all(b"\n").unwrap();
        stdin.flush().unwrap();
        loop {
            let value = self
                .messages
                .recv_timeout(self.deadline.saturating_duration_since(Instant::now()))
                .expect("ordinary native ACP did not complete the selected public method");
            if value["method"] == "session/update"
                && self
                    .active
                    .as_ref()
                    .is_some_and(|id| value["params"]["sessionId"] == *id)
            {
                let update = &value["params"]["update"];
                if update["sessionUpdate"] == "agent_message_chunk" {
                    self.text.push_str(&text(&update["content"]));
                }
            }
            if value["id"] == self.id {
                assert!(
                    value.get("error").is_none(),
                    "ordinary ACP rejected the selected saved model/method"
                );
                return value["result"].clone();
            }
            assert!(
                !(value.get("id").is_some() && value.get("method").is_some()),
                "ordinary native unexpectedly requested a client capability"
            );
        }
    }
    fn stop(&mut self) {
        self.child.stdin.take();
        // ACP is a resident native protocol server. After its actual completed
        // prompt the consumer explicitly stops its owned Job, as the POSIX journey does.
        self.child.terminate().unwrap();
        wait(&mut self.child, Instant::now() + Duration::from_secs(10));
        self.reader.take().unwrap().join().unwrap();
        self.error.take().unwrap().join().unwrap();
    }
}
impl Drop for Acp {
    fn drop(&mut self) {
        if self.reader.is_some() {
            let _ = self.child.terminate();
            let deadline = Instant::now() + Duration::from_secs(10);
            let mut stopped = false;
            while Instant::now() < deadline {
                if self.child.exit_code().is_ok_and(|value| value.is_some())
                    && self.child.empty().unwrap_or(false)
                {
                    stopped = true;
                    break;
                }
                thread::sleep(Duration::from_millis(10));
            }
            // Preserve the original red failure without an unbounded pipe join
            // if native owned-stop evidence is itself unavailable.
            if stopped {
                if let Some(reader) = self.reader.take() {
                    let _ = reader.join();
                }
                if let Some(error) = self.error.take() {
                    let _ = error.join();
                }
            }
        }
    }
}
pub fn text(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        Value::Array(values) => values.iter().map(text).collect::<Vec<_>>().join(""),
        Value::Object(values) => values
            .iter()
            .filter(|(key, _)| {
                matches!(
                    key.as_str(),
                    "text"
                        | "content"
                        | "input"
                        | "output"
                        | "messages"
                        | "instructions"
                        | "system"
                )
            })
            .map(|(_, value)| text(value))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}
