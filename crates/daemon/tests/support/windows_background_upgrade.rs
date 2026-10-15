//! Genuine candidate-to-candidate upgrade; both manifests bind real source SHAs.
use std::path::PathBuf;
use std::time::Duration;

use serde_json::Value;

#[path = "windows_background_upgrade_transport.rs"]
pub(super) mod transport;

use super::windows_background_upgrade_business::Business;
use super::{
    Cleanup, Inputs, assert_status, fixture, windows_account, windows_background_task,
    windows_token_launcher,
};
use transport::{Cleanup as TaskCleanup, Owner};

fn desired_port(gateway: &Value) -> u16 {
    gateway["connect_address"]
        .as_str()
        .unwrap()
        .parse::<std::net::SocketAddr>()
        .unwrap()
        .port()
}

#[test]
fn task_native_candidate_upgrade_recovers_prepared_rollback_and_preserves_business_state() {
    let _privileges = windows_account::Privileges::enable();
    let windows_background_task::Owner { token, session } = windows_background_task::owner();
    let staged = windows_token_launcher::StagedFixture::create(&token);
    task_upgrade(Owner::Token(&token), session);
    drop(staged);
}

/// Same-OS standard-owner evidence is distinct from the original full/Win11
/// baseline gate. Its wrapper supplies independently admitted genuine packages.
#[test]
fn task_real_standard_logged_on_owner_upgrade_recovers_rollback_and_preserves_business_state() {
    let (_, session) = super::windows_background_standard::real_standard_session();
    task_upgrade(Owner::Current, session);
}

fn standard_upgrade_admission(candidate: &Inputs, observed: &Value) -> String {
    use super::windows_background_standard::{require_absent, task_absent};
    use hiroute_host_runtime::StandaloneLayout;
    use hiroute_platform::windows::background;

    let layout = StandaloneLayout::from_environment().unwrap();
    assert!(
        observed["home"].as_str().map(std::path::Path::new) == Some(layout.home.as_path())
            && observed["state"].as_str().map(std::path::Path::new)
                == Some(layout.state_root.as_path())
            && observed["sid"] == background::current_sid_string().unwrap(),
        "standard upgrade reexecution must retain actual KnownFolders and current SID"
    );
    for (path, stage) in [
        (layout.marker_path.clone(), "upgrade-initial-marker"),
        (
            layout.data_root.join("service/windows-background.json"),
            "upgrade-initial-definition",
        ),
        (
            layout.data_root.join("service/install-journal.json"),
            "upgrade-initial-journal",
        ),
        (
            layout.home.join(".local/bin/hiroute.exe"),
            "upgrade-initial-cli",
        ),
        (
            layout.home.join(".local/bin/hirouted.exe"),
            "upgrade-initial-daemon",
        ),
    ] {
        require_absent(&path, stage);
    }
    super::windows_background_upgrade_business::admit_original_config(&layout.home);
    let definition = background::Definition::new(
        background::Manager::Task,
        candidate.admin_cli.with_file_name("hirouted.exe"),
    )
    .unwrap();
    task_absent(definition.name.clone(), "upgrade-initial-task");
    definition.name
}

