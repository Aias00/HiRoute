//! Public deletion + analysis regression. SQL cases model an unaware writer;
//! exact released-binary upgrade acceptance remains separate evidence.
use super::*;
use rusqlite::params;

fn read(store: &LocalObservationStore, q: ObservationAnalysisQueryV1) -> ObservationAnalysisPageV1 {
    read_at(store, q, 20_000)
}

fn read_at(
    store: &LocalObservationStore,
    mut q: ObservationAnalysisQueryV1,
    now: i64,
) -> ObservationAnalysisPageV1 {
    q.currency = q.view.monetary().then(|| "USD".into());
    q.valuation_kind = q
        .view
        .monetary()
        .then_some(PriceValuationKindV1::UsageEstimate);
    let reader = ObservationReaderContext::local_user(
        WorkspaceId::default(),
        "delete-analysis".into(),
        1,
        now + 1_000_000,
        true,
        true,
    )
    .unwrap();
    let range = AnalysisTimeRangeV1 {
        requested_from_ms: 0,
        requested_to_ms: now,
        effective_from_ms: 0,
        effective_to_ms: now,
        from_utc_offset_seconds: 0,
        to_utc_offset_seconds: 0,
        clipped_for_retention: false,
    };
    let page = store
        .observed_analysis(&reader, &q, Some(&range), None, now)
        .unwrap();
    assert!(
        matches!(
            page.status,
            AnalysisStatusV1::Ready | AnalysisStatusV1::Partial
        ),
        "{page:?}"
    );
    page
}

fn check(store: &LocalObservationStore, groups: usize, requests: u64, gap: bool) {
    let conversations = read(
        store,
        ObservationAnalysisQueryV1::new(AnalysisViewV1::Conversations),
    );
    assert_eq!(
        conversations.conversations.len(),
        groups,
        "{conversations:?}"
    );
    assert_eq!(
        conversations
            .conversations
            .iter()
            .map(|c| c.request_count)
            .sum::<u64>(),
        requests
    );
    if gap {
        assert_eq!(conversations.status, AnalysisStatusV1::Partial);
        assert!(
            conversations
                .conversations
                .iter()
                .any(|c| c.lineage_partial),
            "{conversations:?}"
        );
    }
    for group in [AnalysisCostGroupV1::Session, AnalysisCostGroupV1::Request] {
        let mut q = ObservationAnalysisQueryV1::new(AnalysisViewV1::CostRanking);
        q.group_by = Some(group);
        let costs = read(store, q);
        assert_eq!(
            costs.costs.iter().map(|c| c.request_count).sum::<u64>(),
            requests,
            "{costs:?}"
        );
        assert_eq!(
            costs
                .costs
                .iter()
                .filter_map(|c| c.known_sum_micros)
                .sum::<u64>(),
            requests * 117,
            "{costs:?}"
        );
    }
}

fn chain(store: &LocalObservationStore) -> [Fixture; 3] {
    let a = native(store, "delete-joint-a");
    let b = native(store, "delete-joint-b");
    let c = native(store, "delete-joint-c");
    store.settle_pending_valuations(16).unwrap();
    store
        .ingest_native_continuation(&event(
            &c,
            vec![
                a.session.to_string(),
                b.session.to_string(),
                c.session.to_string(),
            ],
            1,
        ))
        .unwrap();
    check(store, 1, 3, false);
    [a, b, c]
}

fn delete(
    store: &LocalObservationStore,
    f: &Fixture,
    class: DeletionDataClass,
    rollups: bool,
    at: i64,
) {
    let principal = ObservationPrincipalV1::local_user(f.workspace.clone());
    let spec = SessionDeletionSpecV1 {
        workspace_id: f.workspace.clone(),
        session_id: f.session.clone(),
        data_class: class,
        delete_rollups: rollups,
    };
    let p = store.preview_session_deletion(&principal, &spec).unwrap();
    store
        .apply_session_deletion(&principal, &spec, p.store_revision, &p.change_digest, at)
        .unwrap();
}

