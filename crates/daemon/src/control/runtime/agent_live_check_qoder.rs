//! A real Qoder print call selects the persisted provider; no route or credential overlay.
use super::*;

const NATIVE_LOGIN_REQUIRED: &str = "LIVE_NATIVE_LOGIN_REQUIRED";

impl LocalControlAdapter {
    #[cfg(any(unix, windows))]
    pub(super) fn execute_qoder_live_attempt(
        &self,
        context_id: &str,
        model: &str,
        trust: &FrozenExecutionTrustV1,
        deadline: Instant,
    ) -> Result<CanonicalDigest, AttemptFailure> {
        #[cfg(unix)]
        use std::os::unix::process::CommandExt;
        let unavailable = || AttemptFailure {
            executed: false,
            reason: CLIENT_FAILED,
        };
        let executable = self
            .scanner
            .qoder_executable_target()
            .ok_or_else(unavailable)?;
        let context = self
            .scanner
            .qoder_native_context()
            .map_err(|_| unavailable())?;
        let mut working = LiveWorkspace::new().map_err(|_| unavailable())?;
        let safety = working.path().join("safety.json");
        let safety_bytes = serde_json::to_vec(&serde_json::json!({
            "disableAllHooks":true,"general":{"enableAutoUpdate":false,"sessionRetention":{"enabled":false}},
            "plugins":{"autoUpdate":false},"autoMemoryEnabled":false,"autoMemoryUserScopeEnabled":false,
            "dream":{"enabled":false},"promptSuggestionEnabled":false
        })).map_err(|_| unavailable())?;
        hiroute_platform::durable::write_private_new(&safety, &safety_bytes)
            .map_err(|_| unavailable())?;
        #[cfg(unix)]
        let output_path = working.path().join("native-output.private");
        #[cfg(unix)]
        use std::os::unix::fs::OpenOptionsExt;
        #[cfg(unix)]
        let output = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&output_path)
            .map_err(|_| unavailable())?;
        let native_model = self
            .scanner
            .qoder_saved_plan_selector(
                &hiroute_application::agent_connection::additional_model_provider_id(context_id),
                model,
            )
            .map_err(|_| unavailable())?;
        let mut command = Command::new(executable);
        command
            .arg("--cwd")
            .arg(working.path())
            .arg("--config-dir")
            .arg(&context.config_root)
            .args(["--setting-sources", "user", "--settings"])
            .arg(&safety)
            .args([
                "--model",
                &native_model,
                "--print",
                "--no-session-persistence",
                "--output-format",
                "stream-json",
                "--strict-mcp-config",
                "--mcp-config",
                "{\"mcpServers\":{}}",
                "--tools",
                "",
                "--permission-mode",
                "dont_ask",
                "--max-model-request-retries",
                "0",
                "-p",
                LIVE_PROMPT,
            ])
            .env_clear()
            .env("HOME", &context.home)
            .env("QODER_CONFIG_DIR", &context.config_root)
            .env("TMPDIR", working.path())
            .current_dir(working.path());
        #[cfg(windows)]
        {
            // Retain only absolute executable-search directories. No ambient
            // provider credentials, routing overrides or relative project paths.
            let search = std::env::var_os("PATH").unwrap_or_default();
            let directories = std::env::split_paths(&search)
                .filter(|path| path.is_absolute())
                .collect::<Vec<_>>();
            command
                .env(
                    "PATH",
                    std::env::join_paths(directories).map_err(|_| unavailable())?,
                )
                .env("TEMP", working.path())
                .env("TMP", working.path());
        }
        #[cfg(unix)]
        command
            .env("PATH", "/usr/local/bin:/usr/bin:/bin:/opt/homebrew/bin")
            .stdin(Stdio::null())
            .stdout(Stdio::from(output))
            .stderr(Stdio::null())
            .process_group(0);
        #[cfg(windows)]
        let (bytes, success) = windows::run_captured(&command, false, deadline, &mut working)?;
        #[cfg(unix)]
        let (bytes, success) = {
            let child = command.spawn().map_err(|_| unavailable())?;
            let mut child = hiroute_integrations::NativeProbeProcess::new(child);
            let failure = |reason| AttemptFailure {
                executed: true,
                reason,
            };
            let result = (|| {
                loop {
                    if Instant::now() >= deadline {
                        return Err(failure(CHECK_TIMEOUT));
                    }
                    if std::fs::metadata(&output_path)
                        .map_err(|_| failure(CLIENT_OUTPUT_INVALID))?
                        .len()
                        > OUTPUT_LIMIT as u64
                    {
                        return Err(failure(CLIENT_OUTPUT_INVALID));
                    }
                    if let Some(success) = child.observe().map_err(|_| failure(CLIENT_FAILED))? {
                        break Ok(success);
                    }
                    thread::sleep(Duration::from_millis(20));
                }
            })();
            if child.stop().is_err() {
                working.keep();
                return Err(failure(CLIENT_FAILED));
            }
            let success = result?;
            let mut bytes = zeroize::Zeroizing::new(Vec::new());
            std::fs::File::open(&output_path)
                .map_err(|_| failure(CLIENT_OUTPUT_INVALID))?
                .take(OUTPUT_LIMIT as u64 + 1)
                .read_to_end(&mut bytes)
                .map_err(|_| failure(CLIENT_OUTPUT_INVALID))?;
            if bytes.len() > OUTPUT_LIMIT {
                return Err(failure(CLIENT_OUTPUT_INVALID));
            }
            (bytes.to_vec(), success)
        };
        let bytes = zeroize::Zeroizing::new(bytes);
        let failure = |reason| AttemptFailure {
            executed: true,
            reason,
        };
        let identity = parse_qoder_output(&bytes, success).map_err(failure)?;
        let response_digest = CanonicalDigest::of_bytes(&bytes);
        self.verify_live_receipt(trust, &[identity], &response_digest, deadline)
    }

    #[cfg(not(any(unix, windows)))]
    pub(super) fn execute_qoder_live_attempt(
        &self,
        _context_id: &str,
        _model: &str,
        _trust: &FrozenExecutionTrustV1,
        _deadline: Instant,
    ) -> Result<CanonicalDigest, AttemptFailure> {
        Err(AttemptFailure {
            executed: false,
            reason: CLIENT_FAILED,
        })
    }
}

