-- Schema v3: explicit content-hash signatures for drift detection.
-- Existing `sha` remains the UI/editor-facing file version; `blob_sha` is
-- the projection's Git-tree comparison key for future incremental rebuilds.

ALTER TABLE files ADD COLUMN blob_sha TEXT;
ALTER TABLE nodes ADD COLUMN blob_sha TEXT;

CREATE INDEX idx_files_target_blob_sha ON files(target_id, blob_sha);
CREATE INDEX idx_nodes_target_blob_sha ON nodes(target_id, blob_sha);
