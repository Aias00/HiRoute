//! Optional Omarchy files. Native Agent configuration and service lifecycle are not owned here.
pub mod assets;
mod files;
pub mod launch;
pub mod notifications;
pub mod process;
mod types;
use serde::Deserialize;
use std::collections::BTreeSet;
use std::ffi::OsString;
use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
pub use types::*;

pub trait PluginControl {
    fn validate(&self, directory: &Path) -> Result<(), Error>;
    fn state(&self, timeout: Duration) -> Result<Option<bool>, Error>;
    fn enable(&self, enabled: bool) -> Result<(), Error>;
}
pub struct PublicPluginControl {
    pub executable: PathBuf,
}
impl PluginControl for PublicPluginControl {
    fn validate(&self, directory: &Path) -> Result<(), Error> {
        let out = process::run(
            &self.executable,
            &[
                "plugin".into(),
                "validate".into(),
                directory.as_os_str().into(),
            ],
            Duration::from_secs(5),
        )?;
        if out.success {
            Ok(())
        } else {
            Err(Error::PluginFailed)
        }
    }
    fn state(&self, timeout: Duration) -> Result<Option<bool>, Error> {
        let out = process::run(
            &self.executable,
            &["plugin".into(), "list".into(), "--json".into()],
            timeout,
        )?;
        if !out.success {
            return Err(Error::PluginFailed);
        }
        #[derive(Deserialize)]
        struct Row {
            id: String,
            enabled: bool,
        }
        let rows: Vec<Row> =
            serde_json::from_slice(&out.stdout).map_err(|_| Error::PluginFailed)?;
        if rows.len() > 512 {
            return Err(Error::PluginFailed);
        }
        let mut matches = rows.into_iter().filter(|row| row.id == PLUGIN_ID);
        let first = matches.next().map(|row| row.enabled);
        if matches.next().is_some() {
            return Err(Error::PluginFailed);
        }
        Ok(first)
    }
    fn enable(&self, enabled: bool) -> Result<(), Error> {
        let args: Vec<OsString> = if enabled {
            vec![
                "plugin".into(),
                "enable".into(),
                PLUGIN_ID.into(),
                "--section".into(),
                "right".into(),
            ]
        } else {
            vec!["plugin".into(), "disable".into(), PLUGIN_ID.into()]
        };
        if process::run(&self.executable, &args, Duration::from_secs(5))?.success {
            Ok(())
        } else {
            Err(Error::PluginFailed)
        }
    }
}

