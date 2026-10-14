//! Installed Host management, independent of the model and Worker journeys.
//! The foreground fixture has no registered manager; its negative doctor/logs
//! results are kept distinct from the complete-package background helper.
#[path = "windows_management_service.rs"]
mod service;
use service::assert_doctor;

use super::windows_headless_business::{Business, assert_operation, cli};
use super::windows_routing_business::{Plan, grant};
use super::windows_user_models::Contexts;
use super::{InstalledProduct, assert_no_secret, security};
use hiroute_application_api::{AgentModelSettingsStateV2, AgentSettingsStatusV2};
use hiroute_domain::AgentCollaborationTriggerModeV2;
use hiroute_platform::windows::job::{Child, Launch, Stream};
use serde_json::{Value, json};
use std::ffi::OsString;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::time::{Duration, Instant};
use zeroize::Zeroizing;

const AGENT: &str = "agent_pi_default";
const BOUND: u64 = hiroute_application_api::LOCAL_CONTROL_MAX_FRAME_BYTES as u64;

/// Requires a fresh InstalledProduct and a daemon started with contexts.variables().
/// Public transactions alone create the model and Skill files. This does not
/// claim an ordinary model call or a registered background manager.
pub fn run(product: &InstalledProduct, contexts: &Contexts) {
    let mut commands = Commands::new(product, contexts);
    let doctor = commands.ok(&["service", "doctor"], None);
    assert_doctor(&doctor, false);
    let (exit, logs) = commands.call(&["service", "logs"], None);
    assert_ne!(
        exit, 0,
        "an absent native manager must not supply log authority"
    );
    assert_eq!(logs["error"]["code"], "DAEMON_UNAVAILABLE");

    let model_file = contexts.pi.join("models.json");
    let defaults = contexts.pi.join("settings.json");
    let skill = contexts.pi.join("skills/hiroute-collaboration/SKILL.md");
    assert!(!model_file.exists() && !skill.exists());
    let scan = cli(product, &["agents", "scan"], None);
    let agents: Vec<_> = scan["data"]["agents"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|agent| agent["agent_id"] == AGENT)
        .collect();
    assert_eq!(
        agents.len(),
        1,
        "the selected real npm Pi must be discovered once"
    );
    let context = agents[0]["context_id"].as_str().unwrap().to_owned();
    let check = commands.ok(
        &["agents", "check", AGENT, "--scope", "collaboration"],
        None,
    );
    assert_native_check(&check);
    // The actual native client check may initialize its own defaults. Retain
    // those observed bytes before Configure rather than attributing native
    // initialization to a later product transaction.
    let original_default = Snapshot::optional(&defaults);
    let source = Business::save(product);
    let plan = Plan::create(
        product,
        source.binding_id(),
        "windows-management-model-plan",
    );
    let configure = json!({"schema_version":{"major":2,"minor":0},"context_id":context,
        "model":{"intent":"configure","settings":{"mode":"pi_additional",
            "allowed_plan_ids":[plan.plan_id],"plan_protocols":{&plan.plan_id:"responses"}}},
        "collaboration":{"intent":"keep"},"access_token":{"intent":"keep"}});
    settings(
        product,
        "connect",
        &configure,
        "windows-management-model-configure",
    );
    let configured = status(&mut commands);
    let model = configured.model().unwrap();
    assert_eq!(model.state, AgentModelSettingsStateV2::Configured);
    assert!(model.current_selection.is_some() && model.restore_point_ref.is_some());
    let token = grant(product, &format!("agent-connection/{context}"));
    let model_snapshot = Snapshot::required(&model_file, true);
    assert_same(
        &original_default,
        &Snapshot::optional(&defaults),
        "model configure preserves defaults",
    );
    assert!(
        !skill.exists(),
        "model configuration must not install collaboration"
    );

    let preview = commands.ok(
        &[
            "agents",
            "collaboration",
            "configure",
            AGENT,
            "--trigger",
            "explicit",
            "--preview",
        ],
        None,
    );
    assert_eq!(preview["data"]["applicable"], true);
    assert!(
        !skill.exists(),
        "host preview must have no native Skill effect"
    );
    preserved(
        &mut commands,
        &configured,
        &model_snapshot,
        &model_file,
        &original_default,
        &defaults,
    );

    let explicit =
        configure_collaboration(&mut commands, "explicit", "windows-management-explicit");
    let explicit_id = operation(product, &explicit);
    collaboration(
        &status(&mut commands),
        Some(AgentCollaborationTriggerModeV2::Explicit),
    );
    let explicit_skill = Snapshot::required(&skill, true);
    assert_skill(&explicit_skill, false);
    let repeated =
        configure_collaboration(&mut commands, "explicit", "windows-management-explicit");
    assert_eq!(operation(product, &repeated), explicit_id);
    assert_same(
        &Some(explicit_skill),
        &Snapshot::optional(&skill),
        "exact host replay preserves Skill bytes and native permissions",
    );
    preserved(
        &mut commands,
        &configured,
        &model_snapshot,
        &model_file,
        &original_default,
        &defaults,
    );

    let default = configure_collaboration(
        &mut commands,
        "delegate-by-default",
        "windows-management-default",
    );
    let default_id = operation(product, &default);
    assert_ne!(default_id, explicit_id);
    collaboration(
        &status(&mut commands),
        Some(AgentCollaborationTriggerModeV2::DelegateByDefault),
    );
    let default_skill = Snapshot::required(&skill, true);
    assert_skill(&default_skill, true);
    let after_check = commands.ok(
        &["agents", "check", AGENT, "--scope", "collaboration"],
        None,
    );
    assert_native_check(&after_check);
    let repeated = configure_collaboration(
        &mut commands,
        "delegate-by-default",
        "windows-management-default",
    );
    assert_eq!(operation(product, &repeated), default_id);
    preserved(
        &mut commands,
        &configured,
        &model_snapshot,
        &model_file,
        &original_default,
        &defaults,
    );

    let (exit, reused) = commands.call(
        &[
            "agents",
            "collaboration",
            "disable",
            AGENT,
            "--yes",
            "--idempotency-key",
            "windows-management-default",
        ],
        None,
    );
    assert_ne!(
        exit, 0,
        "a different intent must not reuse the accepted host receipt"
    );
    assert_eq!(reused["error"]["code"], "IDEMPOTENCY_KEY_REUSED");
    collaboration(
        &status(&mut commands),
        Some(AgentCollaborationTriggerModeV2::DelegateByDefault),
    );
    assert_same(
        &Some(default_skill),
        &Snapshot::optional(&skill),
        "rejected intent cannot change the Skill",
    );

    let disabled_preview = commands.ok(
        &["agents", "collaboration", "disable", AGENT, "--preview"],
        None,
    );
    assert_eq!(disabled_preview["data"]["applicable"], true);
    collaboration(
        &status(&mut commands),
        Some(AgentCollaborationTriggerModeV2::DelegateByDefault),
    );
    assert!(
        skill.is_file(),
        "disable preview must keep the configured Skill"
    );
    let disabled = commands.ok(
        &[
            "agents",
            "collaboration",
            "disable",
            AGENT,
            "--yes",
            "--idempotency-key",
            "windows-management-disable",
        ],
        None,
    );
    let disabled_id = operation(product, &disabled);
    collaboration(&status(&mut commands), None);
    assert!(
        !skill.exists(),
        "host Disable must restore the newly created native Skill"
    );
    let repeated = commands.ok(
        &[
            "agents",
            "collaboration",
            "disable",
            AGENT,
            "--yes",
            "--idempotency-key",
            "windows-management-disable",
        ],
        None,
    );
    assert_eq!(operation(product, &repeated), disabled_id);
    preserved(
        &mut commands,
        &configured,
        &model_snapshot,
        &model_file,
        &original_default,
        &defaults,
    );
    let retained_token = grant(product, &format!("agent-connection/{context}"));
    assert!(
        *retained_token == *token,
        "independent collaboration changes preserve model authority"
    );

    let restore = json!({"schema_version":{"major":2,"minor":0},"context_id":context,
        "model":{"intent":"restore","restore_point_ref":model.restore_point_ref},
        "collaboration":{"intent":"keep"},"access_token":{"intent":"keep"}});
    settings(
        product,
        "restore",
        &restore,
        "windows-management-model-restore",
    );
    assert!(!model_file.exists() && !skill.exists());
    assert_same(
        &original_default,
        &Snapshot::optional(&defaults),
        "independent model Restore preserves defaults",
    );
    assert_eq!(
        status(&mut commands).model().unwrap().state,
        AgentModelSettingsStateV2::NotConfigured
    );
    println!(
        "Windows installed management: public collaboration status/configure/default/disable, exact receipt replay, independent model authority/native permissions/defaults; foreground doctor unhealthy and manager-unavailable logs remain explicit"
    );
}