fn parse_qoder_output(
    bytes: &[u8],
    process_succeeded: bool,
) -> Result<NativeAgentObservationIdentityV1, &'static str> {
    let invalid = if process_succeeded {
        CLIENT_OUTPUT_INVALID
    } else {
        CLIENT_FAILED
    };
    let mut result = None;
    for line in bytes
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.iter().all(u8::is_ascii_whitespace))
    {
        let value: serde_json::Value = serde_json::from_slice(line).map_err(|_| invalid)?;
        if value["type"] == "result" {
            if result.is_some() || !value["subtype"].is_string() {
                return Err(invalid);
            }
            let message = value["result"].as_str().ok_or(invalid)?.trim();
            result = Some(if value["is_error"].as_bool().ok_or(invalid)? {
                // Qoder 1.1.65 reports this native prerequisite as a terminal error,
                // even with subtype=success and exit 0. Never expose its raw output.
                Err(if message == "Not logged in · Please run /login" {
                    NATIVE_LOGIN_REQUIRED
                } else {
                    CLIENT_FAILED
                })
            } else if !process_succeeded {
                Err(CLIENT_FAILED)
            } else {
                if value["subtype"] != "success" || message != LIVE_PROMPT_RESPONSE {
                    return Err(invalid);
                }
                let session_id = value["session_id"].as_str().ok_or(invalid)?;
                if !valid_native_identity(session_id) {
                    return Err(invalid);
                }
                Ok(NativeAgentObservationIdentityV1::QoderSession {
                    session_id: session_id.into(),
                })
            });
        }
    }
    result.unwrap_or(Err(invalid))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn qoder_live_requires_successful_terminal_marker_and_exact_native_identity() {
        let valid = br#"{"type":"result","subtype":"success","is_error":false,"result":"HIROUTE_LIVE_CHECK_OK","session_id":"native-session"}"#;
        assert!(
            matches!(parse_qoder_output(valid, true),Ok(NativeAgentObservationIdentityV1::QoderSession{session_id}) if session_id=="native-session")
        );
        for changed in [
            String::from_utf8(valid.to_vec())
                .unwrap()
                .replace("\"is_error\":false", "\"is_error\":true"),
            String::from_utf8(valid.to_vec())
                .unwrap()
                .replace("HIROUTE_LIVE_CHECK_OK", "echo only"),
            format!(
                "{}\n{}",
                std::str::from_utf8(valid).unwrap(),
                std::str::from_utf8(valid).unwrap()
            ),
        ] {
            assert!(parse_qoder_output(changed.as_bytes(), true).is_err());
        }
        assert_eq!(parse_qoder_output(valid, false).unwrap_err(), CLIENT_FAILED);
    }

    #[test]
    fn qoder_live_reports_native_login_failure_regardless_of_exit_status() {
        let native_failure = "{\"type\":\"result\",\"subtype\":\"success\",\"is_error\":true,\"result\":\"Not logged in · Please run /login\",\"duration_ms\":0,\"total_cost_usd\":0}";
        for process_succeeded in [true, false] {
            assert_eq!(
                parse_qoder_output(native_failure.as_bytes(), process_succeeded).unwrap_err(),
                NATIVE_LOGIN_REQUIRED
            );
        }
        let unknown_error =
            native_failure.replace("Not logged in · Please run /login", "Model unavailable");
        assert_eq!(
            parse_qoder_output(unknown_error.as_bytes(), true).unwrap_err(),
            CLIENT_FAILED
        );
    }

    #[test]
    fn qoder_live_does_not_infer_login_from_success_or_invalid_output() {
        let native_failure = "{\"type\":\"result\",\"subtype\":\"success\",\"is_error\":true,\"result\":\"Not logged in · Please run /login\"}";
        for invalid in [
            native_failure.replace("\"is_error\":true", "\"is_error\":false"),
            native_failure.replace("\"is_error\":true", "\"is_error\":\"true\""),
            native_failure.replace("\"subtype\":\"success\",", ""),
            format!("{native_failure}\n{native_failure}"),
            format!("{native_failure}\ninvalid json"),
            "Not logged in · Please run /login".into(),
        ] {
            assert_eq!(
                parse_qoder_output(invalid.as_bytes(), true).unwrap_err(),
                CLIENT_OUTPUT_INVALID
            );
            assert_eq!(
                parse_qoder_output(invalid.as_bytes(), false).unwrap_err(),
                CLIENT_FAILED
            );
        }
    }
}
