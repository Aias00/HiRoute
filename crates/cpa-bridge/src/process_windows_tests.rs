//! Actual Windows backend evidence, without invoking a shell or emulating a Unix executable.
use super::*;

const SECRET: &str = "synthetic_cpa_stdin_only_secret_0123456789";

fn fixture_launch(root: &std::path::Path, mode: &[u8]) -> CpaLaunch {
    let binary = std::env::var_os("HIROUTE_WINDOWS_CPA_FIXTURE")
        .map(PathBuf::from)
        .expect("Windows native CPA fixture must be prepared; this gate cannot skip");
    assert!(binary.is_absolute() && binary.is_file());
    // The outer TempDir only owns cleanup. An elevated Windows token can give
    // a std-created directory Administrators ownership; never adopt that object
    // by changing its ACL after creation.
    let work_dir = root.join("private");
    drop(hiroute_platform::windows::security::create_owner_only_directory(&work_dir).unwrap());
    let config_path = work_dir.join("config.yaml");
    hiroute_platform::durable::write_private_new(&config_path, mode).unwrap();
    CpaLaunch {
        binary: VerifiedCpaBinary::fixture(
            binary,
            semver::Version::new(7, 2, 140),
            "fixture".into(),
        ),
        config_path,
        work_dir,
        management_password: Arc::new(
            crate::config::SecretText::from_owned(SECRET.into()).unwrap(),
        ),
        proxy_environment: crate::proxy_environment::ProxyEnvironment::capture([
            ("NO_PROXY".into(), "localhost,127.0.0.1".into()),
            (
                "OPENAI_API_KEY".into(),
                "must-not-cross-child-environment".into(),
            ),
            (
                "MANAGEMENT_PASSWORD".into(),
                "must-not-cross-child-environment".into(),
            ),
            ("BASH_ENV".into(), "must-not-cross-child-environment".into()),
        ]),
    }
}

#[test]
fn native_bootstrap_delivers_stdin_eof_and_minimal_windows_environment() {
    let root = tempfile::tempdir().unwrap();
    let launch = fixture_launch(root.path(), b"bootstrap");
    let mut child = StdCpaProcessBackend.spawn(&launch).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(exit) = child.try_exit().unwrap() {
            assert_eq!(exit.code, Some(0));
            break;
        }
        assert!(
            Instant::now() < deadline,
            "native CPA bootstrap did not complete"
        );
        thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(
        std::fs::read(launch.work_dir.join("bootstrap.ok")).unwrap(),
        b"stdin-eof-and-explicit-environment"
    );
}

#[test]
fn native_root_exit_cannot_report_stopped_until_descendant_job_is_empty() {
    let root = tempfile::tempdir().unwrap();
    let launch = fixture_launch(root.path(), b"tree");
    let mut child = StdCpaProcessBackend.spawn(&launch).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(exit) = child.try_exit().unwrap() {
            assert_eq!(exit.code, Some(0));
            break;
        }
        assert!(
            Instant::now() < deadline,
            "native CPA tree did not finish cleanup"
        );
        thread::sleep(Duration::from_millis(10));
    }
    for name in ["descendant.pid", "grandchild.pid"] {
        let pid: u32 = std::fs::read_to_string(launch.work_dir.join(name))
            .unwrap()
            .parse()
            .unwrap();
        assert!(
            !hiroute_platform::windows::job::process_is_running(pid).unwrap(),
            "CPA descendant survived a reported root exit"
        );
    }
}

#[test]
fn numeric_pid_is_never_an_orphan_termination_capability() {
    assert!(matches!(
        StdCpaProcessBackend.attach_authenticated(std::process::id()),
        Err(CpaProcessError::UntrustedOrphan)
    ));
    assert!(matches!(
        attach_recovered_process(&StdCpaProcessBackend, std::process::id()),
        Err(crate::CpaLifecycleError::UntrustedOrphan)
    ));
}
