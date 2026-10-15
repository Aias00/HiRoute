//! Causal controls for the real fixture producer, without a provider account.
use super::*;

#[test]
fn qoder_tool_receipt_uses_bash_without_changing_other_native_shell_contracts() {
    let (name, script) = receipt_script(WorkerHarnessV1::QoderCli, "literal ' $ & 中文");
    assert_eq!(name, "receipt.sh");
    assert!(script.starts_with("printf '%s\\n' ") && !script.contains("Write-Output"));
    assert!(!uses_powershell(WorkerHarnessV1::QoderCli));
    for harness in [WorkerHarnessV1::CodexCli, WorkerHarnessV1::DeepseekHarness] {
        let (name, script) = receipt_script(harness, "literal ' receipt");
        assert_eq!(name, "receipt.ps1");
        assert_eq!(script, "Write-Output 'literal '' receipt'\n");
    }
    for harness in [WorkerHarnessV1::ClaudeCode, WorkerHarnessV1::Pi] {
        assert_eq!(receipt_script(harness, "receipt").0, "receipt.sh");
    }
}

#[cfg(unix)]
#[test]
fn qoder_tool_receipt_bash_execution_catches_the_former_powershell_producer() {
    let receipt = "literal ' $ & 中文";
    let (_, actual) = receipt_script(WorkerHarnessV1::QoderCli, receipt);
    let old = format!("Write-Output {}\n", ps_literal(receipt));
    let execute = |script: &str| {
        std::process::Command::new("/bin/bash")
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .args(["-c", script])
            .output()
            .unwrap()
    };
    let before = execute(&old);
    assert_eq!(before.status.code(), Some(127));
    assert!(before.stdout.is_empty());
    let after = execute(&actual);
    assert!(after.status.success() && after.stderr.is_empty());
    assert!(after.stdout == format!("{receipt}\n").as_bytes());
}