fn task_upgrade(owner: Owner<'_>, session: u32) {
    let candidate = Inputs::load();
    let baseline = Inputs::baseline();
    let baseline_manifest: Value =
        serde_json::from_slice(&std::fs::read(&baseline.manifest).unwrap()).unwrap();
    let candidate_manifest: Value =
        serde_json::from_slice(&std::fs::read(&candidate.manifest).unwrap()).unwrap();
    assert_eq!(baseline_manifest["target"], candidate_manifest["target"]);
    assert_ne!(
        baseline_manifest["revision"], candidate_manifest["revision"],
        "upgrade gate requires two actual committed source revisions"
    );
    assert_ne!(
        baseline.version_directory, candidate.version_directory,
        "candidate roots must differ; same package repair cannot prove version replacement"
    );
    let layout = owner.action("layout", Duration::from_secs(20));
    let standard_task =
        matches!(owner, Owner::Current).then(|| standard_upgrade_admission(&candidate, &layout));
    let old_cli = baseline.owner_cli(&layout);
    assert_eq!(owner.install(&baseline)["installed"], true);
    let mut cleanup = TaskCleanup::new(owner, &baseline, old_cli.clone());
    owner.cli(&old_cli, &["service", "start"]);
    let first = owner.action("inspect", Duration::from_secs(60));
    assert_eq!(first["session"], session);
    let port = desired_port(&first["gateway"]);
    // Public production configuration persists a deliberately fixed listener
    // and actually restarts the daemon. Neither raw SQLite nor a test file is
    // used as a stand-in for user-visible business state.
    let configured = owner.cli(
        &old_cli,
        &[
            "gateway",
            "set",
            "--address",
            "127.0.0.1",
            "--port",
            &port.to_string(),
        ],
    );
    assert_eq!(configured["ready"], true);
    assert_eq!(configured["config"]["desired"]["port"], port);
    let old = owner.action("transaction-verify", Duration::from_secs(45));
    let mut business = Business::save_with(owner, &old_cli);
    let failed = owner.transaction_install(&candidate, "transaction-fail-install");
    assert_ne!(failed["command"]["code"], 0);
    assert_eq!(failed["after"]["journal"]["old_marker"], old["marker"]);
    assert_ne!(
        failed["after"]["journal"]["new_marker"]["install_root"],
        old["marker"]["install_root"]
    );
    assert_ne!(
        failed["after"]["cli_identity"], old["cli_identity"],
        "fault must follow replacement with the real candidate entry object"
    );
    let blocked = owner.command(
        &old_cli,
        vec![
            "service".into(),
            "start".into(),
            "--output".into(),
            "json".into(),
        ],
    );
    assert_ne!(
        blocked["code"], 0,
        "prepared upgrade may not admit an old or new business runtime"
    );
    // Retry with the independently verified original package. Production recover
    // restores the old registration/files before this original-package repair.
    assert_eq!(owner.install(&baseline)["installed"], true);
    let restored = owner.action("transaction-verify", Duration::from_secs(45));
    assert_eq!(restored["marker"], old["marker"]);
    assert_eq!(restored["definition"], old["definition"]);
    assert_eq!(restored["manifest"], old["manifest"]);
    assert_eq!(restored["cli_identity"], old["cli_identity"]);
    assert_eq!(restored["daemon_identity"], old["daemon_identity"]);
    owner.cli(&old_cli, &["service", "start"]);
    let recovered_old = owner.action("inspect", Duration::from_secs(60));
    assert_eq!(desired_port(&recovered_old["gateway"]), port);
    assert_eq!(
        recovered_old["gateway"]["config"]["desired"],
        configured["config"]["desired"]
    );
    business.recovered_with(owner, &old_cli, true);
    // The next attempt is a real committed candidate replacement while old
    // business is running; the fixture retains and waits on the actual old
    // process before accepting installation completion.
    let installed = owner.transaction_install(&candidate, "transaction-upgrade-install");
    assert_eq!(installed["command"]["code"], 0);
    let new_cli = candidate.owner_cli(&layout);
    cleanup.upgraded(&candidate, new_cli.clone());
    let upgraded = owner.action("transaction-verify", Duration::from_secs(45));
    assert_ne!(
        upgraded["marker"]["install_root"],
        old["marker"]["install_root"]
    );
    assert_eq!(upgraded["manifest"], candidate_manifest);
    assert_ne!(upgraded["cli_identity"], old["cli_identity"]);
    assert_ne!(upgraded["daemon_identity"], old["daemon_identity"]);
    let status = owner.cli(&new_cli, &["service", "status"]);
    assert_eq!(status["schema"], "hiroute.standalone-service-status/v1");
    assert_eq!(status["manager"], "windows-task-user");
    assert_eq!(status["manager_active"], false);
    assert_eq!(status["local_control_ready"], false);
    owner.cli(&new_cli, &["service", "start"]);
    let current = owner.action("inspect", Duration::from_secs(60));
    assert_eq!(current["session"], session);
    assert_eq!(desired_port(&current["gateway"]), port);
    assert_eq!(
        current["gateway"]["config"]["desired"],
        configured["config"]["desired"]
    );
    assert!(
        current["pid"] != recovered_old["pid"]
            || current["creation_time"] != recovered_old["creation_time"]
    );
    business.recovered_with(owner, &new_cli, false);
    business.restore_with(owner, &new_cli);
    assert_eq!(owner.uninstall(&candidate, &new_cli)["uninstalled"], true);
    cleanup.disarm();
    assert!(!PathBuf::from(layout["marker"].as_str().unwrap()).exists());
    if let Some(name) = standard_task {
        use super::windows_background_standard::{require_absent, task_absent};
        let layout = hiroute_host_runtime::StandaloneLayout::from_environment().unwrap();
        for (path, stage) in [
            (layout.marker_path, "upgrade-uninstalled-marker"),
            (
                layout.data_root.join("service/windows-background.json"),
                "upgrade-uninstalled-definition",
            ),
            (
                layout.data_root.join("service/install-journal.json"),
                "upgrade-uninstalled-journal",
            ),
            (
                layout.home.join(".local/bin/hiroute.exe"),
                "upgrade-uninstalled-cli",
            ),
            (
                layout.home.join(".local/bin/hirouted.exe"),
                "upgrade-uninstalled-daemon",
            ),
        ] {
            require_absent(&path, stage);
        }
        task_absent(name, "upgrade-uninstalled-task");
    }
    let disposition = owner.request(
        fixture::Request {
            action: "cleanup-state".into(),
            program: None,
            arguments: vec![],
        },
        Duration::from_secs(20),
    );
    assert_eq!(disposition["removed"], false);
    assert_eq!(disposition["preserved"], true);
    println!(
        "actual committed candidate upgrade: prepared fault/old package rollback, exact saved Source/Key/binding/route/operation/Agent restore identities, authenticated Gateway and retained public usage receipts, untouched external Agent restoration, new entry FileIDs and actual old daemon exit verified"
    );
}