/// The foreground module exposes this only for a genuinely installed ready manager.
#[allow(dead_code)]
pub fn run_service(cli: &mut impl FnMut(&[&str]) -> Value) {
    service::run(cli);
}

fn assert_native_check(envelope: &Value) {
    assert_eq!(envelope["data"]["skill_loading"], "proven");
    assert_eq!(envelope["data"]["trusted_cli_execution"], "proven");
}
fn status(commands: &mut Commands<'_>) -> AgentSettingsStatusV2 {
    let value = commands.ok(&["agents", "collaboration", "status", AGENT], None);
    let status: AgentSettingsStatusV2 = serde_json::from_value(value["data"].clone()).unwrap();
    assert!(status.valid());
    status
}
fn collaboration(status: &AgentSettingsStatusV2, mode: Option<AgentCollaborationTriggerModeV2>) {
    let facet = status.collaboration().unwrap();
    match mode {
        Some(mode) => {
            assert_eq!(facet.state, AgentModelSettingsStateV2::Configured);
            assert_eq!(facet.current_selection.as_ref().unwrap().trigger_mode, mode);
            assert!(facet.restore_point_ref.is_some());
        }
        None => {
            assert_eq!(facet.state, AgentModelSettingsStateV2::Restored);
            assert!(facet.current_selection.is_none() && facet.restore_point_ref.is_none());
        }
    }
}
fn configure_collaboration(commands: &mut Commands<'_>, trigger: &str, key: &str) -> Value {
    commands.ok(
        &[
            "agents",
            "collaboration",
            "configure",
            AGENT,
            "--trigger",
            trigger,
            "--idempotency-key",
            key,
        ],
        None,
    )
}
fn operation(product: &InstalledProduct, envelope: &Value) -> String {
    let id = envelope["operation"]["operation_id"]
        .as_str()
        .or_else(|| envelope["data"]["operation_id"].as_str())
        .unwrap()
        .to_owned();
    assert_operation(product, &id);
    id
}
fn settings(product: &InstalledProduct, verb: &str, spec: &Value, key: &str) {
    let preview = cli(
        product,
        &["agents", verb, "preview"],
        Some(&json!({"spec":spec})),
    );
    let p = &preview["data"];
    assert_eq!(p["applicable"], true);
    assert_ne!(p["resident_service"]["login_item_required"], true);
    let applied = cli(
        product,
        &["agents", verb, "apply"],
        Some(&json!({"spec":p["spec"],
        "accept_digest":p["accept_digest"],"dependency_digest":p["dependency_digest"],
        "expected_revisions":p["expected_revisions"],"idempotency_key":key})),
    );
    operation(product, &applied);
}
fn preserved(
    commands: &mut Commands<'_>,
    before: &AgentSettingsStatusV2,
    snapshot: &Snapshot,
    model: &Path,
    defaults: &Option<Snapshot>,
    default_path: &Path,
) {
    let current = status(commands);
    let previous = before.model().unwrap();
    let current = current.model().unwrap();
    assert_eq!(current.state, AgentModelSettingsStateV2::Configured);
    assert_eq!(current.current_selection, previous.current_selection);
    assert_eq!(current.restore_point_ref, previous.restore_point_ref);
    assert!(
        snapshot.same(&Snapshot::required(model, true)),
        "collaboration must preserve exact model bytes/token/Owner/DACL"
    );
    assert_same(
        defaults,
        &Snapshot::optional(default_path),
        "collaboration must preserve native defaults and their permissions",
    );
}
fn assert_skill(skill: &Snapshot, default: bool) {
    let text = std::str::from_utf8(&skill.bytes).unwrap();
    assert!(text.contains("hiroute worker plans") && text.contains("hiroute worker exec"));
    let explicit = "Use delegation only when the user explicitly requests it.";
    let automatic = "Delegate suitable executable work by default.";
    assert_eq!(text.contains(explicit), !default);
    assert_eq!(text.contains(automatic), default);
}
struct Snapshot {
    bytes: Zeroizing<Vec<u8>>,
    permissions: Vec<u8>,
}
impl Snapshot {
    fn optional(path: &Path) -> Option<Self> {
        match security::open_identity(path, false) {
            Ok(file) => Some(Self::held(file, false)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(_) => {
                panic!("native file snapshot must preserve its real reparse-free readable identity")
            }
        }
    }
    fn required(path: &Path, private: bool) -> Self {
        Self::held(security::open_identity(path, false).unwrap(), private)
    }
    fn held(mut file: File, private: bool) -> Self {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Security::{
            DACL_SECURITY_INFORMATION, GetKernelObjectSecurity, OWNER_SECURITY_INFORMATION,
        };
        if private {
            security::verify_owner_only_handle(&file, false).unwrap();
        }
        assert_eq!(security::file_link_count(&file).unwrap(), 1);
        let information = OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION;
        let mut needed = 0;
        // SAFETY: held-object query with the documented zero-length sizing call.
        unsafe {
            GetKernelObjectSecurity(
                file.as_raw_handle(),
                information,
                std::ptr::null_mut(),
                0,
                &mut needed,
            );
        }
        assert!(needed > 0 && needed <= 65536);
        let mut permissions = vec![0; needed as usize];
        // SAFETY: exact owned destination size returned by the native kernel.
        assert_ne!(
            unsafe {
                GetKernelObjectSecurity(
                    file.as_raw_handle(),
                    information,
                    permissions.as_mut_ptr().cast(),
                    needed,
                    &mut needed,
                )
            },
            0
        );
        let mut bytes = Zeroizing::new(Vec::new());
        Read::by_ref(&mut file)
            .take(262145)
            .read_to_end(&mut bytes)
            .unwrap();
        assert!(bytes.len() <= 262144);
        Self { bytes, permissions }
    }
    fn same(&self, other: &Self) -> bool {
        *self.bytes == *other.bytes && self.permissions == other.permissions
    }
}
fn assert_same(before: &Option<Snapshot>, after: &Option<Snapshot>, message: &str) {
    assert!(
        match (before, after) {
            (None, None) => true,
            (Some(before), Some(after)) => before.same(after),
            _ => false,
        },
        "{message}"
    );
}

/// Owned files keep capture bounded without detached readers or EOF joins. Each
/// native CLI and any native check tools remain in one held Job until exit+empty.
struct Commands<'a> {
    product: &'a InstalledProduct,
    environment: Vec<(OsString, OsString)>,
    sequence: usize,
}
impl<'a> Commands<'a> {
    fn new(product: &'a InstalledProduct, contexts: &Contexts) -> Self {
        let mut environment: Vec<_> = std::env::vars_os()
            .filter(|(key, _)| {
                matches!(
                    key.to_string_lossy().to_ascii_uppercase().as_str(),
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
                        | "PROGRAMW6432"
                        | "TEMP"
                        | "TMP"
                        | "PATH"
                        | "USERNAME"
                        | "USERDOMAIN"
                )
            })
            .collect();
        for (key, value) in contexts.variables() {
            environment.retain(|(existing, _)| {
                !existing
                    .to_string_lossy()
                    .eq_ignore_ascii_case(&key.to_string_lossy())
            });
            environment.push((key.clone(), value.clone()));
        }
        Self {
            product,
            environment,
            sequence: 0,
        }
    }
    fn ok(&mut self, args: &[&str], body: Option<&Value>) -> Value {
        let (exit, value) = self.call(args, body);
        let code = value["error"]["code"].as_str().unwrap_or("none");
        assert_eq!(
            exit, 0,
            "installed management command failed with closed code: {code}"
        );
        assert!(matches!(
            value["status"].as_str(),
            Some("succeeded" | "accepted")
        ));
        value
    }
    fn call(&mut self, args: &[&str], body: Option<&Value>) -> (i32, Value) {
        self.sequence += 1;
        let file = |label| {
            security::create_owner_only_file(
                &self
                    .product
                    .root
                    .join(format!("management-{}-{label}.capture", self.sequence)),
            )
            .unwrap()
        };
        let mut output = file("output");
        let mut error = file("error");
        let mut input = if let Some(body) = body {
            let mut input = file("input");
            let bytes = serde_json::to_vec(body).unwrap();
            assert!(bytes.len() <= 65536);
            assert_no_secret(&bytes);
            input.write_all(&bytes).unwrap();
            input.seek(SeekFrom::Start(0)).unwrap();
            Some(input)
        } else {
            None
        };
        let mut argv: Vec<_> = args.iter().map(OsString::from).collect();
        if input.is_some() {
            argv.push("--request-stdin".into());
        }
        argv.extend(["--output", "json", "--timeout", "90s"].map(OsString::from));
        let stage = match args {
            ["service", "doctor"] => "doctor",
            ["service", "logs"] => "logs",
            ["agents", "check", ..] => "native-collaboration-check",
            ["agents", "collaboration", "configure", ..] => "collaboration-configure",
            ["agents", "collaboration", "status", ..] => "collaboration-status",
            ["agents", "collaboration", "disable", ..] => "collaboration-disable",
            _ => "unknown",
        };
        assert_ne!(
            stage, "unknown",
            "management command diagnostics must remain closed"
        );
        eprintln!(
            "Windows installed management checkpoint: stage={stage}, sequence={}",
            self.sequence
        );
        let child = Child::spawn(Launch {
            program: &self.product.stable,
            args: &argv,
            env: &self.environment,
            cwd: self.product.stable.parent(),
            stdin: match input.as_mut() {
                Some(input) => Stream::File(input),
                None => Stream::Null,
            },
            stdout: Stream::File(&output),
            stderr: Stream::File(&error),
        })
        .unwrap();
        let deadline = Instant::now() + Duration::from_secs(100);
        let exit = loop {
            if let Some(exit) = child.exit_code().unwrap() {
                break exit;
            }
            if Instant::now() >= deadline {
                child.terminate().unwrap();
                panic!("installed management exceeded native CLI deadline at stage={stage}");
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        while !child.empty().unwrap() {
            assert!(
                Instant::now() < deadline,
                "installed management leaked native tools at stage={stage}"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        let read = |file: &mut File| {
            assert!(file.metadata().unwrap().len() <= BOUND);
            file.seek(SeekFrom::Start(0)).unwrap();
            let mut bytes = Vec::new();
            file.take(BOUND + 1).read_to_end(&mut bytes).unwrap();
            assert!(bytes.len() as u64 <= BOUND);
            assert_no_secret(&bytes);
            bytes
        };
        let bytes = read(&mut output);
        assert!(
            read(&mut error).is_empty(),
            "public management CLI emitted unexpected stderr"
        );
        (
            exit,
            serde_json::from_slice(&bytes)
                .expect("installed management returns the real MachineEnvelope"),
        )
    }
}
