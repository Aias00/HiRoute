#![cfg(windows)]
//! Required native product gates: actual Task Scheduler and standard-owner SCM.
//! Run serially in a disposable CI profile with a verified candidate package.
//! A selected gate fails when prerequisites are missing; fixtures never stand in
//! for actual service launch, Gateway readiness, or Local Control.

#[path = "support/windows_account.rs"]
mod windows_account;
#[path = "support/windows_background_fixture.rs"]
mod windows_background_fixture;
#[path = "support/windows_background_task.rs"]
mod windows_background_task;
#[path = "support/windows_background_upgrade.rs"]
mod windows_background_upgrade;
#[path = "support/windows_candidate_source.rs"]
mod windows_candidate_source;
#[path = "support/windows_execution_host.rs"]
mod windows_execution_host;
#[path = "support/windows_subscription_login.rs"]
mod windows_subscription_login;
#[path = "support/windows_token_launcher.rs"]
mod windows_token_launcher;

use std::ffi::OsString;
use std::fs::File;
use std::path::PathBuf;
use std::thread;
use std::time::{Duration, Instant};

use hiroute_platform::windows::{background, service};
use serde_json::Value;
use windows_background_fixture as fixture;

struct Inputs {
    python: PathBuf,
    installer: PathBuf,
    manifest: PathBuf,
    archive: PathBuf,
    admin_cli: PathBuf,
    version_directory: String,
    _source: windows_candidate_source::Sources,
}
impl Inputs {
    fn load() -> Self {
        Self::from_package(
            "HIROUTE_WINDOWS_PACKAGE_MANIFEST",
            "HIROUTE_WINDOWS_PACKAGE_ARCHIVE",
        )
    }
    fn baseline() -> Self {
        Self::from_package(
            "HIROUTE_WINDOWS_BASELINE_PACKAGE_MANIFEST",
            "HIROUTE_WINDOWS_BASELINE_PACKAGE_ARCHIVE",
        )
    }
    fn from_package(manifest_name: &str, archive_name: &str) -> Self {
        let manifest = fixture::required(manifest_name);
        let archive = fixture::required(archive_name);
        let document: Value = serde_json::from_slice(&std::fs::read(&manifest).unwrap()).unwrap();
        assert_eq!(document["distribution"], "integration-candidate");
        assert_eq!(document["target"], "x86_64-pc-windows-msvc");
        let version = document["version"].as_str().unwrap();
        let revision = document["revision"].as_str().unwrap();
        assert!(revision.len() >= 12);
        windows_execution_host::current();
        let source = windows_candidate_source::Sources::open();
        if manifest_name == "HIROUTE_WINDOWS_PACKAGE_MANIFEST"
            && let Some(expected) = windows_candidate_source::Sources::revision()
        {
            assert_eq!(
                revision, expected,
                "package and compiled gate candidate SHA differ"
            );
        }
        let admin_cli = fixture::prepared_cli();
        windows_execution_host::image(&admin_cli);
        windows_execution_host::image(&admin_cli.with_file_name("hirouted.exe"));
        windows_execution_host::image(
            &admin_cli
                .parent()
                .unwrap()
                .parent()
                .unwrap()
                .join("libexec/cliproxyapi.exe"),
        );
        Self {
            python: fixture::required("HIROUTE_WINDOWS_PYTHON"),
            installer: source.installer(),
            manifest,
            archive,
            admin_cli,
            version_directory: format!("{version}-{}", &revision[..12]),
            _source: source,
        }
    }
    fn python(&self, token: &File, arguments: Vec<String>) -> Value {
        fixture::command(
            token,
            &self.python,
            std::iter::once(self.installer.to_string_lossy().into_owned())
                .chain(arguments)
                .collect(),
        )
    }
    fn install_arguments(&self, scm: bool) -> Vec<String> {
        let mut args = vec![
            "install".into(),
            "--manifest".into(),
            self.manifest.to_string_lossy().into_owned(),
            "--archive".into(),
            self.archive.to_string_lossy().into_owned(),
        ];
        if scm {
            args.extend([
                "--manager".into(),
                "scm".into(),
                "--external-manager".into(),
            ]);
        }
        args
    }
    fn install(&self, token: &File, scm: bool) -> Value {
        fixture::accepted(&self.python(token, self.install_arguments(scm))).clone()
    }
    fn transaction_install(&self, token: &File, action: &str) -> Value {
        fixture::owner(
            token,
            fixture::Request {
                action: action.into(),
                program: Some(self.python.clone()),
                arguments: std::iter::once(self.installer.to_string_lossy().into_owned())
                    .chain(self.install_arguments(false))
                    .collect(),
            },
            Duration::from_secs(90),
        )
    }
    fn finalize(&self, token: &File, cli: &std::path::Path) -> Value {
        fixture::accepted(&self.python(
            token,
            vec![
                "finalize".into(),
                "--cli".into(),
                cli.to_string_lossy().into_owned(),
            ],
        ))
        .clone()
    }
    fn uninstall(&self, token: &File, cli: &std::path::Path, scm: bool) -> Value {
        let mut args = vec![
            "uninstall".into(),
            "--cli".into(),
            cli.to_string_lossy().into_owned(),
        ];
        if scm {
            args.extend([
                "--manager".into(),
                "scm".into(),
                "--external-manager".into(),
            ]);
        }
        fixture::accepted(&self.python(token, args)).clone()
    }
    fn admin_attempt(
        &self,
        account: &windows_account::Account,
        operation: &str,
        enabled: Option<bool>,
    ) -> Value {
        let mut args = vec![
            self.installer.as_os_str().to_owned(),
            "admin-scm".into(),
            "--cli".into(),
            self.admin_cli.as_os_str().to_owned(),
            "--account".into(),
            OsString::from(&account.account),
            "--operation".into(),
            operation.into(),
            "--password-stdin".into(),
        ];
        if let Some(enabled) = enabled {
            args.extend([
                "--enabled".into(),
                if enabled { "on" } else { "off" }.into(),
            ]);
        }
        // Only this anonymous stdin carries synthetic credentials. Neither owner
        // subprocess environment nor argv nor package/journal contains them.
        let result = fixture::process(
            None,
            &self.python,
            &args,
            Some(account.password.as_bytes()),
            true,
            Duration::from_secs(75),
            false,
        );
        result
    }
    fn admin(
        &self,
        account: &windows_account::Account,
        operation: &str,
        enabled: Option<bool>,
    ) -> Value {
        fixture::accepted(&self.admin_attempt(account, operation, enabled)).clone()
    }
    fn owner_layout(&self, token: &File) -> Value {
        fixture::owner(
            token,
            fixture::Request {
                action: "layout".into(),
                program: None,
                arguments: vec![],
            },
            Duration::from_secs(20),
        )
    }
    fn owner_cli(&self, layout: &Value) -> PathBuf {
        PathBuf::from(layout["home"].as_str().unwrap())
            .join(".local/lib/hiroute")
            .join(&self.version_directory)
            .join("hiroute.exe")
    }
}

