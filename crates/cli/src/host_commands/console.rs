use super::*;
use hiroute_host_runtime::{ConsoleListenerDesiredV1, ConsoleListenerStore};
mod configuration;

pub(super) fn execute(arguments: &[String], globals: &Globals) -> CliExecution {
    if globals.help {
        return CliExecution{exit_code:0,stdout:"Usage\n  hiroute console open [--browser] [--loopback-only]\n  hiroute console init [--mode off|loopback|network] [--listen IP] [--port N] [--advertise-address IP] [--allow-plaintext-network]\n  hiroute console status\n  hiroute console enable [--listen IP] [--port N] [--advertise-address IP] [--allow-plaintext-network]\n  hiroute console disable\n  hiroute console token create --name NAME [--expires DAYS]\n  hiroute console token list\n  hiroute console token revoke ID\nAccess tokens are owner credentials. Network HTTP is unencrypted. Configuration takes effect on the next service start; open only mints a one-use URL for the actual listener and never starts listening.\n".into(),stderr:String::new()};
    }
    if globals.agent.is_some() || globals.has_capability() {
        return failure(ErrorCode::CapabilityDenied, globals.request_id.clone());
    }
    let result = command(arguments, globals);
    match result {
        Ok(value) if globals.output == OutputMode::Text => CliExecution {
            exit_code: 0,
            stdout: value
                .get("url")
                .and_then(|v| v.as_str())
                .map(|v| format!("{v}\n"))
                .unwrap_or_else(|| {
                    format!(
                        "{}\n",
                        serde_json::to_string_pretty(&value).unwrap_or_default()
                    )
                }),
            stderr: String::new(),
        },
        Ok(value) => success(value, globals.request_id.clone()),
        Err(code) => {
            let mut result = failure(code, globals.request_id.clone());
            if code == ErrorCode::ActionRequired {
                result.stderr = "The console must be configured and listening. Run `hiroute console status`, configure it with `hiroute console init`, then start/restart the service. Network configuration requires `--allow-plaintext-network`; wildcard binds also require `--advertise-address IP`. `console open` never starts a listener.\n".into();
            }
            result
        }
    }
}
fn call(
    operation: &str,
    payload: serde_json::Value,
    globals: &Globals,
) -> Result<serde_json::Value, ErrorCode> {
    let envelope = LocalControlClient::cli_from_environment()
        .map_err(|_| ErrorCode::DaemonUnavailable)?
        .with_timeout(globals.timeout.unwrap_or(Duration::from_secs(30)))
        .call(LocalControlWireRequestV2 {
            schema_version: LOCAL_CONTROL_SCHEMA_V2,
            request_id: globals
                .request_id
                .clone()
                .unwrap_or_else(|| "console-owner".into()),
            operation_id: operation.into(),
            payload,
            protected_grant: None,
        })
        .map_err(|_| ErrorCode::DaemonUnavailable)?;
    if let Some(error) = envelope.error {
        return Err(error.code);
    }
    envelope.data.ok_or(ErrorCode::DaemonUnavailable)
}
fn command(args: &[String], globals: &Globals) -> Result<serde_json::Value, ErrorCode> {
    match args {
        [open, rest @ ..] if open == "open" => {
            let (open_browser, loopback_only) = configuration::open_options(rest)?;
            let value = call(
                "CreateConsoleLaunch",
                json!({"loopback_only":loopback_only}),
                globals,
            )?;
            if open_browser {
                browser(
                    value
                        .get("url")
                        .and_then(|v| v.as_str())
                        .ok_or(ErrorCode::DaemonUnavailable)?,
                )?;
            }
            Ok(value)
        }
        [token, list] if token == "token" && list == "list" => {
            call("ListConsoleTokens", json!({}), globals)
        }
        [token, revoke, id] if token == "token" && revoke == "revoke" => {
            call("RevokeConsoleToken", json!({"id":id}), globals)
        }
        [token, create, options @ ..] if token == "token" && create == "create" => {
            let mut name = None;
            let mut days = None;
            let mut it = options.iter();
            while let Some(flag) = it.next() {
                match flag.as_str() {
                    "--name" if name.is_none() => name = it.next().cloned(),
                    "--expires" if days.is_none() => {
                        days = Some(
                            it.next()
                                .ok_or(ErrorCode::InvalidArguments)?
                                .parse::<u32>()
                                .map_err(|_| ErrorCode::InvalidArguments)?,
                        )
                    }
                    _ => return Err(ErrorCode::InvalidArguments),
                }
            }
            call(
                "CreateConsoleToken",
                json!({"name":name.ok_or(ErrorCode::InvalidArguments)?,"expires_days":days}),
                globals,
            )
        }
        [status] if status == "status" => {
            serde_json::to_value(store()?.read().map_err(|_| ErrorCode::DaemonUnavailable)?)
                .map_err(|_| ErrorCode::Internal)
        }
        [disable] if disable == "disable" => serde_json::to_value(
            store()?
                .configure(ConsoleListenerDesiredV1::default())
                .map_err(|_| ErrorCode::DaemonUnavailable)?,
        )
        .map_err(|_| ErrorCode::Internal),
        [setup, options @ ..] if setup == "enable" || setup == "init" => {
            let desired = configuration::parse(options, setup == "init")?;
            serde_json::to_value(
                store()?
                    .configure(desired)
                    .map_err(|_| ErrorCode::DaemonUnavailable)?,
            )
            .map_err(|_| ErrorCode::Internal)
        }
        _ => Err(ErrorCode::InvalidArguments),
    }
}
fn store() -> Result<ConsoleListenerStore, ErrorCode> {
    let layout = StandaloneLayout::from_environment().map_err(|_| ErrorCode::DaemonUnavailable)?;
    Ok(ConsoleListenerStore::new(layout.gateway_config_root()))
}
fn browser(url: &str) -> Result<(), ErrorCode> {
    let mut command = {
        #[cfg(target_os = "macos")]
        {
            std::process::Command::new("open")
        }
        #[cfg(windows)]
        {
            std::process::Command::new("explorer.exe")
        }
        #[cfg(not(any(target_os = "macos", windows)))]
        {
            std::process::Command::new("xdg-open")
        }
    };
    command
        .arg(url)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|_| ErrorCode::ActionRequired)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn collaboration_never_mints_or_changes_console_credentials() {
        for capability in [false, true] {
            let globals = Globals {
                agent: Some("test".into()),
                capability_fd: capability.then_some(4),
                ..Default::default()
            };
            for args in [
                vec!["open"],
                vec!["token", "create", "--name", "test"],
                vec!["enable"],
            ] {
                let result = execute(
                    &args.into_iter().map(str::to_owned).collect::<Vec<_>>(),
                    &globals,
                );
                assert!(
                    result.stdout.contains("CAPABILITY_DENIED"),
                    "{}",
                    result.stdout
                );
            }
        }
    }
}
