use super::*;
use std::cell::{Cell, RefCell};
use std::os::unix::fs::PermissionsExt;

pub(super) struct FakePlugin {
    enabled: Cell<bool>,
    polls: Cell<u32>,
    discover_after: u32,
    fail_validate: bool,
    never: bool,
    calls: RefCell<Vec<String>>,
}
impl FakePlugin {
    pub(super) fn ready() -> Self {
        Self {
            enabled: Cell::new(false),
            polls: Cell::new(0),
            discover_after: 0,
            fail_validate: false,
            never: false,
            calls: RefCell::new(Vec::new()),
        }
    }
}
impl PluginControl for FakePlugin {
    fn validate(&self, _: &Path) -> Result<(), Error> {
        self.calls.borrow_mut().push("validate".into());
        if self.fail_validate {
            Err(Error::PluginFailed)
        } else {
            Ok(())
        }
    }
    fn state(&self, _: Duration) -> Result<Option<bool>, Error> {
        let n = self.polls.get();
        self.polls.set(n + 1);
        Ok((!self.never && n >= self.discover_after).then(|| self.enabled.get()))
    }
    fn enable(&self, enabled: bool) -> Result<(), Error> {
        self.calls.borrow_mut().push(format!("enabled={enabled}"));
        self.enabled.set(enabled);
        Ok(())
    }
}
pub(super) fn fixture() -> (tempfile::TempDir, Context, Vec<Asset>) {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().canonicalize().unwrap().join("home");
    let context = Context {
        state: home.join(".local/state/hiroute/integrations"),
        home,
        release: "0.3.2-fixture.1".into(),
        omarchy_command: "/fixture/bin/omarchy".into(),
    };
    let mut assets = Vec::new();
    for name in [
        "hiroute-codex-routed.desktop",
        "hiroute-claude-code-routed.desktop",
        "hiroute-console.desktop",
    ] {
        assets.push(Asset {
            path: context.home.join(".local/share/applications").join(name),
            kind: ItemKind::DesktopEntry,
            bytes: name.as_bytes().to_vec(),
        });
    }
    assets.push(Asset {
        path: context
            .home
            .join(".local/share/icons/hicolor/scalable/apps/hiroute.svg"),
        kind: ItemKind::Icon,
        bytes: b"<svg/>".to_vec(),
    });
    for name in [
        "manifest.json",
        "Panel.qml",
        "Summary.js",
        "i18n/en.json",
        "i18n/zh.json",
    ] {
        assets.push(Asset {
            path: context.plugin_directory().join(name),
            kind: ItemKind::Plugin,
            bytes: format!("fixture:{name}").into_bytes(),
        });
    }
    (temp, context, assets)
}
pub(super) fn enable(context: &Context, assets: &[Asset], plugin: &FakePlugin) -> Manifest {
    context
        .enable(assets, plugin, None, false, Duration::from_millis(100))
        .unwrap()
}

