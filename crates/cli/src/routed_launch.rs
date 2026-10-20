//! Readiness checks precede every launch. Native fallback is an explicit terminal action;
//! child streams, environment, working directory and process supervision keep their owners.
mod args;
mod process;

use super::{CliExecution, LocalControlClient, managed_launch};
use hiroute_application_api::{
    AgentLaunchCheckV1, ErrorCode, ErrorV1, LOCAL_CONTROL_SCHEMA_V2, LocalControlWireRequestV2,
    MachineEnvelopeV2, MachineStatus, RoutedLaunchAgentV1, RoutedLaunchDescriptorV1,
    RoutedLaunchReadV1, RoutedLaunchRequestV1, SystemStatusV1,
};
use serde_json::{Value, json};
use std::ffi::OsString;
use std::io::{BufRead, IsTerminal, Write};

pub(super) fn execute(arguments: &[OsString]) -> CliExecution {
    let invocation = match args::parse(arguments) {
        Ok(invocation) => invocation,
        Err(option) => {
            let code = if hiroute_application_api::MANAGED_CLAUDE_CALLER_FORBIDDEN_OPTIONS_V1
                .contains(&option)
            {
                // Retain the released Claude caller-auth rejection contract (exit 3).
                ErrorCode::AgentAuthPrecedenceConflict
            } else {
                ErrorCode::InvalidArguments
            };
            let mut result = super::failure(code, None);
            result.stderr =
                format!("Rejected launcher argument: {option}. Run hiroute agent launch --help.\n");
            return result;
        }
    };
    let mut check = AgentLaunchCheckV1::new(invocation.agent, invocation.context.clone());
    let service = query("GetSystemStatus", json!({})).and_then(|value| {
        serde_json::from_value::<SystemStatusV1>(value).map_err(|_| QueryFailure::Client)
    });
    let ready = service.as_ref().is_ok_and(|s| {
        s.daemon == "role_all"
            && s.gateway == "ready"
            && s.control_store == "ready"
            && s.observation_store == "ready"
            && s.local_control.ends_with(":authenticated")
    });
    if !check.record(
        "service",
        ready,
        "service_unavailable",
        "hiroute service start",
    ) {
        // A service that answers but is not ready is a readiness failure; an unreachable one
        // keeps the released unavailable result.
        let released = matches!(service, Err(QueryFailure::Client)).then(unavailable);
        return refused(&invocation, check, released);
    }
    let read = query(
        "GetManagedAgentLaunchDescriptor",
        json!(RoutedLaunchRequestV1 {
            agent: invocation.agent,
            context: invocation.context.clone(),
        }),
    )
    .and_then(|value| {
        serde_json::from_value::<RoutedLaunchReadV1>(value).map_err(|_| QueryFailure::Client)
    });
    let read = match read {
        Ok(read) => read,
        Err(failure) => {
            check.record(
                "connection",
                false,
                "connection_unavailable",
                "hiroute agents connect status --help",
            );
            let released = match failure {
                QueryFailure::Client => unavailable(),
                QueryFailure::Service(response) => super::render(*response),
            };
            return refused(&invocation, check, Some(released));
        }
    };
    let claude_independent =
        read.activation_mode == Some(hiroute_application_api::AgentActivationModeV1::ManagedLaunch);
    check.context = read.check.context.clone();
    check.candidate_contexts = read.check.candidate_contexts.clone();
    if read.check.schema != "hiroute.agent-launch-check/v1"
        || read.check.agent != invocation.agent
        || invocation
            .context
            .as_ref()
            .is_some_and(|context| read.check.context.as_ref() != Some(context))
        || read.check.checks.len() > 3
        || read.check.candidate_contexts.len() > 32
    {
        check.record(
            "connection",
            false,
            "invalid_launch_response",
            "hiroute agents connect status --help",
        );
        return refused(&invocation, check, Some(unavailable()));
    }
    if invocation.context.is_none() && read.check.candidate_contexts.len() > 1 {
        check.record(
            "connection",
            false,
            "ambiguous_context",
            "hiroute agent launch --help",
        );
        return refused(&invocation, check, None);
    }
    let expected: &[&str] = if invocation.agent == RoutedLaunchAgentV1::Codex {
        &["connection", "plans"]
    } else {
        &["connection", "plans", "managed_launch"]
    };
    if read.check.launchable
        && (read.check.candidate_contexts.len() != 1
            || read.check.candidate_contexts.first() != read.check.context.as_ref()
            || read
                .check
                .checks
                .iter()
                .map(|entry| entry.check.as_str())
                .collect::<Vec<_>>()
                != expected
            || read
                .check
                .checks
                .iter()
                .any(|entry| !entry.ok || entry.reason_code.is_some() || entry.repair.is_some()))
    {
        check.record(
            "connection",
            false,
            "invalid_launch_response",
            "hiroute agents connect status --help",
        );
        return refused(&invocation, check, Some(unavailable()));
    }
    check.checks.extend(read.check.checks);
    check.launchable = read.check.launchable && check.checks.iter().all(|entry| entry.ok);
    let descriptor = read.descriptor.filter(|_| check.launchable);
    if !check.launchable || descriptor.is_none() {
        check.launchable = false;
        let released = if check.checks.iter().all(|entry| entry.ok) {
            check.record(
                "connection",
                false,
                "descriptor_unavailable",
                "hiroute agents connect preview --help",
            );
            Some(unavailable())
        } else {
            // A named connection that does not exist keeps its released not-found result.
            (invocation.context.is_some()
                && check
                    .checks
                    .iter()
                    .any(|entry| entry.reason_code.as_deref() == Some("connection_missing")))
            .then(|| super::failure(ErrorCode::ResourceNotFound, Some(super::next_request_id())))
        };
        return refused(&invocation, check, released);
    }
    let descriptor = descriptor.expect("checked above");
    let connection = check
        .context
        .as_ref()
        .map(|context| format!("agent-connection/{context}"));
    let valid = match &descriptor {
        RoutedLaunchDescriptorV1::Codex(d) => {
            invocation.agent == RoutedLaunchAgentV1::Codex
                && d.validate().is_ok()
                && connection.as_deref() == Some(d.connection_id.as_str())
        }
        RoutedLaunchDescriptorV1::ClaudeCode(d) => {
            invocation.agent == RoutedLaunchAgentV1::ClaudeCode
                && super::current_executable()
                    .is_ok_and(|executable| d.validate_trusted_helper(&executable).is_ok())
                && connection.as_deref() == Some(d.connection_id.as_str())
        }
    };
    if !valid {
        check.record(
            "connection",
            false,
            "invalid_launch_descriptor",
            "hiroute agents connect preview --help",
        );
        return refused(&invocation, check, Some(unavailable()));
    }
    match descriptor {
        RoutedLaunchDescriptorV1::Codex(d) => {
            if !hiroute_integrations::codex_launch_executable_is_current(&d.snapshot) {
                check.checks.truncate(1);
                check.record(
                    "connection",
                    false,
                    "installation_changed",
                    "hiroute agents connect preview --help",
                );
                return refused(&invocation, check, None);
            }
            if invocation.check {
                return super::success(check, Some(super::next_request_id()));
            }
            let mut command = match process::selected(std::path::Path::new(&d.snapshot.executable))
            {
                Ok(command) => command,
                Err(error) => return launched(Err(error)),
            };
            command
                .args(["--profile", d.snapshot.profile_name.as_str()])
                .args(&invocation.child)
                .env("CODEX_HOME", &d.snapshot.codex_home);
            launched(managed_launch::spawn_and_wait(&mut command))
        }
        RoutedLaunchDescriptorV1::ClaudeCode(d) => {
            if claude_independent {
                let preflight = std::env::var_os("HOME")
                    .or_else(|| std::env::var_os("USERPROFILE"))
                    .map(std::path::PathBuf::from)
                    .zip(std::env::current_dir().ok())
                    .and_then(|(home, project)| {
                        hiroute_integrations::AgentFilesystemLayoutV1::from_process(&home, &project)
                            .claude_independent_launch_preflight(
                                d.executable.clone(),
                                std::env::var_os("CLAUDE_CODE_OAUTH_TOKEN").is_some(),
                            )
                            .ok()
                    });
                if preflight
                    .as_ref()
                    .is_none_or(|facts| !facts.is_launchable())
                {
                    let repair = preflight.as_ref().and_then(|facts| facts.conflicts.first())
                        .map(|conflict| format!("Remove routing/auth overrides from {:?} {} and retry hiroute agent launch --check", conflict.layer, conflict.source_ref))
                        .unwrap_or_else(|| "Repair unreadable project or managed Claude settings, then retry hiroute agent launch --check".into());
                    check.checks.pop();
                    check.record("managed_launch", false, "managed_launch_conflict", &repair);
                    return refused(&invocation, check, None);
                }
            }
            let mut native = vec![
                OsString::from("--agent"),
                OsString::from("claude-code"),
                OsString::from("--context"),
                OsString::from(check.context.as_deref().expect("validated context")),
                OsString::from("--"),
            ];
            native.extend_from_slice(&invocation.child);
            let native = match managed_launch::parse_invocation(&native) {
                Ok(native) => native,
                Err(error) => {
                    check.record(
                        "managed_launch",
                        false,
                        "caller_auth_override",
                        "hiroute agent launch --help",
                    );
                    let code = match error {
                        managed_launch::ManagedLaunchFailure::AuthPrecedenceConflict => {
                            ErrorCode::AgentAuthPrecedenceConflict
                        }
                        _ => ErrorCode::InvalidArguments,
                    };
                    let released = super::failure(code, Some(super::next_request_id()));
                    return refused(&invocation, check, Some(released));
                }
            };
            let executable = match super::current_executable() {
                Ok(executable) => executable,
                Err(()) => return launched(Err(managed_launch::ManagedLaunchFailure::Unavailable)),
            };
            let sanitized = d
                .environment_removals
                .iter()
                .any(|name| std::env::var_os(name).is_some());
            let mut process = match managed_launch::prepare(&d, &native, &executable) {
                Ok(process) => process,
                Err(_) => {
                    check.checks.pop();
                    check.record(
                        "managed_launch",
                        false,
                        "managed_launch_conflict",
                        "hiroute agent launch --help",
                    );
                    return refused(&invocation, check, Some(unavailable()));
                }
            };
            if invocation.check {
                return super::success(check, Some(super::next_request_id()));
            }
            let mut result = launched(managed_launch::spawn_and_wait(process.command_mut()));
            if sanitized {
                result.stderr.push_str("MANAGED_LAUNCH_ENV_SANITIZED\n");
            }
            result
        }
    }
}