struct Cleanup<'a> {
    inputs: &'a Inputs,
    token: &'a File,
    cli: PathBuf,
    account: Option<&'a windows_account::Account>,
    armed: bool,
}
impl Drop for Cleanup<'_> {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        // Failed required gates still attempt exact owner-scoped maintenance.
        // Never use taskkill, guessed PIDs, guessed service names, or recursive
        // deletion of the runner's existing profile as a cleanup fallback.
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = fixture::cli(self.token, &self.cli, &["service", "stop"]);
            let pending = self
                .inputs
                .uninstall(self.token, &self.cli, self.account.is_some());
            if let Some(account) = self.account {
                assert_eq!(pending["pending_admin"], true);
                self.inputs.admin(account, "uninstall", None);
                self.inputs.finalize(self.token, &self.cli);
            }
        }));
        if result.is_err() {
            eprintln!(
                "native background gate exact-owner cleanup failed; preserve failed evidence"
            );
        }
    }
}

fn assert_status(token: &File, cli: &std::path::Path, manager: &str, active: bool) -> Value {
    let status = fixture::cli(token, cli, &["service", "status"]);
    assert_eq!(status["schema"], "hiroute.standalone-service-status/v1");
    assert_eq!(status["manager"], manager);
    assert_eq!(status["manager_active"], active);
    assert_eq!(status["local_control_ready"], active);
    status
}

/// Keep real CPA authorization URLs and their opaque state in memory. The
/// ordinary fixture::accepted diagnostic prints envelopes, so OAuth operations
/// use fixed diagnostics and return only the actual parsed public envelope.
fn subscription_cli(
    token: &File,
    cli: &std::path::Path,
    arguments: &[&str],
    request: Option<&Value>,
) -> Value {
    let arguments = arguments
        .iter()
        .map(OsString::from)
        .chain([OsString::from("--timeout"), OsString::from("45s")])
        .collect::<Vec<_>>();
    let input = request.map(|value| serde_json::to_vec(value).unwrap());
    let result = fixture::process(
        Some(token),
        cli,
        &arguments,
        input.as_deref(),
        false,
        Duration::from_secs(60),
        false,
    );
    assert!(result["code"] == 0, "public subscription command failed");
    assert!(
        result["result"].is_object(),
        "public subscription result is missing"
    );
    result["result"].clone()
}

