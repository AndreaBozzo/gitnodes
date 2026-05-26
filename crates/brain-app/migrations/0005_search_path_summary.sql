-- Schema v5: make search resilient to real Brain naming.
-- Path is indexed so searches like "pikachu" can find files whose title/body
-- do not repeat the slug; summary is indexed for graph-derived descriptions.

DROP TABLE IF EXISTS node_search_fts;

CREATE VIRTUAL TABLE node_search_fts USING fts5(
    target_id UNINDEXED,
    node_id UNINDEXED,
    path,
    node_type UNINDEXED,
    title,
    tags,
    summary,
    body_text,
    tokenize = 'unicode61'
);

INSERT INTO node_search_fts (target_id, node_id, path, node_type, title, tags, summary, body_text)
SELECT
    target_id,
    node_id,
    path,
    node_type,
    title,
    tags_json,
    summary,
    COALESCE(body_text, '')
FROM nodes
WHERE is_virtual = 0
  AND path <> '';