fn unaware_delete(store: &LocalObservationStore, f: &Fixture, full: bool, rollups: bool) {
    // Source deletion shape from v0.3.1 13856abd. No analysis cleanup, new Rust
    // deletion helper or candidate-native serialization participates.
    let mut db = store.connection.lock();
    let tx = db.transaction().unwrap();
    tx.execute("INSERT INTO observation_session_barriers_v2 VALUES(?1,?2,2000) ON CONFLICT(workspace_id,session_id) DO UPDATE SET through_ms=MAX(through_ms,excluded.through_ms)",params![f.workspace.as_str(),f.session.as_str()]).unwrap();
    tx.execute("INSERT OR IGNORE INTO observation_request_tombstones_v2 SELECT workspace_id,request_id,2000 FROM logical_requests WHERE workspace_id=?1 AND session_id=?2",params![f.workspace.as_str(),f.session.as_str()]).unwrap();
    if full {
        for table in [
            "valuation_attempt_inputs_v2",
            "valuation_pending_v2",
            "valuation_records_v2",
            "valuation_contributions_v2",
            "valuation_requests_v2",
        ] {
            tx.execute(&format!("DELETE FROM {table} WHERE workspace_id=?1 AND request_id IN(SELECT request_id FROM logical_requests WHERE workspace_id=?1 AND session_id=?2)"),params![f.workspace.as_str(),f.session.as_str()]).unwrap();
        }
        tx.execute(
            "DELETE FROM logical_requests WHERE workspace_id=?1 AND session_id=?2",
            params![f.workspace.as_str(), f.session.as_str()],
        )
        .unwrap();
    }
    if rollups {
        tx.execute(
            "DELETE FROM observation_tombstones WHERE workspace_id=?1 AND session_id=?2",
            params![f.workspace.as_str(), f.session.as_str()],
        )
        .unwrap();
        if full {
            tx.execute(
                "DELETE FROM sessions WHERE workspace_id=?1 AND session_id=?2",
                params![f.workspace.as_str(), f.session.as_str()],
            )
            .unwrap();
        }
    } else {
        tx.execute(
            "INSERT OR REPLACE INTO observation_tombstones VALUES(?1,?2,'user_deleted',?3,2000)",
            params![
                f.workspace.as_str(),
                f.session.as_str(),
                if full {
                    "facts_and_content"
                } else {
                    "content_only"
                }
            ],
        )
        .unwrap();
    }
    tx.execute(
        "UPDATE observation_meta SET value=CAST(value AS INTEGER)+1 WHERE key='store_revision'",
        [],
    )
    .unwrap();
    tx.commit().unwrap();
}

fn new_request(store: &LocalObservationStore, session: &SessionId, suffix: &str) -> Fixture {
    new_request_at(store, session, suffix, 5000)
}

fn new_request_at(
    store: &LocalObservationStore,
    session: &SessionId,
    suffix: &str,
    offset: u64,
) -> Fixture {
    let mut f = Fixture::new(suffix);
    f.session = session.clone();
    for mut e in facts(&f, UsageFrameKindV1::Cumulative) {
        e.occurred_at_unix_nanos += offset * 1_000_000;
        store.ingest_fact(&e, &[]).unwrap();
    }
    store.settle_pending_valuations(16).unwrap();
    f
}

#[test]
fn actual_expiry_leaves_a_neutral_gap_without_fabricating_a_full_delete() {
    let dir = tempfile::tempdir().unwrap();
    let store = open_store(dir.path());
    let a = new_request_at(
        &store,
        &Fixture::new("expiry-a").session,
        "expiry-a",
        7_200_000,
    );
    let b = native(&store, "expiry-b");
    let c = new_request_at(
        &store,
        &Fixture::new("expiry-c").session,
        "expiry-c",
        7_200_000,
    );
    store.settle_pending_valuations(16).unwrap();
    let mut proof = event(
        &c,
        vec![
            a.session.to_string(),
            b.session.to_string(),
            c.session.to_string(),
        ],
        1,
    );
    proof.ingress_at_ms = 8_000_000;
    store.ingest_native_continuation(&proof).unwrap();
    let now = SEVEN_DAYS_MILLIS + 200;
    assert_eq!(store.expire_request_details(now, 200).unwrap(), 1);
    for phase in 0..2 {
        if phase == 1 {
            store.reconcile_analysis_batch(now).unwrap();
        }
        let p = read_at(
            &store,
            ObservationAnalysisQueryV1::new(AnalysisViewV1::Conversations),
            now,
        );
        assert_eq!(p.conversations.len(), 2);
        assert!(
            p.conversations
                .iter()
                .flat_map(|c| &c.members)
                .any(|m| m.gap_before.as_deref() == Some("predecessor_unavailable"))
        );
        let p = read_at(
            &store,
            ObservationAnalysisQueryV1::new(AnalysisViewV1::CostRanking),
            now,
        );
        assert_eq!(
            p.costs
                .iter()
                .filter_map(|c| c.known_sum_micros)
                .sum::<u64>(),
            234
        );
        assert_eq!(
            store
                .connection
                .lock()
                .query_row(
                    "SELECT COUNT(*) FROM observation_analysis_cuts_v1",
                    [],
                    |r| r.get::<_, u64>(0)
                )
                .unwrap(),
            0
        );
    }
}

