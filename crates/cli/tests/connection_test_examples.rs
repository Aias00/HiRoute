//! Exercise the installed connection-test help and request examples as a caller sees them.
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    io::Write,
    path::Path,
    process::{Command, Output, Stdio},
};

fn invoke(root: &Path, args: &[&str], input: &[u8]) -> Output {
    let root = root.canonicalize().unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_hiroute"))
        .args(args)
        .current_dir(&root)
        .env("HOME", &root)
        .env("HIROUTE_RUNTIME_DIR", &root)
        .env_remove("XDG_STATE_HOME")
        .env_remove("XDG_DATA_HOME")
        .env_remove("XDG_RUNTIME_DIR")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(input).unwrap();
    child.wait_with_output().unwrap()
}

fn contract(root: &Path) -> Value {
    let output = invoke(
        root,
        &[
            "schema",
            "show",
            "--command-id",
            "compute.connection.test",
            "--include-schema",
            "--output",
            "json",
        ],
        b"",
    );
    assert!(output.status.success());
    serde_json::from_slice::<Value>(&output.stdout).unwrap()["data"].clone()
}

#[test]
fn published_connection_test_commands_declare_their_request_source() {
    let root = tempfile::tempdir().unwrap();
    let contract = contract(root.path());
    let examples = contract["descriptor"]["help"]["examples"]
        .as_array()
        .unwrap();
    let commands: Vec<_> = examples
        .iter()
        .filter_map(Value::as_str)
        .filter(|example| example.starts_with("hiroute compute connection test "))
        .collect();
    assert!(!commands.is_empty());
    for example in commands {
        assert!(
            example.contains("--request-file ")
                || (example.contains("--request-stdin ") && example.contains(" < ")),
            "published example omits its request source: {example}"
        );
    }
}

#[test]
fn published_native_and_registered_examples_execute_the_displayed_commands() {
    let root = tempfile::tempdir().unwrap();
    let contract = contract(root.path());
    let requests = contract["schemas"]["examples"].as_array().unwrap();
    let kinds: BTreeSet<_> = requests
        .iter()
        .map(|request| request["kind"].as_str().unwrap())
        .collect();
    assert_eq!(kinds, BTreeSet::from(["native", "registered"]));
    for request in requests {
        let kind = request["kind"].as_str().unwrap();
        let inference_path = if kind == "native" {
            "/request/draft/inference_model_id"
        } else {
            "/request/inference_model_id"
        };
        assert_eq!(request.pointer(inference_path), Some(&Value::Null));
        let file = root.path().join(format!("{kind}-request.json"));
        std::fs::write(file, serde_json::to_vec(request).unwrap()).unwrap();
    }

    let help = invoke(
        root.path(),
        &["compute", "connection", "test", "--help"],
        b"",
    );
    assert!(help.status.success());
    let help = String::from_utf8(help.stdout).unwrap();
    let mut executed = BTreeSet::new();
    for example in contract["descriptor"]["help"]["examples"]
        .as_array()
        .unwrap()
    {
        let example = example.as_str().unwrap();
        assert!(help.contains(example));
        let mut args: Vec<_> = example.split_ascii_whitespace().collect();
        assert_eq!(args.remove(0), "hiroute");
        let mut input = Vec::new();
        let document = if let Some(index) = args.iter().position(|arg| *arg == "<") {
            assert_eq!(index + 2, args.len());
            let document = args[index + 1];
            input = std::fs::read(root.path().join(document)).unwrap();
            args.truncate(index);
            Some(document)
        } else {
            args.iter()
                .position(|arg| *arg == "--request-file")
                .map(|index| args[index + 1])
        };
        let output = invoke(root.path(), &args, &input);
        if let Some(document) = document {
            assert_eq!(
                output.status.code(),
                Some(6),
                "{example}: {}",
                String::from_utf8_lossy(&output.stdout)
            );
            let envelope: Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(envelope["error"]["code"], "DAEMON_UNAVAILABLE");
            executed.insert(document.to_owned());
        } else {
            assert!(output.status.success(), "{example}");
        }
    }
    assert_eq!(
        executed,
        BTreeSet::from([
            "native-request.json".to_owned(),
            "registered-request.json".to_owned()
        ])
    );
    let guidance = contract["schemas"]["example_guidance"].as_str().unwrap();
    for prerequisite in [
        "compute connection options",
        "protected-input register",
        "expected_catalog",
        "inference_model_id",
        "reachability=not_run",
        "authentication=unknown",
        "directory=not_run",
        "inference=not_run",
    ] {
        assert!(
            guidance.contains(prerequisite),
            "missing prerequisite: {prerequisite}"
        );
    }
}

#[test]
fn reported_invalid_connection_requests_have_safe_actionable_diagnostics() {
    let root = tempfile::tempdir().unwrap();
    let flat = serde_json::to_vec(&json!({
        "schema":"hiroute.compute-connection-test-request/v1", "kind":"native",
        "connection_option_id":"openrouter.free.global.v1",
        "endpoint_profile_id":"endpoint.openrouter.free.global.v1", "endpoint_profile_revision":1,
        "authentication":{"kind":"provider_api_key", "input_ref":{"candidate":"PRIVATE_INPUT_MARKER"}},
        "probe":{"kind":"inventory", "model_selection":{"mode":"none"}}
    })).unwrap();
    let cases = [
        (
            vec!["--non-interactive", "--output", "json"],
            b"".as_slice(),
        ),
        (vec!["--request-stdin", "--output", "json"], b"".as_slice()),
        (vec!["--request-stdin", "--output", "json"], flat.as_slice()),
        (
            vec!["--request-stdin", "--output", "json"],
            br#"{"schema":"hiroute.compute-connection-test-request/v1"}"#.as_slice(),
        ),
        (
            vec!["--request-stdin", "--output.json"],
            br#"{"schema":"hiroute.compute-connection-test-request/v1"}"#.as_slice(),
        ),
        (
            vec!["--request-stdin", "--output", "json"],
            br#"{"kind":"registered"}"#.as_slice(),
        ),
    ];
    for (options, input) in cases {
        for rich in [false, true] {
            let mut args = vec!["compute", "connection", "test"];
            args.extend(options.iter().copied());
            if rich {
                args.push("--error-details");
            }
            let output = invoke(root.path(), &args, input);
            assert_eq!(output.status.code(), Some(2));
            let envelope: Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(envelope["error"]["code"], "INVALID_ARGUMENTS");
            assert_eq!(envelope["status"], "usage_error");
            if rich {
                assert!(envelope["data"]["diagnostic"]["path"].is_string());
                assert!(envelope["data"]["diagnostic"]["category"].is_string());
            }
            let stderr = String::from_utf8_lossy(&output.stderr);
            assert!(stderr.contains(
                "hiroute schema show --command-id compute.connection.test --include-schema"
            ));
            for bytes in [&output.stdout, &output.stderr] {
                assert!(bytes.len() < 4096);
                assert!(!String::from_utf8_lossy(bytes).contains("PRIVATE_INPUT_MARKER"));
            }
        }
    }
}