fn assert_native_service_absent(name: &str) {
    use std::ptr::null;
    use windows_sys::Win32::Foundation::{ERROR_SERVICE_DOES_NOT_EXIST, GetLastError};
    use windows_sys::Win32::System::Services::{
        CloseServiceHandle, OpenSCManagerW, OpenServiceW, SC_MANAGER_CONNECT, SERVICE_QUERY_STATUS,
    };
    let manager = unsafe { OpenSCManagerW(null(), null(), SC_MANAGER_CONNECT) };
    assert!(!manager.is_null(), "native SCM observation failed");
    let service = unsafe {
        OpenServiceW(
            manager,
            windows_account::wide(name).as_ptr(),
            SERVICE_QUERY_STATUS,
        )
    };
    let error = unsafe { GetLastError() };
    if !service.is_null() {
        unsafe {
            CloseServiceHandle(service);
        }
    }
    unsafe {
        CloseServiceHandle(manager);
    }
    assert!(
        service.is_null() && error == ERROR_SERVICE_DOES_NOT_EXIST,
        "candidate must have no native SCM registration before successful authenticated install"
    );
}

fn await_actual_restart(
    token: &File,
    cli: &std::path::Path,
    manager: &str,
    previous: &Value,
) -> Value {
    // Both managers currently specify a real 60 second recovery delay. Waiting
    // here observes real production policy instead of editing a test task/service.
    let deadline = Instant::now() + Duration::from_secs(115);
    loop {
        let result = fixture::command(
            token,
            cli,
            vec![
                "service".into(),
                "status".into(),
                "--output".into(),
                "json".into(),
            ],
        );
        if result["code"] == 0
            && result["result"]["data"]["manager_active"] == true
            && result["result"]["data"]["local_control_ready"] == true
        {
            let recovered = fixture::inspect(token, false);
            assert!(
                recovered["pid"] != previous["pid"]
                    || recovered["creation_time"] != previous["creation_time"],
                "unexpected failure must create a new actual daemon process"
            );
            assert_status(token, cli, manager, true);
            return recovered;
        }
        assert!(
            Instant::now() < deadline,
            "required background recovery did not start a new healthy business runtime"
        );
        thread::sleep(Duration::from_secs(2));
    }
}

#[test]
fn windows_background_owner_fixture() {
    fixture::entry();
}

