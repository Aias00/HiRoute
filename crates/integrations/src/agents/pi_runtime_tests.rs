use super::*;
use std::os::unix::fs::PermissionsExt;

fn native_fixture(root: &Path, body: &str) -> PathBuf {
    let path = root.join("node-body.sh");
    std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
    path
}

#[test]
fn supported_cold_pi_node_is_not_rejected_after_two_seconds() {
    let root = tempfile::tempdir().unwrap();
    let node = root.path().join("node");
    std::fs::write(&node, "#!/bin/sh\nsleep 3; printf 'v24.19.0\\n'\n").unwrap();
    std::fs::set_permissions(&node, std::fs::Permissions::from_mode(0o700)).unwrap();
    assert!(validate_pi_node(&node).is_ok());
}

#[test]
fn pi_checks_preserve_stage_reason_and_owned_timeout_cleanup() {
    use NativeDependencyFailureReasonV1 as Reason;
    for (body, expected) in [
        ("echo v22.18.0", Reason::Unsupported),
        ("echo 'private native error'", Reason::InvalidOutput),
        ("exit 7", Reason::ProcessFailed),
    ] {
        let root = tempfile::tempdir().unwrap();
        let node = native_fixture(root.path(), body);
        assert_eq!(
            validate_pi_node(&node),
            Err(NativeDependencyFailureV1 {
                check: NativeDependencyCheckV1::PiNodeVersion,
                reason: expected,
            })
        );
    }
    let root = tempfile::tempdir().unwrap();
    let node = native_fixture(root.path(), "exec /bin/sleep 20");
    let failure = bounded_check(
        std::process::Command::new(&node),
        NativeDependencyCheckV1::PiSdk,
        std::time::Duration::from_millis(50),
        128,
    )
    .unwrap_err();
    assert_eq!(failure.check, NativeDependencyCheckV1::PiSdk);
    assert_eq!(failure.reason, Reason::Timeout);
    assert!(failure.reason.retryable());
    assert!(
        !failure
            .to_string()
            .contains(&node.to_string_lossy().to_string())
    );
}

#[cfg(target_os = "linux")]
#[test]
fn pi_node_fixture_preserves_version_with_a_writable_input() {
    let root = tempfile::tempdir().unwrap();
    let node = native_fixture(root.path(), "echo v22.18.0");
    let writable_input = std::fs::OpenOptions::new()
        .write(true)
        .open(root.path().join("node-body.sh"))
        .unwrap();
    assert_eq!(
        validate_pi_node(&node),
        Err(NativeDependencyFailureV1 {
            check: NativeDependencyCheckV1::PiNodeVersion,
            reason: NativeDependencyFailureReasonV1::Unsupported,
        })
    );
    drop(writable_input);
}

#[cfg(target_os = "linux")]
#[test]
fn pi_node_busy_executable_reports_unavailable_before_version_parsing() {
    let root = tempfile::tempdir().unwrap();
    let node = root.path().join("busy-node");
    std::fs::write(&node, "#!/bin/sh\necho v22.18.0\n").unwrap();
    std::fs::set_permissions(&node, std::fs::Permissions::from_mode(0o700)).unwrap();
    let writable_executable = std::fs::OpenOptions::new().write(true).open(&node).unwrap();
    let error = std::process::Command::new(&node)
        .arg("--version")
        .output()
        .unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::ExecutableFileBusy);
    assert_eq!(
        validate_pi_node(&node),
        Err(NativeDependencyFailureV1 {
            check: NativeDependencyCheckV1::PiNodeVersion,
            reason: NativeDependencyFailureReasonV1::Unavailable,
        })
    );
    drop(writable_executable);
}

#[test]
fn cold_pi_sdk_keeps_exact_scope_and_does_not_reuse_failed_capability() {
    let root = tempfile::tempdir().unwrap();
    let cli = root.path().join("pi.js");
    std::fs::write(&cli, "fixture").unwrap();
    std::fs::write(
        root.path().join("package.json"),
        r#"{"name":"@earendil-works/pi-coding-agent","version":"1.1.0","bin":{"pi":"pi.js"}}"#,
    )
    .unwrap();
    let node = native_fixture(
        root.path(),
        r#"
if [ "$1" = --version ]; then echo v24.19.0; exit 0; fi
test "$2" = --check && test "$4" = continue || exit 7
sleep 6
echo hiroute.pi-sdk-capability/v1:ok
"#,
    );
    assert!(check_pi_sdk_capability(&cli, &node, PiSdkCapability::Continue).is_ok());
    let failure = check_pi_sdk_capability(&cli, &node, PiSdkCapability::Worker).unwrap_err();
    assert_eq!(failure.check, NativeDependencyCheckV1::PiSdk);
    assert_eq!(
        failure.reason,
        NativeDependencyFailureReasonV1::ProcessFailed
    );
}
