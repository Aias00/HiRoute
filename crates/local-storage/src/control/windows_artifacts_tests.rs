//! Real native ACLs and restart recovery; Unix v2/v3/v4 fixtures stay unchanged.
use super::*;
use tempfile::tempdir;

fn intent(store: &ManagedArtifactStore) -> ExternalEffectIntentV1 {
    ExternalEffectIntentV1::from_registered_adapter(
        "agent-setup",
        OwnedEffectKind::AgentArtifact,
        "agents/codex",
        store.current_external_fingerprint("agents/codex").unwrap(),
        json!({"configured": true}),
        0o640,
        false,
    )
    .unwrap()
}

fn operation() -> OperationId {
    OperationId::parse("op_77777777777777777777777777777777").unwrap()
}

fn permissions(path: &Path) -> hiroute_platform::file_security::WindowsPermissions {
    let file = hiroute_platform::windows::security::open_identity(path, false).unwrap();
    hiroute_platform::file_security::capture(&file).unwrap()
}

#[test]
fn windows_artifact_restart_restores_original_content_and_native_acl() {
    let fixture = tempdir().unwrap();
    let root = fixture.path().join("artifacts");
    let restore = fixture.path().join("restore");
    let store =
        ManagedArtifactStore::open(&crate::test_storage_authority(), &root, &restore).unwrap();
    let target = root.join("agents/codex");
    fs::create_dir_all(target.parent().unwrap()).unwrap();
    fs::write(&target, b"before").unwrap();
    let original = permissions(&target);
    let intent = intent(&store);
    let operation = operation();
    let effect = store.apply_artifact(&operation, &intent).unwrap();
    let marker = store
        .load_marker(&operation, intent.effect_id())
        .unwrap()
        .unwrap();
    assert_eq!(marker.schema, "hiroute.managed-artifact-marker/windows-v5");
    assert_eq!(marker.before_windows_permissions.as_ref(), Some(&original));
    assert!(marker.after_windows_permissions.is_some());
    drop(store);
    let store =
        ManagedArtifactStore::open(&crate::test_storage_authority(), &root, &restore).unwrap();
    assert!(matches!(
        store.observe_artifact(&operation, &intent).unwrap(),
        EffectReconciliation::Staged(_)
    ));
    store.activate_artifact(&effect).unwrap();
    drop(store);
    let store =
        ManagedArtifactStore::open(&crate::test_storage_authority(), &root, &restore).unwrap();
    assert!(matches!(
        store.observe_artifact(&operation, &intent).unwrap(),
        EffectReconciliation::Applied(_)
    ));
    assert_eq!(
        store.compensate_artifact(&effect).unwrap(),
        CompensationOutcome::Compensated
    );
    assert_eq!(fs::read(&target).unwrap(), b"before");
    assert_eq!(permissions(&target), original);
}

#[test]
fn windows_acl_change_loses_compensation_ownership_even_with_equal_content() {
    let fixture = tempdir().unwrap();
    let root = fixture.path().join("artifacts");
    let store = ManagedArtifactStore::open(
        &crate::test_storage_authority(),
        &root,
        &fixture.path().join("restore"),
    )
    .unwrap();
    let target = root.join("agents/codex");
    fs::create_dir_all(target.parent().unwrap()).unwrap();
    fs::write(&target, b"before").unwrap();
    let original = permissions(&target);
    let intent = intent(&store);
    let operation = operation();
    let effect = store.apply_artifact(&operation, &intent).unwrap();
    store.activate_artifact(&effect).unwrap();
    assert_ne!(permissions(&target), original);
    let configured = fs::read(&target).unwrap();
    hiroute_platform::durable::write_windows_atomic(&target, &configured, &original).unwrap();
    assert!(matches!(
        store.observe_artifact(&operation, &intent).unwrap(),
        EffectReconciliation::OwnershipLost(_)
    ));
    assert_eq!(
        store.compensate_artifact(&effect).unwrap(),
        CompensationOutcome::OwnershipLost
    );
    assert_eq!(fs::read(&target).unwrap(), configured);
    assert_eq!(permissions(&target), original);
}

#[test]
fn windows_backup_authentication_binds_native_permission_facts() {
    let fixture = tempdir().unwrap();
    let root = fixture.path().join("artifacts");
    let restore = fixture.path().join("restore");
    let store =
        ManagedArtifactStore::open(&crate::test_storage_authority(), &root, &restore).unwrap();
    let target = root.join("agents/codex");
    fs::create_dir_all(target.parent().unwrap()).unwrap();
    fs::write(&target, b"before").unwrap();
    let intent = intent(&store);
    let operation = operation();
    store.apply_artifact(&operation, &intent).unwrap();
    let mut marker = store
        .load_marker(&operation, intent.effect_id())
        .unwrap()
        .unwrap();
    let private = hiroute_platform::file_security::private_defaults(false).unwrap();
    assert_ne!(marker.before_windows_permissions.as_ref(), Some(&private));
    marker.before_windows_permissions = Some(private);
    assert!(store.validated_backup(&marker).is_err());
    store.save_marker(&marker).unwrap();
    drop(store);
    assert!(ManagedArtifactStore::open(&crate::test_storage_authority(), &root, &restore).is_err());
    assert_eq!(fs::read(&target).unwrap(), b"before");
}

#[test]
fn windows_owned_directory_cleanup_preserves_user_additions_and_replacements() {
    let fixture = tempdir().unwrap();
    let target = fixture.path().join("owned/nested/settings.json");
    let created = native_directories::prepare_parent(&target).unwrap();
    let sentinel = target.parent().unwrap().join("user.txt");
    fs::write(&sentinel, b"preserve").unwrap();
    assert!(!native_directories::cleanup(&target, &created).unwrap());
    assert_eq!(fs::read(&sentinel).unwrap(), b"preserve");
    fs::remove_file(&sentinel).unwrap();
    let original = fixture.path().join("owned");
    let moved = fixture.path().join("moved");
    hiroute_platform::durable::rename(&original, &moved).unwrap();
    fs::create_dir_all(target.parent().unwrap()).unwrap();
    fs::write(&sentinel, b"replacement").unwrap();
    assert!(!native_directories::cleanup(&target, &created).unwrap());
    assert_eq!(fs::read(&sentinel).unwrap(), b"replacement");
    assert!(moved.join("nested").is_dir());
}
