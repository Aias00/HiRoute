//! Genuine candidate-to-candidate upgrade; both manifests bind real source SHAs.
use std::path::PathBuf;
use std::time::Duration;

use serde_json::Value;

use super::windows_background_upgrade_business::Business;
use super::{
    Cleanup, Inputs, assert_status, fixture, windows_account, windows_background_task,
    windows_token_launcher,
};

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
    let windows_background_task::Owner { token, session } = windows_background_task::owner();
    let staged = windows_token_launcher::StagedFixture::create(&token);
    let layout = candidate.owner_layout(&token);
    let old_cli = baseline.owner_cli(&layout);
    assert_eq!(baseline.install(&token, false)["installed"], true);
    let mut cleanup = Cleanup {
        inputs: &baseline,
        token: &token,
        cli: old_cli.clone(),
        account: None,
        armed: true,
    };
    fixture::cli(&token, &old_cli, &["service", "start"]);
    let first = fixture::inspect(&token, false);
    assert_eq!(first["session"], session);
    let port = desired_port(&first["gateway"]);
    // Public production configuration persists a deliberately fixed listener
    // and actually restarts the daemon. Neither raw SQLite nor a test file is
    // used as a stand-in for user-visible business state.
    let configured = fixture::cli(
        &token,
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
    let old = fixture::transaction_snapshot(&token, true);
    let mut business = Business::save(&token, &old_cli);
    let failed = candidate.transaction_install(&token, "transaction-fail-install");
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
    let blocked = fixture::command(
        &token,
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
    assert_eq!(baseline.install(&token, false)["installed"], true);
    let restored = fixture::transaction_snapshot(&token, true);
    assert_eq!(restored["marker"], old["marker"]);
    assert_eq!(restored["definition"], old["definition"]);
    assert_eq!(restored["manifest"], old["manifest"]);
    assert_eq!(restored["cli_identity"], old["cli_identity"]);
    assert_eq!(restored["daemon_identity"], old["daemon_identity"]);
    fixture::cli(&token, &old_cli, &["service", "start"]);
    let recovered_old = fixture::inspect(&token, false);
    assert_eq!(desired_port(&recovered_old["gateway"]), port);
    assert_eq!(
        recovered_old["gateway"]["config"]["desired"],
        configured["config"]["desired"]
    );
    business.recovered(&token, &old_cli, true);
    // The next attempt is a real committed candidate replacement while old
    // business is running; the fixture retains and waits on the actual old
    // process before accepting installation completion.
    let installed = candidate.transaction_install(&token, "transaction-upgrade-install");
    assert_eq!(installed["command"]["code"], 0);
    let new_cli = candidate.owner_cli(&layout);
    cleanup.inputs = &candidate;
    cleanup.cli = new_cli.clone();
    let upgraded = fixture::transaction_snapshot(&token, true);
    assert_ne!(
        upgraded["marker"]["install_root"],
        old["marker"]["install_root"]
    );
    assert_eq!(upgraded["manifest"], candidate_manifest);
    assert_ne!(upgraded["cli_identity"], old["cli_identity"]);
    assert_ne!(upgraded["daemon_identity"], old["daemon_identity"]);
    assert_status(&token, &new_cli, "windows-task-user", false);
    fixture::cli(&token, &new_cli, &["service", "start"]);
    let current = fixture::inspect(&token, false);
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
    business.recovered(&token, &new_cli, false);
    business.restore(&token, &new_cli);
    assert_eq!(
        candidate.uninstall(&token, &new_cli, false)["uninstalled"],
        true
    );
    cleanup.armed = false;
    assert!(!PathBuf::from(layout["marker"].as_str().unwrap()).exists());
    fixture::owner(
        &token,
        fixture::Request {
            action: "cleanup-state".into(),
            program: None,
            arguments: vec![],
        },
        Duration::from_secs(20),
    );
    drop(staged);
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
