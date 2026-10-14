//! Bounded installed-source capability inspection; no native command, plugin or helper runs.
use hiroute_domain::CanonicalDigest;
use std::path::{Path, PathBuf};

pub struct HermesSupportFacts {
    pub version: String,
    pub middleware: bool,
    pub skills: bool,
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
    for executable in executables.drain(..) {
        let Ok(executable) = executable.canonicalize() else {
            continue;
        };
        if !executable.is_file() {
            continue;
        }
        if let Some(parent) = executable.parent() {
            roots.push(parent.to_path_buf());
            if let Some(p) = parent.parent() {
                roots.push(p.to_path_buf());
            }
        }
        let Some(text) = bounded_text(&executable, 64 * 1024) else {
            continue;
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
        let supported = plugins.contains("def register_middleware(")
            && plugins.contains("plugins")
            && middleware.contains("def apply_llm_request_middleware(")
            && valid_call;
        let skills = bounded_text(&root.join("tools/skills_tool.py"), 2 * 1024 * 1024)
            .or_else(|| bounded_text(&root.join("hermes_cli/skills.py"), 2 * 1024 * 1024))
            .is_some();
        let project = bounded_text(&root.join("pyproject.toml"), 128 * 1024).unwrap_or_default();
        let version = project
            .lines()
            .find_map(|line| {
                let (k, v) = line.trim().split_once('=')?;
                (k.trim() == "version").then(|| v.trim().trim_matches('"').to_owned())
            })
            .unwrap_or_else(|| "unknown".into());
        let dependency_digest = CanonicalDigest::of(&(
            "hermes-native-interfaces/v1",
            CanonicalDigest::of_bytes(plugins.as_bytes()),
            CanonicalDigest::of_bytes(middleware.as_bytes()),
            calls
                .iter()
                .map(|v| CanonicalDigest::of_bytes(v.as_bytes()))
                .collect::<Vec<_>>(),
            skills,
        ))
        .expect("bounded native proof is serializable");
        return HermesSupportFacts {
            version,
            middleware: supported,
            skills,
            dependency_digest,
        };
    }
    HermesSupportFacts {
        version: "unknown".into(),
        middleware: false,
        skills: false,
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
