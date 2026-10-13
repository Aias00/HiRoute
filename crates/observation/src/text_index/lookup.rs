//! Literal substring lookup, including one/two-character queries. A complete
//! encoded gram is exact; longer needles require folded substring verification.
use crate::LocalObservationStore;
use hiroute_domain::ObservationQueryError as Error;
use rusqlite::{Connection, OptionalExtension, params};
use std::{collections::BTreeSet, time::Instant};

pub(super) fn migrate(db: &Connection) -> rusqlite::Result<()> {
    // The pinned bundled SQLite supports contentless deletion. No duplicate
    // body/gram text is retained; block rowids are the lookup document identity.
    db.execute_batch(
        "CREATE VIRTUAL TABLE IF NOT EXISTS observation_text_lookup_v1 USING
           fts5(grams,content='',contentless_delete=1,detail=none,tokenize='ascii');
         CREATE TABLE IF NOT EXISTS observation_text_lookup_sources_v1(
           workspace TEXT NOT NULL,digest TEXT NOT NULL,PRIMARY KEY(workspace,digest));
         CREATE TRIGGER IF NOT EXISTS observation_text_lookup_delete_block
           AFTER DELETE ON observation_text_blocks_v2 BEGIN
             DELETE FROM observation_text_lookup_v1 WHERE rowid=OLD.rowid;
           END;
         CREATE TRIGGER IF NOT EXISTS observation_text_lookup_delete_source
           AFTER DELETE ON observation_text_index_v2 BEGIN
             DELETE FROM observation_text_lookup_sources_v1
               WHERE workspace=OLD.workspace AND digest=OLD.digest;
           END;",
    )
}

fn grams(text: &str, sizes: std::ops::RangeInclusive<usize>) -> BTreeSet<&str> {
    let mut boundaries: Vec<_> = text.char_indices().map(|(at, _)| at).collect();
    boundaries.push(text.len());
    let mut result = BTreeSet::new();
    for size in sizes {
        for window in boundaries.windows(size + 1) {
            result.insert(&text[window[0]..window[size]]);
        }
    }
    result
}

fn encode(grams: BTreeSet<&str>, separator: &str) -> String {
    use std::fmt::Write;
    let mut result = String::new();
    for gram in grams {
        if !result.is_empty() {
            result.push_str(separator);
        }
        result.push('g');
        for byte in gram.bytes() {
            // ASCII hex cannot introduce FTS operators, token boundaries or a
            // different Unicode tokenizer interpretation.
            write!(result, "{byte:02x}").expect("String write");
        }
    }
    result
}

pub(crate) fn expression(needle: &str) -> String {
    let size = needle.chars().count().min(3);
    encode(grams(needle, size..=size), " AND ")
}

pub(super) fn insert(db: &Connection, block: i64, folded: &str) -> rusqlite::Result<()> {
    db.execute(
        "INSERT OR REPLACE INTO observation_text_lookup_v1(rowid,grams) VALUES(?1,?2)",
        params![block, encode(grams(folded, 1..=3), " ")],
    )?;
    Ok(())
}

#[derive(Default)]
pub(super) struct Backfill {
    after: i64,
    source: Option<(String, String, u64)>,
    complete: bool,
}

impl Backfill {
    /// Consume at most half a maintenance cycle so existing caches cannot
    /// starve indexing of newly captured bodies. Publication is per source.
    pub(super) fn cycle(
        &mut self,
        store: &LocalObservationStore,
        started: Instant,
    ) -> Result<usize, Error> {
        if self.complete {
            return Ok(0);
        }
        let mut bytes = 0;
        while bytes < 4 * 1024 * 1024 && started.elapsed().as_millis() < 250 {
            let mut db = store.connection.lock();
            if self.source.is_none() {
                let through: Option<i64> = db.query_row(
                    "SELECT MAX(rowid) FROM (SELECT rowid FROM observation_text_index_v2 WHERE rowid>?1 ORDER BY rowid LIMIT 256)",
                    [self.after], |r| r.get(0),
                ).map_err(|_| Error::Unavailable)?;
                let Some(through) = through else {
                    // Current source publication already supplies its lookup.
                    // Once released caches are backfilled, do not rescan ready
                    // history on every idle maintenance tick.
                    self.complete = true;
                    break;
                };
                let source: Option<(i64, String, String)> = db.query_row(
                    "SELECT rowid,workspace,digest FROM observation_text_index_v2 i WHERE rowid>?1 AND rowid<=?2 AND state='ready'
                       AND NOT EXISTS(SELECT 1 FROM observation_text_lookup_sources_v1 k WHERE k.workspace=i.workspace AND k.digest=i.digest)
                     ORDER BY rowid LIMIT 1",
                    params![self.after, through], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                ).optional().map_err(|_| Error::Unavailable)?;
                self.after = source.as_ref().map_or(through, |r| r.0);
                self.source = source.map(|(_, workspace, digest)| (workspace, digest, 0));
                if self.source.is_none() {
                    continue;
                }
            }
            let (workspace, digest, ordinal) = self.source.as_mut().ok_or(Error::Unavailable)?;
            let block: Option<(i64, u64, String)> = db.query_row(
                "SELECT b.rowid,b.ordinal,b.folded FROM observation_text_blocks_v2 b
                   JOIN observation_text_index_v2 i ON i.workspace=b.workspace AND i.digest=b.digest AND i.state='ready'
                 WHERE b.workspace=?1 AND b.digest=?2 AND b.ordinal>=?3 ORDER BY b.ordinal LIMIT 1",
                params![workspace.as_str(), digest.as_str(), *ordinal], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            ).optional().map_err(|_| Error::Unavailable)?;
            if let Some((id, at, folded)) = block {
                bytes += folded.len();
                let tx = db.transaction().map_err(|_| Error::Unavailable)?;
                insert(&tx, id, &folded).map_err(|_| Error::Unavailable)?;
                tx.commit().map_err(|_| Error::Unavailable)?;
                *ordinal = at + 1;
            } else {
                db.execute(
                    "INSERT OR IGNORE INTO observation_text_lookup_sources_v1(workspace,digest)
                       SELECT workspace,digest FROM observation_text_index_v2 WHERE workspace=?1 AND digest=?2 AND state='ready'",
                    params![workspace.as_str(), digest.as_str()],
                ).map_err(|_| Error::Unavailable)?;
                self.source = None;
            }
        }
        Ok(bytes)
    }
}
