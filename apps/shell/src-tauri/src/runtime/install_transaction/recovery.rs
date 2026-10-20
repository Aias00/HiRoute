//! Preserve the complete Desktop, standalone payload and native registration before NSIS overlays it.
use super::*;
use hiroute_host_runtime::WindowsDesktopRestore;
use hiroute_host_runtime::{
    RetainedRecoveryPackage, StandaloneLayout, read_standalone_install_record,
};

pub(super) fn prepare(value: &Pending, incoming: &Package, daemon: &Path) -> io::Result<()> {
    if hash(daemon)?
        != incoming
            .files
            .get("bin/hirouted.exe")
            .ok_or_else(invalid)?
            .sha256
    {
        return Err(invalid());
    }
    let layout = StandaloneLayout::from_environment().map_err(|_| invalid())?;
    let Some(previous) = &value.previous_package else {
        // An uninstalled old data group still needs its matching package. The
        // candidate explains that refusal before any installed file is replaced.
        let report = hiroute_host_runtime::checked_upgrade_preflight(
            daemon,
            &layout.storage_root(),
            &data()?.join("missing-original-package"),
        )
        .map_err(io::Error::other)?;
        return windows::preflight_desktop_space(
            incoming,
            &data()?,
            fs::metadata(std::env::current_exe()?)?.len(),
            0,
            Some(&report),
        );
    };
    let marker = read_standalone_install_record(&layout.marker_path)?;
    if marker.install_root != *previous {
        return Err(invalid());
    }
    let package = Package::read(previous)?;
    if !value.old.is_empty()
        && ["hiroute-shell.exe", "uninstall.exe", INVENTORY]
            .iter()
            .any(|name| !value.old.contains_key(*name))
    {
        return Err(invalid());
    }
    let marker_bytes = serde_json::to_vec(&marker).map_err(|_| invalid())?;
    let shell_bytes = fs::metadata(std::env::current_exe()?)?.len();
    let mut copy_bytes: u64 = package.files.values().map(|file| file.size).sum();
    for name in value.old.keys() {
        copy_bytes = copy_bytes.saturating_add(fs::metadata(root()?.join(name))?.len());
    }
    windows::preflight_desktop_space(
        incoming,
        &data()?,
        shell_bytes,
        copy_bytes.saturating_add(1024 * 1024),
        None,
    )?;
    let mut identity = Sha256::new();
    identity.update(serde_json::to_vec(&value.old).map_err(|_| invalid())?);
    identity.update(&marker_bytes);
    identity.update(hash(&marker.service_definition)?);
    identity.update([
        u8::from(value.previous_autostart),
        u8::from(value.shell_login),
    ]);
    let bundle = data()?
        .join("recovery-packages")
        .join(format!("{:x}", identity.finalize()));
    security::ensure_private_directory(&bundle)?;
    // Both installed namespaces are necessary: the shell includes its generated
    // uninstaller/inventory, while the Task runs the independent version root.
    for (name, expected) in &value.old {
        backup_file(
            &data()?.join("install-backups").join(name),
            &bundle.join("desktop").join(name),
            expected,
        )?;
    }
    for (name, item) in &package.files {
        let name = Package::relative(name);
        backup_file(
            &previous.join(name),
            &bundle.join("program").join(name),
            &item.sha256,
        )?;
    }
    backup_file(
        &previous.join("package-manifest.json"),
        &bundle.join("program/package-manifest.json"),
        &hash(&previous.join("package-manifest.json"))?,
    )?;
    let mut files = BTreeMap::new();
    for (name, source) in [
        ("standalone.json", layout.marker_path.clone()),
        ("windows-background.json", marker.service_definition.clone()),
    ] {
        let relative = format!("installation/{name}");
        backup_file(&source, &bundle.join(&relative), &hash(&source)?)?;
        files.insert(relative, source);
    }
    for (source, relative, expected) in windows::recovery_files()? {
        backup_file(&source, &bundle.join(&relative), &expected)?;
        files.insert(relative, source);
    }
    let registration = bundle.join("installation/uninstall.reg");
    if !value.old.is_empty() && !present(&registration)? {
        hiroute_platform::windows::shell_registration::export(&registration)?;
    }
    let locations = WindowsDesktopRestore {
        schema: "hiroute.windows-desktop-restore/v1".into(),
        program: previous.clone(),
        desktop: if value.old.is_empty() {
            None
        } else {
            Some(root()?)
        },
        task_name: hiroute_platform::windows::background::read_definition(
            &marker.service_definition,
        )?
        .name,
        task_autostart: value.previous_autostart,
        shell_login: value.shell_login,
        files,
    };
    locations.validate()?;
    let bytes = serde_json::to_vec(&locations).map_err(|_| invalid())?;
    let path = bundle.join("restore-locations.json");
    if present(&path)? {
        if hash(&path)? != format!("{:x}", Sha256::digest(&bytes)) {
            return Err(invalid());
        }
    } else {
        hiroute_platform::durable::write_private_new(&path, &bytes)?;
    }
    Package::read(&bundle.join("program"))?;
    let retained = RetainedRecoveryPackage::capture(&layout.storage_root(), &bundle, daemon)?;
    let mut expected: std::collections::BTreeSet<String> = package
        .files
        .keys()
        .map(|name| format!("program/{}", Package::relative(name)))
        .collect();
    expected.insert("program/package-manifest.json".into());
    expected.insert("restore-locations.json".into());
    expected.extend(value.old.keys().map(|name| format!("desktop/{name}")));
    expected.extend(locations.files.keys().cloned());
    if !value.old.is_empty() {
        expected.insert("installation/uninstall.reg".into());
    }
    if retained
        .files
        .keys()
        .cloned()
        .collect::<std::collections::BTreeSet<_>>()
        != expected
    {
        return Err(invalid());
    }
    retained.verify(daemon)?;
    let report =
        hiroute_host_runtime::checked_upgrade_preflight(daemon, &layout.storage_root(), &bundle)
            .map_err(io::Error::other)?;
    windows::preflight_desktop_space(incoming, &data()?, shell_bytes, 0, Some(&report))?;
    retained.store()
}
