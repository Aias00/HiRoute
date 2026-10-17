//! Catalog capture and daemon-owned retrieval. Gateway execution keeps its frozen Plan facts.
use super::*;
use crate::model_feed::ReleaseCatalogSnapshot;

impl LocalControlAdapter {
    pub(super) fn current_catalog(&self) -> Option<Arc<ReleaseCatalogSnapshot>> {
        match &self.model_feed {
            Some(feed) => feed.available_catalog(),
            None => self.release_catalog.as_ref().map(|catalog| {
                Arc::new(ReleaseCatalogSnapshot {
                    catalog: catalog.clone(),
                    generation: 0,
                })
            }),
        }
    }

    fn activate_feed_generation(&self, generation: u64) -> Result<(), String> {
        self.stores_lock()
            .map_err(|error| error.to_string())?
            .control()
            .activate_model_feed_generation(&WorkspaceId::default(), generation)
            .map_err(|error| error.to_string())
    }

    fn refresh_model_feed(&self) -> Result<(), String> {
        let feed = self.model_feed.as_ref().ok_or("feed_unavailable")?;
        let result = feed.refresh(|generation| self.activate_feed_generation(generation));
        // Catalog activation and default price projection use the same captured generation.
        // A failed projection does not authorize a stale price override or change frozen ledgers.
        let _ = self.rebuild_price_snapshot();
        result
    }
}

pub(super) fn start_retrieval(adapter: &Arc<LocalControlAdapter>) {
    let weak = Arc::downgrade(adapter);
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(std::time::Duration::from_secs(15));
            let Some(adapter) = weak.upgrade() else {
                break;
            };
            if adapter.startup_recovery_complete.load(Ordering::Acquire)
                && adapter
                    .model_feed
                    .as_ref()
                    .is_some_and(|feed| feed.automatic_attempt_due())
            {
                let _ = adapter.refresh_model_feed();
            }
        }
    });
}
