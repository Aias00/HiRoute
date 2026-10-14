//! One local challenge: native Skill discovery, its contents read, and the installed CLI run.
//! Windows first requires a real denied write as an independent sandbox control.
//! The endpoint never forwards model requests.
use serde_json::{Value, json};
use std::{io::Write, path::Path, process::Command};

mod launch_diagnostic;
mod readonly_control;
mod tool_diagnostic;
#[cfg(test)]
use launch_diagnostic::NativeLaunchStage;
#[cfg(test)]
use launch_diagnostic::native_launch_diagnostic;
use tool_diagnostic::{SelectedTool, ToolResultDiagnostic};

pub(crate) struct CollaborationChallenge {
    discovery: String,
    contents: String,
    command: String,
    expected_cli: String,
    readonly: Option<readonly_control::ReadOnlyControl>,
    selected_tool: SelectedTool,
    issued: bool,
    completed: bool,
}

impl CollaborationChallenge {
    pub(crate) fn prepare(
        home: &Path,
        codex_home: &Path,
        cli: &Path,
    ) -> Result<Self, &'static str> {
        // a956835d ext/skills/src/host_roots.rs binds .agents to dirs::home_dir().
        // Windows resolves that through KnownFolder Profile, ignoring this private HOME.
        // The same upstream supports the selected CODEX_HOME/skills as a user root.
        #[cfg(windows)]
        let skills = codex_home.join("skills");
        #[cfg(not(windows))]
        let skills = {
            let _ = codex_home;
            let parent = home.join(".agents");
            super::probe_files::directory(&parent).map_err(|_| "private skill directory")?;
            parent.join("skills")
        };
        Self::prepare_in(&skills, home, cli, cfg!(windows))
    }

    pub(crate) fn prepare_claude(
        config: &Path,
        home: &Path,
        cli: &Path,
    ) -> Result<(Self, std::path::PathBuf), &'static str> {
        let plugin = config.join("hiroute-native-probe");
        let manifest = plugin.join(".claude-plugin");
        for directory in [&plugin, &manifest] {
            super::probe_files::directory(directory).map_err(|_| "private Skill plugin")?;
        }
        super::probe_files::file(&manifest.join("plugin.json"))
            .and_then(|mut file| {
                file.write_all(br#"{"name":"hiroute-native-probe","version":"0.1.0"}"#)
            })
            .map_err(|_| "private Skill plugin")?;
        Ok((
            Self::prepare_in(&plugin.join("skills"), home, cli, false)?,
            plugin,
        ))
    }

    fn prepare_in(
        skills: &Path,
        home: &Path,
        cli: &Path,
        powershell: bool,
    ) -> Result<Self, &'static str> {
        let mut entropy = [0u8; 16];
        getrandom::fill(&mut entropy).map_err(|_| "challenge entropy")?;
        let suffix: String = entropy.iter().map(|byte| format!("{byte:02x}")).collect();
        let discovery = format!("hiroute-discovery-{suffix}");
        let contents = format!("hiroute-contents-{suffix}");
        let root = skills.join("hiroute-probe");
        for directory in [skills, root.as_path()] {
            super::probe_files::directory(directory).map_err(|_| "private skill directory")?;
        }
        let path = root.join("SKILL.md");
        let text = format!(
            "---\nname: hiroute-probe\ndescription: Read-only HiRoute compatibility check {discovery}\n---\n\nRead this file and run the installed hiroute schema list --output json.\nContent receipt: {contents}\n"
        );
        super::probe_files::file(&path)
            .and_then(|mut file| file.write_all(text.as_bytes()))
            .map_err(|_| "private skill")?;
        let mut command = Command::new(cli);
        command
            .args(["schema", "list", "--output", "json"])
            .env_clear()
            .env("HOME", home)
            .env("PATH", super::native_ingress_probe::native_probe_path());
        #[cfg(unix)]
        let (bytes, success) = {
            let output = command.output().map_err(|_| "trusted CLI baseline")?;
            (output.stdout, output.status.success())
        };
        #[cfg(windows)]
        let (bytes, success) = (
            super::windows_runtime::bounded(
                &command,
                false,
                std::time::Duration::from_secs(3),
                256 * 1024,
            )
            .map_err(|_| "trusted CLI baseline")?
            .to_vec(),
            true,
        );
        let expected_cli = String::from_utf8(bytes).map_err(|_| "CLI output encoding")?;
        let envelope: Value = serde_json::from_str(&expected_cli).map_err(|_| "CLI contract")?;
        if !success
            || envelope["status"] != "succeeded"
            || !envelope["data"]["commands"]
                .as_array()
                .is_some_and(|commands| {
                    commands
                        .iter()
                        .any(|command| command["command_id"] == "schema.list")
                })
        {
            return Err("trusted CLI contract");
        }
        fn quote(path: &Path) -> Result<String, &'static str> {
            Ok(format!(
                "'{}'",
                path.to_str()
                    .ok_or("CLI path encoding")?
                    .replace('\'', "'\\''")
            ))
        }
        let command = if powershell {
            let quote = |path: &Path| {
                path.to_str()
                    .map(|path| format!("'{}'", path.replace('\'', "''")))
                    .ok_or("CLI path encoding")
            };
            format!(
                "Get-Content -Raw -LiteralPath {}; & {} schema list --output json",
                quote(&path)?,
                quote(cli)?
            )
        } else {
            let path = if cfg!(windows) {
                std::path::PathBuf::from(
                    path.to_string_lossy()
                        .trim_start_matches(r"\\?\")
                        .replace('\\', "/"),
                )
            } else {
                path.clone()
            };
            let cli = if cfg!(windows) {
                std::path::PathBuf::from(
                    cli.to_string_lossy()
                        .trim_start_matches(r"\\?\")
                        .replace('\\', "/"),
                )
            } else {
                cli.to_owned()
            };
            format!(
                "/bin/cat {}; {} schema list --output json",
                quote(&path)?,
                quote(&cli)?
            )
        };
        let readonly = None;
        #[cfg(windows)]
        let readonly = if powershell {
            Some(readonly_control::ReadOnlyControl::new(home, &suffix)?)
        } else {
            readonly
        };
        Ok(Self {
            discovery,
            contents,
            command,
            expected_cli: expected_cli.trim().into(),
            readonly,
            selected_tool: SelectedTool::Unknown,
            issued: false,
            completed: false,
        })
    }

    pub(crate) fn complete(&self) -> bool {
        self.completed
    }

    pub(crate) fn verify_after_stop(&self) -> Result<(), &'static str> {
        self.readonly
            .as_ref()
            .map_or(Ok(()), readonly_control::ReadOnlyControl::verify_after_stop)
    }

    pub(crate) fn reply(&mut self, body: &Value) -> Result<Value, &'static str> {
        self.reply_observed(body, ToolResultDiagnostic::emit)
    }

    fn reply_observed(
        &mut self,
        body: &Value,
        mut observe: impl FnMut(&ToolResultDiagnostic),
    ) -> Result<Value, &'static str> {
        if !self.issued {
            // A nonce absent from the user prompt must have come from native Skill discovery.
            if !body.to_string().contains(&self.discovery) {
                return Err("native Skill discovery");
            }
            let tools = body["tools"].as_array().ok_or("native tool declarations")?;
            let name = tools
                .iter()
                .find_map(|tool| {
                    let name = tool["name"].as_str()?;
                    matches!(name, "exec_command" | "shell_command").then_some(name)
                })
                .ok_or("native read-only shell tool")?;
            self.selected_tool = SelectedTool::from_name(name);
            self.issued = true;
            let command = self.readonly.as_ref().map_or_else(
                || self.command.clone(),
                |control| format!("{}; {}", control.command(), self.command),
            );
            return Ok(shell_call(name, &command));
        }
        let result = body["input"].as_array().and_then(|input| {
            input.iter().find(|item| {
                item["type"] == "function_call_output" && item["call_id"] == "call_hiroute_probe"
            })
        });
        // Codex 0.160.0 serializes native tool output as a string OR input_text
        // content items (protocol/models.rs at a956835d). Match only that exact
        // correlated result; unrelated prompt text, images or metadata cannot prove it.
        let output = result.and_then(|item| responses_tool_text(&item["output"]));
        let diagnostic = ToolResultDiagnostic::capture(
            self.selected_tool,
            result,
            output.as_deref(),
            &self.contents,
            &self.expected_cli,
            self.readonly
                .as_ref()
                .map(|control| control.receipt_facts(output.as_deref().unwrap_or_default())),
        );
        // Observe before correlation/readonly rejection: the old early return
        // discarded the only native tool result's diagnostic facts.
        observe(&diagnostic);
        if let Some(control) = self.readonly.as_mut() {
            let output = output
                .as_deref()
                .ok_or("native read-only correlated result")?;
            control.confirm(output)?;
        }
        let content_proved = output
            .as_ref()
            .is_some_and(|text| text.contains(&self.contents));
        let cli_proved = output
            .as_ref()
            .is_some_and(|text| text.contains(&self.expected_cli));
        let proved = content_proved && cli_proved;
        if !proved {
            return Err("native Skill read and trusted CLI execution");
        }
        self.completed = true;
        Ok(
            json!([{"id":"msg_probe", "type":"message", "status":"completed", "role":"assistant", "content":[{"type":"output_text", "text":"OK", "annotations":[]}]}]),
        )
    }

    pub(crate) fn reply_claude(
        &mut self,
        body: &Value,
    ) -> Result<(Value, &'static str), &'static str> {
        if !self.issued {
            // The explicit Claude plugin expands the Skill body but omits its front matter.
            // This private nonce is absent from the user prompt and proves Skill loading.
            if !body.to_string().contains(&self.contents) {
                return Err("native Skill discovery");
            }
            if !body["tools"]
                .as_array()
                .is_some_and(|tools| tools.iter().any(|tool| tool["name"] == "Bash"))
            {
                return Err("native read-only shell tool");
            }
            self.issued = true;
            return Ok((
                json!({"type":"tool_use", "id":"toolu_hiroute_probe", "name":"Bash",
                "input":{"command":self.command, "timeout":10000}}),
                "tool_use",
            ));
        }
        let proved = body["messages"].as_array().is_some_and(|messages| {
            messages.iter().any(|message| {
                message["content"].as_array().is_some_and(|blocks| {
                    blocks.iter().any(|block| {
                        block["type"] == "tool_result"
                            && block["tool_use_id"] == "toolu_hiroute_probe"
                            && block["is_error"] != true
                            && block["content"]
                                .as_str()
                                .map(str::to_owned)
                                .or_else(|| {
                                    block["content"].as_array().map(|parts| {
                                        parts
                                            .iter()
                                            .filter_map(|part| part["text"].as_str())
                                            .collect::<Vec<_>>()
                                            .join("\n")
                                    })
                                })
                                .is_some_and(|output| {
                                    output.contains(&self.contents)
                                        && output.contains(&self.expected_cli)
                                })
                    })
                })
            })
        });
        if !proved {
            return Err("native Skill read and trusted CLI execution");
        }
        self.completed = true;
        Ok((json!({"type":"text", "text":"OK"}), "end_turn"))
    }
}

