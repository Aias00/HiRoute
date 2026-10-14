//! Real npm filesystem layouts exercise the scanner's locator without running
//! a generated shim, JavaScript, Node, or a native Agent.
use super::*;
use hiroute_platform::windows::security;
use std::{fs, io::Write};

struct NpmFixture {
    cleanup: crate::agents::probe_files::PrivateProbeRoot,
    prefix: PathBuf,
    package: PathBuf,
    shim: PathBuf,
    entry: PathBuf,
    marker: PathBuf,
}

impl NpmFixture {
    fn new(local: bool) -> Self {
        let cleanup = crate::agents::probe_files::temporary("hiroute-pi-locator-").unwrap();
        let prefix = cleanup.path().join("npm prefix 中文 & literal");
        directory(&prefix);
        let modules = prefix.join("node_modules");
        directory(&modules);
        let scope = modules.join("@earendil-works");
        directory(&scope);
        let package = scope.join("pi-coding-agent");
        directory(&package);
        let dist = package.join("dist");
        directory(&dist);
        directory(&dist.join("bundle"));
        let bin = if local {
            let bin = modules.join(".bin");
            directory(&bin);
            bin
        } else {
            prefix.clone()
        };
        let marker = cleanup.path().join("shim-must-not-run");
        let shim = bin.join("pi.cmd");
        file(
            &shim,
            format!("@echo forbidden > \"{}\"\r\n", marker.display()).as_bytes(),
        );
        let entry = package.join("dist/bundle/cli.js");
        file(
            &entry,
            b"throw new Error('discovery must not execute this entry');\n",
        );
        file(
            &package.join("package.json"),
            &serde_json::to_vec(&declaration()).unwrap(),
        );
        Self {
            cleanup,
            prefix,
            package,
            shim,
            entry,
            marker,
        }
    }

    fn scanner(&self, selected: &Path) -> FilesystemAgentScannerV1 {
        let mut layout =
            AgentFilesystemLayoutV1::from_process(self.cleanup.path(), self.cleanup.path());
        layout.pi_executable = selected.to_owned();
        // No ambient native user configuration participates in this locator test.
        layout.pi_config_root = self.cleanup.path().join("unconfigured-pi");
        FilesystemAgentScannerV1::new(layout, ClaudeRegistrationIndexV1::default())
    }

    fn write_declaration(&self, value: &serde_json::Value) {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(self.package.join("package.json"))
            .unwrap();
        file.write_all(&serde_json::to_vec(value).unwrap()).unwrap();
        file.sync_all().unwrap();
    }

    fn assert_admitted(&self, selected: &Path) {
        let scanner = self.scanner(selected);
        let admitted = scanner
            .pi_executable_target()
            .expect("the scanner must resolve the package-declared Pi entry");
        let held = security::open_identity(&admitted, false).unwrap();
        let expected = security::open_identity(&self.entry, false).unwrap();
        assert_eq!(
            security::file_identity(&held).unwrap(),
            security::file_identity(&expected).unwrap()
        );
        assert!(
            !admitted
                .extension()
                .is_some_and(|extension| extension.as_encoded_bytes().eq_ignore_ascii_case(b"cmd"))
        );
        let identity = crate::agents::pi_cli_installation(&admitted).unwrap();
        assert_eq!(identity.version, "1.0.2");
        assert_eq!(
            identity.manifest_digest,
            CanonicalDigest::of_bytes(&fs::read(self.package.join("package.json")).unwrap())
        );
        assert!(matches!(
            scanner.pi_settings_discovery().outcome,
            crate::agents::AgentDiscoveryOutcomeV1::Supported { .. }
        ));
        assert!(
            !self.marker.exists(),
            "neither npm shim text nor the JavaScript may run during discovery"
        );
    }
}

fn declaration() -> serde_json::Value {
    serde_json::json!({"name":"@earendil-works/pi-coding-agent", "version":"1.0.2",
        "bin":{"pi":"dist/bundle/cli.js"}})
}

