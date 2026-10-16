//! Current-account Task installation. SCM ownership is never adopted by this path.
use super::*;
use crate::{
    ConsoleListenerDesiredV1, ConsoleListenerStore, StandaloneInstallRecordV1, StandaloneLayout,
};
use fs2::FileExt;
use hiroute_platform::windows::{
    background::{self, Definition, Manager},
    lifecycle, security, task,
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    fs::{self, File},
    path::PathBuf,
    time::Duration,
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstallRequest {
    pub schema: String,
    pub package_directory: PathBuf,
    #[serde(default = "yes")]
    pub autostart: bool,
    #[serde(default)]
    pub console_loopback: bool,
    #[serde(default)]
    pub allow_legacy_stop: bool,
    #[serde(default = "timeout")]
    pub timeout_ms: u64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UninstallRequest {
    pub schema: String,
    #[serde(default)]
    pub remove_data: bool,
    #[serde(default)]
    pub allow_legacy_stop: bool,
    #[serde(default = "timeout")]
    pub timeout_ms: u64,
}
fn yes() -> bool {
    true
}
fn timeout() -> u64 {
    30_000
}
fn deadline(value: u64) -> io::Result<Duration> {
    if !(100..=300_000).contains(&value) {
        return Err(invalid());
    }
    Ok(Duration::from_millis(value))
}
fn layout() -> io::Result<StandaloneLayout> {
    hiroute_platform::windows::install::require_x64()?;
    StandaloneLayout::from_environment().map_err(|_| invalid())
}
fn definition_path(layout: &StandaloneLayout) -> PathBuf {
    layout.data_root.join("service/windows-background.json")
}
fn definition(value: &Value) -> io::Result<Definition> {
    let value: Definition = serde_json::from_value(value.clone()).map_err(|_| invalid())?;
    value.validate()?;
    if value.manager != Manager::Task {
        return Err(io::Error::other("INSTALL_SCM_MANAGED"));
    }
    Ok(value)
}
fn admit_existing(layout: &StandaloneLayout) -> io::Result<()> {
    let path = definition_path(layout);
    if path.try_exists()? {
        let value = background::read_definition(&path)?;
        if value.manager != Manager::Task {
            return Err(io::Error::other("INSTALL_SCM_MANAGED"));
        }
    }
    if let Some(journal) = Journal::read(layout)? {
        definition(&journal.new_definition)?;
        if let Some(old) = &journal.old_definition {
            definition(old)?;
        }
    }
    Ok(())
}
pub fn preflight() -> io::Result<()> {
    admit_existing(&layout()?)
}
fn lock(layout: &StandaloneLayout) -> io::Result<File> {
    // Admission is deliberately before creation of directories or the lock file.
    admit_existing(layout)?;
    let file = lock_file(layout)?;
    admit_existing(layout)?;
    Ok(file)
}
fn lock_file(layout: &StandaloneLayout) -> io::Result<File> {
    security::initialize_current_process_private_defaults()?;
    let parent = security::ensure_private_directory(&layout.data_root.join("service"))?;
    let path = layout.data_root.join("service/install.lock");
    let file = match security::open_read_write(&path) {
        Ok(file) => file,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            security::create_owner_only_file_at(&parent, std::ffi::OsStr::new("install.lock"))?
        }
        Err(e) => return Err(e),
    };
    file.try_lock_exclusive()
        .map_err(|_| io::Error::other("INSTALL_MAINTENANCE_BUSY"))?;
    Ok(file)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FinalizeRequest {
    pub schema: String,
    pub operation: String,
}

/// Complete only an already-prepared Python SCM operation after native registration proof.
pub fn finalize_legacy_scm(request: FinalizeRequest) -> io::Result<Value> {
    use hiroute_platform::windows::service;
    if request.schema != "hiroute.windows-finalize-request/v1" {
        return Err(invalid());
    }
    let layout = layout()?;
    let initial = Journal::read(&layout)?.ok_or_else(invalid)?;
    let native: Definition =
        serde_json::from_value(initial.new_definition.clone()).map_err(|_| invalid())?;
    native.validate()?;
    if native.manager != Manager::Scm {
        return Err(invalid());
    }
    let _lock = lock_file(&layout)?;
    let mut journal = Journal::read(&layout)?.ok_or_else(invalid)?;
    if journal.new_definition != initial.new_definition
        || request.operation
            != if journal.operation == Operation::Install {
                "install"
            } else {
                "uninstall"
            }
    {
        return Err(invalid());
    }
    if journal.phase == Phase::Prepared {
        if journal.operation == Operation::Install {
            journal.apply_files()?;
            write_json(&layout.marker_path, &journal.new_marker)?;
            service::verify_registration(&native)?;
            write_json(&journal.new_marker.service_definition, &native)?;
        } else {
            if !service::registration_absent(&native)? {
                return Err(invalid());
            }
            journal.apply_files()?;
        }
        journal.commit(&layout)?;
    }
    journal.finish(&layout)?;
    Ok(
        json!({"schema":"hiroute.windows-finalize-result/v1","operation":request.operation,"completed":true,"manager":"scm","started":false}),
    )
}
struct NativeHost<'a> {
    layout: &'a StandaloneLayout,
    timeout: Duration,
    allow_legacy_stop: bool,
}
impl ManagerHost for NativeHost<'_> {
    fn quiesce(&mut self, values: &[&Value]) -> io::Result<()> {
        let mut mismatch = false;
        for value in values {
            let value = definition(value)?;
            match task::active(&value) {
                Ok(false) => return Ok(()),
                Ok(true) => {
                    let endpoint = self.layout.lifecycle_endpoint();
                    match lifecycle::drain(&endpoint, &value.daemon, self.timeout) {
                        Ok(()) => {}
                        Err(e)
                            if e.kind() == io::ErrorKind::Unsupported && self.allow_legacy_stop => {
                        }
                        Err(e) => return Err(e),
                    }
                    return lifecycle::stop_with_fallback(
                        &endpoint,
                        &value.daemon,
                        self.timeout,
                        || task::force_stop(&value),
                    );
                }
                Err(_) => mismatch = true,
            }
        }
        if mismatch { Err(invalid()) } else { Ok(()) }
    }
    fn register(&mut self, value: &Value, previous: Option<&Value>) -> io::Result<()> {
        let value = definition(value)?;
        let previous = previous.map(definition).transpose()?;
        value.verify_image()?;
        task::register(&value, previous.as_ref())
    }
    fn unregister(&mut self, value: &Value) -> io::Result<()> {
        task::unregister(&definition(value)?)
    }
}
fn current(
    layout: &StandaloneLayout,
) -> io::Result<Option<(StandaloneInstallRecordV1, Value, Package)>> {
    let marker: Option<StandaloneInstallRecordV1> = read_json(&layout.marker_path, MAX_RECORD)?;
    let value: Option<Value> = read_json(&definition_path(layout), MAX_RECORD)?;
    let package: Option<Package> =
        read_json(&layout.data_root.join("package-manifest.json"), 1024 * 1024)?;
    match (marker, value, package) {
        (None, None, None) => Ok(None),
        (Some(marker), Some(value), Some(package)) => {
            let native = definition(&value)?;
            package.validate()?;
            marker.validate().map_err(|_| invalid())?;
            if native.daemon != marker.install_root.join("hirouted.exe")
                || marker.service_definition != definition_path(layout)
                || marker.target != WINDOWS_TARGET
                || package.version != marker.version
                || !within(
                    &marker.install_root,
                    &layout.home.join(".local/lib/hiroute"),
                )
            {
                return Err(invalid());
            }
            Ok(Some((marker, value, package)))
        }
        _ => Err(invalid()),
    }
}
fn entries(
    root: &Path,
    layout: &StandaloneLayout,
    package: &Package,
) -> Vec<(PathBuf, PathBuf, EntryKind, String)> {
    let mut entries = vec![];
    for name in ["hiroute.exe", "hirouted.exe"] {
        entries.push((
            layout.home.join(".local/bin").join(name),
            root.join(name),
            EntryKind::Hardlink,
            format!("bin/{name}"),
        ));
    }
    for prefix in [".agents", ".claude"] {
        let name = "skills/hiroute-management/SKILL.md";
        entries.push((
            layout.home.join(prefix).join(name),
            root.join(name),
            EntryKind::Copy,
            name.into(),
        ));
    }
    for name in package
        .files
        .keys()
        .filter(|n| n.starts_with("licenses/") || n.starts_with("docs/"))
    {
        entries.push((
            layout.data_root.join(name),
            root.join(name),
            EntryKind::Copy,
            name.clone(),
        ));
    }
    entries.push((
        layout.data_root.join("package-manifest.json"),
        root.join("package-manifest.json"),
        EntryKind::Copy,
        "__package_manifest__".into(),
    ));
    entries
}
fn publish_package(source: &Path, root: &Path, package: &Package) -> io::Result<()> {
    if root.try_exists()? {
        return package.verify_directory(root);
    }
    directory(root.parent().ok_or_else(invalid)?)?;
    let stage = sibling(root)?;
    directory(&stage)?;
    for name in package.files.keys() {
        let relative = Package::relative(name);
        let target = stage.join(relative);
        directory(target.parent().ok_or_else(invalid)?)?;
        copy_new(&source.join(relative), &target)?;
    }
    hiroute_platform::durable::write_private_new(
        &stage.join("package-manifest.json"),
        &package.canonical()?,
    )?;
    package.verify_directory(&stage)?;
    hiroute_platform::durable::rename(&stage, root)
}
pub fn install(request: InstallRequest) -> io::Result<Value> {
    if request.schema != "hiroute.windows-install-request/v1" {
        return Err(invalid());
    }
    hiroute_platform::windows::install::require_x64()?;
    let package = Package::read(&request.package_directory)?;
    let timeout = deadline(request.timeout_ms)?;
    let layout = layout()?;
    let _lock = lock(&layout)?;
    let mut host = NativeHost {
        layout: &layout,
        timeout,
        allow_legacy_stop: request.allow_legacy_stop,
    };
    if let Some(journal) = Journal::read(&layout)? {
        journal.recover(&layout, &mut host)?;
    }
    let old = current(&layout)?;
    let root = layout.home.join(".local/lib/hiroute").join(format!(
        "{}-{}",
        package.version,
        &package.revision[..package.revision.len().min(12)]
    ));
    publish_package(&request.package_directory, &root, &package)?;
    let native = Definition::new(Manager::Task, root.join("hirouted.exe"))?;
    let marker = StandaloneInstallRecordV1 {
        schema_version: crate::STANDALONE_INSTALL_SCHEMA_V1.into(),
        version: package.version.clone(),
        target: WINDOWS_TARGET.into(),
        install_root: root,
        service_definition: definition_path(&layout),
        installed_skills: [".agents", ".claude"]
            .iter()
            .map(|p| layout.home.join(p).join("skills/hiroute-management"))
            .collect(),
        cpa_binary: Some(
            native
                .daemon
                .parent()
                .ok_or_else(invalid)?
                .join("libexec/cliproxyapi.exe"),
        ),
        cpa_sha256: Some(package.cpa_digest().into()),
    };
    let planned = journal::plan(
        entries(&marker.install_root, &layout, &package),
        &layout,
        old.as_ref().map(|o| &o.2),
        false,
    )?;
    let mut journal = Journal {
        schema: JOURNAL_SCHEMA.into(),
        operation: Operation::Install,
        phase: Phase::Prepared,
        old_marker: old.as_ref().map(|o| o.0.clone()),
        old_definition: old.as_ref().map(|o| o.1.clone()),
        new_marker: marker,
        new_definition: serde_json::to_value(&native).map_err(|_| invalid())?,
        files: planned,
    };
    // Drain precedes preparation: an old daemon refusal cannot leave its admission blocked.
    if let Some(old) = &journal.old_definition {
        host.quiesce(&[old])?;
    }
    journal.persist(&layout)?;
    let result = (|| {
        journal.apply_files()?;
        write_json(&layout.marker_path, &journal.new_marker)?;
        host.register(&journal.new_definition, journal.old_definition.as_ref())?;
        write_json(
            &journal.new_marker.service_definition,
            &journal.new_definition,
        )?;
        task::set_autostart(&native, request.autostart)?;
        journal.commit(&layout)?;
        journal.finish(&layout)
    })();
    if let Err(error) = result {
        let _ = journal.recover(&layout, &mut host);
        return Err(error);
    }
    if request.console_loopback {
        ConsoleListenerStore::new(layout.gateway_config_root())
            .configure(ConsoleListenerDesiredV1::loopback())
            .map_err(|_| invalid())?;
    }
    Ok(
        json!({"schema":"hiroute.windows-install-result/v1","installed":true,"version":package.version,"manager":"task","cli":layout.home.join(".local/bin/hiroute.exe"),"started":false}),
    )
}
pub fn uninstall(
    request: UninstallRequest,
    restore_agents: impl FnOnce(&StandaloneLayout) -> io::Result<()>,
) -> io::Result<Value> {
    if request.schema != "hiroute.windows-uninstall-request/v1" {
        return Err(invalid());
    }
    let timeout = deadline(request.timeout_ms)?;
    let layout = layout()?;
    let lock = lock(&layout)?;
    let mut host = NativeHost {
        layout: &layout,
        timeout,
        allow_legacy_stop: request.allow_legacy_stop,
    };
    if let Some(journal) = Journal::read(&layout)? {
        journal.recover(&layout, &mut host)?;
    }
    let Some((marker, value, package)) = current(&layout)? else {
        return Ok(
            json!({"schema":"hiroute.windows-uninstall-result/v1","uninstalled":true,"already_absent":true}),
        );
    };
    let native = definition(&value)?;
    if !task::active(&native)? {
        task::start(&native)?;
    }
    restore_agents(&layout)?;
    let planned = journal::plan(
        entries(&marker.install_root, &layout, &package),
        &layout,
        Some(&package),
        true,
    )?;
    host.quiesce(&[&value])?;
    let mut journal = Journal {
        schema: JOURNAL_SCHEMA.into(),
        operation: Operation::Uninstall,
        phase: Phase::Prepared,
        old_marker: Some(marker.clone()),
        new_marker: marker,
        old_definition: Some(value.clone()),
        new_definition: value,
        files: planned,
    };
    journal.persist(&layout)?;
    host.unregister(&journal.new_definition)?;
    journal.apply_files()?;
    journal.commit(&layout)?;
    journal.finish(&layout)?;
    hiroute_platform::windows::shell_login::remove()?;
    drop(host);
    drop(lock);
    if request.remove_data {
        remove_tree(&layout.data_root)?;
        remove_tree(&layout.state_root)?;
    }
    Ok(
        json!({"schema":"hiroute.windows-uninstall-result/v1","uninstalled":true,"data_removed":request.remove_data,"state_preserved":!request.remove_data}),
    )
}
fn remove_tree(root: &Path) -> io::Result<()> {
    fn admit(path: &Path) -> io::Result<()> {
        let _held = security::open_identity(path, true)?;
        for entry in fs::read_dir(path)? {
            let path = entry?.path();
            let metadata = fs::symlink_metadata(&path)?;
            use std::os::windows::fs::MetadataExt;
            if metadata.file_attributes() & 0x400 != 0 {
                return Err(invalid());
            }
            if metadata.is_dir() {
                admit(&path)?;
            } else {
                let _held = regular(&path)?.ok_or_else(invalid)?;
            }
        }
        Ok(())
    }
    if root.try_exists()? {
        admit(root)?;
        fs::remove_dir_all(root)?;
    }
    Ok(())
}
