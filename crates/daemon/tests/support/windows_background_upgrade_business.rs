//! Public user-owned Source → Secret → fixed Plan → Agent grant → Gateway → receipt,
//! retained across real package rollback/upgrade. Only the independent upstream is controlled.
use super::windows_business_http as http;
#[path = "windows_background_upgrade_io.rs"]
mod native_io;
#[path = "windows_background_upgrade_oracle.rs"]
mod oracle;
pub(super) use native_io::assert_no_secret;
use native_io::{cli, execute, grant, read, register, stage_codex};

use super::fixture;
use hiroute_host_runtime::StandaloneLayout;
use hiroute_platform::windows::security;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::Read;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use zeroize::Zeroizing;

const CANDIDATE: &str = "candidate/native/windows-upgrade";
const KEY: &str = "windows-upgrade-native-save";
const ORIGINAL_CONFIG: &[u8] = b"# untouched external Codex settings\nmodel = \"gpt-5.3-codex\"\nsandbox_mode = \"read-only\"\n[features]\nmulti_agent = false\n";
fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn value_digest(value: &Value) -> String {
    digest(&serde_json::to_vec(value).unwrap())
}
fn source_digest(value: &Value) -> String {
    // Native availability is a fresh query projection; its evaluation clock is
    // not persisted source identity. Keep every typed field and availability
    // assertion while excluding only this explicitly volatile timestamp.
    let typed: hiroute_application_api::ComputeManagedSourceViewV2 =
        serde_json::from_value(value.clone())
            .expect("saved Source must match its current public type");
    let mut projected = serde_json::to_value(typed).unwrap();
    for model in projected["models"].as_array_mut().unwrap() {
        assert_eq!(model["presentation"]["availability"], "available");
        model["presentation"]["evaluated_at_ms"] = json!(0);
    }
    value_digest(&projected)
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Save {
    address: SocketAddr,
    codex: PathBuf,
    codex_sha256: String,
    codex_version: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    session: String,
    request: String,
    usage_sha256: String,
    phase: usize,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Saved {
    source: String,
    source_revision: Value,
    source_sha256: String,
    binding: String,
    save_request: Value,
    save_operation: String,
    plan: String,
    alias: String,
    plan_sha256: String,
    route_request: Value,
    route_operation: String,
    context: String,
    restore: String,
    configured_sha256: String,
    original_permissions: hiroute_platform::file_security::WindowsPermissions,
    codex_sha256: String,
    receipts: Vec<Receipt>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Verify {
    saved: Saved,
    phase: usize,
}

pub struct Business {
    oracle: oracle::Oracle,
    saved: Saved,
}
impl Business {
    pub fn save(token: &File, program: &Path) -> Self {
        let oracle = oracle::Oracle::start();
        let save = Save {
            address: oracle.address(),
            codex: fixture::required("HIROUTE_WINDOWS_UPGRADE_CODEX_BINARY"),
            codex_sha256: std::env::var("HIROUTE_WINDOWS_UPGRADE_CODEX_SHA256")
                .expect("upgrade requires the verified pinned Codex hash"),
            codex_version: std::env::var("HIROUTE_WINDOWS_UPGRADE_CODEX_VERSION")
                .expect("upgrade requires the verified pinned Codex version"),
        };
        let saved: Saved =
            serde_json::from_value(owner(token, program, "upgrade-business-save", &save)).unwrap();
        oracle.assert_counts(&[0], 0);
        Self { oracle, saved }
    }
    pub fn recovered(&mut self, token: &File, program: &Path, rollback: bool) {
        let phase = if rollback { 1 } else { 2 };
        let payload = Verify {
            saved: serde_json::from_value(serde_json::to_value(&self.saved).unwrap()).unwrap(),
            phase,
        };
        self.saved =
            serde_json::from_value(owner(token, program, "upgrade-business-verify", &payload))
                .unwrap();
        let phases: Vec<_> = self
            .saved
            .receipts
            .iter()
            .map(|receipt| receipt.phase)
            .collect();
        self.oracle
            .assert_counts(&phases, self.saved.receipts.len() - 1);
    }
    pub fn restore(self, token: &File, program: &Path) {
        let result = owner(token, program, "upgrade-business-cleanup", &self.saved);
        assert_eq!(result["restored"], true);
        assert_eq!(result["original_config_sha256"], digest(ORIGINAL_CONFIG));
        let phases: Vec<_> = self
            .saved
            .receipts
            .iter()
            .map(|receipt| receipt.phase)
            .collect();
        self.oracle
            .assert_counts(&phases, self.saved.receipts.len() - 1);
    }
}
fn owner(token: &File, program: &Path, action: &str, payload: &impl Serialize) -> Value {
    let encoded = serde_json::to_string(payload).unwrap();
    assert_no_secret(encoded.as_bytes());
    // Includes the fixed Request envelope, not just its nested payload.
    let request = fixture::Request {
        action: action.into(),
        program: Some(program.to_owned()),
        arguments: vec![encoded],
    };
    assert!(
        serde_json::to_vec(&request).unwrap().len() <= 16384,
        "upgrade request exceeds the existing owner boundary"
    );
    fixture::owner(token, request, Duration::from_secs(180))
}

/// Narrow dispatch called only after the existing standard-owner token check.
pub(super) fn run_action(request: &fixture::Request) -> Value {
    assert_eq!(request.arguments.len(), 1);
    assert_no_secret(request.arguments[0].as_bytes());
    let program = request.program.as_ref().unwrap();
    let layout = StandaloneLayout::from_environment().unwrap();
    let installed =
        hiroute_host_runtime::read_standalone_install_record(&layout.marker_path).unwrap();
    assert_eq!(
        security::file_identity(&security::open_identity(program, false).unwrap()).unwrap(),
        security::file_identity(
            &security::open_identity(&installed.install_root.join("hiroute.exe"), false).unwrap()
        )
        .unwrap(),
        "business action must use the actually installed CLI object"
    );
    match request.action.as_str() {
        "upgrade-business-save" => serde_json::to_value(save(
            program,
            &layout,
            serde_json::from_str(&request.arguments[0]).unwrap(),
        ))
        .unwrap(),
        "upgrade-business-verify" => {
            let Verify { mut saved, phase } = serde_json::from_str(&request.arguments[0]).unwrap();
            assert!(phase == 1 || phase == 2);
            assert_eq!(saved.receipts.first().unwrap().phase, 0);
            assert!(
                !saved.receipts.iter().any(|receipt| receipt.phase >= phase),
                "upgrade phases may not be replayed or reordered"
            );
            verify_state(program, &layout, &saved);
            for receipt in &saved.receipts {
                assert_receipt(program, &saved.plan, receipt);
            }
            // No protected candidate registration occurs after the original save.
            // This authenticated model discovery reads only the persisted Key.
            let checked = cli(
                program,
                &["compute", "connection", "test"],
                Some(&json!({"kind":"saved","request":{
                "source_id":saved.source,"expected_source_revision":saved.source_revision,
                "candidate_ref":CANDIDATE,"edit_revision":phase+1,"check_id":format!("check/windows/upgrade-{phase}")}})),
            );
            assert_eq!(checked["data"]["directory"], "available");
            assert_eq!(checked["data"]["authentication"], "verified");
            assert_eq!(
                checked["data"]["candidate"]["existing_source_id"],
                saved.source
            );
            let receipt = traffic(program, &saved, phase);
            assert!(
                !saved
                    .receipts
                    .iter()
                    .any(|old| old.request == receipt.request),
                "new Gateway request reused an old receipt identity"
            );
            saved.receipts.push(receipt);
            serde_json::to_value(saved).unwrap()
        }
        "upgrade-business-cleanup" => {
            let saved: Saved = serde_json::from_str(&request.arguments[0]).unwrap();
            verify_state(program, &layout, &saved);
            let spec = json!({"schema_version":{"major":2,"minor":0},"context_id":saved.context,
                "model":{"intent":"restore","restore_point_ref":saved.restore}});
            settings(program, "restore", &spec, "windows-upgrade-native-restore");
            let status = cli(
                program,
                &["agents", "connect", "status", &saved.context],
                None,
            );
            assert_eq!(status["data"]["state"], "restored");
            assert!(status["data"]["current_selection"].is_null());
            assert!(status["data"]["restore_point_ref"].is_null());
            let file =
                security::open_identity(&layout.home.join(".codex/config.toml"), false).unwrap();
            let permissions = hiroute_platform::file_security::capture(&file).unwrap();
            let bytes = read(file);
            assert!(
                bytes.as_slice() == ORIGINAL_CONFIG,
                "public Restore must retain the untouched original external settings bytes"
            );
            assert_eq!(
                permissions, saved.original_permissions,
                "public Restore must retain the complete original native permissions"
            );
            json!({"restored":true,"original_config_sha256":digest(&bytes)})
        }
        _ => panic!("unknown upgrade business action"),
    }
}

fn save(program: &Path, layout: &StandaloneLayout, input: Save) -> Saved {
    assert!(input.address.ip().is_loopback() && input.address.port() != 0);
    stage_codex(layout, &input.codex, &input.codex_sha256);
    let codex_home = layout.home.join(".codex");
    assert!(
        !codex_home.exists(),
        "upgrade gate requires a disposable owner without preexisting Codex settings"
    );
    security::create_owner_only_directory(&codex_home).unwrap();
    let config = codex_home.join("config.toml");
    hiroute_platform::durable::write_private_new(&config, ORIGINAL_CONFIG).unwrap();
    let original_permissions =
        hiroute_platform::file_security::capture(&security::open_identity(&config, false).unwrap())
            .unwrap();
    register(program);
    let before = cli(program, &["compute", "list"], None);
    assert!(
        before["data"]["sources"].as_array().unwrap().is_empty(),
        "disposable upgrade gate must begin without saved sources"
    );
    let checked = cli(
        program,
        &["compute", "connection", "test"],
        Some(&json!({"kind":"native","request":{
        "draft":draft(input.address),"input_candidate":{"candidate_ref":CANDIDATE,"candidate_revision":1}}})),
    );
    assert_eq!(checked["data"]["authentication"], "verified");
    assert_eq!(checked["data"]["inference"], "verified");
    assert_eq!(checked["data"]["candidate"]["fact_state"], "complete");
    let candidate = &checked["data"]["candidate"];
    let model = candidate["models"]
        .as_array()
        .unwrap()
        .iter()
        .find(|model| model["upstream_model_id"] == oracle::MODEL && model["selectable"] == true)
        .unwrap();
    let snapshot = cli(program, &["compute", "list"], None);
    let preview = cli(
        program,
        &["compute", "connection", "preview"],
        Some(&json!({"change":{
        "schema":"hiroute.compute-management-change/v2","subject":{"kind":"candidate","candidate":candidate["candidate"]},
        "expected_revisions":snapshot["data"]["revisions"],"selected_model_refs":[model["model_ref"]],"intent":"save_ready","key_edits":[]}})),
    );
    let preview = &preview["data"];
    let save_request = json!({"spec":preview["spec"],"accept_digest":preview["accept_digest"],"expected_revisions":preview["expected_revisions"],"idempotency_key":KEY});
    let save_operation = apply(program, &["compute", "connection", "apply"], &save_request);
    let snapshot = cli(program, &["compute", "list"], None);
    assert_eq!(snapshot["data"]["sources"].as_array().unwrap().len(), 1);
    let source = &snapshot["data"]["sources"][0];
    assert_eq!(source["display_name"], "Windows upgrade saved API");
    assert_eq!(source["state"], "ready");
    assert_eq!(source["keys"].as_array().unwrap().len(), 1);
    assert_eq!(source["models"][0]["upstream_model_id"], oracle::MODEL);
    let binding = source["models"][0]["binding_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let options = cli(program, &["routing", "options"], Some(&json!({})));
    assert!(
        options["data"]["candidates"]
            .as_array()
            .unwrap()
            .iter()
            .any(|candidate| candidate["binding_id"] == binding && candidate["routable"] == true)
    );
    assert_eq!(
        cli(
            program,
            &["protected-input", "release", "--candidate", CANDIDATE],
            None
        )["data"]["registered"],
        true
    );
    let editor = json!({"schema":"hiroute.plan-editor/v2","display_name":"Windows upgrade fixed route","purpose":"Exact route/credential/receipt survives native package replacement",
        "mode":"fixed_model","candidates":[{"binding_id":binding}],"smart":{"economy":[],"primary":[],"judgment":{
            "degree":{"simple_threshold_millis":800,"instructions":"Assess task complexity.","simple":"Routine work","complex":"Uncertain work"},
            "competence":{"floor_millis":500,"instructions":"Assess competence.","criteria":["Failed","Partly competent","Competent"]}},
            "reselect_on_user_message":false,"classifier":{"kind":"local_rules"},"complex_keywords":[]},
        "free":{"candidates":[],"primary":[],"primary_fallback":false},"delegation_enabled":false,"requirements":{},
        "limits":{"maximum_attempts":1,"request_timeout_ms":30000,"attempt_timeout_ms":30000}});
    let change = json!({"schema":"hiroute.plan-content-change/v2","target":{"intent":"create","creation_key":"windows-upgrade-fixed-route"},"editor":editor,"consumed_draft":null});
    let preview = cli(
        program,
        &["routing", "preview"],
        Some(&json!({"change":change})),
    );
    let preview = &preview["data"];
    let plan = preview["plan_head"]["reference"]["plan_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let alias = preview["plan_head"]["model_alias"]
        .as_str()
        .unwrap()
        .to_owned();
    let route_request = json!({"change":change,"accept_digest":preview["change_digest"],"expected_revisions":preview["expected_revisions"],"idempotency_key":"windows-upgrade-fixed-route"});
    let route_operation = apply(program, &["routing", "apply"], &route_request);
    let head = cli(program, &["routing", "show", &plan], None)["data"]["head"].clone();
    assert_eq!(head["model_alias"], alias);
    assert_eq!(head["status"], "enabled");
    let scanned = cli(program, &["agents", "scan"], None);
    let agent = scanned["data"]["agents"]
        .as_array()
        .unwrap()
        .iter()
        .find(|agent| agent["agent_id"] == "agent_codex_default")
        .unwrap();
    assert_eq!(agent["supported"], true);
    let version = execute(
        &layout.home.join(".local/bin/codex.exe"),
        &["--version".into()],
        None,
        Duration::from_secs(15),
    );
    assert_eq!(
        version.0, 0,
        "fixed real Codex must run its own version command"
    );
    assert!(
        String::from_utf8_lossy(&version.1)
            .lines()
            .any(|line| line.split_whitespace().last() == Some(input.codex_version.as_str())),
        "staged actual Codex does not report the selected pinned version"
    );
    let context = agent["codex_access"]["root_context_id"]
        .as_str()
        .or_else(|| agent["context_id"].as_str())
        .unwrap()
        .to_owned();
    cli(
        program,
        &[
            "agents",
            "check",
            "agent_codex_default",
            "--scope",
            "native-authentication",
        ],
        None,
    );
    let spec = json!({"schema_version":{"major":2,"minor":0},"context_id":context,"model":{"intent":"configure","settings":{
        "mode":"codex_default","native_model_mode":"hiroute_only","fixed_models":[],"allowed_plan_ids":[plan],"default_selection":{"kind":"plan","plan_id":plan}}}});
    settings(program, "connect", &spec, "windows-upgrade-agent-connect");
    let status = cli(program, &["agents", "connect", "status", &context], None);
    assert_eq!(status["data"]["state"], "configured");
    let restore = status["data"]["restore_point_ref"]
        .as_str()
        .unwrap()
        .to_owned();
    let mut saved = Saved {
        source: source["source_id"].as_str().unwrap().to_owned(),
        source_revision: source["revision"].clone(),
        source_sha256: source_digest(source),
        binding,
        save_request,
        save_operation,
        plan,
        alias,
        plan_sha256: value_digest(&head),
        route_request,
        route_operation,
        context,
        restore,
        configured_sha256: digest(&read(security::open_identity(&config, false).unwrap())),
        original_permissions,
        codex_sha256: input.codex_sha256,
        receipts: Vec::new(),
    };
    saved.receipts.push(traffic(program, &saved, 0));
    saved
}

fn verify_state(program: &Path, layout: &StandaloneLayout, saved: &Saved) {
    let shown = cli(program, &["compute", "show", &saved.source], None);
    assert_eq!(shown["data"]["sources"].as_array().unwrap().len(), 1);
    let source = &shown["data"]["sources"][0];
    assert_eq!(
        source_digest(source),
        saved.source_sha256,
        "saved Source/Key/model identity or configuration changed across package replacement"
    );
    assert_eq!(source["source_id"], saved.source);
    assert_eq!(source["revision"], saved.source_revision);
    assert_eq!(source["models"][0]["binding_id"], saved.binding);
    assert_eq!(
        apply(
            program,
            &["compute", "connection", "apply"],
            &saved.save_request
        ),
        saved.save_operation
    );
    assert_eq!(
        apply(program, &["routing", "apply"], &saved.route_request),
        saved.route_operation
    );
    let shown = cli(program, &["routing", "show", &saved.plan], None);
    assert_eq!(
        value_digest(&shown["data"]["head"]),
        saved.plan_sha256,
        "fixed route head/version/alias changed across package replacement"
    );
    assert_eq!(shown["data"]["head"]["model_alias"], saved.alias);
    assert_eq!(shown["data"]["head"]["status"], "enabled");
    let status = cli(
        program,
        &["agents", "connect", "status", &saved.context],
        None,
    );
    assert_eq!(status["data"]["state"], "configured");
    assert_eq!(status["data"]["restore_point_ref"], saved.restore);
    assert_eq!(
        digest(&read(
            security::open_identity(&layout.home.join(".codex/config.toml"), false).unwrap()
        )),
        saved.configured_sha256,
        "upgrade changed the external configured native settings"
    );
    let mut file =
        security::open_identity(&layout.home.join(".local/bin/codex.exe"), false).unwrap();
    let mut hash = Sha256::new();
    let mut buffer = Zeroizing::new(vec![0u8; 64 * 1024]);
    loop {
        let count = file.read(&mut buffer).unwrap();
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    assert_eq!(
        format!("{:x}", hash.finalize()),
        saved.codex_sha256,
        "package maintenance replaced an unrelated external engine"
    );
}
fn draft(address: SocketAddr) -> Value {
    let declared = |value: Value| json!({"value":value,"basis":"user_declared"});
    json!({"inference_model_id":oracle::MODEL,"candidate_ref":CANDIDATE,"lineage_ref":format!("lineage/{CANDIDATE}"),
        "display_name":"Windows upgrade saved API","existing_source_id":null,"edit_revision":1,"check_id":"check/windows/upgrade-native",
        "base_url":format!("http://{address}/v1"),"base_kind":"api_root","request_path_override":null,"inventory_path_override":"/v1/models",
        "protocol":"responses","protocol_profile_id":"profile/custom/responses","protocol_profile_revision":1,
        "authentication":{"kind":"bearer"},"configuration_revision":1,"models":[{"upstream_model_id":oracle::MODEL,
            "display_name":"Windows upgrade model","catalog_configuration_id":null,"membership":"user_declared","capabilities":{
                "tool":declared(json!(true)),"vision":declared(json!(false)),"streaming":declared(json!(true)),
                "context_tokens":declared(json!(131072)),"max_output_tokens":declared(json!(4096)),
                "native_reasoning":declared(json!({"kind":"fixed","profile":"provider-default"}))}}]})
}
fn settings(program: &Path, verb: &str, spec: &Value, key: &str) {
    let preview = cli(
        program,
        &["agents", verb, "preview"],
        Some(&json!({"spec":spec})),
    );
    let preview = &preview["data"];
    assert_eq!(
        preview["applicable"], true,
        "real native Agent configuration was not admitted"
    );
    let request = json!({"spec":preview["spec"],"accept_digest":preview["accept_digest"],"dependency_digest":preview["dependency_digest"],
        "expected_revisions":preview["expected_revisions"],"idempotency_key":key});
    apply(program, &["agents", verb, "apply"], &request);
}
fn operation(program: &Path, id: &str) {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let current = cli(program, &["operations", "get", id], None);
        let state = current["data"]["state"].as_str().unwrap();
        if state == "succeeded" {
            break;
        }
        assert!(
            !matches!(state, "failed" | "cancelled" | "unknown"),
            "upgrade public operation did not succeed"
        );
        assert!(
            Instant::now() < deadline,
            "upgrade public operation did not settle"
        );
        thread::sleep(Duration::from_millis(30));
    }
}
fn operation_id(value: &Value) -> String {
    value["operation"]["operation_id"]
        .as_str()
        .or_else(|| value["data"]["operation_id"].as_str())
        .unwrap()
        .to_owned()
}
fn apply(program: &Path, args: &[&str], payload: &Value) -> String {
    let applied = cli(program, args, Some(payload));
    let id = operation_id(&applied);
    operation(program, &id);
    let replay = cli(program, args, Some(payload));
    assert_eq!(
        operation_id(&replay),
        id,
        "persisted accepted intent changed its operation identity"
    );
    id
}
fn traffic(program: &Path, saved: &Saved, phase: usize) -> Receipt {
    assert!(phase < oracle::PHASES.len());
    let endpoint = cli(program, &["gateway", "show"], None);
    assert_eq!(endpoint["data"]["ready"], true);
    let gateway: SocketAddr = endpoint["data"]["connect_address"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    assert!(gateway.ip().is_loopback());
    let body = json!({"model":saved.alias,"input":[{"role":"user","content":[{"type":"input_text","text":format!("windows-upgrade-{}",oracle::PHASES[phase])}]}],"stream":false});
    for invalid in [None, Some("invalid-windows-upgrade-token")] {
        assert!(
            matches!(
                http::post(gateway, "/v1/responses", invalid, &body).status,
                401 | 403
            ),
            "an unauthenticated real Gateway request reached the route"
        );
    }
    let token = grant(program, &saved.context);
    let native_session = format!("windows-upgrade-{}", oracle::PHASES[phase]);
    let response = http::post_with_headers(
        gateway,
        "/v1/responses",
        Some(&token),
        &[
            ("thread-id", &native_session),
            ("session-id", &native_session),
        ],
        &body,
    );
    assert_eq!(
        response.status, 200,
        "real saved route/credential could not execute after package maintenance"
    );
    let value = response.json();
    assert_eq!(value["status"], "completed");
    assert_eq!(
        value["output"][0]["content"][0]["text"],
        format!("windows-upgrade-{}-answer", oracle::PHASES[phase])
    );
    assert_eq!(value["usage"]["input_tokens"], oracle::USAGE[phase].0);
    assert_eq!(value["usage"]["output_tokens"], oracle::USAGE[phase].1);
    observed(program, &saved.plan, phase)
}
fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis()
        .try_into()
        .unwrap()
}
fn observation(view: &str, query: Value) -> Value {
    json!({"schema":"hiroute.observation.query/v2","intent":{"view":view,"query":query}})
}
fn query(plan: &str, session: Option<&str>) -> Value {
    json!({"from_ms":0,"to_ms":now(),"session_id":session,"request_id":null,
        "plan_id":plan,"limit":50,"cursor":null,"only_model_switch":false})
}
fn facts(program: &Path, request: &str) -> Value {
    cli(
        program,
        &["sessions", "receipt"],
        Some(&observation(
            "facts",
            json!({"request_id":request,"limit":100,"cursor":null}),
        )),
    )["data"]
        .clone()
}
fn usage(facts: &Value, phase: usize) -> Option<&Value> {
    facts["facts"].as_array().unwrap().iter().find(|fact| {
        fact["input_tokens"] == oracle::USAGE[phase].0
            && fact["output_tokens"] == oracle::USAGE[phase].1
    })
}
fn assert_usage(facts: &Value, fact: &Value, phase: usize) {
    assert_eq!(fact["event_kind"], "usage_and_cache");
    let ordinal = fact["attempt_ordinal"].as_u64().unwrap();
    assert!(
        facts["facts"].as_array().unwrap().iter().any(|start| {
            start["event_kind"] == "attempt_started"
                && start["attempt_ordinal"] == ordinal
                && start["native_model"] == oracle::MODEL
        }),
        "reported usage must belong to the actual model attempt in this request"
    );
    assert_eq!(fact["input_tokens"], oracle::USAGE[phase].0);
    assert_eq!(fact["output_tokens"], oracle::USAGE[phase].1);
    assert_eq!(fact["cache_read_tokens"], 3);
    assert_eq!(fact["reasoning_tokens"], 2);
}
fn observed(program: &Path, plan: &str, phase: usize) -> Receipt {
    let deadline = Instant::now() + Duration::from_secs(40);
    loop {
        let sessions = cli(
            program,
            &["sessions", "list"],
            Some(&observation("sessions", query(plan, None))),
        );
        let mut matches = Vec::new();
        for session in sessions["data"]["sessions"].as_array().unwrap() {
            let session_id = session["session_id"].as_str().unwrap();
            let timeline = cli(
                program,
                &["sessions", "show"],
                Some(&observation("timeline", query(plan, Some(session_id)))),
            );
            for row in timeline["data"]["requests"].as_array().unwrap() {
                if row["final_native_model"].is_null() {
                    continue;
                }
                assert_eq!(row["session_id"], session_id);
                assert_eq!(row["routing_context"]["state"], "recorded");
                assert_eq!(row["routing_context"]["plan_id"], plan);
                assert_eq!(row["final_native_model"], oracle::MODEL);
                let id = row["request_id"].as_str().unwrap();
                let receipt = facts(program, id);
                if let Some(fact) = usage(&receipt, phase) {
                    assert_usage(&receipt, fact, phase);
                    let values = receipt["facts"].as_array().unwrap();
                    assert!(
                        values
                            .windows(2)
                            .all(|pair| pair[0]["sequence"].as_u64().unwrap()
                                < pair[1]["sequence"].as_u64().unwrap())
                    );
                    matches.push(Receipt {
                        session: session_id.into(),
                        request: id.into(),
                        usage_sha256: value_digest(fact),
                        phase,
                    });
                }
            }
        }
        assert!(
            matches.len() <= 1,
            "more than one public receipt matched the one real request"
        );
        if let Some(receipt) = matches.pop() {
            return receipt;
        }
        assert!(
            Instant::now() < deadline,
            "actual upgrade Gateway request did not become publicly observable"
        );
        thread::sleep(Duration::from_millis(100));
    }
}
fn assert_receipt(program: &Path, plan: &str, saved: &Receipt) {
    let timeline = cli(
        program,
        &["sessions", "show"],
        Some(&observation("timeline", query(plan, Some(&saved.session)))),
    );
    let rows: Vec<_> = timeline["data"]["requests"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["request_id"] == saved.request)
        .collect();
    assert_eq!(
        rows.len(),
        1,
        "package maintenance lost the exact existing request identity"
    );
    assert_eq!(rows[0]["session_id"], saved.session);
    assert_eq!(rows[0]["routing_context"]["plan_id"], plan);
    assert_eq!(rows[0]["final_native_model"], oracle::MODEL);
    let facts = facts(program, &saved.request);
    let fact = usage(&facts, saved.phase)
        .expect("package maintenance lost the original actual usage fact");
    assert_usage(&facts, fact, saved.phase);
    assert_eq!(
        value_digest(fact),
        saved.usage_sha256,
        "an old request was replaced or its facts changed"
    );
}