fn directory(path: &Path) {
    let held = security::create_owner_only_directory(path).unwrap();
    security::verify_owner_only_handle(&held, true).unwrap();
}

fn file(path: &Path, bytes: &[u8]) {
    let mut held = security::create_owner_only_file(path).unwrap();
    held.write_all(bytes).unwrap();
    held.sync_all().unwrap();
    security::verify_owner_only_handle(&held, false).unwrap();
}

fn assert_old_locator_misses_declared_identity(fixture: &NpmFixture) {
    // This is the pre-fix production locator expression. The positive scanner
    // assertion below fails against that implementation, before any SDK runs.
    let old = crate::agents::executable::resolve(&fixture.shim)
        .unwrap()
        .unwrap();
    assert!(crate::agents::pi_cli_installation(&old).is_err());
    let old = security::open_identity(&old, false).unwrap();
    let declared = security::open_identity(&fixture.entry, false).unwrap();
    assert_ne!(
        security::file_identity(&old).unwrap(),
        security::file_identity(&declared).unwrap()
    );
}

#[test]
fn windows_pi_scanner_binds_local_npm_shim_to_declared_entry_without_evaluation() {
    let fixture = NpmFixture::new(true);
    assert_old_locator_misses_declared_identity(&fixture);
    let manifest = fixture.package.join("package.json");
    let before = fs::read(&manifest).unwrap();
    let mut ancestors = Vec::new();
    let lease = windows_pi_file(&manifest, &mut ancestors).unwrap();
    assert!(fs::write(&manifest, b"must-not-replace-declaration").is_err());
    assert_eq!(fs::read(&manifest).unwrap(), before);
    let moved = fixture.package.with_file_name("pi-owned-lease-rename");
    assert!(fs::rename(&fixture.package, &moved).is_err());
    assert!(!moved.exists());
    drop(lease);
    drop(ancestors);
    // Released inspection leases must not leave an adopted package lock.
    fs::rename(&fixture.package, &moved).unwrap();
    fs::rename(&moved, &fixture.package).unwrap();
    fixture.assert_admitted(&fixture.shim.with_extension(""));
    fixture.assert_admitted(&fixture.entry);
    let mut relative = declaration();
    relative["bin"]["pi"] = "./dist/bundle/cli.js".into();
    fixture.write_declaration(&relative);
    fixture.assert_admitted(&fixture.shim);
    fixture.assert_admitted(&fixture.entry);
}

#[test]
fn windows_pi_scanner_binds_global_npm_shim_to_declared_entry_without_evaluation() {
    let fixture = NpmFixture::new(false);
    assert_old_locator_misses_declared_identity(&fixture);
    fixture.assert_admitted(&fixture.shim);
    fixture.assert_admitted(&fixture.shim.with_extension(""));
    let mut relative = declaration();
    relative["bin"]["pi"] = "./dist/bundle/cli.js".into();
    fixture.write_declaration(&relative);
    fixture.assert_admitted(&fixture.shim);
}

#[test]
fn windows_pi_direct_entry_ignores_outer_nonregular_manifest_after_nearest_package_match() {
    let fixture = NpmFixture::new(true);
    // The existing sole package reader returns as soon as this nearest exact
    // declared Pi entry matches. It never requires an outer project's unrelated
    // package.json to be a readable regular file.
    directory(&fixture.package.parent().unwrap().join("package.json"));
    assert!(crate::agents::pi_cli_installation(&fixture.entry).is_ok());
    fixture.assert_admitted(&fixture.entry);

    // Skipping the unrelated outer directory must never admit a selected
    // declaration that cannot be held against a concurrent data writer.
    let writer = fs::OpenOptions::new()
        .write(true)
        .open(fixture.package.join("package.json"))
        .unwrap();
    assert!(crate::agents::pi_cli_installation(&fixture.entry).is_ok());
    assert!(
        fixture
            .scanner(&fixture.entry)
            .pi_executable_target()
            .is_none()
    );
    drop(writer);
    fixture.assert_admitted(&fixture.entry);
}