#[test]
fn public_deletion_matrix_keeps_grouping_and_cost_in_sync_before_and_after_catchup() {
    for class in [
        DeletionDataClass::ContentOnly,
        DeletionDataClass::FactsAndContent,
    ] {
        for rollups in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let store = open_store(dir.path());
            let [_, b, _] = chain(&store);
            install_content(&store, &b, b"joint-delete-body", 200);
            delete(&store, &b, class.clone(), rollups, 2000);
            let full = class == DeletionDataClass::FactsAndContent;
            check(
                &store,
                if full { 2 } else { 1 },
                if full { 2 } else { 3 },
                full,
            );
            store.reconcile_analysis_batch(20_000).unwrap();
            check(
                &store,
                if full { 2 } else { 1 },
                if full { 2 } else { 3 },
                full,
            );
        }
    }
}

#[test]
fn pinned_conversation_and_cost_scopes_preserve_content_only_and_reject_full_deletion() {
    for full in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let store = open_store(dir.path());
        let [_, b, _] = chain(&store);
        let initial = read(
            &store,
            ObservationAnalysisQueryV1::new(AnalysisViewV1::Conversations),
        );
        let reader = ObservationReaderContext::local_user(
            WorkspaceId::default(),
            "delete-analysis".into(),
            1,
            1_020_000,
            true,
            true,
        )
        .unwrap();
        let mut costs = ObservationAnalysisQueryV1::new(AnalysisViewV1::CostRanking);
        costs.currency = Some("USD".into());
        costs.valuation_kind = Some(PriceValuationKindV1::UsageEstimate);
        costs.analysis_scope = initial.analysis_scope.clone();
        assert_eq!(
            store
                .observed_analysis(&reader, &costs, None, None, 20_000)
                .unwrap()
                .costs[0]
                .known_sum_micros,
            Some(351)
        );
        delete(
            &store,
            &b,
            if full {
                DeletionDataClass::FactsAndContent
            } else {
                DeletionDataClass::ContentOnly
            },
            false,
            2000,
        );
        for phase in 0..2 {
            if phase == 1 {
                store.reconcile_analysis_batch(20_000).unwrap();
            }
            let mut conversations = ObservationAnalysisQueryV1::new(AnalysisViewV1::Conversations);
            conversations.analysis_scope = initial.analysis_scope.clone();
            for q in [&conversations, &costs] {
                let result = store.observed_analysis(&reader, q, None, None, 20_000);
                if full {
                    assert!(
                        matches!(result, Err(crate::query_v2::ObservationV2Error::Stale)),
                        "{result:?}"
                    );
                } else {
                    let p = result.unwrap();
                    assert_eq!(p.status, AnalysisStatusV1::Ready);
                    if q.view.monetary() {
                        assert_eq!(p.costs[0].known_sum_micros, Some(351));
                    } else {
                        assert_eq!(p.conversations[0].request_count, 3);
                    }
                }
            }
        }
    }
}

#[test]
fn unaware_deletion_scopes_and_erased_tombstones_cannot_revive_a_deleted_bridge() {
    for full in [false, true] {
        for rollups in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let store = open_store(dir.path());
            let [a, b, c] = chain(&store);
            unaware_delete(&store, &b, full, rollups);
            if full && !rollups {
                unaware_delete(&store, &b, false, false);
            }
            check(
                &store,
                if full { 2 } else { 1 },
                if full { 2 } else { 3 },
                full,
            );
            store.reconcile_analysis_batch(20_000).unwrap();
            check(
                &store,
                if full { 2 } else { 1 },
                if full { 2 } else { 3 },
                full,
            );
            if full {
                let revived = new_request(&store, &b.session, "unaware-b-revived");
                store
                    .ingest_native_continuation(&event(
                        &revived,
                        vec![a.session.to_string(), b.session.to_string()],
                        20,
                    ))
                    .unwrap();
                store
                    .ingest_native_continuation(&event(
                        &c,
                        vec![
                            a.session.to_string(),
                            b.session.to_string(),
                            c.session.to_string(),
                        ],
                        21,
                    ))
                    .unwrap();
                check(&store, 3, 3, true);
                store.reconcile_analysis_batch(20_000).unwrap();
                check(&store, 3, 3, true);
            }
        }
    }
}