/// Requires a real logged-on runner account, then uses LUA owner processes and
/// the real Task action. A fresh LogonUser token cannot fake an interactive session.
#[test]
fn task_native_background_lua_gateway_control_stop_crash_recovery_and_autostart() {
    let _privileges = windows_account::Privileges::enable();
    let input = Inputs::load();
    let windows_background_task::Owner {
        token,
        session: expected_session,
    } = windows_background_task::owner();
    let staged = windows_token_launcher::StagedFixture::create(&token);
    let sid = background::current_sid_string().unwrap();
    windows_account::assert_standard(&token, &sid);
    let layout = input.owner_layout(&token);
    let cli = input.owner_cli(&layout);
    let installed = input.install(&token, false);
    let mut cleanup = Cleanup {
        inputs: &input,
        token: &token,
        cli: cli.clone(),
        account: None,
        armed: true,
    };
    assert_eq!(installed["installed"], true);
    assert_eq!(installed["started"], false);
    assert_eq!(
        assert_status(&token, &cli, "windows-task-user", false)["autostart_enabled"],
        false
    );
    assert_eq!(
        fixture::cli(&token, &cli, &["service", "autostart", "enable"])["autostart_enabled"],
        true
    );
    assert_eq!(
        fixture::cli(&token, &cli, &["service", "autostart", "disable"])["autostart_enabled"],
        false
    );
    assert_eq!(
        fixture::cli(&token, &cli, &["service", "start"])["local_control_ready"],
        true
    );
    let first = fixture::inspect(&token, false);
    assert_eq!(first["sid"], sid);
    assert_eq!(first["session"], expected_session);
    assert_status(&token, &cli, "windows-task-user", true);
    let oauth_pending = windows_subscription_login::run_before_restart(&mut |args, request| {
        subscription_cli(&token, &cli, args, request)
    });
    assert_eq!(
        fixture::cli(&token, &cli, &["service", "stop"])["local_control_ready"],
        false
    );
    assert_status(&token, &cli, "windows-task-user", false);
    // Fail after real file publication using a native deny-delete handle on the
    // last destination. Re-entering the shipped install command runs rollback
    // from its prepared journal before completing repair of this same package.
    let failure = input.transaction_install(&token, "transaction-fail-install");
    assert_ne!(failure["command"]["code"], 0);
    assert_eq!(failure["after"]["journal"]["phase"], "prepared");
    assert_eq!(input.install(&token, false)["installed"], true);
    let repaired = fixture::transaction_snapshot(&token, true);
    assert_eq!(repaired["marker"], failure["before"]["marker"]);
    assert_eq!(repaired["definition"], failure["before"]["definition"]);
    assert_eq!(repaired["manifest"], failure["before"]["manifest"]);
    assert_status(&token, &cli, "windows-task-user", false);
    // A deliberate stop must stay stopped rather than invoke failure recovery.
    thread::sleep(Duration::from_secs(2));
    assert_status(&token, &cli, "windows-task-user", false);
    fixture::cli(&token, &cli, &["service", "start"]);
    windows_subscription_login::assert_after_restart(
        &mut |args, request| subscription_cli(&token, &cli, args, request),
        oauth_pending,
    );
    let crashed = fixture::inspect(&token, true);
    let recovered = await_actual_restart(&token, &cli, "windows-task-user", &crashed);
    assert_eq!(recovered["session"], expected_session);
    fixture::forced_stop(&token);
    assert_status(&token, &cli, "windows-task-user", false);
    // This crosses the real PT1M failure recovery interval. Merely observing an
    // immediate exit cannot prove Scheduler interpreted this as a user stop.
    thread::sleep(Duration::from_secs(65));
    assert_status(&token, &cli, "windows-task-user", false);
    assert_eq!(input.uninstall(&token, &cli, false)["uninstalled"], true);
    cleanup.armed = false;
    assert!(!PathBuf::from(layout["marker"].as_str().unwrap()).exists());
    assert!(
        !PathBuf::from(layout["home"].as_str().unwrap())
            .join(".local/bin/hiroute.exe")
            .exists()
    );
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
        "actual Task product gate: LUA, WTS session, Gateway v2, Local Control, graceful stop, forced recovery, autostart, deletion verified"
    );
}

