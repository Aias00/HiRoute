-- Frozen DDL from v0.3.1, 13856abd873da5b4373d39322493b4cc105c4867.
-- crates/observation/src/text_index/mod.rs, first migration statement.
CREATE TABLE IF NOT EXISTS observation_text_index_v2(
            workspace TEXT NOT NULL,digest TEXT NOT NULL,state TEXT NOT NULL,published INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY(workspace,digest));
         CREATE TABLE IF NOT EXISTS observation_text_blocks_v2(
            workspace TEXT NOT NULL,digest TEXT NOT NULL,ordinal INTEGER NOT NULL,
            original_start INTEGER NOT NULL,primary_start INTEGER NOT NULL,
            folded TEXT NOT NULL,offsets BLOB NOT NULL,original TEXT NOT NULL,
            UNIQUE(workspace,digest,ordinal));
         CREATE INDEX IF NOT EXISTS observation_text_blocks_source ON observation_text_blocks_v2(workspace,digest,ordinal);
         CREATE INDEX IF NOT EXISTS observation_text_index_published ON observation_text_index_v2(published);