/// Why a launch read produced no usable data.
enum QueryFailure {
    /// The service could not be reached, or its reply could not be used.
    Client,
    /// The service answered with a typed error.
    Service(Box<MachineEnvelopeV2<Value>>),
}

fn query(operation: &str, payload: Value) -> Result<Value, QueryFailure> {
    let request = LocalControlWireRequestV2 {
        schema_version: LOCAL_CONTROL_SCHEMA_V2,
        request_id: super::next_request_id(),
        operation_id: operation.into(),
        payload,
        protected_grant: None,
    };
    let response = LocalControlClient::cli_from_environment()
        .and_then(|client| client.call(request))
        .map_err(|_| QueryFailure::Client)?;
    if response.status != MachineStatus::Succeeded {
        return Err(QueryFailure::Service(Box::new(response)));
    }
    response.data.ok_or(QueryFailure::Client)
}

fn unavailable() -> CliExecution {
    super::failure(ErrorCode::DaemonUnavailable, Some(super::next_request_id()))
}

/// `--check` reports every failed check as exit 78 with the check. A launch keeps the released
/// result when one exists (usage, conflict, not found, unavailable); 78 covers only the readiness
/// checks the service reports after it answers.
fn refused(
    invocation: &args::Invocation,
    check: AgentLaunchCheckV1,
    released: Option<CliExecution>,
) -> CliExecution {
    let failed = check.checks.iter().find(|entry| !entry.ok);
    let reason = failed
        .and_then(|entry| entry.reason_code.as_deref())
        .unwrap_or("launch_unavailable");
    let repair = failed
        .and_then(|entry| entry.repair.as_deref())
        .unwrap_or("hiroute agent launch --help");
    let message = format!("HiRoute launch refused: {reason}. Repair: {repair}.\n");
    let interactive = invocation.fallback_prompt
        && !invocation.check
        && std::io::stdin().is_terminal()
        && std::io::stdout().is_terminal();
    let mut emitted = false;
    if interactive {
        let mut stderr = std::io::stderr().lock();
        let shown = write!(
            stderr,
            "{message}Start {} without HiRoute? [y/N] ",
            invocation.agent.as_str()
        )
        .and_then(|_| stderr.flush())
        .is_ok();
        drop(stderr);
        emitted = shown;
        let mut answer = String::new();
        if shown
            && std::io::Read::take(std::io::stdin().lock(), 4)
                .read_line(&mut answer)
                .is_ok()
            && matches!(answer.as_str(), "y" | "y\n" | "y\r\n")
        {
            eprintln!("This session is not routed through HiRoute.");
            let executable = match invocation.agent {
                RoutedLaunchAgentV1::Codex => "codex",
                RoutedLaunchAgentV1::ClaudeCode => "claude",
            };
            let mut command = match process::native(executable) {
                Ok(command) => command,
                Err(error) => return launched(Err(error)),
            };
            command.args(&invocation.child);
            return launched(managed_launch::spawn_and_wait(&mut command));
        }
    }
    if let Some(mut result) = released.filter(|_| !invocation.check) {
        if !emitted {
            result.stderr.insert_str(0, &message);
        }
        return result;
    }
    let mut result = super::render(MachineEnvelopeV2::failed_with_data(
        ErrorV1::new(ErrorCode::ActionRequired),
        serde_json::to_value(check).expect("check serializes"),
        Some(super::next_request_id()),
    ));
    result.exit_code = 78;
    if !invocation.json && !invocation.check {
        result.stdout.clear();
    }
    result.stderr = if emitted { String::new() } else { message };
    result
}

fn launched(result: Result<u8, managed_launch::ManagedLaunchFailure>) -> CliExecution {
    match result {
        Ok(exit_code) => CliExecution {
            exit_code,
            stdout: String::new(),
            stderr: String::new(),
        },
        Err(_) => {
            let mut result = super::failure(ErrorCode::CapabilityUnavailable, None);
            result.stderr =
                "The Agent could not be started or supervised. No native retry was started.\n"
                    .into();
            result
        }
    }
}