/// Fresh native non-admin SAM owner; every user operation actually executes in
/// that token. Admin credentials are authenticated by the production stdin helper.
#[test]
fn scm_standard_owner_pending_admin_finalize_session_zero_gateway_control_and_delete() {
    let _privileges = windows_account::Privileges::enable();
    let input = Inputs::load();
    let account = windows_account::Account::create();
    let token = account.token.as_ref().unwrap();
    let staged = windows_token_launcher::StagedFixture::create(token);
    let layout = input.owner_layout(token);
    assert_eq!(layout["sid"], account.sid);
    assert_eq!(
        PathBuf::from(layout["home"].as_str().unwrap()),
        account.profile
    );
    let cli = input.owner_cli(&layout);
    let pending = input.install(token, true);
    let mut cleanup = Cleanup {
        inputs: &input,
        token,
        cli: cli.clone(),
        account: Some(&account),
        armed: true,
    };
    assert_eq!(pending["pending_admin"], true);
    assert_eq!(pending["completed"], false);
    assert_eq!(pending["owner_sid"], account.sid);
    let journal = fixture::transaction_snapshot(token, false)["journal"].clone();
    let reopened = input.install(token, true);
    assert_eq!(
        reopened, pending,
        "prepared SCM installation must reopen without manufacturing completion"
    );
    assert_eq!(
        fixture::transaction_snapshot(token, false)["journal"],
        journal
    );
    let denied = fixture::command(
        token,
        &cli,
        vec![
            "service".into(),
            "start".into(),
            "--output".into(),
            "json".into(),
        ],
    );
    assert_ne!(
        denied["code"], 0,
        "prepared maintenance must refuse business startup"
    );
    // The standard owner can alter their own binaries but cannot turn these
    // mutable bytes into an SCM host. The independently verified admin package
    // supplies the privileged source, and mismatch must have no machine writes.
    let altered = fixture::owner(
        token,
        fixture::Request {
            action: "tamper-daemon".into(),
            program: None,
            arguments: vec![],
        },
        Duration::from_secs(60),
    );
    let bad_host = PathBuf::from(altered["definition"]["service_host"].as_str().unwrap());
    assert!(
        !bad_host.parent().unwrap().exists(),
        "tampered host cache must begin absent"
    );
    assert_native_service_absent(altered["definition"]["name"].as_str().unwrap());
    let rejected = input.admin_attempt(&account, "install", None);
    let restored = fixture::owner(
        token,
        fixture::Request {
            action: "restore-daemon".into(),
            program: None,
            arguments: vec![],
        },
        Duration::from_secs(60),
    );
    assert_eq!(restored["modified"], true);
    assert_ne!(
        rejected["code"], 0,
        "tampered owner image cannot publish a privileged host"
    );
    assert_native_service_absent(altered["definition"]["name"].as_str().unwrap());
    assert!(
        !bad_host.parent().unwrap().exists(),
        "tampered_owner_image_cannot_publish_privileged_host: rejection must precede every machine host/cache write"
    );
    input.admin(&account, "install", None);
    let reopened = input.install(token, true);
    assert_eq!(
        reopened, pending,
        "actual SCM registration still requires owner finalization on reentry"
    );
    assert_eq!(
        fixture::transaction_snapshot(token, false)["journal"],
        journal
    );
    // Registration alone is insufficient: owner admission stays closed until
    // its real machine-definition/hash verification commits the prepared journal.
    let denied = fixture::command(
        token,
        &cli,
        vec![
            "service".into(),
            "start".into(),
            "--output".into(),
            "json".into(),
        ],
    );
    assert_ne!(denied["code"], 0);
    let finished = input.finalize(token, &cli);
    assert_eq!(finished["completed"], true);
    assert_eq!(finished["operation"], "install");
    assert_eq!(finished["started"], false);
    fixture::transaction_snapshot(token, true);
    assert_eq!(
        assert_status(token, &cli, "windows-scm-user", false)["autostart_enabled"],
        false
    );
    // Owner can query/start/stop; machine ChangeConfig remains admin-only.
    let denied = fixture::command(
        token,
        &cli,
        vec![
            "service".into(),
            "autostart".into(),
            "enable".into(),
            "--output".into(),
            "json".into(),
        ],
    );
    assert_ne!(denied["code"], 0);
    input.admin(&account, "set_autostart", Some(true));
    assert_eq!(
        assert_status(token, &cli, "windows-scm-user", false)["autostart_enabled"],
        true
    );
    input.admin(&account, "set_autostart", Some(false));
    assert_eq!(
        assert_status(token, &cli, "windows-scm-user", false)["autostart_enabled"],
        false
    );
    fixture::cli(token, &cli, &["service", "start"]);
    let first = fixture::inspect(token, false);
    assert_eq!(first["sid"], account.sid);
    assert_eq!(
        first["session"], 0,
        "real SCM business runtime must operate without an interactive owner logon"
    );
    assert_status(token, &cli, "windows-scm-user", true);
    let oauth_pending = windows_subscription_login::run_before_restart(&mut |args, request| {
        subscription_cli(token, &cli, args, request)
    });
    fixture::cli(token, &cli, &["service", "stop"]);
    assert_status(token, &cli, "windows-scm-user", false);
    fixture::cli(token, &cli, &["service", "start"]);
    windows_subscription_login::assert_after_restart(
        &mut |args, request| subscription_cli(token, &cli, args, request),
        oauth_pending,
    );
    let crashed = fixture::inspect(token, true);
    let recovered = await_actual_restart(token, &cli, "windows-scm-user", &crashed);
    assert_eq!(recovered["session"], 0);
    fixture::cli(token, &cli, &["service", "stop"]);
    let removal = input.uninstall(token, &cli, true);
    assert_eq!(removal["pending_admin"], true);
    assert_eq!(removal["operation"], "uninstall");
    assert_eq!(
        input.uninstall(token, &cli, true),
        removal,
        "prepared removal reentry must preserve the exact pending administrator operation"
    );
    let too_early = input.python(
        token,
        vec![
            "finalize".into(),
            "--cli".into(),
            cli.to_string_lossy().into_owned(),
        ],
    );
    assert_ne!(
        too_early["code"], 0,
        "owner cannot declare an existing SCM registration removed"
    );
    input.admin(&account, "uninstall", None);
    let removed = input.finalize(token, &cli);
    assert_eq!(removed["completed"], true);
    assert_eq!(removed["operation"], "uninstall");
    cleanup.armed = false;
    drop(cleanup);
    assert!(!PathBuf::from(layout["marker"].as_str().unwrap()).exists());
    assert!(!account.profile.join(".local/bin/hiroute.exe").exists());
    drop(staged);
    account.finish();
    println!(
        "actual SCM product gate: authenticated standard owner, pending admission, finalize, session 0 medium daemon, Gateway v2, Local Control, graceful stop, forced recovery, machine autostart, confirmed deletion verified"
    );
}
