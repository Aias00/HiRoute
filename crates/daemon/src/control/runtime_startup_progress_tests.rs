use super::*;

#[test]
fn ordinary_startup_and_reopen_do_not_claim_a_history_upgrade() {
    #[cfg(unix)]
    if crate::test_support::isolated_agent_home(
        "control::runtime::startup_progress_tests::ordinary_startup_and_reopen_do_not_claim_a_history_upgrade",
    ) {
        return;
    }
    let directory = crate::test_support::private_tempdir();
    for _ in 0..2 {
        let (sender, receiver) = std::sync::mpsc::channel();
        let runtime = ProductionControlRuntime::open_inner_with_cpa_and_overrides(
            directory.path(),
            crate::release_catalog::fixture_catalog(),
            None,
            None,
            true,
            RuntimeOpenOverrides {
                storage_startup: hiroute_local_storage::StorageStartupOptions {
                    upgrade_progress: Some(sender),
                    ..Default::default()
                },
                ..Default::default()
            },
        )
        .unwrap();
        assert!(
            receiver.try_recv().is_err(),
            "ordinary startup must retain its normal readiness deadline"
        );
        drop(runtime);
    }
}