#[test]
fn a_proven_new_child_can_follow_new_requests_without_reconnecting_deleted_history() {
    let dir = tempfile::tempdir().unwrap();
    let store = open_store(dir.path());
    let [a, b, c] = chain(&store);
    delete(&store, &b, DeletionDataClass::FactsAndContent, false, 2000);
    delete(&store, &b, DeletionDataClass::ContentOnly, false, 3000);
    let revived = new_request(&store, &b.session, "new-parent-request");
    let d = new_request(&store, &Fixture::new("new-child").session, "new-child");
    let mut proof = event(
        &d,
        vec![
            a.session.to_string(),
            b.session.to_string(),
            d.session.to_string(),
        ],
        30,
    );
    proof.ingress_at_ms = 10_000;
    proof.proof_observation.as_mut().unwrap().observed_at_ms = 10_000;
    proof.current_state = Some(NativeCurrentStateV1 {
        value: NativeSessionStateV1::Open,
        observation: NativeProofObservationV1 {
            observer_id: "native-observer".into(),
            sequence: 30,
            observed_at_ms: 10_000,
            age_ms: 0,
        },
        started_at_ms: Some(5000),
    });
    store.ingest_native_continuation(&proof).unwrap();
    check(&store, 3, 4, true);
    let p = read(
        &store,
        ObservationAnalysisQueryV1::new(AnalysisViewV1::Conversations),
    );
    let g = p
        .conversations
        .iter()
        .find(|g| g.analysis_session_id == b.session.as_str())
        .unwrap();
    assert_eq!(g.request_count, 2);
    assert_eq!(g.member_count, 2);
    assert!(g.members.iter().any(|m| m.session_id == d.session.as_str()));
    assert!(
        !g.members
            .iter()
            .any(|m| m.session_id == a.session.as_str() || m.session_id == c.session.as_str())
    );
    let bodies: Vec<String> = store
        .connection
        .lock()
        .prepare("SELECT body FROM observation_native_events_v1")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert!(
        bodies.iter().all(|b| !b.contains("started_at_ms")),
        "strict v1 bodies stay unchanged"
    );
    store.reconcile_analysis_batch(20_000).unwrap();
    check(&store, 3, 4, true);
    drop(store);
    let store = open_store(dir.path());
    check(&store, 3, 4, true);
    let replay = event(
        &revived,
        vec![a.session.to_string(), b.session.to_string()],
        40,
    );
    store.ingest_native_continuation(&replay).unwrap();
    check(&store, 3, 4, true);
}

#[test]
fn unknown_old_near_cut_or_skewed_origin_is_not_new_compression_proof() {
    for (birth, observed) in [
        (None, 10_000),
        (Some(1000), 10_000),
        (Some(2500), 10_000),
        (Some(5000), 50_000),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let store = open_store(dir.path());
        let [_, b, _] = chain(&store);
        delete(&store, &b, DeletionDataClass::FactsAndContent, false, 2000);
        new_request(&store, &b.session, "origin-parent");
        let d = new_request(
            &store,
            &Fixture::new("origin-child").session,
            "origin-child",
        );
        let mut proof = event(&d, vec![b.session.to_string(), d.session.to_string()], 50);
        proof.ingress_at_ms = 10_000;
        proof.proof_observation.as_mut().unwrap().observed_at_ms = 10_000;
        proof.current_state = Some(NativeCurrentStateV1 {
            value: NativeSessionStateV1::Open,
            observation: NativeProofObservationV1 {
                observer_id: "origin-probe".into(),
                sequence: 50,
                observed_at_ms: observed,
                age_ms: 0,
            },
            started_at_ms: birth,
        });
        store.ingest_native_continuation(&proof).unwrap();
        check(&store, 4, 4, true);
        store.reconcile_analysis_batch(20_000).unwrap();
        check(&store, 4, 4, true);
    }
}

#[test]
fn a_reused_native_id_with_older_retained_requests_is_not_a_new_child() {
    let dir = tempfile::tempdir().unwrap();
    let store = open_store(dir.path());
    let [_, b, c] = chain(&store);
    delete(&store, &b, DeletionDataClass::FactsAndContent, false, 2000);
    new_request(&store, &b.session, "reused-id-parent");
    let reused = new_request(&store, &c.session, "reused-id-child");
    let mut proof = event(
        &reused,
        vec![b.session.to_string(), c.session.to_string()],
        60,
    );
    proof.ingress_at_ms = 10_000;
    proof.proof_observation.as_mut().unwrap().observed_at_ms = 10_000;
    proof.current_state = Some(NativeCurrentStateV1 {
        value: NativeSessionStateV1::Open,
        observation: NativeProofObservationV1 {
            observer_id: "reused-id".into(),
            sequence: 60,
            observed_at_ms: 10_000,
            age_ms: 0,
        },
        started_at_ms: Some(5000),
    });
    store.ingest_native_continuation(&proof).unwrap();
    for phase in 0..2 {
        if phase == 1 {
            store.reconcile_analysis_batch(20_000).unwrap();
        }
        check(&store, 3, 4, true);
        let p = read(
            &store,
            ObservationAnalysisQueryV1::new(AnalysisViewV1::Conversations),
        );
        assert_eq!(
            p.conversations
                .iter()
                .find(|g| g.analysis_session_id == b.session.as_str())
                .unwrap()
                .request_count,
            1
        );
    }
}

