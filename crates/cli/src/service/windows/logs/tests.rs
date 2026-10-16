//! Actual kernel objects and the production diagnostic writer exercise the log reader.
//! These tests prove the reader boundary; they do not establish installed Task/SCM readiness.

use super::*;
use hiroute_diagnostics::event::{DiagnosticEvent, StageBegin, StartupStage};
use hiroute_diagnostics::level::DiagnosticLevel;
use hiroute_diagnostics::record::{Component, DiagnosticRecordV1};
use hiroute_diagnostics::runtime::{DiagnosticRuntime, RuntimeConfig};
use hiroute_platform::windows::security;
use hiroute_platform_test_support::junction;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

fn private_directory(path: &Path) {
    let file = security::create_owner_only_directory(path).unwrap();
    security::verify_owner_only_handle(&file, true).unwrap();
}

fn private_file(path: &Path, bytes: &[u8]) {
    let mut file = security::create_owner_only_file(path).unwrap();
    file.write_all(bytes).unwrap();
    file.sync_all().unwrap();
    security::verify_owner_only_handle(&file, false).unwrap();
}

fn root(outer: &tempfile::TempDir) -> PathBuf {
    let state = outer.path().join("state 空格");
    private_directory(&state);
    let root = state.join("diagnostics");
    private_directory(&root);
    root
}

fn old_path_read(path: &Path) -> Vec<u8> {
    // The replaced reader used ordinary File::open after directory enumeration.
    let mut file = std::fs::File::open(path).unwrap();
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).unwrap();
    bytes
}

#[test]
fn windows_service_logs_reads_actual_daemon_writer_and_bounds_rotated_tails() {
    let outer = tempfile::tempdir().unwrap();
    let path = root(&outer);
    let runtime = DiagnosticRuntime::start(RuntimeConfig {
        root: path.clone(),
        role: ProcessRole::Daemon,
        component: Component::Daemon,
        parent_session_id: None,
        level_override: Some(DiagnosticLevel::Debug),
    });
    runtime.emit(DiagnosticEvent::StageBegin(StageBegin {
        stage: StartupStage::ReadyWait,
    }));
    runtime.shutdown();
    let held = PrivateDir::open_existing(&path).unwrap();
    let role = held.open_existing_child("daemon").unwrap().unwrap();
    let mut previous = role.create_new(&previous_log_file(1)).unwrap();
    let mut bytes = b"discarded-before-tail".to_vec();
    let mut tail = vec![b'x'; MAX_LOG_BYTES];
    let suffix = b"actual-rotated-tail";
    tail[MAX_LOG_BYTES - suffix.len()..].copy_from_slice(suffix);
    bytes.extend_from_slice(&tail);
    previous.append(&bytes).unwrap();
    previous.sync().unwrap();
    drop(previous);
    private_file(&path.join("zz-unknown-root"), b"root-non-log-must-not-leak");
    private_file(
        &path.join("daemon/zz-unknown-role"),
        b"role-non-log-must-not-leak",
    );
    let layout = StandaloneLayout {
        home: outer.path().to_owned(),
        state_root: path.parent().unwrap().to_owned(),
        data_root: outer.path().join("data"),
        runtime_root: outer.path().join("run"),
        marker_path: outer.path().join("standalone.json"),
    };
    let envelope = read(&layout, "windows-task-user").unwrap();
    assert_eq!(envelope["schema"], "hiroute.standalone-service-logs/v1");
    assert_eq!(envelope["manager"], "windows-task-user");
    assert_eq!(envelope["diagnostics_root"], json!(path));
    let output = envelope["files"].as_array().unwrap();
    assert_eq!(output.len(), 2);
    assert_eq!(output[0]["file"], "daemon/current.jsonl");
    assert_eq!(output[1]["file"], "daemon/previous-1.jsonl");
    let current = output[0]["output"].as_str().unwrap();
    let records: Vec<_> = current
        .lines()
        .map(|line| DiagnosticRecordV1::parse_line(line.as_bytes()).unwrap())
        .collect();
    assert!(records.iter().any(|record| {
        matches!(
            record.event,
            DiagnosticEvent::StageBegin(StageBegin {
                stage: StartupStage::ReadyWait
            })
        )
    }));
    let rotated = output[1]["output"].as_str().unwrap();
    assert_eq!(rotated.as_bytes(), tail);
    assert_eq!(rotated.len(), MAX_LOG_BYTES);
    let serialized = serde_json::to_string(&output).unwrap();
    assert!(!serialized.contains("discarded-before-tail"));
    assert!(!serialized.contains("non-log-must-not-leak"));
    assert!(!serialized.contains("correlation.key"));
    assert!(!serialized.contains("settings.json"));
}

