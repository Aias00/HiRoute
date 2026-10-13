//! The instance's native context is borrowed; it is never a cleanup target.
use super::{DelegationErrorV1, Path, PathBuf, WorkerHarnessV1, path_string};
use std::ffi::OsString;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeWorkerContext {
    home: PathBuf,
    config_root: PathBuf,
    borrowed: bool,
}

impl NativeWorkerContext {
    /// Resolve only context selectors, never credentials or the entire service environment.
    /// An explicit Pilot/test HOME is the effective user context, not a hint to search elsewhere.
    pub fn from_environment(harness: WorkerHarnessV1) -> Result<Self, DelegationErrorV1> {
        Self::from_lookup(harness, |name| std::env::var_os(name))
    }

    fn from_lookup(
        harness: WorkerHarnessV1,
        lookup: impl Fn(&str) -> Option<OsString>,
    ) -> Result<Self, DelegationErrorV1> {
        let home = lookup("HOME")
            .or_else(|| {
                #[cfg(windows)]
                {
                    lookup("USERPROFILE")
                }
                #[cfg(not(windows))]
                {
                    None
                }
            })
            .map(PathBuf::from)
            .ok_or(DelegationErrorV1::CapabilityUnavailable)?;
        let (variable, default) = match harness {
            WorkerHarnessV1::CodexCli => ("CODEX_HOME", ".codex"),
            WorkerHarnessV1::ClaudeCode => ("CLAUDE_CONFIG_DIR", ".claude"),
            WorkerHarnessV1::Pi => ("PI_CODING_AGENT_DIR", ".pi/agent"),
            WorkerHarnessV1::DeepseekHarness => ("DSH_HOME", ".dsh"),
            WorkerHarnessV1::QoderCli => {
                let selected = lookup("QODER_CONFIG_DIR").map(PathBuf::from);
                let context = hiroute_integrations::agents::QoderNativeContext::from_selected(
                    &home,
                    selected.as_deref(),
                )
                .map_err(|_| DelegationErrorV1::InvalidArguments)?;
                return Self::borrowed(&context.home, &context.config_root);
            }
        };
        let config_root = lookup(variable)
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(default));
        Self::borrowed(&home, &config_root)
    }

    /// Reconstruct an already bound context without consulting today's environment on Continue.
    pub fn borrowed(home: &Path, config_root: &Path) -> Result<Self, DelegationErrorV1> {
        Self::new(home.to_owned(), config_root.to_owned(), true)
    }

    /// Explicit legacy/probe context. Its directories remain owned by the existing material path.
    pub fn isolated(private_root: &Path, session_root: &Path) -> Result<Self, DelegationErrorV1> {
        Self::new(private_root.join("home"), session_root.to_owned(), false)
    }

    fn new(home: PathBuf, config_root: PathBuf, borrowed: bool) -> Result<Self, DelegationErrorV1> {
        for path in [&home, &config_root] {
            if !path.is_absolute()
                || path
                    .components()
                    .any(|part| part == std::path::Component::ParentDir)
            {
                return Err(DelegationErrorV1::InvalidArguments);
            }
            path_string(path)?;
        }
        Ok(Self {
            home,
            config_root,
            borrowed,
        })
    }

    pub fn home(&self) -> &Path {
        &self.home
    }

    pub fn config_root(&self) -> &Path {
        &self.config_root
    }

    pub fn is_borrowed(&self) -> bool {
        self.borrowed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instance_home_and_explicit_native_roots_are_respected_without_reading_other_env() {
        #[cfg(windows)]
        let (home, root) = ("C:\\isolated-pilot", "C:\\explicit-native-root");
        #[cfg(not(windows))]
        let (home, root) = ("/isolated-pilot", "/explicit-native-root");
        for (harness, variable, default) in [
            (WorkerHarnessV1::CodexCli, "CODEX_HOME", ".codex"),
            (WorkerHarnessV1::ClaudeCode, "CLAUDE_CONFIG_DIR", ".claude"),
            (WorkerHarnessV1::QoderCli, "QODER_CONFIG_DIR", ".qoder"),
        ] {
            let lookup = |name: &str| match name {
                "HOME" => Some(OsString::from(home)),
                key if key == variable => Some(OsString::from(root)),
                unexpected => panic!("unexpected ambient environment read: {unexpected}"),
            };
            let explicit = NativeWorkerContext::from_lookup(harness, lookup).unwrap();
            assert_eq!(explicit.home(), Path::new(home));
            assert_eq!(explicit.config_root(), Path::new(root));
            assert!(explicit.is_borrowed());
            let fallback = NativeWorkerContext::from_lookup(harness, |name| {
                (name == "HOME").then(|| OsString::from(home))
            })
            .unwrap();
            assert_eq!(fallback.config_root(), Path::new(home).join(default));
        }
    }

    #[test]
    fn missing_or_invalid_context_does_not_fall_back_to_another_users_home() {
        assert_eq!(
            NativeWorkerContext::from_lookup(WorkerHarnessV1::CodexCli, |_| None),
            Err(DelegationErrorV1::CapabilityUnavailable)
        );
        for invalid in ["", "relative", "/root/../elsewhere", "/root\nother"] {
            assert_eq!(
                NativeWorkerContext::borrowed(Path::new(invalid), Path::new("/native")),
                Err(DelegationErrorV1::InvalidArguments)
            );
            assert_eq!(
                NativeWorkerContext::borrowed(Path::new("/instance"), Path::new(invalid)),
                Err(DelegationErrorV1::InvalidArguments)
            );
        }
    }
}