#[test]
fn later_source_evidence_rechecks_a_previously_admitted_native_origin() {
    let dir = tempfile::tempdir().unwrap();
    let store = open_store(dir.path());
    let [_, b, _] = chain(&store);
    delete(&store, &b, DeletionDataClass::FactsAndContent, false, 2000);
    new_request(&store, &b.session, "late-origin-parent");
    let d = new_request(
        &store,
        &Fixture::new("late-origin-child").session,
        "late-origin-child",
    );
    let mut proof = event(&d, vec![b.session.to_string(), d.session.to_string()], 70);
    proof.ingress_at_ms = 10_000;
    proof.proof_observation.as_mut().unwrap().observed_at_ms = 10_000;
    proof.current_state = Some(NativeCurrentStateV1 {
        value: NativeSessionStateV1::Open,
        observation: NativeProofObservationV1 {
            observer_id: "late-origin".into(),
            sequence: 70,
            observed_at_ms: 10_000,
            age_ms: 0,
        },
        started_at_ms: Some(5000),
    });
    store.ingest_native_continuation(&proof).unwrap();
    check(&store, 3, 4, true);
    let pinned = read(
        &store,
        ObservationAnalysisQueryV1::new(AnalysisViewV1::Conversations),
    );
    // Delayed retained evidence can contradict a physical-ID creation claim.
    // Rechecking live sources must not wait for the next native lookup or GC.
    new_request_at(&store, &d.session, "late-origin-older-request", 0);
    let reader = ObservationReaderContext::local_user(
        WorkspaceId::default(),
        "delete-analysis".into(),
        1,
        1_020_000,
        true,
        true,
    )
    .unwrap();
    let mut q = ObservationAnalysisQueryV1::new(AnalysisViewV1::Conversations);
    q.analysis_scope = pinned.analysis_scope;
    assert!(matches!(
        store.observed_analysis(&reader, &q, None, None, 20_000),
        Err(crate::query_v2::ObservationV2Error::Stale)
    ));
    check(&store, 4, 5, true);
    store.reconcile_analysis_batch(20_000).unwrap();
    check(&store, 4, 5, true);
}

#[test]
fn a_fresh_creation_sample_cannot_upgrade_an_older_cached_relation_proof() {
    let dir = tempfile::tempdir().unwrap();
    let store = open_store(dir.path());
    let [_, b, _] = chain(&store);
    delete(&store, &b, DeletionDataClass::FactsAndContent, false, 2000);
    new_request(&store, &b.session, "cached-origin-parent");
    let d = new_request(
        &store,
        &Fixture::new("cached-origin-child").session,
        "cached-origin-child",
    );
    let mut own = event(&d, vec![d.session.to_string()], 80);
    own.complete = false;
    own.ingress_at_ms = 10_000;
    own.proof_observation.as_mut().unwrap().observed_at_ms = 10_000;
    own.current_state = Some(NativeCurrentStateV1 {
        value: NativeSessionStateV1::Open,
        observation: NativeProofObservationV1 {
            observer_id: "cached-origin".into(),
            sequence: 80,
            observed_at_ms: 10_000,
            age_ms: 0,
        },
        started_at_ms: Some(5000),
    });
    store.ingest_native_continuation(&own).unwrap();
    let mut cached = event(&d, vec![b.session.to_string(), d.session.to_string()], 81);
    cached.ingress_at_ms = 10_000;
    cached.current_state = own.current_state;
    store.ingest_native_continuation(&cached).unwrap();
    check(&store, 4, 4, true);
    store.reconcile_analysis_batch(20_000).unwrap();
    check(&store, 4, 4, true);
    let mut verified = event(&d, vec![b.session.to_string(), d.session.to_string()], 82);
    verified.ingress_at_ms = 11_000;
    verified.proof_observation.as_mut().unwrap().observed_at_ms = 10_000;
    store.ingest_native_continuation(&verified).unwrap();
    check(&store, 3, 4, true);
}
