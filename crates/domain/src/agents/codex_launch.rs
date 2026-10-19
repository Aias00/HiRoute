//! Frozen launch facts for the standalone Codex profile. Native configuration is not a
//! source for reconstructing these facts at launch time.
use serde::{Deserialize, Serialize};

use crate::CanonicalDigest;

pub const CODEX_LAUNCH_SNAPSHOT_SCHEMA_V1: &str = "hiroute.codex-launch-snapshot/v1";
pub const MANAGED_CODEX_LAUNCH_DESCRIPTOR_SCHEMA_V1: &str =
    "hiroute.managed-codex-launch-descriptor/v1";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CodexLaunchSnapshotV1 {
    pub schema: String,
    pub executable: String,
    pub codex_home: String,
    pub profile_name: String,
    pub installation_identity: CanonicalDigest,
}

impl CodexLaunchSnapshotV1 {
    pub fn validate(&self) -> Result<(), super::ManagedLaunchError> {
        if self.schema != CODEX_LAUNCH_SNAPSHOT_SCHEMA_V1
            || self.profile_name != "hiroute"
            || !absolute_path(&self.executable)
            || !absolute_path(&self.codex_home)
            || CanonicalDigest::parse(self.installation_identity.as_str().to_owned()).is_err()
        {
            return Err(super::ManagedLaunchError::InvalidDescriptor);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ManagedCodexLaunchDescriptorV1 {
    pub schema: String,
    pub connection_id: String,
    pub snapshot: CodexLaunchSnapshotV1,
    pub snapshot_digest: CanonicalDigest,
    pub grant_generation: u64,
    pub publication_digest: CanonicalDigest,
    pub caller_forbidden_options: Vec<String>,
}

impl ManagedCodexLaunchDescriptorV1 {
    pub fn trusted(
        connection_id: String,
        snapshot: CodexLaunchSnapshotV1,
        grant_generation: u64,
        publication_digest: CanonicalDigest,
    ) -> Result<Self, super::ManagedLaunchError> {
        let snapshot_digest = CanonicalDigest::of(&snapshot)
            .map_err(|_| super::ManagedLaunchError::InvalidDescriptor)?;
        let value = Self {
            schema: MANAGED_CODEX_LAUNCH_DESCRIPTOR_SCHEMA_V1.into(),
            connection_id,
            snapshot,
            snapshot_digest,
            grant_generation,
            publication_digest,
            caller_forbidden_options: forbidden_options(),
        };
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), super::ManagedLaunchError> {
        self.snapshot.validate()?;
        if self.schema != MANAGED_CODEX_LAUNCH_DESCRIPTOR_SCHEMA_V1
            || self.grant_generation == 0
            || !self.connection_id.starts_with("agent-connection/")
            || self.connection_id.len() <= "agent-connection/".len()
            || self.connection_id.len() > 256
            || self.connection_id.contains("..")
            || self.connection_id.contains("//")
            || !self
                .connection_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-' | b'/' | b':'))
            || CanonicalDigest::of(&self.snapshot).ok().as_ref() != Some(&self.snapshot_digest)
            || CanonicalDigest::parse(self.publication_digest.as_str().to_owned()).is_err()
            || self.caller_forbidden_options != forbidden_options()
        {
            return Err(super::ManagedLaunchError::InvalidDescriptor);
        }
        Ok(())
    }
}

fn forbidden_options() -> Vec<String> {
    [
        "-p",
        "--profile",
        "--oss",
        "-c profile",
        "-c profiles.*",
        "-c model_provider",
        "-c model_providers.*",
    ]
    .map(str::to_owned)
    .to_vec()
}

fn absolute_path(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 4096
        && !value.contains(['\0', '\r', '\n'])
        && (value.starts_with('/')
            || value.starts_with("\\\\")
            || (value.as_bytes().get(1) == Some(&b':')
                && value
                    .as_bytes()
                    .first()
                    .is_some_and(u8::is_ascii_alphabetic)
                && value
                    .as_bytes()
                    .get(2)
                    .is_some_and(|b| matches!(b, b'\\' | b'/'))))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot() -> CodexLaunchSnapshotV1 {
        CodexLaunchSnapshotV1 {
            schema: CODEX_LAUNCH_SNAPSHOT_SCHEMA_V1.into(),
            executable: "/opt/codex/bin/codex".into(),
            codex_home: "/fixture/codex-home".into(),
            profile_name: "hiroute".into(),
            installation_identity: CanonicalDigest::of_bytes(b"installation"),
        }
    }

    #[test]
    fn descriptor_binds_snapshot_and_authority() {
        let descriptor = ManagedCodexLaunchDescriptorV1::trusted(
            "agent-connection/agent-context/codex/profile".into(),
            snapshot(),
            2,
            CanonicalDigest::of_bytes(b"publication"),
        )
        .unwrap();
        let mut changed = descriptor.clone();
        changed.snapshot.executable = "/opt/replaced/codex".into();
        assert!(changed.validate().is_err());
        changed = descriptor.clone();
        changed.grant_generation = 0;
        assert!(changed.validate().is_err());
        changed = descriptor;
        changed.caller_forbidden_options.clear();
        assert!(changed.validate().is_err());
    }

    #[test]
    fn snapshot_rejects_relative_paths_and_unknown_fields() {
        let mut value = snapshot();
        value.executable = "codex".into();
        assert!(value.validate().is_err());
        let mut encoded = serde_json::to_value(snapshot()).unwrap();
        encoded["credential"] = "not permitted".into();
        assert!(serde_json::from_value::<CodexLaunchSnapshotV1>(encoded).is_err());
    }
}
