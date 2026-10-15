//! Bounded installed-source capability inspection; no native command, plugin or helper runs.
use hiroute_domain::CanonicalDigest;
use std::path::{Path, PathBuf};

pub struct HermesSupportFacts {
    pub version: String,
    pub middleware: bool,
    pub skills: bool,
    pub shell: bool,
    pub chat_sdk: bool,
    pub responses_sdk: bool,
    pub messages_sdk: bool,
    pub dependency_digest: CanonicalDigest,
}

pub fn hermes_support_facts() -> HermesSupportFacts {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let executable = selected_hermes_executable(
        super::executable::resolve(Path::new("hermes")),
        home.as_deref(),
    );
    inspect_installation(executable)
}

fn selected_hermes_executable(
    primary: Result<Option<PathBuf>, super::executable::ProbeFailure>,
    home: Option<&Path>,
) -> Option<PathBuf> {
    match primary {
        Ok(Some(path)) => Some(path),
        // A broken selected command cannot be replaced by an unrelated installation.
        Err(_) => None,
        Ok(None) => super::executable::resolve(
            &home
                .filter(|home| home.is_absolute())?
                .join(".hermes-bundle/venv/bin/hermes"),
        )
        .ok()
        .flatten(),
    }
}

fn inspect_installation(executables: impl IntoIterator<Item = PathBuf>) -> HermesSupportFacts {
    let mut roots = Vec::new();
    let mut interpreters = Vec::new();
    for executable in executables {
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
    roots.extend(editable_source_roots(&sdk_paths));
    let sdk = inspect_sdks(&sdk_paths);
    roots.sort();
    roots.dedup();
    for root in roots.into_iter().take(256) {
        let plugins =
            bounded_text(&root.join("hermes_cli/plugins.py"), 2 * 1024 * 1024).unwrap_or_default();
        let middleware = bounded_text(&root.join("hermes_cli/middleware.py"), 2 * 1024 * 1024)
            .unwrap_or_default();
        let skill_source =
            bounded_text(&root.join("tools/skills_tool.py"), 2 * 1024 * 1024).unwrap_or_default();
        if plugins.is_empty() && middleware.is_empty() && skill_source.is_empty() {
            continue;
        }
        let shell_source =
            bounded_text(&root.join("tools/terminal_tool.py"), 2 * 1024 * 1024).unwrap_or_default();
        let shell =
            shell_source.contains("def terminal_tool(") && shell_source.contains("terminal");
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
        // Preserve the existing dependency digest inputs for accepted-operation continuity.
        // These optional upstream replay sources do not determine plugin admission.
        let replay = [
            "agent/message_sanitization.py",
            "agent/reasoning_params.py",
            "agent/agent_runtime_helpers.py",
        ]
        .map(|path| bounded_text(&root.join(path), 2 * 1024 * 1024).unwrap_or_default());
        let dependency_digest = CanonicalDigest::of(&(
            "hermes-native-interfaces/v3",
            &sdk.digest,
            CanonicalDigest::of_bytes(plugins.as_bytes()),
            CanonicalDigest::of_bytes(middleware.as_bytes()),
            calls
                .iter()
                .map(|v| CanonicalDigest::of_bytes(v.as_bytes()))
                .collect::<Vec<_>>(),
            CanonicalDigest::of_bytes(config.as_bytes()),
            CanonicalDigest::of_bytes(skill_source.as_bytes()),
            replay.map(|source| CanonicalDigest::of_bytes(source.as_bytes())),
        ))
        .expect("bounded native proof is serializable");
        return HermesSupportFacts {
            version,
            middleware: supported,
            skills,
            shell,
            chat_sdk: sdk.chat,
            responses_sdk: sdk.responses,
            messages_sdk: sdk.messages,
            dependency_digest,
        };
    }
    HermesSupportFacts {
        version: "unknown".into(),
        middleware: false,
        skills: false,
        shell: false,
        chat_sdk: false,
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

/// PEP 610 installation metadata locates editable sources without importing their finder.
fn editable_source_roots(paths: &[PathBuf]) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    for site in paths.iter().take(32) {
        let Ok(entries) = std::fs::read_dir(site) else {
            continue;
        };
        for entry in entries.take(4096).flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if !name.starts_with("hermes_agent-") || !name.ends_with(".dist-info") {
                continue;
            }
            let Some(metadata) = bounded_text(&entry.path().join("direct_url.json"), 16 * 1024)
            else {
                continue;
            };
            let Ok(value) = serde_json::from_str::<serde_json::Value>(&metadata) else {
                continue;
            };
            if value["dir_info"]["editable"] != true {
                continue;
            }
            let Some(url) = value["url"]
                .as_str()
                .and_then(|url| reqwest::Url::parse(url).ok())
            else {
                continue;
            };
            if url.scheme() != "file" || url.query().is_some() || url.fragment().is_some() {
                continue;
            }
            if let Ok(root) = url.to_file_path()
                && root.is_absolute()
                && root.is_dir()
            {
                roots.push(root);
            }
        }
    }
    roots
}
struct HermesSdkSupport {
    chat: bool,
    responses: bool,
    messages: bool,
    digest: CanonicalDigest,
}

fn inspect_sdks(paths: &[PathBuf]) -> HermesSdkSupport {
    let mut chat = false;
    let mut responses = false;
    let mut messages = false;
    let mut evidence = Vec::new();
    for path in paths {
        for (package, resource, protocol) in [
            (
                "openai",
                "resources/chat/completions/completions.py",
                "chat",
            ),
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
            if protocol == "chat" {
                chat |= supported;
            } else if protocol == "responses" {
                responses |= supported;
            } else {
                messages |= supported;
            }
        }
    }
    HermesSdkSupport {
        chat,
        responses,
        messages,
        digest: CanonicalDigest::of(&("hermes-selected-sdk-interfaces/v2", evidence))
            .expect("bounded sdk evidence"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn hermes_packaged_installation_is_inspected_without_running_its_launcher() {
        use std::os::unix::fs::PermissionsExt;
        let home = tempfile::tempdir().unwrap();
        let prefix = home.path().join(".hermes-bundle/venv");
        let executable = prefix.join("bin/hermes");
        std::fs::create_dir_all(executable.parent().unwrap()).unwrap();
        let sentinel = home.path().join("must-not-execute");
        std::fs::write(
            &executable,
            format!(
                "#!{}\nraise RuntimeError({:?})\n",
                prefix.join("bin/python3").display(),
                sentinel
            ),
        )
        .unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        let site = prefix.join("lib/python3.13/site-packages");
        for (file, text) in [
            ("hermes_cli/plugins.py", "plugins\ndef register_middleware("),
            (
                "hermes_cli/middleware.py",
                "def apply_llm_request_middleware(",
            ),
            ("hermes_cli/config.py", "def load_config_readonly("),
            (
                "agent/turn_api_request.py",
                "apply_llm_request_middleware(session_id=agent.session_id, model=agent.model, provider=agent.provider, base_url=agent.base_url, api_mode=agent.api_mode)",
            ),
            (
                "tools/skills_tool.py",
                "def _skills_dir(\nget_hermes_home() / \"skills\"",
            ),
            (
                "openai/resources/chat/completions/completions.py",
                "def create(self, *, extra_headers: dict): pass",
            ),
        ] {
            let path = site.join(file);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        let selected = selected_hermes_executable(Ok(None), Some(home.path())).unwrap();
        assert_eq!(selected, executable.canonicalize().unwrap());
        let support = inspect_installation([selected]);
        assert!(support.middleware && support.skills && support.chat_sdk);
        assert!(!support.responses_sdk && !support.messages_sdk);
        assert!(!sentinel.exists());
        // Preserve first-command selection, even if another package would pass inspection.
        let first = home.path().join("selected/hermes");
        assert_eq!(
            selected_hermes_executable(Ok(Some(first.clone())), Some(home.path())),
            Some(first)
        );
        assert!(
            selected_hermes_executable(
                Err(super::super::executable::ProbeFailure::NotExecutable),
                Some(home.path())
            )
            .is_none()
        );
    }

    #[test]
    fn chat_sdk_headers_are_required_independently_of_other_transports() {
        let prefix = tempfile::tempdir().unwrap();
        let site = prefix.path();
        let chat = site.join("openai/resources/chat/completions/completions.py");
        std::fs::create_dir_all(chat.parent().unwrap()).unwrap();
        std::fs::write(&chat, "def create(self): pass\n").unwrap();
        let missing = inspect_sdks(&[site.to_path_buf()]);
        assert!(!missing.chat && !missing.responses && !missing.messages);
        std::fs::write(&chat, "def create(self, *, extra_headers: dict): pass\n").unwrap();
        let supported = inspect_sdks(&[site.to_path_buf()]);
        assert!(supported.chat && !supported.responses && !supported.messages);
        assert_ne!(missing.digest, supported.digest);
        std::fs::remove_file(chat).unwrap();
        assert!(!inspect_sdks(&[site.to_path_buf()]).chat);
    }

    #[test]
    fn editable_source_locator_uses_only_selected_local_install_metadata() {
        let root = tempfile::tempdir().unwrap();
        let site = root.path().join("site");
        let info = site.join("hermes_agent-0.19.1.dist-info");
        std::fs::create_dir_all(&info).unwrap();
        let source = root.path().join("native source");
        std::fs::create_dir(&source).unwrap();
        let metadata = info.join("direct_url.json");
        let url = reqwest::Url::from_directory_path(&source).unwrap();
        for (url, editable, expected) in [
            (url.as_str(), true, vec![source.clone()]),
            (url.as_str(), false, vec![]),
            ("https://example.invalid/hermes", true, vec![]),
            ("file://remote.example.invalid/native", true, vec![]),
        ] {
            std::fs::write(
                &metadata,
                serde_json::to_vec(&serde_json::json!({
                "url":url,"dir_info":{"editable":editable}}))
                .unwrap(),
            )
            .unwrap();
            assert_eq!(editable_source_roots(std::slice::from_ref(&site)), expected);
        }
        std::fs::write(&metadata, "import forbidden_native_code").unwrap();
        assert!(editable_source_roots(&[site]).is_empty());
    }

    #[test]
    fn selected_interpreter_sdks_are_protocol_specific_and_digest_bound() {
        let prefix = tempfile::tempdir().unwrap();
        let site = prefix.path().join("lib/python3.11/site-packages");
        let responses = site.join("openai/resources/responses/responses.py");
        std::fs::create_dir_all(responses.parent().unwrap()).unwrap();
        std::fs::write(
            &responses,
            "def create(self, *, extra_headers: dict):\n    pass\n",
        )
        .unwrap();
        let paths = python_package_paths(&[prefix.path().join("bin/python")]);
        assert_eq!(paths, vec![site.clone()]);
        let first = inspect_sdks(&paths);
        assert!(first.responses);
        assert!(!first.messages);
        let messages = site.join("anthropic/resources/messages/messages.py");
        std::fs::create_dir_all(messages.parent().unwrap()).unwrap();
        std::fs::write(&messages, "def create(self):\n    pass\n").unwrap();
        let without_headers = inspect_sdks(&paths);
        assert!(!without_headers.messages);
        assert_ne!(first.digest, without_headers.digest);
        std::fs::write(
            &messages,
            "def create(self, *, extra_headers: dict):\n    pass\n",
        )
        .unwrap();
        let both = inspect_sdks(&paths);
        assert!(both.responses && both.messages);
        assert_ne!(both.digest, without_headers.digest);
        std::fs::remove_file(responses).unwrap();
        let messages_only = inspect_sdks(&paths);
        assert!(!messages_only.responses && messages_only.messages);
        assert_ne!(both.digest, messages_only.digest);
    }
}
