//! One local challenge: native Skill discovery, its contents read, and the installed CLI run.
//! The endpoint issues only this fixed read-only command; it never forwards model requests.
use serde_json::{Value, json};
use std::{io::Write, path::Path, process::Command};

pub(crate) struct CollaborationChallenge {
    discovery: String,
    contents: String,
    command: String,
    expected_cli: String,
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
        Ok(Self {
            discovery,
            contents,
            command,
            expected_cli: expected_cli.trim().into(),
            issued: false,
            completed: false,
        })
    }

    pub(crate) fn complete(&self) -> bool {
        self.completed
    }

    pub(crate) fn reply(&mut self, body: &Value) -> Result<Value, &'static str> {
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
            let args = if name == "exec_command" {
                json!({"cmd":self.command, "max_output_tokens":2000, "yield_time_ms":1000, "login":false})
            } else {
                json!({"command":self.command, "timeout_ms":10000})
            };
            self.issued = true;
            return Ok(
                json!([{"type":"function_call", "id":"fc_hiroute_probe", "call_id":"call_hiroute_probe", "name":name, "arguments":args.to_string(), "status":"completed"}]),
            );
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
        let content_proved = output
            .as_ref()
            .is_some_and(|text| text.contains(&self.contents));
        let cli_proved = output
            .as_ref()
            .is_some_and(|text| text.contains(&self.expected_cli));
        let proved = content_proved && cli_proved;
        if !proved {
            eprintln!(
                "native collaboration checkpoint: correlated={} content-items={} skill-content={} cli-output={} output-bytes={} expected-cli-bytes={} truncated={}",
                result.is_some(),
                result.is_some_and(|item| item["output"].is_array()),
                content_proved,
                cli_proved,
                output.as_ref().map_or(0, String::len),
                self.expected_cli.len(),
                output
                    .as_ref()
                    .is_some_and(|text| text.contains("truncated"))
            );
            // Closed flags distinguish a native launch/sandbox/command failure
            // without exposing tool output, private paths or authentication data.
            let text = output.as_deref().unwrap_or_default();
            eprintln!(
                "native collaboration failure flags: process-launch={} access-denied={} sandbox={} command-not-found={} powershell-parser={} dll-init={}",
                text.contains("CreateProcess") || text.contains("Failed to spawn"),
                text.contains("Access is denied")
                    || text.contains("os error 5")
                    || text.contains("PermissionDenied"),
                text.contains("sandbox") || text.contains("Sandbox"),
                text.contains("not recognized") || text.contains("command not found"),
                text.contains("ParserError") || text.contains("UnexpectedToken"),
                text.contains("0xc0000142")
                    || text.contains("0xC0000142")
                    || text.contains("-1073741502")
                    || text.contains("1114")
            );
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
            issued: false,
            completed: false,
        }
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
}