#[test]
fn windows_pi_scanner_rejects_unknown_producer_escaping_bin_and_identity_mismatch() {
    let fixture = NpmFixture::new(true);
    let unknown = fixture.shim.with_file_name("unknown.cmd");
    file(&unknown, b"@exit /b 99\r\n");
    assert!(fixture.scanner(&unknown).pi_executable_target().is_none());

    let outside = fixture.package.parent().unwrap().join("outside.js");
    file(
        &outside,
        b"throw new Error('outside producer must not run');\n",
    );
    for value in [
        serde_json::json!({"name":"unknown-producer","version":"1.0.2","bin":{"pi":"dist/bundle/cli.js"}}),
        serde_json::json!({"name":"@earendil-works/pi-coding-agent","version":"1.0.2","bin":{"pi":"../outside.js"}}),
        serde_json::json!({"name":"@earendil-works/pi-coding-agent","version":"1.0.2","bin":{"pi":outside}}),
        serde_json::json!({"name":"@earendil-works/pi-coding-agent","version":"1.0.2","bin":{"another":"dist/bundle/cli.js"}}),
    ] {
        fixture.write_declaration(&value);
        assert!(
            fixture
                .scanner(&fixture.shim)
                .pi_executable_target()
                .is_none()
        );
    }
    fixture.write_declaration(&declaration());
    let other = fixture.entry.with_file_name("other.js");
    file(
        &other,
        b"throw new Error('undeclared entry must not run');\n",
    );
    assert!(fixture.scanner(&other).pi_executable_target().is_none());
    fixture.assert_admitted(&fixture.shim);
}

#[test]
fn windows_pi_scanner_rejects_original_package_and_entry_junctions() {
    let fixture = NpmFixture::new(false);
    let package_target = fixture.package.with_file_name("pi-package-owned-target");
    fs::rename(&fixture.package, &package_target).unwrap();
    junction(&fixture.package, &package_target).unwrap();
    // Canonicalization hides this alias. Pi must inspect the original package
    // path before normalizing the declared entry.
    assert_eq!(
        fs::canonicalize(&fixture.entry).unwrap(),
        fs::canonicalize(package_target.join("dist/bundle/cli.js")).unwrap()
    );
    assert!(
        fixture
            .scanner(&fixture.shim)
            .pi_executable_target()
            .is_none()
    );
    fs::remove_dir(&fixture.package).unwrap();
    fs::rename(&package_target, &fixture.package).unwrap();

    let dist = fixture.package.join("dist");
    let dist_target = fixture.package.join("dist-owned-target");
    fs::rename(&dist, &dist_target).unwrap();
    junction(&dist, &dist_target).unwrap();
    assert_eq!(
        fs::canonicalize(&fixture.entry).unwrap(),
        fs::canonicalize(dist_target.join("bundle/cli.js")).unwrap()
    );
    assert!(
        fixture
            .scanner(&fixture.shim)
            .pi_executable_target()
            .is_none()
    );
    assert!(
        fixture
            .scanner(&fixture.entry)
            .pi_executable_target()
            .is_none()
    );
    fs::remove_dir(&dist).unwrap();
    fs::rename(&dist_target, &dist).unwrap();

    let prefix_alias = fixture.cleanup.path().join("npm-prefix-owned-alias");
    junction(&prefix_alias, &fixture.prefix).unwrap();
    assert!(
        fixture
            .scanner(&prefix_alias.join("pi.cmd"))
            .pi_executable_target()
            .is_none()
    );
    fs::remove_dir(prefix_alias).unwrap();

    // The selected entry is an ordinary native file, but its manifest declares
    // a second spelling through a package-local junction. The old no-inspection
    // parser hides that alias while canonicalizing the declared bin.
    let declared_alias = fixture.package.join("alias");
    junction(&declared_alias, &fixture.package.join("dist/bundle")).unwrap();
    let mut aliased = declaration();
    aliased["bin"]["pi"] = "alias/cli.js".into();
    fixture.write_declaration(&aliased);
    assert!(crate::agents::pi_cli_installation(&fixture.entry).is_ok());
    assert!(
        fixture
            .scanner(&fixture.entry)
            .pi_executable_target()
            .is_none()
    );
    assert!(
        fixture
            .scanner(&fixture.shim)
            .pi_executable_target()
            .is_none()
    );
    fixture.write_declaration(&declaration());
    fs::remove_dir(declared_alias).unwrap();
    fixture.assert_admitted(&fixture.entry);
    fixture.assert_admitted(&fixture.shim);
}

