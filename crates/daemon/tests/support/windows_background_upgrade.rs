//! Genuine candidate-to-candidate upgrade; both manifests bind real source SHAs.
use std::path::PathBuf;
use std::time::Duration;

use serde_json::Value;

use super::{Cleanup, Inputs, assert_status, fixture, windows_account, windows_token_launcher};

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
    let token = windows_account::current_restricted();
    let staged = windows_token_launcher::StagedFixture::create(&token);
    let account = hiroute_platform::windows::service::account_name_for_sid(
        &windows_account::token_sid(&token),
    )
    .unwrap();
    let session = windows_account::interactive_session(&account);
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
        "actual committed candidate upgrade: partial publication, preserved prepared journal, old package rollback/recovery, fixed Gateway business state, exact new entry identities and actual old daemon exit verified"
    );
}
