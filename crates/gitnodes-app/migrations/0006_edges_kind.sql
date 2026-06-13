PRAGMA foreign_keys = OFF;

CREATE TABLE IF NOT EXISTS edges_new (
    target_id INTEGER NOT NULL,
    from_id INTEGER NOT NULL,
    to_id INTEGER NOT NULL,
    kind TEXT NOT NULL DEFAULT 'body',
    PRIMARY KEY (target_id, from_id, to_id, kind),
    FOREIGN KEY(target_id) REFERENCES targets(id) ON DELETE CASCADE
);

INSERT OR IGNORE INTO edges_new (target_id, from_id, to_id, kind)
SELECT target_id, from_id, to_id, 'body'
FROM edges;

DROP TABLE edges;
ALTER TABLE edges_new RENAME TO edges;

CREATE INDEX IF NOT EXISTS idx_edges_target_from ON edges(target_id, from_id);

PRAGMA foreign_keys = ON;