fn shell_call(name: &str, command: &str) -> Value {
    let args = if name == "exec_command" {
        json!({"cmd":command, "max_output_tokens":2000, "yield_time_ms":1000, "login":false})
    } else {
        json!({"command":command, "timeout_ms":10000})
    };
    json!([{"type":"function_call", "id":"fc_hiroute_probe", "call_id":"call_hiroute_probe", "name":name, "arguments":args.to_string(), "status":"completed"}])
}

fn responses_tool_text(output: &Value) -> Option<String> {
    if let Some(text) = output.as_str() {
        return Some(text.to_owned());
    }
    let text = output
        .as_array()?
        .iter()
        .filter_map(|item| {
            (item["type"] == "input_text")
                .then(|| item["text"].as_str())
                .flatten()
        })
        .collect::<Vec<_>>();
    (!text.is_empty()).then(|| text.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn challenge() -> CollaborationChallenge {
        CollaborationChallenge {
            discovery: "discovery-nonce".into(),
            contents: "contents-nonce".into(),
            command: "/trusted/hiroute schema list --output json".into(),
            expected_cli: "CLI-contract-output".into(),
            readonly: None,
            selected_tool: SelectedTool::Unknown,
            issued: false,
            completed: false,
        }
    }

    #[test]
    fn codex_readonly_control_precedes_the_skill_and_cli_and_requires_a_correlated_denial() {
        let root = tempfile::tempdir().unwrap();
        let mut check = challenge();
        check.readonly =
            Some(readonly_control::ReadOnlyControl::new(root.path(), "nonce").unwrap());
        let tools = json!([{"name":"exec_command"}]);
        let first = check
            .reply(&json!({"input":"discovery-nonce", "tools":tools}))
            .unwrap();
        assert_eq!(first[0]["call_id"], "call_hiroute_probe");
        let args: Value = serde_json::from_str(first[0]["arguments"].as_str().unwrap()).unwrap();
        assert!(
            args["cmd"]
                .as_str()
                .unwrap()
                .contains("[System.IO.File]::WriteAllText(")
        );
        assert!(args["cmd"].as_str().unwrap().ends_with(&check.command));
        for (call, output) in [
            (
                "wrong-call",
                "hiroute-readonly-attempt-nonce\nhiroute-readonly-denied-nonce\ncontents-nonce CLI-contract-output",
            ),
            ("call_hiroute_probe", "contents-nonce CLI-contract-output"),
            (
                "call_hiroute_probe",
                "hiroute-readonly-attempt-nonce\nhiroute-readonly-denied-nonce",
            ),
        ] {
            assert!(check.reply(&json!({"tools":tools,"input":[{"type":"function_call_output", "call_id":call,"output":output}]})).is_err());
            assert!(!check.complete());
        }
        check.reply(&json!({"tools":tools,"input":[{"type":"function_call_output", "call_id":"call_hiroute_probe","output":[{"type":"input_text","text":"hiroute-readonly-attempt-nonce\nhiroute-readonly-denied-nonce\ncontents-nonce CLI-contract-output"}]}]})).unwrap();
        assert!(check.complete());
        check.verify_after_stop().unwrap();
    }

    #[test]
    fn readonly_rejection_observes_closed_tool_facts_before_early_return() {
        use tool_diagnostic::{OutputShape, ReturnPhase};
        let root = tempfile::tempdir().unwrap();
        let mut check = challenge();
        check.readonly =
            Some(readonly_control::ReadOnlyControl::new(root.path(), "nonce").unwrap());
        check
            .reply(&json!({"input":"discovery-nonce", "tools":[{"name":"exec_command"}]}))
            .unwrap();
        let text = "Wall time: 0.0142 seconds\nProcess exited with code -1073741502\nOutput:\nprivate-path-token ParserError";
        let body = json!({"input":[{"type":"function_call_output","call_id":"call_hiroute_probe","output":[{"type":"input_text","text":text}]}]});
        let mut observations = 0;
        let result = check.reply_observed(&body, |facts| {
            observations += 1;
            assert_eq!(facts.selected_tool, SelectedTool::ExecCommand);
            assert!(facts.correlated);
            assert_eq!(facts.output_shape, OutputShape::ContentItems);
            assert_eq!(facts.text_items, 1);
            assert_eq!(facts.output_bytes, text.len());
            assert_eq!(facts.header.phase, ReturnPhase::TerminalReturned);
            assert_eq!(facts.header.terminal_code, Some(-1073741502));
            assert!(facts.powershell_parser);
            assert!(facts.dll_init);
            assert_eq!(facts.receipts.as_ref().unwrap().attempt_matches, 0);
            assert_eq!(facts.receipts.as_ref().unwrap().denial_matches, 0);
            assert!(!format!("{facts:?}").contains("private-path-token"));
        });
        assert_eq!(result, Err("native read-only write rejection"));
        // Moving observe after confirm (the old order) makes this regression fail.
        assert_eq!(observations, 1);
        assert!(!check.complete());
        assert!(check.verify_after_stop().is_err());
    }

    #[test]
    fn readonly_facts_preserve_missing_correlation_and_echo_duplicate_gates() {
        use tool_diagnostic::OutputShape;
        let root = tempfile::tempdir().unwrap();
        let mut check = challenge();
        check.readonly =
            Some(readonly_control::ReadOnlyControl::new(root.path(), "nonce").unwrap());
        check
            .reply(&json!({"input":"discovery-nonce", "tools":[{"name":"shell_command"}]}))
            .unwrap();
        let mut observed = false;
        assert_eq!(check.reply_observed(&json!({"input":[{"type":"function_call_output","call_id":"wrong-call","output":"contents-nonce CLI-contract-output"}]}), |facts| {
            observed = true;
            assert_eq!(facts.selected_tool, SelectedTool::ShellCommand);
            assert!(!facts.correlated);
            assert_eq!(facts.output_shape, OutputShape::Missing);
            assert_eq!(facts.output_bytes, 0);
        }), Err("native read-only correlated result"));
        assert!(observed);
        for (text, attempt_count, denial_count) in [
            (
                "echo 'hiroute-readonly-attempt-nonce'; echo 'hiroute-readonly-denied-nonce'; contents-nonce CLI-contract-output",
                0,
                0,
            ),
            (
                "hiroute-readonly-attempt-nonce\nhiroute-readonly-attempt-nonce\nhiroute-readonly-denied-nonce\ncontents-nonce CLI-contract-output",
                2,
                1,
            ),
        ] {
            assert_eq!(check.reply_observed(&json!({"input":[{"type":"function_call_output","call_id":"call_hiroute_probe","output":text}]}), |facts| {
                let receipt = facts.receipts.as_ref().unwrap();
                assert_eq!(receipt.attempt_matches, attempt_count);
                assert_eq!(receipt.denial_matches, denial_count);
                assert!(receipt.attempt_substring);
                assert!(receipt.denial_substring);
                assert!(facts.skill_content && facts.cli_output);
            }), Err("native read-only write rejection"));
            assert!(!check.complete());
        }
        let text = "hiroute-readonly-attempt-nonce\nhiroute-readonly-denied-nonce\ncontents-nonce CLI-contract-output";
        let mut facts_seen = false;
        check.reply_observed(&json!({"input":[{"type":"function_call_output","call_id":"call_hiroute_probe","output":text}]}), |facts| {
            facts_seen = true;
            assert_eq!(facts.receipts.as_ref().unwrap().attempt_matches, 1);
            assert_eq!(facts.receipts.as_ref().unwrap().denial_matches, 1);
        }).unwrap();
        assert!(facts_seen && check.complete());
        check.verify_after_stop().unwrap();
    }

    #[test]
    fn collaboration_requires_native_discovery_and_correlated_content_and_cli_output() {
        let mut check = challenge();
        assert!(
            check
                .reply(&json!({"tools":[{"name":"exec_command"}]}))
                .is_err()
        );
        let call = check
            .reply(&json!({"input":"discovery-nonce", "tools":[{"name":"exec_command"}]}))
            .unwrap();
        assert_eq!(call[0]["call_id"], "call_hiroute_probe");
        let arguments: Value =
            serde_json::from_str(call[0]["arguments"].as_str().unwrap()).unwrap();
        assert_eq!(arguments["login"], false);
        assert!(!check.complete());
        for (id, output) in [
            ("wrong-call", "contents-nonce CLI-contract-output"),
            ("call_hiroute_probe", "contents-nonce"),
            ("call_hiroute_probe", "CLI-contract-output"),
        ] {
            assert!(check.reply(&json!({"input":[{"type":"function_call_output", "call_id":id, "output":output}]})).is_err());
            assert!(!check.complete());
        }
        check.reply(&json!({"input":[{"type":"function_call_output", "call_id":"call_hiroute_probe", "output":"contents-nonce CLI-contract-output"}]})).unwrap();
        assert!(check.complete());
    }

    #[test]
    fn codex_collaboration_accepts_correlated_native_text_content_items_only() {
        let mut check = challenge();
        check
            .reply(&json!({"input":"discovery-nonce", "tools":[{"name":"exec_command"}]}))
            .unwrap();
        for output in [
            json!([{"type":"input_image","text":"contents-nonce CLI-contract-output"}]),
            json!([{"type":"input_text","text":"contents-nonce"}]),
            json!({"text":"contents-nonce CLI-contract-output"}),
        ] {
            assert!(check.reply(&json!({"input":[{"type":"function_call_output","call_id":"call_hiroute_probe","output":output}]})).is_err());
            assert!(!check.complete());
        }
        let output = json!([{"type":"input_text","text":"contents-nonce"},
            {"type":"input_text","text":"CLI-contract-output"}]);
        assert!(check.reply(&json!({"input":[{"type":"function_call_output","call_id":"wrong-call","output":output}]})).is_err());
        check.reply(&json!({"input":[{"type":"function_call_output","call_id":"call_hiroute_probe","output":output}]})).unwrap();
        assert!(check.complete());
    }

    #[test]
    fn claude_collaboration_requires_discovered_skill_and_correlated_tool_result() {
        let mut check = challenge();
        check.expected_cli = r#"{"status":"succeeded"}"#.into();
        assert!(
            check
                .reply_claude(&json!({"tools":[{"name":"Bash"}]}))
                .is_err()
        );
        let (tool, stop) = check
            .reply_claude(
                &json!({"messages":[{"content":"contents-nonce"}], "tools":[{"name":"Bash"}]}),
            )
            .unwrap();
        assert_eq!(tool["id"], "toolu_hiroute_probe");
        assert_eq!(stop, "tool_use");
        for (id, output) in [
            ("wrong-tool", "contents-nonce {\"status\":\"succeeded\"}"),
            ("toolu_hiroute_probe", "contents-nonce"),
            ("toolu_hiroute_probe", "{\"status\":\"succeeded\"}"),
        ] {
            assert!(check.reply_claude(&json!({"messages":[{"content":[{"type":"tool_result", "tool_use_id":id, "content":output}]}]})).is_err());
            assert!(!check.complete());
        }
        assert!(check.reply_claude(&json!({"messages":[{"content":[{"type":"tool_result", "tool_use_id":"toolu_hiroute_probe", "is_error":true, "content":"contents-nonce {\"status\":\"succeeded\"}"}]}]})).is_err());
        assert!(!check.complete());
        let (answer, stop) = check.reply_claude(&json!({"messages":[{"content":[{"type":"tool_result", "tool_use_id":"toolu_hiroute_probe", "content":"contents-nonce {\"status\":\"succeeded\"}"}]}]})).unwrap();
        assert_eq!(answer["text"], "OK");
        assert_eq!(stop, "end_turn");
        assert!(check.complete());
    }

    #[test]
    fn native_launch_diagnostic_preserves_fixed_os_codes_and_api_stages() {
        for (text, stage, os_error) in [
            (
                "Failed to create unified exec process: missing executable (os error 2)",
                NativeLaunchStage::UnifiedExecCreate,
                Some(2),
            ),
            (
                "CreateProcess { message: \"CreateProcessAsUserW failed: 1314 (fixed error) | cwd=private | cmd=private\" }",
                NativeLaunchStage::CreateProcessAsUserW,
                Some(1314),
            ),
            (
                "CreateProcessWithLogonW failed: 1385",
                NativeLaunchStage::CreateProcessWithLogonW,
                Some(1385),
            ),
            (
                "CreateProcessW `private` in cwd `private` failed: fixed error (os error 267)",
                NativeLaunchStage::CreateProcessW,
                Some(267),
            ),
            (
                "Failed to spawn: Os { code: 87, kind: InvalidInput, message: private }",
                NativeLaunchStage::Spawn,
                Some(87),
            ),
            (
                "CreateProcess { message: \"unclassified launch reason\" }",
                NativeLaunchStage::UnifiedExecCreate,
                None,
            ),
        ] {
            let diagnostic = native_launch_diagnostic(text);
            assert_eq!(diagnostic.stage, stage);
            assert_eq!(diagnostic.os_error, os_error);
            assert!(!diagnostic.ambiguous_os_error);
        }
    }

    #[test]
    fn native_launch_diagnostic_rejects_ambiguous_codes_and_never_emits_native_text() {
        const PRIVATE: &str = "private-path-command-and-credential-never-emitted";
        for text in [
            format!("{PRIVATE}: 552 5408 10035 CreateProcess"),
            format!("{PRIVATE}: (os error -5) (os error 5suffix)"),
            format!("{PRIVATE}: Os {{ code: secret, kind: Other }}"),
            format!("{PRIVATE}: (os error 4294967296)"),
            format!("{PRIVATE}: CreateProcessAsUserW failed: 123path"),
        ] {
            let diagnostic = native_launch_diagnostic(&text);
            assert_eq!(diagnostic.os_error, None);
            assert!(!diagnostic.ambiguous_os_error);
            assert!(!format!("{diagnostic:?}").contains(PRIVATE));
        }
        let diagnostic = native_launch_diagnostic(&format!(
            "Failed to spawn: {PRIVATE} (os error 2) Os {{ code: 5, kind: Other }}"
        ));
        assert_eq!(diagnostic.stage, NativeLaunchStage::Spawn);
        assert_eq!(diagnostic.os_error, None);
        assert!(diagnostic.ambiguous_os_error);
        assert!(!format!("{diagnostic:?}").contains(PRIVATE));
        assert_eq!(
            native_launch_diagnostic("Failed to spawn: (os error 2) (os error 2)").os_error,
            Some(2)
        );
    }
}
