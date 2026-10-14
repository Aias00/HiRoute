//! Real public cancellation after a native tool child has actually started. The
//! same Job/Worker boundary must stop that child before publishing cancellation.
use super::{InstalledProduct, command, exec_args, oracle, run, write_new};
use hiroute_platform::windows::security;
use serde_json::{Value, json};
use std::ffi::OsString;
use std::fs::File;
use std::io::{self, Read};
use std::os::windows::io::{AsRawHandle, FromRawHandle};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::{WAIT_OBJECT_0, WAIT_TIMEOUT};
use windows_sys::Win32::System::Threading::{
    OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE, WaitForSingleObject,
};

pub(super) struct CancellationReceipts {
    pub late: Vec<PathBuf>,
    pub residual_task: String,
}

pub(super) fn verify(
    product: &InstalledProduct,
    workspace: &Path,
    plan: &str,
    nonce: &str,
) -> CancellationReceipts {
    let executors = command(
        product,
        &["worker", "executors"],
        None,
        Duration::from_secs(30),
    );
    let pi = executors["data"]["executors"]
        .as_array()
        .unwrap()
        .iter()
        .find(|executor| executor["harness"] == "pi")
        .unwrap();
    assert_eq!(pi["start_approve_all"]["state"], "ready");
    assert_eq!(pi["cancel"]["state"], "ready");
    assert_eq!(pi["continue_session"]["state"], "unknown");
    assert_eq!(pi["continue_session"]["reason"], "capability_unverified");
    let prompt = workspace.join("cancel-task.txt");
    write_new(
        &prompt,
        format!(
            "{}: Perform the bounded cancellation receipt task using the native shell tool.\n",
            oracle::CANCEL
        )
        .as_bytes(),
    );
    let accepted = command(
        product,
        &exec_args(plan, workspace, &prompt, "windows-public-worker-cancel"),
        None,
        Duration::from_secs(180),
    );
    let run_id = accepted["data"]["run_id"].as_str().unwrap();
    let started = workspace.join("cancel-started.txt");
    let late = workspace.join("cancel-late.txt");
    let deadline = Instant::now() + Duration::from_secs(30);
    let expected_started = format!("started-{nonce}");
    loop {
        let ready = started_receipt_ready(&started, &expected_started)
            .expect("native cancellation receipt could not be read through its held identity");
        assert!(
            Instant::now() < deadline,
            "real native tool child did not publish its complete receipt before the cancellation deadline"
        );
        if ready {
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }
    assert!(!late.exists());
    let cancelled = command(
        product,
        &[
            "worker",
            "cancel",
            "--run",
            run_id,
            "--idempotency-key",
            "windows-public-cancel",
        ],
        None,
        Duration::from_secs(30),
    );
    assert_eq!(cancelled["data"]["run_id"], run_id);
    let mut current = command(
        product,
        &["worker", "wait", "--run", run_id, "--wait-timeout", "20"],
        None,
        Duration::from_secs(30),
    );
    let deadline = Instant::now() + Duration::from_secs(30);
    while current["data"]["run_state"] != "cancelled" {
        assert!(!matches!(
            current["data"]["run_state"].as_str(),
            Some("failed" | "succeeded" | "unknown")
        ));
        assert!(
            Instant::now() < deadline,
            "cancelled public Worker did not settle its exact native scope"
        );
        current = command(
            product,
            &["worker", "wait", "--run", run_id, "--wait-timeout", "5"],
            None,
            Duration::from_secs(10),
        );
    }
    assert!(
        !current["next_actions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|action| action["command_id"] == "worker.cleanup.confirm"),
        "known stopped Job must not be reported as residual cleanup success"
    );
    let result = command(
        product,
        &["worker", "result", "--run", run_id],
        None,
        Duration::from_secs(30),
    );
    assert_eq!(result["data"]["run_id"], run_id);
    assert_eq!(result["data"]["run_state"], "cancelled");
    assert!(
        !late.exists(),
        "native tool continued writing after public cancellation"
    );
    let read = command(
        product,
        &["worker", "read", "--run", run_id, "--max-bytes", "1024"],
        None,
        Duration::from_secs(30),
    );
    let revision = read["data"]["state_revision"].as_u64().unwrap().to_string();
    let arguments = [
        "worker",
        "cleanup",
        "confirm",
        "--run",
        run_id,
        "--expected-revision",
        &revision,
        "--idempotency-key",
        "windows-clean-run-cannot-ack-residual",
        "--handled",
        "--output",
        "json",
        "--timeout",
        "10s",
    ];
    let (exit, bytes, _) = run(
        &product.stable,
        &arguments.iter().map(OsString::from).collect::<Vec<_>>(),
        product.stable.parent(),
        None,
        Duration::from_secs(15),
    );
    let rejected: Value = serde_json::from_slice(&bytes).unwrap();
    assert_ne!(
        exit, 0,
        "known-clean state must reject fabricated residual confirmation"
    );
    assert_eq!(rejected["error"]["code"], "REVISION_CONFLICT");
    assert_eq!(rejected["status"], json!("conflict"));
    let (residual_late, residual_task) = residual(product, workspace, plan, nonce);
    CancellationReceipts {
        late: vec![late, residual_late],
        residual_task,
    }
}

/// Real Windows sharing failure, followed by explicit user handling and the
/// released confirmation producer. Confirmation acknowledges the occupied slot;
/// it does not perform filesystem cleanup or claim that the run root was deleted.
fn residual(
    product: &InstalledProduct,
    workspace: &Path,
    plan: &str,
    nonce: &str,
) -> (PathBuf, String) {
    let prompt = workspace.join("residual-task.txt");
    write_new(
        &prompt,
        format!(
            "{}: Start the bounded native cancellation receipt task.\n",
            oracle::RESIDUAL
        )
        .as_bytes(),
    );
    let accepted = command(
        product,
        &exec_args(plan, workspace, &prompt, "windows-public-worker-residual"),
        None,
        Duration::from_secs(180),
    );
    let run_id = accepted["data"]["run_id"].as_str().unwrap();
    let task_id = accepted["data"]["task_id"].as_str().unwrap();
    let started = workspace.join("residual-started.json");
    let late = workspace.join("residual-late.txt");
    let deadline = Instant::now() + Duration::from_secs(30);
    let binding: Value = loop {
        if let Ok(file) = security::open_identity(&started, false) {
            assert!(file.metadata().unwrap().len() <= 4096);
            let mut bytes = Vec::new();
            file.take(4097).read_to_end(&mut bytes).unwrap();
            if let Ok(value) = serde_json::from_slice(&bytes) {
                break value;
            }
        }
        assert!(
            Instant::now() < deadline,
            "native residual tool did not produce its bounded process binding"
        );
        thread::sleep(Duration::from_millis(10));
    };
    assert_eq!(binding["receipt"], format!("started-{nonce}"));
    let pid = u32::try_from(binding["pid"].as_u64().unwrap()).unwrap();
    assert_ne!(pid, 0);
    // The locator comes only from the actual native tool's $PID. Pin its kernel
    // object while it is live, and observe that same handle after cancellation.
    let process = unsafe {
        OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
            0,
            pid,
        )
    };
    assert!(
        !process.is_null(),
        "cannot pin the actual native residual tool process"
    );
    // SAFETY: OpenProcess returned a fresh owned kernel handle; File closes it once.
    let process = unsafe { File::from_raw_handle(process) };
    assert_eq!(
        unsafe { WaitForSingleObject(process.as_raw_handle(), 0) },
        WAIT_TIMEOUT
    );

    let temporary = PathBuf::from(binding["temp"].as_str().unwrap());
    assert!(temporary.is_absolute());
    assert_eq!(temporary.file_name().unwrap(), "tmp");
    let root = std::fs::canonicalize(temporary.parent().unwrap()).unwrap();
    let base = std::fs::canonicalize(
        product
            .layout
            .storage_root()
            .join("delegation-workers/runs"),
    )
    .unwrap();
    assert_eq!(root.parent().unwrap(), base);
    let name = root.file_name().unwrap().to_str().unwrap();
    assert!(name.len() == 64 && name.bytes().all(|byte| byte.is_ascii_hexdigit()));
    let base_handle = security::open_owner_only(&base, true).unwrap();
    let root_handle = security::open_owner_only(&root, true).unwrap();
    let parent_handle = security::open_owner_only(root.parent().unwrap(), true).unwrap();
    assert_eq!(
        security::file_identity(&parent_handle).unwrap(),
        security::file_identity(&base_handle).unwrap()
    );
    let sdk = root.join("pi-sdk-contract.mjs");
    let mut deny_delete = security::open_owner_only(&sdk, false).unwrap();
    let sdk_identity = security::file_identity(&deny_delete).unwrap();
    // Exactly the production material producer, without a second fixture copy.
    let expected = include_bytes!("../../../integrations/src/agents/pi_sdk_contract.mjs");
    assert_eq!(deny_delete.metadata().unwrap().len(), expected.len() as u64);
    let mut actual = Vec::new();
    (&mut deny_delete)
        .take(expected.len() as u64 + 1)
        .read_to_end(&mut actual)
        .unwrap();
    assert_eq!(actual, expected);
    assert!(!late.exists());

    command(
        product,
        &[
            "worker",
            "cancel",
            "--run",
            run_id,
            "--idempotency-key",
            "windows-public-residual-cancel",
        ],
        None,
        Duration::from_secs(30),
    );
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let observed = command(
            product,
            &["worker", "wait", "--run", run_id, "--wait-timeout", "5"],
            None,
            Duration::from_secs(10),
        );
        if observed["data"]["run_state"] == "cancelled" {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "native residual cancellation did not settle"
        );
        assert!(!matches!(
            observed["data"]["run_state"].as_str(),
            Some("succeeded" | "failed" | "unknown")
        ));
    }
    assert_eq!(
        unsafe { WaitForSingleObject(process.as_raw_handle(), 10_000) },
        WAIT_OBJECT_0,
        "the exact native tool process remains live after cancellation"
    );
    assert!(!late.exists());
    let listed = command(product, &["worker", "list"], None, Duration::from_secs(30));
    let task = listed["data"]["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|task| task["task_id"] == task_id)
        .unwrap();
    assert_eq!(task["run"]["run_id"], run_id);
    assert_eq!(task["run"]["state"], "cancelled");
    assert_eq!(
        task["run"]["cleanup"], "unknown",
        "a real deny-delete conflict must remain observable as residual cleanup"
    );
    let read = command(
        product,
        &["worker", "read", "--run", run_id, "--max-bytes", "1024"],
        None,
        Duration::from_secs(30),
    );
    assert_eq!(read["data"]["run_state"], "cancelled");
    let revision = read["data"]["state_revision"].as_u64().unwrap().to_string();
    drop(deny_delete);
    // Handle only this test's exact locked material. The daemon retains its run
    // directory identity; an acknowledgement does not assert automatic root removal.
    let handled = security::open_delete_at(&root_handle, sdk.file_name().unwrap(), false).unwrap();
    assert_eq!(security::file_identity(&handled).unwrap(), sdk_identity);
    security::delete_by_handle(&handled).unwrap();
    drop(handled);
    let args = [
        "worker",
        "cleanup",
        "confirm",
        "--run",
        run_id,
        "--expected-revision",
        &revision,
        "--idempotency-key",
        "windows-public-residual-handled",
        "--handled",
    ];
    let confirmed = command(product, &args, None, Duration::from_secs(30));
    assert_eq!(confirmed["data"]["task_id"], task_id);
    assert_eq!(confirmed["data"]["run_id"], run_id);
    assert_eq!(confirmed["data"]["state"], "cancelled");
    assert_eq!(confirmed["data"]["cleanup"], "residual_acknowledged");
    let replay = command(product, &args, None, Duration::from_secs(30));
    assert_eq!(confirmed["data"], replay["data"]);
    assert_eq!(
        confirmed["data"]["state_revision"].as_u64().unwrap(),
        revision.parse::<u64>().unwrap() + 1
    );
    assert!(!late.exists());
    (late, task_id.to_owned())
}

/// Shell redirection creates the file before printf publishes its bytes. Retain a
/// reparse-free read handle, but allow the real producer to finish writing. Empty
/// and partial contents are pending within the original deadline, never success.
fn started_receipt_ready(path: &Path, expected: &str) -> io::Result<bool> {
    let file = match security::open_identity(path, false) {
        Ok(file) => file,
        Err(error)
            if error.kind() == io::ErrorKind::NotFound || error.raw_os_error() == Some(32) =>
        {
            return Ok(false);
        }
        Err(error) => return Err(error),
    };
    const BOUND: u64 = 4096;
    if file.metadata()?.len() > BOUND || security::file_link_count(&file)? != 1 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "native cancellation receipt exceeds its bound or aliases another file",
        ));
    }
    let mut bytes = Vec::new();
    file.take(BOUND + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > BOUND {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "native cancellation receipt grew beyond its bound",
        ));
    }
    let text = std::str::from_utf8(&bytes)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "native receipt is not UTF-8"))?;
    Ok(text.trim() == expected)
}