#[cfg(windows)]
#[test]
fn qoder_tool_bash_resources_bind_native_spellings_and_keep_empty_output_unproven() {
    use hiroute_platform::windows::security;
    use std::io::Write;
    let outer = tempfile::tempdir().unwrap();
    let root = outer.path().join("qoder tools 中文 ' ! &");
    security::create_owner_only_directory(&root).unwrap();
    let home = root.join("home");
    let config = root.join("config");
    let workspace = root.join("workspace");
    for path in [&home, &config, &workspace] {
        security::create_owner_only_directory(path).unwrap();
    }
    let mut fixture =
        ToolsFixture::prepare(WorkerHarnessV1::QoderCli, &home, &config, &workspace).unwrap();
    for skill in &fixture.skills {
        assert!(
            skill
                .script
                .extension()
                .is_some_and(|extension| extension == "sh")
        );
        assert!(
            fs::read_to_string(&skill.script)
                .unwrap()
                .starts_with("printf '%s\\n' ")
        );
    }
    let mut writer = security::open_read_write(&fixture.target).unwrap();
    writer.set_len(0).unwrap();
    writer
        .write_all(format!("{}\n", fixture.after).as_bytes())
        .unwrap();
    drop(writer);
    let command = fixture.qoder_bash_command().unwrap();
    assert!(
        command.starts_with("cat '")
            && command.contains(" && bash '")
            && !command.contains("Get-Content")
    );
    assert!(!command.contains(r"\\?\") && !command.contains("Write-Output"));
    assert!(
        fixture
            .qoder_bash_inputs
            .iter()
            .all(|binding| binding.verify().is_ok())
    );
    assert!(security::open_read_write(&fixture.target).is_err());
    assert!(
        fixture.assert_effects().is_err(),
        "empty output is not an executed nonce receipt"
    );
    let path = fixture.target.clone();
    drop(fixture);
    assert!(
        security::open_read_write(&path).is_ok(),
        "owned fixture leases end with their lifecycle"
    );
}

/// This is a native-filesystem/oracle control, not an upstream Agent execution.
/// The full Worker gate separately proves owned stop before this final check.
#[cfg(windows)]
#[test]
fn qoder_tool_final_effects_reject_late_same_identity_output_truncation() {
    use hiroute_platform::{file_security, windows::security};
    use std::io::Write;
    let outer = tempfile::tempdir().unwrap();
    let root = outer.path().join("qoder late output 中文 ' &");
    security::create_owner_only_directory(&root).unwrap();
    let home = root.join("home");
    let config = root.join("config");
    let workspace = root.join("workspace");
    for path in [&home, &config, &workspace] {
        security::create_owner_only_directory(path).unwrap();
    }
    let fixture =
        ToolsFixture::prepare(WorkerHarnessV1::QoderCli, &home, &config, &workspace).unwrap();
    let discovery = fixture
        .skills
        .iter()
        .map(|skill| skill.discovery.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let mut body = json!({"input":[{"type":"message","role":"user","content":[{"type":"input_text","text":discovery}]}],
        "tools":[{"name":"Skill"},{"name":"Read"},{"name":"Edit"},{"name":"Bash"}]});
    let append = |body: &mut Value, id: &str, output: String| {
        body["input"].as_array_mut().unwrap().push(json!({
            "type":"function_call_output","call_id":id,"is_error":false,"output":output
        }));
    };
    let mut oracle = ToolsOracle::new(fixture);
    oracle.respond(&body).unwrap();
    for id in [0, 1] {
        let text = fs::read_to_string(&oracle.fixture.skills[id].file).unwrap();
        append(&mut body, CALL_IDS[id], text);
        oracle.respond(&body).unwrap();
    }
    let text = fs::read_to_string(&oracle.fixture.target).unwrap();
    append(&mut body, CALL_IDS[2], text);
    oracle.respond(&body).unwrap();
    {
        let mut edited = security::open_read_write(&oracle.fixture.target).unwrap();
        edited.set_len(0).unwrap();
        writeln!(edited, "{}", oracle.fixture.after).unwrap();
        edited.sync_all().unwrap();
    }
    append(&mut body, CALL_IDS[3], oracle.fixture.after.clone());
    oracle.respond(&body).unwrap();
    let output = &oracle.fixture.shell_receipt;
    {
        let mut writer = security::open_read_write(output).unwrap();
        writeln!(writer, "{}", oracle.fixture.receipt).unwrap();
        writer.sync_all().unwrap();
    }
    let held = security::open_identity(output, false).unwrap();
    let identity = security::file_identity(&held).unwrap();
    let descriptor = file_security::capture(&held).unwrap();
    let text = std::iter::once(oracle.fixture.after.as_str())
        .chain(std::iter::once(oracle.fixture.receipt.as_str()))
        .chain(
            oracle
                .fixture
                .skills
                .iter()
                .flat_map(|skill| [skill.contents.as_str(), skill.executed.as_str()]),
        )
        .collect::<Vec<_>>()
        .join("\n");
    append(&mut body, CALL_IDS[4], text);
    oracle.respond(&body).unwrap();
    assert_eq!(oracle.next, CALL_IDS.len());
    oracle.assert_effects().unwrap();
    // Simulate the concrete late action after the correlated terminal request:
    // close the actual writer before evaluating the final stopped-scope oracle.
    {
        let writer = security::open_read_write(&oracle.fixture.shell_receipt).unwrap();
        writer.set_len(0).unwrap();
        writer.sync_all().unwrap();
    }
    assert_eq!(security::file_identity(&held).unwrap(), identity);
    assert!(file_security::capture(&held).unwrap() == descriptor);
    assert!(
        oracle
            .fixture
            .qoder_bash_inputs
            .iter()
            .all(|binding| binding.verify().is_ok())
    );
    assert!(
        oracle.assert_effects().is_err(),
        "a former live receipt cannot authorize changed final output bytes"
    );
}
