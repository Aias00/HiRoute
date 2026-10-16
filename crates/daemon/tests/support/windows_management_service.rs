//! Real package background doctor/logs, callable by the Task/SCM CLI fixture.
//! Caller performs the bounded native process launch, guards its captured output,
//! and returns the public envelope. This module needs no foreground fixture types.
use serde_json::Value;

/// Only a genuinely installed, ready Task/SCM package may call this helper.
/// Execute in its actual owner context: the independent held-directory read
/// must not use an administrator's authority to bypass another owner's ACL.
/// The foreground management journey does not call this helper; absence of its execution is not positive log coverage.
#[allow(dead_code)]
pub fn run(cli: &mut impl FnMut(&[&str]) -> Value) {
    let doctor = cli(&["service", "doctor"]);
    assert_eq!(doctor["status"], "succeeded");
    assert_doctor(&doctor, true);
    let logs = cli(&["service", "logs"]);
    assert_eq!(logs["status"], "succeeded");
    assert_eq!(logs["data"]["schema"], "hiroute.standalone-service-logs/v1");
    assert!(matches!(
        logs["data"]["manager"].as_str(),
        Some("windows-task-user" | "windows-scm-user")
    ));
    let root: std::path::PathBuf =
        serde_json::from_value(logs["data"]["diagnostics_root"].clone()).unwrap();
    let held = hiroute_diagnostics::files::PrivateDir::open_existing(&root).unwrap();
    let daemon = held.open_existing_child("daemon").unwrap().unwrap();
    let files = logs["data"]["files"].as_array().unwrap();
    assert!(!files.is_empty() && files.len() <= 2);
    let mut names = std::collections::BTreeSet::new();
    let mut records = 0;
    for file in files {
        let name = file["file"].as_str().unwrap();
        assert!(matches!(
            name,
            "daemon/current.jsonl" | "daemon/previous-1.jsonl"
        ));
        assert!(names.insert(name));
        let native_name = name.strip_prefix("daemon/").unwrap();
        let mut native = daemon.open_read(native_name).unwrap().unwrap();
        native.recheck().unwrap();
        let real = native.read_tail(256 * 1024).unwrap();
        native.recheck().unwrap();
        assert!(
            !real.is_empty(),
            "logs must name a real nonempty daemon writer file"
        );
        let actual: Vec<_> = real
            .split(|byte| *byte == b'\n')
            .filter_map(|line| {
                hiroute_diagnostics::record::DiagnosticRecordV1::parse_line(line).ok()
            })
            .collect();
        let output = file["output"].as_str().unwrap();
        assert!(!output.is_empty() && output.len() <= 256 * 1024 + 2);
        // A byte tail may begin inside its first JSON line. Every subsequent
        // line must be an actual typed diagnostic record, not raw provider text.
        let mut matched = false;
        for (index, line) in output.lines().enumerate() {
            match hiroute_diagnostics::record::DiagnosticRecordV1::parse_line(line.as_bytes()) {
                Ok(record) => {
                    // The daemon writer can also contain its Gateway/CPA subsystem
                    // records; the held role directory supplies process identity.
                    matched |= actual.contains(&record);
                    records += 1;
                }
                Err(_) if index == 0 && output.len() >= 256 * 1024 => {}
                Err(_) => {
                    panic!("public installed logs must contain actual closed diagnostic records")
                }
            }
        }
        assert!(
            matched,
            "public log records must correlate to the held daemon writer file"
        );
    }
    assert!(
        records > 0,
        "positive installed logs require real daemon records"
    );
}

pub(super) fn assert_doctor(envelope: &Value, managed: bool) {
    let data = &envelope["data"];
    assert_eq!(data["schema"], "hiroute.standalone-service-doctor/v1");
    assert_eq!(data["healthy"], managed);
    let checks = data["checks"].as_array().unwrap();
    assert_eq!(checks.len(), 5);
    for (name, expected) in [
        ("install_marker", true),
        ("hiroute_binary", true),
        ("hirouted_binary", true),
        ("service_definition", managed),
        ("local_control", true),
    ] {
        let selected: Vec<_> = checks
            .iter()
            .filter(|entry| entry["name"] == name)
            .collect();
        assert_eq!(selected.len(), 1);
        assert_eq!(
            selected[0]["ok"], expected,
            "doctor check must report actual installed state: {name}"
        );
    }
}
