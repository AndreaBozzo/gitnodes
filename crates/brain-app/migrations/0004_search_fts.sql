-- Schema v4: target-scoped full-text search over projected Brain nodes.
-- The table is a derived read model rebuilt from `nodes` alongside the rest of
-- the projection; Git remains the source of truth.

CREATE VIRTUAL TABLE IF NOT EXISTS node_search_fts USING fts5(
    target_id UNINDEXED,
    node_id UNINDEXED,
    path UNINDEXED,
    node_type UNINDEXED,
    title,
    tags,
    body_text,
    tokenize = 'unicode61'
);

INSERT INTO node_search_fts (target_id, node_id, path, node_type, title, tags, body_text)
SELECT
    target_id,
    node_id,
    path,
    node_type,
    title,
    tags_json,
    COALESCE(body_text, '')
FROM nodes
WHERE is_virtual = 0
  AND path <> '';
