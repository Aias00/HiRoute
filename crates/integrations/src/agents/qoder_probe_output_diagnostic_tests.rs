//! Real, owned-file I/O controls for the otherwise account-required probe.
//! No native client, model, credential, user profile or dynamic error text.
use super::*;
use std::io::{Error, ErrorKind, Seek, SeekFrom};

fn file(path: &Path, read: bool) -> File {
    std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .read(read)
        .open(path)
        .unwrap()
}

fn failed(path: &Path, observer: &File) -> (Error, OutputReadDiagnostic) {
    let mut observed = None;
    let error = observed_final_output(path, observer, 2, 2, &Instant::now(), |value| {
        observed = Some(value);
    })
    .unwrap_err();
    (
        error,
        observed.expect("the actual final read failure must be observed"),
    )
}

#[test]
fn qoder_final_read_diagnostic_distinguishes_native_oversize_from_path_open_failure() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("owned-output");
    let observer = file(&path, true);
    observer.set_len(512 * 1024 + 1).unwrap();
    let (error, diagnostic) = failed(&path, &observer);
    assert_eq!(error.kind(), ErrorKind::Other);
    assert_eq!(diagnostic.phase, "final_read");
    assert_eq!(diagnostic.observer_length, Some(512 * 1024 + 1));
    assert!(diagnostic.observer_metadata_error.is_none());
    assert_eq!(
        diagnostic.read_error,
        Some(ClosedOutputIo::from_error(&error))
    );
    assert_eq!(diagnostic.sample_stage, "complete");
    assert_eq!(diagnostic.sampled_bytes, Some(512 * 1024));
    assert_eq!(diagnostic.record_counts, Some([0, 0, 0, 0, 0, 0, 1]));
    assert!(diagnostic.sample_error.is_none());

    // Keep the exact original object while making its pathname unavailable.
    // Native Windows delete-sharing behavior is a required actual test, not
    // assumed to have the same errno/kind as Unix.
    std::fs::remove_file(&path).unwrap();
    let direct_open = File::open(&path).unwrap_err();
    let (open_error, unavailable) = failed(&path, &observer);
    assert_eq!(open_error.kind(), direct_open.kind());
    assert_eq!(open_error.raw_os_error(), direct_open.raw_os_error());
    assert_eq!(unavailable.phase, "final_read");
    assert_eq!(unavailable.observer_length, Some(512 * 1024 + 1));
    assert_eq!(
        unavailable.read_error,
        Some(ClosedOutputIo::from_error(&direct_open))
    );
    assert_eq!(unavailable.sample_stage, "complete");
    assert_ne!(unavailable.read_error, diagnostic.read_error);
}

#[test]
fn qoder_output_sampling_failure_keeps_retained_length_and_original_io() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("write-only-owned-output");
    let mut observer = file(&path, false);
    observer
        .write_all(b"fixed fixture bytes never exported")
        .unwrap();
    observer.flush().unwrap();
    let expected_length = observer.metadata().unwrap().len();
    let mut direct = observer.try_clone().unwrap();
    direct.seek(SeekFrom::Start(0)).unwrap();
    let direct_read = direct.read(&mut [0; 1]).unwrap_err();
    let diagnostic = output_read_diagnostic(&observer, "live_guard", None, 0, 0, Duration::ZERO);
    assert_eq!(diagnostic.phase, "live_guard");
    assert_eq!(diagnostic.observer_length, Some(expected_length));
    assert!(diagnostic.observer_metadata_error.is_none());
    assert!(diagnostic.read_error.is_none());
    assert_eq!(diagnostic.sample_stage, "read");
    assert_eq!(
        diagnostic.sample_error,
        Some(ClosedOutputIo::from_error(&direct_read))
    );
    assert!(diagnostic.sampled_bytes.is_none() && diagnostic.record_counts.is_none());
    let closed = serde_json::to_string(&diagnostic).unwrap();
    assert!(!closed.contains("fixture bytes") && !closed.contains("write-only-owned-output"));
}

#[test]
fn qoder_output_other_io_error_is_not_claimed_as_a_size_limit_and_success_is_unchanged() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("owned-small-output");
    let mut observer = file(&path, true);
    observer.write_all(b"{\"type\":\"result\"}\n").unwrap();
    observer.flush().unwrap();
    let mut called = false;
    let bytes =
        observed_final_output(&path, &observer, 3, 3, &Instant::now(), |_| called = true).unwrap();
    assert!(!called);
    assert_eq!(bytes.as_slice(), b"{\"type\":\"result\"}\n");
    let other = Error::other("dynamic internal error text must never cross this boundary");
    let diagnostic =
        output_read_diagnostic(&observer, "final_read", Some(&other), 3, 3, Duration::ZERO);
    assert_eq!(
        diagnostic.read_error,
        Some(ClosedOutputIo {
            kind: "other",
            raw_os_error: None
        })
    );
    assert_eq!(diagnostic.observer_length, Some(bytes.len() as u64));
    assert_eq!(diagnostic.record_counts, Some([0, 0, 0, 1, 0, 0, 1]));
    let closed = serde_json::to_value(diagnostic).unwrap();
    assert_eq!(closed.as_object().unwrap().len(), 11);
    assert!(!closed.to_string().contains("dynamic internal error"));
    assert!(closed.get("size_limit_cause").is_none());
    let unmapped = Error::from_raw_os_error(0x7fff_1234);
    assert_eq!(
        ClosedOutputIo::from_error(&unmapped).raw_os_error,
        Some(0x7fff_1234)
    );
}
