use super::*;

#[test]
fn blob_made_live_after_gc_candidate_discovery_keeps_its_file_and_metadata() {
    let root = tempfile::tempdir().unwrap();
    let store =
        LocalObservationStore::open(root.path(), crate::DigestAuthority::new([2; 32])).unwrap();
    let digest = crate::query_v2::search_tests::source(&store, b"captured content");
    store
        .connection
        .lock()
        .execute("UPDATE content_instances_v2 SET state='installing'", [])
        .unwrap();
    let candidates = store.garbage_candidates(32).unwrap();
    assert_eq!(candidates.len(), 1);
    // Reproduce a capture publication between the read snapshot and removal.
    store
        .connection
        .lock()
        .execute("UPDATE content_instances_v2 SET state='complete'", [])
        .unwrap();
    assert_eq!(
        store.collect_unreferenced_candidates(&candidates).unwrap(),
        0
    );
    assert_eq!(
        std::fs::read(root.path().join("indexed-source")).unwrap(),
        b"captured content"
    );
    assert!(
        store
            .connection
            .lock()
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM content_blobs_v2 WHERE blob_digest=?1)",
                [digest],
                |row| row.get::<_, bool>(0)
            )
            .unwrap()
    );
}