#[test]
fn windows_service_logs_refuses_hardlinks_and_junctions_before_reading_external_bytes() {
    let outer = tempfile::tempdir().unwrap();
    let path = root(&outer);
    let role_path = path.join("daemon");
    private_directory(&role_path);
    let outside = outer.path().join("outside");
    private_directory(&outside);
    let nonce = b"owned-external-log-refusal-nonce";
    private_file(&outside.join("external"), nonce);
    std::fs::hard_link(outside.join("external"), path.join("zz-old-root-link")).unwrap();
    std::fs::hard_link(outside.join("external"), role_path.join(CURRENT_LOG_FILE)).unwrap();
    // Both old directory classification and old ordinary reads admit the real links.
    let entry = std::fs::read_dir(&path)
        .unwrap()
        .map(Result::unwrap)
        .find(|entry| entry.file_name() == "zz-old-root-link")
        .unwrap();
    assert!(entry.file_type().unwrap().is_file());
    assert_eq!(old_path_read(&entry.path()), nonce);
    assert_eq!(old_path_read(&role_path.join(CURRENT_LOG_FILE)), nonce);
    let held = PrivateDir::open_existing(&path).unwrap();
    assert!(matches!(
        read_files(&held),
        Err(FileSafetyError::UnsafeFile)
    ));
    std::fs::remove_file(role_path.join(CURRENT_LOG_FILE)).unwrap();
    std::fs::remove_dir(&role_path).unwrap();
    private_file(&outside.join(CURRENT_LOG_FILE), nonce);
    junction::create(&role_path, &outside).unwrap();
    assert_eq!(old_path_read(&role_path.join(CURRENT_LOG_FILE)), nonce);
    assert!(read_files(&held).is_err());
    std::fs::remove_dir(&role_path).unwrap();
    // A valid log remains readable after refusing the hostile entries. Unknown root
    // hardlinks are ignored; neither the reader nor cleanup changes external bytes.
    private_directory(&role_path);
    private_file(&role_path.join(CURRENT_LOG_FILE), b"legitimate-log");
    let output = read_files(&held).unwrap();
    assert_eq!(output.len(), 1);
    assert_eq!(output[0]["output"], "legitimate-log");
    assert!(!serde_json::to_string(&output).unwrap().contains("nonce"));
    assert_eq!(old_path_read(&outside.join("external")), nonce);
    assert_eq!(old_path_read(&outside.join(CURRENT_LOG_FILE)), nonce);
    std::fs::remove_file(path.join("zz-old-root-link")).unwrap();
}

#[test]
fn windows_service_logs_keeps_held_root_identity_and_does_not_create_missing_roles() {
    let outer = tempfile::tempdir().unwrap();
    let path = root(&outer);
    let held = PrivateDir::open_existing(&path).unwrap();
    assert!(read_files(&held).unwrap().is_empty());
    assert_eq!(held.list_names().unwrap(), Vec::<String>::new());
    assert!(!path.join("daemon").exists());
    assert!(held.open_existing_child("../outside").is_err());
    private_directory(&path.join("daemon"));
    private_file(&path.join("daemon/current.jsonl"), b"held-original-log");
    let original_identity = held.identity();
    let moved = outer.path().join("moved-original");
    std::fs::rename(&path, &moved).unwrap();
    private_directory(&path);
    private_directory(&path.join("daemon"));
    private_file(
        &path.join("daemon/current.jsonl"),
        b"replacement-must-not-be-read",
    );
    assert_eq!(held.identity(), original_identity);
    let output = read_files(&held).unwrap();
    assert_eq!(output.len(), 1);
    assert_eq!(output[0]["output"], "held-original-log");
    assert_eq!(
        old_path_read(&path.join("daemon/current.jsonl")),
        b"replacement-must-not-be-read"
    );
    assert_eq!(
        old_path_read(&moved.join("daemon/current.jsonl")),
        b"held-original-log"
    );
}
