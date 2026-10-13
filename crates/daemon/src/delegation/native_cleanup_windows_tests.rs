//! Native no-model deletion gates: identity and budgets apply to held Windows objects.
use super::*;
use hiroute_domain::{
    WorkspaceId,
    delegation::{
        DELEGATION_NATIVE_ROOT_MARKER_FILE_V1 as MARKER, DelegationNativeFilesystemIdentityV1,
        DelegationNativeRootStateV1, WorkerHarnessV1,
    },
};
use hiroute_platform::windows::security::{
    create_owner_only_directory, file_identity, open_identity, set_owner_only,
};
use std::path::Path;

fn record(base: &Path, root: &Path) -> DelegationNativeRootV1 {
    let base_identity = file_identity(&open_identity(base, true).unwrap()).unwrap();
    let root_identity = file_identity(&open_identity(root, true).unwrap()).unwrap();
    let mut record = DelegationNativeRootV1 {
        workspace_id: WorkspaceId::default(),
        task_id: "task-native-delete".into(),
        root_generation: 1,
        harness: WorkerHarnessV1::CodexCli,
        workspace_root_identity: "workspace-root".into(),
        relative_root: root.file_name().unwrap().to_str().unwrap().into(),
        creation_nonce: "creation-native-delete".into(),
        state: DelegationNativeRootStateV1::Deleting,
        use_revision: 1,
        managed_base_path: Some(base.to_str().unwrap().into()),
        filesystem_identity: Some(DelegationNativeFilesystemIdentityV1 {
            scheme: "windows-volume-file-index-v1".into(),
            base_device: base_identity.0.into(),
            base_inode: base_identity.1,
            root_device: root_identity.0.into(),
            root_inode: root_identity.1,
            marker_device: 0,
            marker_inode: 0,
        }),
        cleanup_claim: None,
        deletion_batches: 0,
        last_cleanup_failure: None,
    };
    let marker = root.join(MARKER);
    hiroute_platform::durable::write_private_new(
        &marker,
        &serde_json::to_vec(&record.ownership_marker()).unwrap(),
    )
    .unwrap();
    let identity = file_identity(&open_identity(&marker, false).unwrap()).unwrap();
    let expected = record.filesystem_identity.as_mut().unwrap();
    expected.marker_device = identity.0.into();
    expected.marker_inode = identity.1;
    record
}

#[test]
fn windows_native_cleanup_progress_preserves_marker_until_final_budget() {
    let fixture = tempfile::tempdir().unwrap();
    set_owner_only(fixture.path(), true).unwrap();
    let path = fixture.path().join("owned-root");
    drop(create_owner_only_directory(&path).unwrap());
    let root = record(fixture.path(), &path);
    for index in 0..70 {
        hiroute_platform::durable::write_private_new(&path.join(format!("entry-{index}")), b"x")
            .unwrap();
    }
    assert_eq!(
        delete_native_root_batch(&root, 64),
        NativeDeletionOutcome::Deferred {
            processed_entries: 64
        }
    );
    assert!(path.join(MARKER).exists());
    assert_eq!(
        delete_native_root_batch(&root, 6),
        NativeDeletionOutcome::Deferred {
            processed_entries: 6
        }
    );
    assert!(path.join(MARKER).exists());
    assert_eq!(
        delete_native_root_batch(&root, 1),
        NativeDeletionOutcome::Deferred {
            processed_entries: 0
        }
    );
    assert_eq!(
        delete_native_root_batch(&root, 2),
        NativeDeletionOutcome::Removed {
            processed_entries: 2
        }
    );
    assert!(!path.exists());
}

#[test]
fn windows_native_cleanup_refuses_replaced_marker_without_deleting_payload() {
    let fixture = tempfile::tempdir().unwrap();
    set_owner_only(fixture.path(), true).unwrap();
    let path = fixture.path().join("owned-root");
    drop(create_owner_only_directory(&path).unwrap());
    let root = record(fixture.path(), &path);
    hiroute_platform::durable::write_private_new(&path.join("payload"), b"keep").unwrap();
    let marker = path.join(MARKER);
    std::fs::rename(&marker, path.join("old-marker")).unwrap();
    hiroute_platform::durable::write_private_new(
        &marker,
        &serde_json::to_vec(&root.ownership_marker()).unwrap(),
    )
    .unwrap();
    assert_eq!(
        delete_native_root_batch(&root, 64),
        NativeDeletionOutcome::Blocked {
            kind: DelegationNativeCleanupFailureKindV1::IdentityMismatch,
            processed_entries: 0,
        }
    );
    assert_eq!(std::fs::read(path.join("payload")).unwrap(), b"keep");
}