#[cfg(test)]
mod tests {
    use super::started_receipt_ready;
    use hiroute_platform::windows::security;
    use std::io::Write;

    #[test]
    fn cancellation_waits_for_the_complete_native_receipt_after_file_creation() {
        let root = std::env::temp_dir().join(format!(
            "hiroute-cancel-publication-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        drop(security::create_owner_only_directory(&root).unwrap());
        struct Cleanup(std::path::PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                std::fs::remove_dir_all(&self.0).unwrap();
            }
        }
        let _cleanup = Cleanup(root.clone());
        let path = root.join("cancel-started.txt");
        let expected = "started-publication-nonce";
        assert!(!started_receipt_ready(&path, expected).unwrap());
        drop(security::create_owner_only_file(&path).unwrap());
        let mut producer = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
        // This is the observed source20 race: is_file is true while the old
        // immediate read sees an empty string. No sleep/scheduler luck is needed.
        assert!(path.is_file());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "");
        assert!(!started_receipt_ready(&path, expected).unwrap());
        producer.write_all(b"started-publication-").unwrap();
        producer.flush().unwrap();
        assert!(!started_receipt_ready(&path, expected).unwrap());
        producer.write_all(b"nonce\n").unwrap();
        producer.flush().unwrap();
        assert!(started_receipt_ready(&path, expected).unwrap());
        assert!(!started_receipt_ready(&path, "started-another-nonce").unwrap());
        drop(producer);
        assert!(started_receipt_ready(&path, expected).unwrap());
    }
}