#[cfg(windows)]
pub(crate) fn windows_workspace(
    path: &str,
) -> Result<(hiroute_domain::delegation::DelegationWorkspaceV1, String), DelegationErrorV1> {
    use hiroute_platform::windows::security::{file_identity, open_identity};
    // Inspect each absolute component through held directory handles before accepting it.
    let requested = std::path::Path::new(path);
    let handle = open_identity(requested, true).map_err(|_| DelegationErrorV1::InvalidArguments)?;
    let (volume, index) =
        file_identity(&handle).map_err(|_| DelegationErrorV1::InvalidArguments)?;
    let canonical = native_path(
        &std::fs::canonicalize(requested).map_err(|_| DelegationErrorV1::InvalidArguments)?,
    );
    let current =
        open_identity(&canonical, true).map_err(|_| DelegationErrorV1::InvalidArguments)?;
    if file_identity(&current).map_err(|_| DelegationErrorV1::InvalidArguments)? != (volume, index)
    {
        return Err(DelegationErrorV1::InvalidArguments);
    }
    let mut ancestry = Vec::new();
    for ancestor in canonical.ancestors() {
        let file =
            open_identity(ancestor, true).map_err(|_| DelegationErrorV1::InvalidArguments)?;
        let (volume, index) =
            file_identity(&file).map_err(|_| DelegationErrorV1::InvalidArguments)?;
        ancestry.push(format!("file/{volume}/{index}"));
    }
    ancestry.reverse();
    let root_identity = format!("file/{volume}/{index}");
    if ancestry.last() != Some(&root_identity) {
        return Err(DelegationErrorV1::InvalidArguments);
    }
    let workspace = hiroute_domain::delegation::DelegationWorkspaceV1 {
        root_identity,
        volume_identity: format!("volume/{volume}"),
        ancestry,
    };
    workspace.validate()?;
    Ok((
        workspace,
        canonical
            .to_str()
            .ok_or(DelegationErrorV1::InvalidArguments)?
            .to_owned(),
    ))
}

/// Native Node/CLI cwd and history encoders use ordinary Win32 path spelling.
/// Rust canonicalize's verbatim prefix is an I/O detail, not native history identity.
pub(crate) fn native_path(path: &Path) -> PathBuf {
    #[cfg(windows)]
    if let Some(text) = path.to_str() {
        if let Some(path) = text.strip_prefix(r"\\?\UNC\") {
            return PathBuf::from(format!(r"\\{path}"));
        }
        if let Some(path) = text.strip_prefix(r"\\?\") {
            return PathBuf::from(path);
        }
    }
    path.to_owned()
}

#[cfg(all(test, windows))]
mod windows_tests {
    use super::*;

    #[test]
    fn windows_context_respects_home_and_only_falls_back_to_current_userprofile() {
        let fixture = tempfile::tempdir().unwrap();
        let explicit = fixture.path().join("explicit-home");
        let profile = fixture.path().join("userprofile");
        let context =
            NativeWorkerContext::from_lookup(WorkerHarnessV1::CodexCli, |name| match name {
                "HOME" => Some(explicit.clone().into_os_string()),
                "USERPROFILE" => panic!("HOME takes precedence"),
                _ => None,
            })
            .unwrap();
        assert_eq!(context.home(), explicit);
        let context =
            NativeWorkerContext::from_lookup(WorkerHarnessV1::CodexCli, |name| match name {
                "HOME" => None,
                "USERPROFILE" => Some(profile.clone().into_os_string()),
                _ => None,
            })
            .unwrap();
        assert_eq!(context.home(), profile);
        assert_eq!(context.config_root(), profile.join(".codex"));
    }

    #[test]
    fn workspace_identity_uses_native_objects_without_verbatim_history_prefix() {
        let fixture = tempfile::tempdir().unwrap();
        let path = fixture.path().join("workspace % ! 空 格");
        std::fs::create_dir(&path).unwrap();
        let extended = std::fs::canonicalize(&path).unwrap();
        let (first, native) = windows_workspace(extended.to_str().unwrap()).unwrap();
        assert!(!native.starts_with(r"\\?\"));
        assert_eq!(windows_workspace(&native).unwrap().0, first);
        let renamed = fixture.path().join("old-workspace");
        std::fs::rename(&path, &renamed).unwrap();
        std::fs::create_dir(&path).unwrap();
        assert_ne!(windows_workspace(path.to_str().unwrap()).unwrap().0, first);
    }
}