/// Test-owned mount point, using the documented FSCTL_SET_REPARSE_POINT ABI.
/// No cmd parser, Developer Mode, or symbolic-link privilege is needed.
fn junction(alias: &Path, target: &Path) -> std::io::Result<()> {
    use std::os::windows::{fs::OpenOptionsExt, io::AsRawHandle};
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn DeviceIoControl(
            handle: *mut std::ffi::c_void,
            code: u32,
            input: *const std::ffi::c_void,
            input_len: u32,
            output: *mut std::ffi::c_void,
            output_len: u32,
            returned: *mut u32,
            overlapped: *mut std::ffi::c_void,
        ) -> i32;
    }
    if !alias.is_absolute() || !target.is_absolute() {
        return Err(std::io::ErrorKind::InvalidInput.into());
    }
    directory(alias);
    let target = target
        .to_str()
        .ok_or(std::io::ErrorKind::InvalidInput)?
        .replace('/', "\\");
    let target = target.strip_prefix("\\\\?\\").unwrap_or(&target);
    let substitute = format!("\\??\\{target}").encode_utf16().collect::<Vec<_>>();
    let print = target.encode_utf16().collect::<Vec<_>>();
    let length = 16 + (substitute.len() + print.len() + 2) * 2;
    if length > 16384 {
        return Err(std::io::ErrorKind::InvalidInput.into());
    }
    let mut aligned = vec![0u32; length.div_ceil(4)];
    // SAFETY: the u32-aligned allocation owns this complete writable byte range.
    let bytes =
        unsafe { std::slice::from_raw_parts_mut(aligned.as_mut_ptr().cast::<u8>(), length) };
    bytes[0..4].copy_from_slice(&0xa0000003u32.to_le_bytes());
    bytes[4..6].copy_from_slice(&((length - 8) as u16).to_le_bytes());
    bytes[10..12].copy_from_slice(&((substitute.len() * 2) as u16).to_le_bytes());
    bytes[12..14].copy_from_slice(&(((substitute.len() + 1) * 2) as u16).to_le_bytes());
    bytes[14..16].copy_from_slice(&((print.len() * 2) as u16).to_le_bytes());
    for (index, word) in substitute
        .into_iter()
        .chain(Some(0))
        .chain(print)
        .chain(Some(0))
        .enumerate()
    {
        bytes[16 + index * 2..18 + index * 2].copy_from_slice(&word.to_le_bytes());
    }
    let file = fs::OpenOptions::new()
        .write(true)
        .custom_flags(0x02000000 | 0x00200000)
        .open(alias)?;
    let mut returned = 0;
    // SAFETY: the newly created owned directory receives a bounded, fully
    // initialized mount-point buffer; output and asynchronous operation unused.
    if unsafe {
        DeviceIoControl(
            file.as_raw_handle().cast(),
            0x000900a4,
            aligned.as_ptr().cast(),
            length as u32,
            std::ptr::null_mut(),
            0,
            &mut returned,
            std::ptr::null_mut(),
        )
    } == 0
    {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}