/// The independently verified baseline administrator package must be available;
/// the candidate CLI cannot establish trust for different baseline host bytes.
#[test]
#[ignore = "optional SCM upgrade; current-user logged-on Task is the required background contract"]
fn scm_native_two_phase_candidate_upgrade_preserves_saved_route_secret_and_receipts() {
    let _privileges = windows_account::Privileges::enable();
    let candidate = Inputs::load();
    let mut baseline = Inputs::baseline();
    let baseline_manifest: Value =
        serde_json::from_slice(&std::fs::read(&baseline.manifest).unwrap()).unwrap();
    let candidate_manifest: Value =
        serde_json::from_slice(&std::fs::read(&candidate.manifest).unwrap()).unwrap();
    assert_eq!(baseline_manifest["target"], candidate_manifest["target"]);
    assert_ne!(
        baseline_manifest["revision"],
        candidate_manifest["revision"]
    );
    assert_ne!(baseline.version_directory, candidate.version_directory);
    baseline.admin_cli = fixture::required("HIROUTE_WINDOWS_BASELINE_ADMIN_CLI");
    assert_eq!(baseline.admin_cli.file_name().unwrap(), "hiroute.exe");
    for (path, name) in [
        (baseline.admin_cli.clone(), "bin/hiroute.exe"),
        (
            baseline.admin_cli.with_file_name("hirouted.exe"),
            "bin/hirouted.exe",
        ),
        (
            baseline
                .admin_cli
                .parent()
                .unwrap()
                .parent()
                .unwrap()
                .join("libexec/cliproxyapi.exe"),
            "libexec/cliproxyapi.exe",
        ),
    ] {
        super::windows_execution_host::image(&path);
        use sha2::{Digest, Sha256};
        use std::io::Read;
        let mut image = hiroute_platform::windows::security::open_identity(&path, false).unwrap();
        let mut hash = Sha256::new();
        let mut buffer = [0; 64 * 1024];
        loop {
            let count = image.read(&mut buffer).unwrap();
            if count == 0 {
                break;
            }
            hash.update(&buffer[..count]);
        }
        assert_eq!(
            format!("{:x}", hash.finalize()),
            baseline_manifest["files"][name]["sha256"],
            "baseline administrator package differs from the actual verified baseline manifest"
        );
    }
    let account = windows_account::Account::create();
    let token = account.token.as_ref().unwrap();
    let staged = windows_token_launcher::StagedFixture::create(token);
    let layout = candidate.owner_layout(token);
    let old_cli = baseline.owner_cli(&layout);
    assert_eq!(layout["sid"], account.sid);
    let pending = baseline.install(token, true);
    let mut cleanup = Cleanup {
        inputs: &baseline,
        token,
        cli: old_cli.clone(),
        account: Some(&account),
        armed: true,
    };
    assert_eq!(pending["pending_admin"], true);
    assert_eq!(pending["completed"], false);
    baseline.admin(&account, "install", None);
    assert_eq!(baseline.finalize(token, &old_cli)["completed"], true);
    fixture::cli(token, &old_cli, &["service", "start"]);
    let first = fixture::inspect(token, false);
    assert_eq!(first["sid"], account.sid);
    assert_eq!(first["session"], 0);
    let port = desired_port(&first["gateway"]);
    let configured = fixture::cli(
        token,
        &old_cli,
        &[
            "gateway",
            "set",
            "--address",
            "127.0.0.1",
            "--port",
            &port.to_string(),
        ],
    );
    assert_eq!(configured["ready"], true);
    let old = fixture::transaction_snapshot(token, true);
    let mut business = Business::save(token, &old_cli);
    // Preparation invokes the production exact-owner stop, then publishes the
    // genuine candidate. No new business runtime is admitted before admin proof.
    let prepared = candidate.install(token, true);
    cleanup.inputs = &candidate;
    let new_cli = candidate.owner_cli(&layout);
    cleanup.cli = new_cli.clone();
    assert_eq!(prepared["pending_admin"], true);
    assert_eq!(prepared["completed"], false);
    let pending_snapshot = fixture::transaction_snapshot(token, false);
    assert_eq!(pending_snapshot["journal"]["old_marker"], old["marker"]);
    assert_eq!(
        pending_snapshot["journal"]["old_definition"],
        old["definition"]
    );
    assert_ne!(
        pending_snapshot["marker"]["install_root"],
        old["marker"]["install_root"]
    );
    assert_ne!(pending_snapshot["cli_identity"], old["cli_identity"]);
    assert_ne!(pending_snapshot["daemon_identity"], old["daemon_identity"]);
    let denied = fixture::command(
        token,
        &new_cli,
        vec![
            "service".into(),
            "start".into(),
            "--output".into(),
            "json".into(),
        ],
    );
    assert_ne!(
        denied["code"], 0,
        "prepared SCM upgrade admitted a business runtime"
    );
    assert_eq!(
        candidate.install(token, true),
        prepared,
        "SCM prepared upgrade was not idempotent"
    );
    assert_eq!(
        fixture::transaction_snapshot(token, false)["journal"],
        pending_snapshot["journal"]
    );
    candidate.admin(&account, "install", None);
    assert_eq!(
        candidate.install(token, true),
        prepared,
        "registered upgrade skipped owner finalization"
    );
    assert_eq!(
        fixture::transaction_snapshot(token, false)["journal"],
        pending_snapshot["journal"]
    );
    let denied = fixture::command(
        token,
        &new_cli,
        vec![
            "service".into(),
            "start".into(),
            "--output".into(),
            "json".into(),
        ],
    );
    assert_ne!(
        denied["code"], 0,
        "SCM machine registration alone admitted upgraded business"
    );
    let finished = candidate.finalize(token, &new_cli);
    assert_eq!(finished["completed"], true);
    assert_eq!(finished["operation"], "install");
    assert_eq!(finished["started"], false);
    let upgraded = fixture::transaction_snapshot(token, true);
    assert_eq!(upgraded["manifest"], candidate_manifest);
    assert_eq!(upgraded["cli_identity"], pending_snapshot["cli_identity"]);
    assert_eq!(
        upgraded["daemon_identity"],
        pending_snapshot["daemon_identity"]
    );
    assert_status(token, &new_cli, "windows-scm-user", false);
    fixture::cli(token, &new_cli, &["service", "start"]);
    let current = fixture::inspect(token, false);
    assert_eq!(current["sid"], account.sid);
    assert_eq!(current["session"], 0);
    assert!(current["pid"] != first["pid"] || current["creation_time"] != first["creation_time"]);
    assert_eq!(desired_port(&current["gateway"]), port);
    assert_eq!(
        current["gateway"]["config"]["desired"],
        configured["config"]["desired"]
    );
    business.recovered(token, &new_cli, false);
    business.restore(token, &new_cli);
    fixture::cli(token, &new_cli, &["service", "stop"]);
    assert_eq!(
        candidate.uninstall(token, &new_cli, true)["pending_admin"],
        true
    );
    candidate.admin(&account, "uninstall", None);
    assert_eq!(candidate.finalize(token, &new_cli)["completed"], true);
    cleanup.armed = false;
    super::assert_native_service_absent(upgraded["definition"]["name"].as_str().unwrap());
    assert!(!PathBuf::from(layout["marker"].as_str().unwrap()).exists());
    drop(cleanup);
    drop(staged);
    drop(account);
    println!(
        "actual SCM two-package upgrade: authenticated baseline/candidate administrator packages, prepared/registered reentry, owner finalization, session-zero new daemon, retained Source/Secret/fixed route/Agent restoration and original public request facts verified"
    );
}