#[test]
fn omarchy_enable_owns_exactly_the_assets_and_repeat_is_idempotent() {
    let (_temp, context, assets) = fixture();
    let plugin = FakePlugin::ready();
    let first = enable(&context, &assets, &plugin);
    assert_eq!(first.items.len(), 9);
    assert!(first.enabled && first.notifications && plugin.enabled.get());
    for asset in &assets {
        assert_eq!(fs::read(&asset.path).unwrap(), asset.bytes);
        assert_eq!(
            fs::metadata(&asset.path).unwrap().permissions().mode() & 0o777,
            0o644
        );
    }
    assert_eq!(
        fs::metadata(context.manifest_path())
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    assert_eq!(enable(&context, &assets, &plugin), first);
    assert!(!context.transaction_path().exists());
    assert!(
        context
            .status(&assets)
            .unwrap()
            .items
            .iter()
            .all(|i| i.state == ItemState::Present)
    );
}

#[test]
fn omarchy_preexisting_unowned_item_is_a_conflict_before_any_state_write() {
    let (_temp, context, assets) = fixture();
    let existing = &assets[0].path;
    fs::create_dir_all(existing.parent().unwrap()).unwrap();
    fs::write(existing, b"user-owned").unwrap();
    fs::set_permissions(existing, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(matches!(
        context.enable(
            &assets,
            &FakePlugin::ready(),
            None,
            false,
            Duration::from_millis(50)
        ),
        Err(Error::Conflict(_))
    ));
    assert_eq!(fs::read(existing).unwrap(), b"user-owned");
    assert!(!context.state.exists());
}

#[test]
fn omarchy_validation_and_discovery_failure_roll_back_every_new_item() {
    for discovery in [false, true] {
        let (_temp, context, assets) = fixture();
        let mut plugin = FakePlugin::ready();
        plugin.fail_validate = !discovery;
        plugin.never = discovery;
        assert!(
            context
                .enable(&assets, &plugin, None, false, Duration::from_millis(40))
                .is_err()
        );
        for asset in &assets {
            assert!(!asset.path.exists());
        }
        assert!(!context.manifest_path().exists());
        assert!(!context.transaction_path().exists());
        assert!(!context.plugin_directory().exists());
        assert!(!plugin.enabled.get());
    }
}

#[test]
fn omarchy_delayed_discovery_precedes_enable_and_is_verified() {
    let (_temp, context, assets) = fixture();
    let mut plugin = FakePlugin::ready();
    plugin.discover_after = 2;
    context
        .enable(&assets, &plugin, None, false, Duration::from_millis(180))
        .unwrap();
    assert!(plugin.polls.get() >= 4);
    assert_eq!(
        plugin.calls.borrow().as_slice(),
        ["validate", "enabled=true"]
    );
}

#[test]
fn omarchy_disable_keeps_user_edits_and_removes_the_rest() {
    let (_temp, context, assets) = fixture();
    let plugin = FakePlugin::ready();
    enable(&context, &assets, &plugin);
    fs::write(&assets[0].path, b"user-edit").unwrap();
    assert_eq!(
        context.status(&assets).unwrap().items[0].state,
        ItemState::Changed
    );
    let result = context.disable(&plugin).unwrap();
    assert_eq!(result.kept_changed, [assets[0].path.clone()]);
    assert_eq!(fs::read(&assets[0].path).unwrap(), b"user-edit");
    for asset in &assets[1..] {
        assert!(!asset.path.exists());
    }
    assert!(!context.load().unwrap().unwrap().enabled);
    assert!(!plugin.enabled.get());
}

#[test]
fn omarchy_symlink_drift_is_reported_and_never_followed_or_removed() {
    let (temp, context, assets) = fixture();
    let plugin = FakePlugin::ready();
    enable(&context, &assets, &plugin);
    let foreign = temp.path().join("foreign");
    fs::write(&foreign, b"foreign").unwrap();
    fs::remove_file(&assets[0].path).unwrap();
    std::os::unix::fs::symlink(&foreign, &assets[0].path).unwrap();
    assert_eq!(
        context.status(&assets).unwrap().items[0].state,
        ItemState::Changed
    );
    assert_eq!(
        context.disable(&plugin).unwrap().kept_changed,
        [assets[0].path.clone()]
    );
    assert_eq!(fs::read(foreign).unwrap(), b"foreign");
    assert!(assets[0].path.is_symlink());
}

#[test]
fn omarchy_refresh_updates_unchanged_assets_and_preserves_edits_and_bar_placement() {
    let (_temp, mut context, mut assets) = fixture();
    let plugin = FakePlugin::ready();
    enable(&context, &assets, &plugin);
    fs::write(&assets[0].path, b"user-edit").unwrap();
    context.release = "0.3.2-fixture.2".into();
    assets[1].bytes = b"new-release".to_vec();
    assert_eq!(
        context.status(&assets).unwrap().items[1].state,
        ItemState::Outdated
    );
    context
        .enable(&assets, &plugin, None, true, Duration::from_millis(100))
        .unwrap();
    assert_eq!(fs::read(&assets[0].path).unwrap(), b"user-edit");
    assert_eq!(fs::read(&assets[1].path).unwrap(), b"new-release");
    assert_eq!(
        plugin
            .calls
            .borrow()
            .iter()
            .filter(|c| *c == "enabled=true")
            .count(),
        1
    );
}

#[test]
fn omarchy_recovery_uses_the_staged_identity_and_keeps_a_later_user_edit() {
    for changed in [false, true] {
        let (_temp, context, assets) = fixture();
        files::directory(&context.state).unwrap();
        let asset = &assets[0];
        let (stage, device, inode) = files::stage(&asset.path, &asset.bytes).unwrap();
        let item = OwnedItem {
            path: asset.path.clone(),
            sha256: files::hash(&asset.bytes),
            release: context.release.clone(),
            kind: asset.kind,
        };
        let after = Manifest {
            schema: MANIFEST_SCHEMA.into(),
            generation: files::nonce().unwrap(),
            enabled: true,
            notifications: true,
            first_run_opened: false,
            omarchy_command: context.omarchy_command.clone(),
            items: vec![item.clone()],
        };
        let t = Transaction {
            schema: TRANSACTION_SCHEMA.into(),
            before: None,
            after,
            changes: vec![Change {
                after: item,
                before: None,
                before_bytes: None,
                stage,
                device,
                inode,
            }],
            plugin_was_enabled: false,
            enable_attempted: false,
        };
        context.save_transaction(&t).unwrap();
        files::apply(&t.changes[0]).unwrap();
        if changed {
            fs::write(&asset.path, b"later-user-edit").unwrap();
            assert!(context.recover(&FakePlugin::ready()).is_err());
            assert_eq!(fs::read(&asset.path).unwrap(), b"later-user-edit");
            assert!(context.transaction_path().exists());
        } else {
            context.recover(&FakePlugin::ready()).unwrap();
            assert!(!asset.path.exists());
            assert!(!context.transaction_path().exists());
        }
    }
}

#[test]
fn omarchy_public_plugin_commands_preserve_argv_and_accept_public_list_extensions() {
    let temp = tempfile::tempdir().unwrap();
    let program = temp.path().join("omarchy fake");
    fs::write(
        &program,
        r#"#!/usr/bin/env python3
import json, pathlib, sys
p=pathlib.Path(sys.argv[0]);args=sys.argv[1:];state=p.with_suffix('.state')
with p.with_suffix('.calls').open('a') as f: f.write(json.dumps(args)+'\n')
if args==['plugin','list','--json']:
    enabled=state.exists() and state.read_text()=='on'
    rows=[{'id':'hiroute.status','enabled':enabled,'section':'right','future_field':True}]
    if state.exists() and state.read_text()=='duplicate': rows=rows+rows
    print(json.dumps(rows))
elif args[:2]==['plugin','validate']: pass
elif args==['plugin','enable','hiroute.status','--section','right']: state.write_text('on')
elif args==['plugin','disable','hiroute.status']: state.write_text('off')
else: sys.exit(2)
"#,
    )
    .unwrap();
    fs::set_permissions(&program, fs::Permissions::from_mode(0o700)).unwrap();
    let control = PublicPluginControl {
        executable: program.clone(),
    };
    let folder = temp.path().join("literal ; $(not-a-command)");
    control.validate(&folder).unwrap();
    assert_eq!(control.state(Duration::from_secs(2)).unwrap(), Some(false));
    control.enable(true).unwrap();
    assert_eq!(control.state(Duration::from_secs(2)).unwrap(), Some(true));
    control.enable(false).unwrap();
    let calls: Vec<Vec<String>> = fs::read_to_string(program.with_extension("calls"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(calls[0], ["plugin", "validate", folder.to_str().unwrap()]);
    assert_eq!(
        calls[2],
        ["plugin", "enable", "hiroute.status", "--section", "right"]
    );
    fs::write(program.with_extension("state"), "duplicate").unwrap();
    assert!(matches!(
        control.state(Duration::from_secs(2)),
        Err(Error::PluginFailed)
    ));
}
