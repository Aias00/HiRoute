//! Bounded installed-source capability inspection; no native command, plugin or helper runs.
use hiroute_domain::CanonicalDigest;
use std::path::{Path, PathBuf};

pub struct HermesSupportFacts {
    pub version: String,
    pub middleware: bool,
    pub skills: bool,
    pub responses_sdk: bool,
    pub messages_sdk: bool,
    pub dependency_digest: CanonicalDigest,
}

pub fn hermes_support_facts() -> HermesSupportFacts {
    let mut executables: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|v| {
            std::env::split_paths(&v)
                .take(64)
                .map(|p| p.join("hermes"))
                .collect()
        })
        .unwrap_or_default();
    let mut roots = Vec::new();
    let mut interpreters = Vec::new();
    for executable in executables.drain(..) {
        let Ok(Some(executable)) = super::executable::resolve(&executable) else {
            continue;
        };
        if let Some(parent) = executable.parent() {
            roots.push(parent.to_path_buf());
            if let Some(p) = parent.parent() {
                roots.push(p.to_path_buf());
            }
        }
        let Some(text) = bounded_text(&executable, 64 * 1024) else {
            break;
        };
        // Installed console scripts and the official shell launcher expose literal Python/
        // project locations. Read only those locations; never evaluate launcher text or env.
        for word in
            text.split(|c: char| c.is_whitespace() || matches!(c, '"' | '\'' | '(' | ')' | ','))
        {
            let raw = word.strip_prefix("#!").unwrap_or(word);
            let path = Path::new(raw);
            if !path.is_absolute() || raw.contains(['$', '`', '\0']) {
                continue;
            }
            if path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("python"))
            {
                interpreters.push(path.to_path_buf());
            }
            let mut ancestor = path.parent();
            for _ in 0..4 {
                let Some(parent) = ancestor else {
                    break;
                };
                roots.push(parent.to_path_buf());
                ancestor = parent.parent();
            }
        }
        // Native execution selects the first executable on PATH, not another installation
        // discovered later merely because its interfaces would pass.
        break;
    }
    let sdk_paths = python_package_paths(&interpreters);
    roots.extend(sdk_paths.iter().cloned());
    let sdk = inspect_sdks(&sdk_paths);
    roots.sort();
    roots.dedup();
    for root in roots.into_iter().take(256) {
        let Some(plugins) = bounded_text(&root.join("hermes_cli/plugins.py"), 2 * 1024 * 1024)
        else {
            continue;
        };
        let Some(middleware) =
            bounded_text(&root.join("hermes_cli/middleware.py"), 2 * 1024 * 1024)
        else {
            continue;
        };
        let calls = ["agent/turn_api_request.py", "agent/conversation_loop.py"]
            .into_iter()
            .filter_map(|p| bounded_text(&root.join(p), 2 * 1024 * 1024))
            .collect::<Vec<_>>();
        let valid_call = calls.iter().any(|call| {
            let compact: String = call.chars().filter(|c| !c.is_whitespace()).collect();
            compact.contains("apply_llm_request_middleware(")
                && [
                    "session_id=agent.session_id",
                    "model=agent.model",
                    "provider=agent.provider",
                    "base_url=agent.base_url",
                    "api_mode=agent.api_mode",
                ]
                .iter()
                .all(|field| compact.contains(field))
        });
        let config =
            bounded_text(&root.join("hermes_cli/config.py"), 2 * 1024 * 1024).unwrap_or_default();
        let supported = config.contains("def load_config_readonly(")
            && plugins.contains("def register_middleware(")
            && plugins.contains("plugins")
            && middleware.contains("def apply_llm_request_middleware(")
            && valid_call;
        let skill_source =
            bounded_text(&root.join("tools/skills_tool.py"), 2 * 1024 * 1024).unwrap_or_default();
        let skills = skill_source.contains("def _skills_dir(")
            && skill_source.contains("get_hermes_home() / \"skills\"");
        let project = bounded_text(&root.join("pyproject.toml"), 128 * 1024).unwrap_or_default();
        let version = project
            .lines()
            .find_map(|line| {
                let (k, v) = line.trim().split_once('=')?;
                (k.trim() == "version").then(|| v.trim().trim_matches('"').to_owned())
            })
            .unwrap_or_else(|| "unknown".into());
        let dependency_digest = CanonicalDigest::of(&(
            "hermes-native-interfaces/v2",
            &sdk.2,
            CanonicalDigest::of_bytes(plugins.as_bytes()),
            CanonicalDigest::of_bytes(middleware.as_bytes()),
            calls
                .iter()
                .map(|v| CanonicalDigest::of_bytes(v.as_bytes()))
                .collect::<Vec<_>>(),
            CanonicalDigest::of_bytes(config.as_bytes()),
            CanonicalDigest::of_bytes(skill_source.as_bytes()),
        ))
        .expect("bounded native proof is serializable");
        return HermesSupportFacts {
            version,
            middleware: supported,
            skills,
            responses_sdk: sdk.0,
            messages_sdk: sdk.1,
            dependency_digest,
        };
    }
    HermesSupportFacts {
        version: "unknown".into(),
        middleware: false,
        skills: false,
        responses_sdk: false,
        messages_sdk: false,
        dependency_digest: CanonicalDigest::of_bytes(b"hermes-native-interfaces/unavailable"),
    }
}

fn bounded_text(path: &Path, bound: u64) -> Option<String> {
    let metadata = std::fs::symlink_metadata(path).ok()?;
    if !metadata.is_file() || metadata.len() > bound {
        return None;
    }
    std::fs::read_to_string(path).ok()
}

/// Follow only the literal launcher's interpreter installation. No imports or environment search.
fn python_package_paths(interpreters: &[PathBuf]) -> Vec<PathBuf> {
    let mut packages = Vec::new();
    for interpreter in interpreters.iter().take(8) {
        let Some(prefix) = interpreter.parent().and_then(Path::parent) else {
            continue;
        };
        for library in ["lib", "lib64"] {
            let Ok(versions) = std::fs::read_dir(prefix.join(library)) else {
                continue;
            };
            for version in versions.take(32).flatten() {
                let name = version.file_name();
                if !name.to_string_lossy().starts_with("python3.") {
                    continue;
                }
                for directory in ["site-packages", "dist-packages"] {
                    let path = version.path().join(directory);
                    if path.is_dir() {
                        packages.push(path);
                    }
                }
            }
        }
    }
    packages.sort();
    packages.dedup();
    packages
}
fn inspect_sdks(paths: &[PathBuf]) -> (bool, bool, CanonicalDigest) {
    let mut responses = false;
    let mut messages = false;
    let mut evidence = Vec::new();
    for path in paths {
        for (package, resource, protocol) in [
            ("openai", "resources/responses/responses.py", "responses"),
            ("anthropic", "resources/messages/messages.py", "messages"),
        ] {
            let module = path.join(package).join(resource);
            let source = bounded_text(&module, 2 * 1024 * 1024);
            let supported = source.as_ref().is_some_and(|source| {
                source.contains("def create(") && source.contains("extra_headers:")
            });
            evidence.push((
                module,
                source
                    .as_ref()
                    .map(|source| CanonicalDigest::of_bytes(source.as_bytes())),
            ));
            if protocol == "responses" {
                responses |= supported;
            } else {
                messages |= supported;
            }
        }
    }
    (
        responses,
        messages,
        CanonicalDigest::of(&("hermes-selected-sdk-interfaces/v1", evidence))
            .expect("bounded sdk evidence"),
    )
}
