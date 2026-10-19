use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const MANIFEST_SCHEMA: &str = "hiroute.omarchy-integration/v1";
pub const TRANSACTION_SCHEMA: &str = "hiroute.omarchy-transaction/v1";
pub const PLUGIN_ID: &str = "hiroute.status";
pub const ACCOUNT_BOUNDARY: &str = "Routed sessions use the subscription connection configured in HiRoute. They do not follow Omarchy's active account.";

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemKind {
    DesktopEntry,
    Icon,
    Plugin,
}

#[derive(Clone)]
pub struct Asset {
    pub path: PathBuf,
    pub kind: ItemKind,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OwnedItem {
    pub path: PathBuf,
    pub sha256: String,
    pub release: String,
    pub kind: ItemKind,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub schema: String,
    pub generation: String,
    pub enabled: bool,
    pub notifications: bool,
    pub first_run_opened: bool,
    pub omarchy_command: PathBuf,
    pub items: Vec<OwnedItem>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemState {
    Present,
    Missing,
    Changed,
    Outdated,
}
#[derive(Debug, Serialize)]
pub struct ItemStatus {
    pub path: PathBuf,
    pub kind: ItemKind,
    pub state: ItemState,
}
#[derive(Debug, Serialize)]
pub struct FileStatus {
    pub enabled: bool,
    pub notifications: bool,
    pub recovery_required: bool,
    pub items: Vec<ItemStatus>,
    pub account_boundary: &'static str,
}
#[derive(Debug, Serialize)]
pub struct DisableResult {
    pub disabled: bool,
    pub kept_changed: Vec<PathBuf>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Change {
    pub after: OwnedItem,
    pub before: Option<OwnedItem>,
    pub before_bytes: Option<Vec<u8>>,
    pub stage: PathBuf,
    pub device: u64,
    pub inode: u64,
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Transaction {
    pub schema: String,
    pub before: Option<Manifest>,
    pub after: Manifest,
    pub changes: Vec<Change>,
    pub plugin_was_enabled: bool,
    pub enable_attempted: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Omarchy was not detected")]
    NotOmarchy,
    #[error("a current-user standalone Linux installation is required")]
    InstallationRequired,
    #[error("the Omarchy integration record is invalid")]
    InvalidRecord,
    #[error("another integration operation is running")]
    Busy,
    #[error("an unowned or changed integration item was preserved: {0}")]
    Conflict(PathBuf),
    #[error("the public Omarchy plugin command failed")]
    PluginFailed,
    #[error("the Omarchy plugin was not discovered or enabled before its deadline")]
    RegistrationTimeout,
    #[error("integration recovery could not safely restore the original items")]
    RecoveryRequired,
    #[error(
        "the at-most-once notification record reached its capacity; no delivery was retried or discarded"
    )]
    NotificationCapacity,
    #[error(transparent)]
    Io(#[from] std::io::Error),
}
