use super::*;
use std::{fs, os::unix::fs::PermissionsExt, path::PathBuf};

fn private_directory() -> tempfile::TempDir {
    let directory = tempfile::tempdir().unwrap();
    fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700)).unwrap();
    directory
}
fn command(directory: &Path, mode: &str) -> Command {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--exact",
            "updates::installer::control::tests::helper_process",
            "--nocapture",
        ])
        .env("HIROUTE_UPDATER_TEST_DIRECTORY", directory)
        .env("HIROUTE_UPDATER_TEST_MODE", mode);
    command
}
fn start_test(directory: &Path, mode: &str) -> Result<Installer, String> {
    start_command(
        &directory.join("handoff.json"),
        command(directory, mode),
        Duration::from_secs(2),
    )
}
fn gone(pid: u32) -> bool {
    nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid as i32), None)
        == Err(nix::errno::Errno::ESRCH)
}
fn wait_for(path: &Path) {
    let deadline = Instant::now() + Duration::from_secs(3);
    while !path.exists() {
        assert!(
            Instant::now() < deadline,
            "missing helper result: {}",
            path.display()
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

// Real subprocesses exercise the parent's ownership and OS reaping, without an App
// fixture pretending to prove a production package installation.
#[test]
fn helper_process() {
    let Some(path) = std::env::var_os("HIROUTE_UPDATER_TEST_DIRECTORY") else {
        return;
    };
    let path = PathBuf::from(path);
    fs::write(path.join("pid"), std::process::id().to_string()).unwrap();
    match std::env::var("HIROUTE_UPDATER_TEST_MODE").unwrap().as_str() {
        "failure" => {
            eprintln!("UPGRADE_PACKAGE_CHANGED");
            std::process::exit(42);
        }
        "no_ack" => std::thread::sleep(Duration::from_secs(30)),
        "bad_ack" => {
            write_private(&path.join("started"), b"partial").unwrap();
            std::thread::sleep(Duration::from_secs(30));
        }
        "exit_after_ack" => {
            write_private(&path.join("started"), STARTED).unwrap();
            std::process::exit(43);
        }
        "ready" => {
            let directory = PrivateDir::open_existing(&path).unwrap();
            write_private(&path.join("started"), STARTED).unwrap();
            if await_commit(
                &directory,
                nix::unistd::getppid().as_raw() as u32,
                Duration::from_secs(5),
            )
            .is_ok()
            {
                write_private(&path.join("authorized"), b"yes").unwrap();
            }
        }
        mode => panic!("unexpected helper mode {mode}"),
    }
}

#[test]
fn preflight_failure_returns_concrete_code_and_reaps_child() {
    let directory = private_directory();
    assert_eq!(
        start_test(directory.path(), "failure").err().unwrap(),
        "UPGRADE_PACKAGE_CHANGED"
    );
    let pid = fs::read_to_string(directory.path().join("pid"))
        .unwrap()
        .parse()
        .unwrap();
    assert!(gone(pid));
    assert!(!directory.path().join("authorized").exists());
}

#[test]
fn readiness_does_not_authorize_installation_and_drop_reaps_helper() {
    let directory = private_directory();
    let installer = start_test(directory.path(), "ready").unwrap();
    let pid = installer.child.id();
    assert!(!directory.path().join("authorized").exists());
    drop(installer);
    assert_eq!(fs::read(directory.path().join("decision")).unwrap(), CANCEL);
    assert!(gone(pid));
    assert!(!directory.path().join("authorized").exists());
}

#[test]
fn explicit_commit_detaches_the_authorized_helper() {
    let directory = private_directory();
    let installer = start_test(directory.path(), "ready").unwrap();
    let pid = installer.child.id();
    installer.commit().unwrap();
    wait_for(&directory.path().join("authorized"));
    // Installer intentionally detached the real child. waitpid prevents a zombie in
    // this test process while retaining the same production detach semantics.
    nix::sys::wait::waitpid(nix::unistd::Pid::from_raw(pid as i32), None).unwrap();
    assert_eq!(fs::read(directory.path().join("decision")).unwrap(), COMMIT);
}

#[test]
fn dead_helper_cannot_be_committed() {
    let directory = private_directory();
    // Publish an ACK then terminate; wait explicitly to make the race deterministic.
    let directory_handle = PrivateDir::open_existing(directory.path()).unwrap();
    let mut child = command(directory.path(), "exit_after_ack")
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    child.wait().unwrap();
    let installer = Installer {
        child,
        directory: directory_handle,
        committed: false,
    };
    assert_eq!(
        installer.commit().unwrap_err(),
        "UPGRADE_INSTALLER_EXIT_FAILED"
    );
    assert_eq!(fs::read(directory.path().join("decision")).unwrap(), CANCEL);
}

#[test]
fn missing_or_malformed_ack_never_leaves_a_helper_alive() {
    for mode in ["no_ack", "bad_ack"] {
        let directory = private_directory();
        let error = start_command(
            &directory.path().join("handoff.json"),
            command(directory.path(), mode),
            Duration::from_millis(300),
        )
        .err()
        .unwrap();
        assert_eq!(
            error,
            if mode == "no_ack" {
                "UPGRADE_INSTALLER_START_TIMEOUT"
            } else {
                "UPGRADE_INSTALLER_INVALID"
            }
        );
        let pid = fs::read_to_string(directory.path().join("pid"))
            .unwrap()
            .parse()
            .unwrap();
        assert!(gone(pid));
        assert!(!directory.path().join("authorized").exists());
    }
}

#[test]
fn parent_death_and_commit_timeout_are_not_authorization() {
    let directory = private_directory();
    let handle = PrivateDir::open_existing(directory.path()).unwrap();
    let mut parent = Command::new("/usr/bin/true").spawn().unwrap();
    let pid = parent.id();
    parent.wait().unwrap();
    assert_eq!(
        await_commit(&handle, pid, Duration::from_secs(1)).unwrap_err(),
        "UPGRADE_INSTALLER_COMMIT_REQUIRED"
    );
    assert_eq!(
        await_commit(&handle, std::process::id(), Duration::from_millis(30)).unwrap_err(),
        "UPGRADE_INSTALLER_COMMIT_TIMEOUT"
    );
    write_private(&directory.path().join("decision"), CANCEL).unwrap();
    assert_eq!(
        await_commit(&handle, std::process::id(), Duration::from_secs(1)).unwrap_err(),
        "UPGRADE_INSTALLER_CANCELLED"
    );
}

#[test]
fn malformed_decisions_and_untrusted_error_output_are_closed() {
    let directory = private_directory();
    let handle = PrivateDir::open_existing(directory.path()).unwrap();
    write_private(&directory.path().join("decision"), b"commit").unwrap();
    assert_eq!(
        await_commit(&handle, std::process::id(), Duration::from_secs(1)).unwrap_err(),
        "UPGRADE_INSTALLER_DECISION_INVALID"
    );
    write_private(
        &directory.path().join("failed.json"),
        br#"{"code":"/private/data/secret"}"#,
    )
    .unwrap();
    write_private(
        &directory.path().join("installer.log"),
        b"UPGRADE_UNKNOWN\nprivate text",
    )
    .unwrap();
    let status = Command::new("/usr/bin/false").status().unwrap();
    assert_eq!(failure(&handle, status), "UPGRADE_INSTALLER_EXIT_FAILED");
}
