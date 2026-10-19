use hiroute_application_api::{AgentGrantRawRequestV1, RoutedLaunchAgentV1};
use std::ffi::OsString;

pub(super) struct Invocation {
    pub agent: RoutedLaunchAgentV1,
    pub context: Option<String>,
    pub check: bool,
    pub fallback_prompt: bool,
    pub json: bool,
    pub child: Vec<OsString>,
}

pub(super) fn parse(arguments: &[OsString]) -> Result<Invocation, &'static str> {
    let mut agent = None;
    let mut context = None;
    let mut check = false;
    let mut fallback = None;
    let mut json = false;
    let mut index = 0;
    let mut delimiter = false;
    while index < arguments.len() {
        let argument = arguments[index]
            .to_str()
            .ok_or("non-UTF-8 launcher option")?;
        if argument == "--" {
            delimiter = true;
            index += 1;
            break;
        }
        if argument == "--check" {
            if check {
                return Err("--check");
            }
            check = true;
            index += 1;
            continue;
        }
        let (option, inline) = argument
            .split_once('=')
            .map_or((argument, None), |(k, v)| (k, Some(v)));
        let value = match inline {
            Some(value) => value,
            None => {
                index += 1;
                arguments
                    .get(index)
                    .and_then(|value| value.to_str())
                    .ok_or("missing launcher option value")?
            }
        };
        match option {
            "--agent" if agent.is_none() => {
                agent = Some(match value {
                    "codex" => RoutedLaunchAgentV1::Codex,
                    "claude-code" => RoutedLaunchAgentV1::ClaudeCode,
                    _ => return Err("--agent"),
                })
            }
            "--context" if context.is_none() => {
                AgentGrantRawRequestV1::new(&format!("agent-connection/{value}"))
                    .map_err(|_| "--context")?;
                context = Some(value.to_owned());
            }
            "--native-fallback" if fallback.is_none() => {
                fallback = Some(match value {
                    "never" => false,
                    "prompt" => true,
                    _ => return Err("--native-fallback"),
                })
            }
            "--output" if !json && value == "json" => json = true,
            _ => return Err("unknown or repeated launcher option"),
        }
        index += 1;
    }
    if !check && !delimiter {
        return Err("--");
    }
    let invocation = Invocation {
        agent: agent.ok_or("--agent")?,
        context,
        check,
        fallback_prompt: fallback.unwrap_or(false),
        json,
        child: arguments[index..].to_vec(),
    };
    if invocation.agent == RoutedLaunchAgentV1::Codex {
        if let Some(option) = forbidden_codex_argument(&invocation.child) {
            return Err(option);
        }
    } else {
        for argument in invocation
            .child
            .iter()
            .take_while(|argument| *argument != "--")
        {
            for option in hiroute_application_api::MANAGED_CLAUDE_CALLER_FORBIDDEN_OPTIONS_V1 {
                let bytes = argument.as_encoded_bytes();
                if bytes == option.as_bytes()
                    || bytes
                        .strip_prefix(option.as_bytes())
                        .is_some_and(|suffix| suffix.starts_with(b"="))
                {
                    return Err(option);
                }
            }
        }
    }
    Ok(invocation)
}

fn forbidden_codex_argument(arguments: &[OsString]) -> Option<&'static str> {
    let mut index = 0;
    while index < arguments.len() {
        let argument = &arguments[index];
        if argument == "--" {
            break;
        }
        let Some(text) = argument.to_str() else {
            index += 1;
            continue;
        };
        if text == "--profile" || text.starts_with("--profile=") {
            return Some("--profile/-p");
        }
        if text == "--oss" || text.starts_with("--oss=") {
            return Some("--oss");
        }
        let value = if text == "--config" {
            index += 1;
            arguments.get(index).and_then(|value| value.to_str())
        } else if let Some(value) = text.strip_prefix("--config=") {
            Some(value)
        } else if let Some(cluster) = text.strip_prefix('-').filter(|s| !s.starts_with('-')) {
            // Clap accepts short-option clusters. A value-taking option consumes the rest
            // of its cluster (or the next argv), so letters inside its value are not flags.
            let mut config = None;
            for (offset, flag) in cluster.char_indices() {
                if flag == 'p' {
                    return Some("--profile/-p");
                }
                if flag == 'c' {
                    let rest = &cluster[offset + 1..];
                    config = if rest.is_empty() {
                        index += 1;
                        arguments.get(index).and_then(|value| value.to_str())
                    } else {
                        Some(rest.strip_prefix('=').unwrap_or(rest))
                    };
                    break;
                }
                if matches!(flag, 'a' | 'C' | 'i' | 'm' | 'o' | 's' | 'S') {
                    if offset + 1 == cluster.len() {
                        index += 1;
                    }
                    break;
                }
            }
            config
        } else {
            None
        };
        if value.is_some_and(hiroute_integrations::codex_cli_override_changes_routing) {
            return Some("--config/-c routing override");
        }
        index += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    fn arguments(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }

    #[test]
    fn check_does_not_require_a_context_or_child_delimiter() {
        let parsed = parse(&arguments(&["--agent", "codex", "--check"])).unwrap();
        assert!(parsed.context.is_none() && parsed.check && !parsed.fallback_prompt);
    }

    #[test]
    fn codex_rejects_profile_and_provider_overrides_without_exposing_values() {
        for child in [
            vec!["-p", "other"],
            vec!["--profile=other"],
            vec!["-pother"],
            vec!["-hpother"],
            vec!["-hcmodel_provider='other'"],
            vec!["--oss"],
            vec!["-c", "profile='other'"],
            vec!["--config=profiles.hiroute.model='other'"],
            vec!["-cmodel_provider='other'"],
            vec!["-c=model_providers.hiroute.base_url='other'"],
            vec!["--config", "\"model_provider\"='other'"],
        ] {
            assert!(
                forbidden_codex_argument(&arguments(&child)).is_some(),
                "{child:?}"
            );
        }
        assert!(
            forbidden_codex_argument(&arguments(&[
                "-m",
                "hiroute-other",
                "-mprofile-literal",
                "--approve-for-me",
                "-c",
                "model_reasoning_effort='high'",
                "--",
                "--profile=prompt text"
            ]))
            .is_none()
        );
    }

    #[test]
    fn original_child_arguments_and_explicit_fallback_are_retained() {
        let parsed = parse(&arguments(&[
            "--agent=claude-code",
            "--native-fallback=prompt",
            "--",
            "--permission-mode",
            "auto",
            "hello",
        ]))
        .unwrap();
        assert!(parsed.fallback_prompt);
        assert_eq!(
            parsed.child,
            arguments(&["--permission-mode", "auto", "hello"])
        );
        assert!(parse(&arguments(&["--agent", "codex"])).is_err());
        assert!(
            parse(&arguments(&[
                "--agent",
                "codex",
                "--context",
                "../wrong",
                "--"
            ]))
            .is_err()
        );
    }
}