pub struct Context {
    pub home: PathBuf,
    pub state: PathBuf,
    pub release: String,
    pub omarchy_command: PathBuf,
}
impl Context {
    pub fn from_environment(require_detection: bool) -> Result<Self, Error> {
        if !cfg!(target_os = "linux") {
            return Err(Error::NotOmarchy);
        }
        let layout =
            crate::StandaloneLayout::from_environment().map_err(|_| Error::InstallationRequired)?;
        files::no_aliases(&layout.home)?;
        let (_, marker) = files::read(&layout.marker_path, 0o600, 16 * 1024)?
            .ok_or(Error::InstallationRequired)?;
        if marker.uid() != nix::unistd::geteuid().as_raw() {
            return Err(Error::InstallationRequired);
        }
        let record = crate::read_standalone_install_record(&layout.marker_path)
            .map_err(|_| Error::InstallationRequired)?;
        if record.target != crate::standalone_host_target() {
            return Err(Error::InstallationRequired);
        }
        let state = layout.state_root.join("integrations");
        let mut context = Self {
            home: layout.home,
            state,
            release: record.version,
            omarchy_command: PathBuf::new(),
        };
        let command = find_executable("omarchy");
        if require_detection {
            let root = std::env::var_os("OMARCHY_PATH")
                .map(PathBuf::from)
                .filter(|p| p.is_dir());
            if root.is_none() || command.is_none() {
                return Err(Error::NotOmarchy);
            }
        }
        context.omarchy_command = match command {
            Some(command) => command,
            None => context
                .load()?
                .map(|m| m.omarchy_command)
                .ok_or(Error::NotOmarchy)?,
        };
        Ok(context)
    }
    pub fn manifest_path(&self) -> PathBuf {
        self.state.join("omarchy.v1.json")
    }
    fn transaction_path(&self) -> PathBuf {
        self.state.join("omarchy-transaction.v1.json")
    }
    pub fn plugin_directory(&self) -> PathBuf {
        self.home.join(".config/omarchy/plugins/hiroute.status")
    }
    pub fn stable_cli(&self) -> PathBuf {
        self.home.join(".local/bin/hiroute")
    }
    fn allowed(&self, item: &OwnedItem) -> bool {
        match item.kind {
            ItemKind::DesktopEntry => [
                "hiroute-codex-routed.desktop",
                "hiroute-claude-code-routed.desktop",
                "hiroute-console.desktop",
            ]
            .iter()
            .any(|n| item.path == self.home.join(".local/share/applications").join(n)),
            ItemKind::Icon => {
                item.path
                    == self
                        .home
                        .join(".local/share/icons/hicolor/scalable/apps/hiroute.svg")
            }
            ItemKind::Plugin => [
                "manifest.json",
                "Panel.qml",
                "Summary.js",
                "i18n/en.json",
                "i18n/zh.json",
            ]
            .iter()
            .any(|n| item.path == self.plugin_directory().join(n)),
        }
    }
    fn validate_manifest(&self, m: &Manifest) -> Result<(), Error> {
        if m.schema != MANIFEST_SCHEMA
            || m.generation.len() != 32
            || !m.generation.bytes().all(|b| b.is_ascii_hexdigit())
            || !m.omarchy_command.is_absolute()
            || m.items.len() > 32
        {
            return Err(Error::InvalidRecord);
        }
        let mut paths = BTreeSet::new();
        for item in &m.items {
            if !self.allowed(item)
                || !paths.insert(&item.path)
                || item.sha256.len() != 64
                || !item.sha256.bytes().all(|b| b.is_ascii_hexdigit())
                || item.release.is_empty()
                || item.release.len() > 64
                || !item
                    .release
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
            {
                return Err(Error::InvalidRecord);
            }
        }
        Ok(())
    }
    pub fn load(&self) -> Result<Option<Manifest>, Error> {
        let Some((bytes, _)) = files::read(&self.manifest_path(), 0o600, 128 * 1024)? else {
            return Ok(None);
        };
        let manifest = serde_json::from_slice(&bytes).map_err(|_| Error::InvalidRecord)?;
        self.validate_manifest(&manifest)?;
        Ok(Some(manifest))
    }
    pub fn status(&self, assets: &[Asset]) -> Result<FileStatus, Error> {
        let manifest = self.load()?;
        let mut items = Vec::new();
        for item in manifest
            .as_ref()
            .map(|m| m.items.as_slice())
            .unwrap_or_default()
        {
            let state = match files::read(&item.path, 0o644, 512 * 1024) {
                Ok(None) => ItemState::Missing,
                Err(Error::Conflict(_)) => ItemState::Changed,
                Ok(Some((bytes, _))) if files::hash(&bytes) != item.sha256 => ItemState::Changed,
                Ok(Some(_))
                    if item.release != self.release
                        || assets
                            .iter()
                            .find(|a| a.path == item.path)
                            .is_none_or(|a| files::hash(&a.bytes) != item.sha256) =>
                {
                    ItemState::Outdated
                }
                Ok(Some(_)) => ItemState::Present,
                Err(e) => return Err(e),
            };
            items.push(ItemStatus {
                path: item.path.clone(),
                kind: item.kind,
                state,
            });
        }
        Ok(FileStatus {
            enabled: manifest.as_ref().is_some_and(|m| m.enabled),
            notifications: manifest.as_ref().is_some_and(|m| m.notifications),
            recovery_required: self.transaction_path().try_exists()?,
            items,
            account_boundary: ACCOUNT_BOUNDARY,
        })
    }
    fn check_assets(&self, assets: &[Asset]) -> Result<(), Error> {
        let mut seen = BTreeSet::new();
        if assets.is_empty() || assets.len() > 32 {
            return Err(Error::InvalidRecord);
        }
        for a in assets {
            let item = OwnedItem {
                path: a.path.clone(),
                kind: a.kind,
                sha256: files::hash(&a.bytes),
                release: self.release.clone(),
            };
            if !self.allowed(&item) || !seen.insert(&a.path) || a.bytes.len() > 512 * 1024 {
                return Err(Error::InvalidRecord);
            }
        }
        Ok(())
    }
    pub fn preflight_assets(&self, assets: &[Asset]) -> Result<(), Error> {
        self.preflight(assets, false).map(|_| ())
    }
    fn preflight(&self, assets: &[Asset], refresh: bool) -> Result<Option<Manifest>, Error> {
        self.check_assets(assets)?;
        let before = self.load()?;
        if before.is_none() && self.plugin_directory().try_exists()? {
            return Err(Error::Conflict(self.plugin_directory()));
        }
        for asset in assets {
            let old = before
                .as_ref()
                .and_then(|m| m.items.iter().find(|i| i.path == asset.path));
            match (old, files::read(&asset.path, 0o644, 512 * 1024)) {
                (None, Ok(None)) => {}
                (None, _) => return Err(Error::Conflict(asset.path.clone())),
                (Some(_), Ok(None)) => {}
                (Some(old), Ok(Some((bytes, _)))) if files::hash(&bytes) == old.sha256 => {}
                (Some(_), _) if refresh => {}
                _ => return Err(Error::Conflict(asset.path.clone())),
            }
        }
        Ok(before)
    }
    fn save(&self, manifest: &Manifest) -> Result<(), Error> {
        self.validate_manifest(manifest)?;
        files::write_private(
            &self.manifest_path(),
            &serde_json::to_vec(manifest).map_err(|_| Error::InvalidRecord)?,
        )
    }
    fn save_transaction(&self, t: &Transaction) -> Result<(), Error> {
        files::write_private(
            &self.transaction_path(),
            &serde_json::to_vec(t).map_err(|_| Error::InvalidRecord)?,
        )
    }
    fn load_transaction(&self) -> Result<Option<Transaction>, Error> {
        let Some((bytes, _)) = files::read(&self.transaction_path(), 0o600, 8 * 1024 * 1024)?
        else {
            return Ok(None);
        };
        let t: Transaction = serde_json::from_slice(&bytes).map_err(|_| Error::InvalidRecord)?;
        if t.schema != TRANSACTION_SCHEMA || t.changes.len() > 32 {
            return Err(Error::InvalidRecord);
        }
        self.validate_manifest(&t.after)?;
        if let Some(m) = &t.before {
            self.validate_manifest(m)?;
        }
        for c in &t.changes {
            if !self.allowed(&c.after)
                || c.stage.parent() != c.after.path.parent()
                || !c
                    .stage
                    .file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with(".hiroute-omarchy-") && n.len() == 49)
                || c.before
                    .as_ref()
                    .is_some_and(|b| b.path != c.after.path || b.kind != c.after.kind)
                || c.before.is_some() != c.before_bytes.is_some()
                || c.before
                    .as_ref()
                    .zip(c.before_bytes.as_ref())
                    .is_some_and(|(b, bytes)| files::hash(bytes) != b.sha256)
            {
                return Err(Error::InvalidRecord);
            }
            files::no_aliases(&c.stage)?;
        }
        Ok(Some(t))
    }
    fn recover_locked(&self, control: &impl PluginControl) -> Result<(), Error> {
        let Some(t) = self.load_transaction()? else {
            return Ok(());
        };
        if self.load()?.as_ref() != Some(&t.after) {
            if t.enable_attempted && !t.plugin_was_enabled {
                control.enable(false).map_err(|_| Error::RecoveryRequired)?;
            }
            files::rollback(self, &t)?;
        }
        for c in &t.changes {
            files::remove_stage(c)?;
        }
        fs::remove_file(self.transaction_path())?;
        if t.before.is_none() && self.load()?.is_none() {
            self.remove_empty_plugin_directories();
        }
        Ok(())
    }
    pub fn recover(&self, control: &impl PluginControl) -> Result<(), Error> {
        let _lock = files::lock(self)?;
        self.recover_locked(control)
    }
    pub fn enable(
        &self,
        assets: &[Asset],
        control: &impl PluginControl,
        notifications: Option<bool>,
        refresh: bool,
        registration_timeout: Duration,
    ) -> Result<Manifest, Error> {
        if !self.transaction_path().try_exists()? {
            if refresh
                && let Some(m) = self.load()?
                && !m.enabled
            {
                return Ok(m);
            }
            self.preflight(assets, refresh)?;
        }
        let _lock = files::lock(self)?;
        self.recover_locked(control)?;
        let before = self.preflight(assets, refresh)?;
        if refresh && before.is_none() {
            return Err(Error::InvalidRecord);
        }
        let was_enabled = control
            .state(registration_timeout.min(Duration::from_secs(1)))?
            .unwrap_or(false);
        let mut after = Manifest {
            schema: MANIFEST_SCHEMA.into(),
            generation: before
                .as_ref()
                .map(|m| m.generation.clone())
                .unwrap_or_default(),
            enabled: if refresh {
                before.as_ref().is_some_and(|m| m.enabled)
            } else {
                true
            },
            notifications: notifications
                .or_else(|| before.as_ref().map(|m| m.notifications))
                .unwrap_or(true),
            first_run_opened: before.as_ref().is_some_and(|m| m.first_run_opened),
            omarchy_command: self.omarchy_command.clone(),
            items: Vec::new(),
        };
        let mut changes = Vec::new();
        // Stages are private to this attempt until the journal is durable. Clean them
        // on preparation failure; after publication the journal owns their recovery.
        let prepared = (|| {
            for asset in assets {
                let old = before
                    .as_ref()
                    .and_then(|m| m.items.iter().find(|i| i.path == asset.path));
                let current = files::read(&asset.path, 0o644, 512 * 1024);
                let unchanged = match &current {
                    Ok(None) => true,
                    Ok(Some((bytes, _))) => old.is_some_and(|old| files::hash(bytes) == old.sha256),
                    Err(_) => false,
                };
                if refresh && old.is_some() && !unchanged {
                    after.items.push(old.unwrap().clone());
                    continue;
                }
                let bytes = current?;
                let original = bytes.as_ref().map(|(b, _)| b.clone());
                let item = OwnedItem {
                    path: asset.path.clone(),
                    kind: asset.kind,
                    sha256: files::hash(&asset.bytes),
                    release: self.release.clone(),
                };
                if original
                    .as_ref()
                    .is_none_or(|b| files::hash(b) != item.sha256)
                {
                    let (stage, device, inode) = files::stage(&asset.path, &asset.bytes)?;
                    changes.push(Change {
                        after: item.clone(),
                        before: original.as_ref().and(old.cloned()),
                        before_bytes: original,
                        stage,
                        device,
                        inode,
                    });
                }
                after.items.push(item);
            }
            Ok::<(), Error>(())
        })();
        if let Err(error) = prepared {
            for change in &changes {
                files::remove_stage(change)?;
            }
            if before.is_none() {
                self.remove_empty_plugin_directories();
            }
            return Err(error);
        }
        if changes.is_empty() && before.as_ref() == Some(&after) && (refresh || was_enabled) {
            return Ok(after);
        }
        after.generation = files::nonce()?;
        let mut transaction = Transaction {
            schema: TRANSACTION_SCHEMA.into(),
            before,
            after,
            changes,
            plugin_was_enabled: was_enabled,
            enable_attempted: false,
        };
        if let Err(error) = self.save_transaction(&transaction) {
            for change in &transaction.changes {
                files::remove_stage(change)?;
            }
            if transaction.before.is_none() {
                self.remove_empty_plugin_directories();
            }
            return Err(error);
        }
        let applied = (|| {
            for change in &transaction.changes {
                files::apply(change)?;
            }
            control.validate(&self.plugin_directory())?;
            wait_registered(control, false, registration_timeout)?;
            if !refresh && !was_enabled {
                transaction.enable_attempted = true;
                self.save_transaction(&transaction)?;
                control.enable(true)?;
                wait_registered(control, true, registration_timeout)?;
            }
            self.save(&transaction.after)?;
            Ok(())
        })();
        if let Err(error) = applied {
            self.recover_locked(control)
                .map_err(|_| Error::RecoveryRequired)?;
            return Err(error);
        }
        self.recover_locked(control)?;
        Ok(transaction.after)
    }
    pub fn configure_notifications(&self, enabled: bool) -> Result<Manifest, Error> {
        let _lock = files::lock(self)?;
        if self.transaction_path().exists() {
            return Err(Error::RecoveryRequired);
        }
        let mut m = self.load()?.ok_or(Error::InvalidRecord)?;
        m.notifications = enabled;
        self.save(&m)?;
        Ok(m)
    }
    pub fn mark_first_run_opened(&self) -> Result<(), Error> {
        let _lock = files::lock(self)?;
        if self.transaction_path().exists() {
            return Err(Error::RecoveryRequired);
        }
        let mut m = self.load()?.ok_or(Error::InvalidRecord)?;
        m.first_run_opened = true;
        self.save(&m)
    }
    pub fn disable(&self, control: &impl PluginControl) -> Result<DisableResult, Error> {
        if self.load()?.is_none() && !self.transaction_path().exists() {
            return Ok(DisableResult {
                disabled: true,
                kept_changed: Vec::new(),
            });
        }
        let _lock = files::lock(self)?;
        self.recover_locked(control)?;
        let Some(mut manifest) = self.load()? else {
            return Ok(DisableResult {
                disabled: true,
                kept_changed: Vec::new(),
            });
        };
        control.enable(false)?;
        let mut kept = Vec::new();
        for item in &manifest.items {
            if files::matches(&item.path, item)? {
                fs::remove_file(&item.path)?;
            } else {
                match fs::symlink_metadata(&item.path) {
                    Ok(_) => kept.push(item.clone()),
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) => return Err(e.into()),
                }
            }
        }
        let kept_changed = kept.iter().map(|i| i.path.clone()).collect();
        if kept.is_empty() {
            fs::remove_file(self.manifest_path())?;
        } else {
            manifest.items = kept;
            manifest.enabled = false;
            self.save(&manifest)?;
        }
        self.remove_empty_plugin_directories();
        Ok(DisableResult {
            disabled: true,
            kept_changed,
        })
    }
    fn remove_empty_plugin_directories(&self) {
        if files::no_aliases(&self.plugin_directory()).is_err() {
            return;
        }
        let _ = fs::remove_dir(self.plugin_directory().join("i18n"));
        let _ = fs::remove_dir(self.plugin_directory());
    }
}
fn wait_registered(
    control: &impl PluginControl,
    enabled: bool,
    timeout: Duration,
) -> Result<(), Error> {
    let start = Instant::now();
    loop {
        let remaining = timeout.saturating_sub(start.elapsed());
        if remaining.is_zero() {
            return Err(Error::RegistrationTimeout);
        }
        if control
            .state(remaining.min(Duration::from_secs(1)))?
            .is_some_and(|state| !enabled || state)
        {
            return Ok(());
        }
        std::thread::sleep(remaining.min(Duration::from_millis(50)));
    }
}
pub fn find_executable(name: &str) -> Option<PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH")?)
        .filter(|p| p.is_absolute())
        .map(|p| p.join(name))
        .find(|p| {
            p.metadata()
                .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        })
}

#[cfg(test)]
mod tests;
