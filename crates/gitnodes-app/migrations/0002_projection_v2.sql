-- Schema v2: body + frontmatter columns feed the upcoming FTS5 index and
-- richer admin/search; node_authors is the substrate for Activity Stream;
-- last_rebuild_duration_ms makes rebuild cost visible in admin status.

ALTER TABLE files ADD COLUMN body_text TEXT;
ALTER TABLE files ADD COLUMN frontmatter_json TEXT;

ALTER TABLE nodes ADD COLUMN body_text TEXT;
ALTER TABLE nodes ADD COLUMN frontmatter_json TEXT;

CREATE TABLE node_authors (
    target_id INTEGER NOT NULL,
    node_id INTEGER NOT NULL,
    author TEXT NOT NULL,
    role TEXT NOT NULL DEFAULT 'author',
    PRIMARY KEY (target_id, node_id, author, role),
    FOREIGN KEY(target_id) REFERENCES targets(id) ON DELETE CASCADE,
    FOREIGN KEY(target_id, node_id) REFERENCES nodes(target_id, node_id) ON DELETE CASCADE
);

CREATE INDEX idx_node_authors_author ON node_authors(target_id, author);

ALTER TABLE projection_sync_state ADD COLUMN last_rebuild_duration_ms INTEGER;
