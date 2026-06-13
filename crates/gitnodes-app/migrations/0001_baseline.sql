-- Baseline schema: the exact state produced by the pre-versioned
-- `migrate()` function. Rust-side preflight heals older `targets`
-- tables before this file runs; the SQL here can then claim an existing
-- prod DB as version 1 while preserving rows.

CREATE TABLE IF NOT EXISTS targets (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    key TEXT NOT NULL UNIQUE,
    org TEXT NOT NULL,
    repo TEXT NOT NULL,
    branch TEXT NOT NULL,
    registered_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    registered_by TEXT,
    source TEXT NOT NULL DEFAULT 'env_default',
    default_branch TEXT
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_targets_org_repo
    ON targets(org, repo);

CREATE INDEX IF NOT EXISTS idx_targets_key ON targets(key);

CREATE TABLE IF NOT EXISTS projection_sync_state (
    target_id INTEGER PRIMARY KEY,
    status TEXT NOT NULL DEFAULT 'stale',
    last_attempt_at TEXT,
    last_success_at TEXT,
    last_error_at TEXT,
    last_error TEXT,
    last_reason TEXT,
    file_count INTEGER NOT NULL DEFAULT 0,
    node_count INTEGER NOT NULL DEFAULT 0,
    edge_count INTEGER NOT NULL DEFAULT 0,
    FOREIGN KEY(target_id) REFERENCES targets(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS files (
    target_id INTEGER NOT NULL,
    path TEXT NOT NULL,
    sha TEXT NOT NULL,
    size_bytes INTEGER NOT NULL,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (target_id, path),
    FOREIGN KEY(target_id) REFERENCES targets(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_files_target_path ON files(target_id, path);

CREATE TABLE IF NOT EXISTS nodes (
    target_id INTEGER NOT NULL,
    node_id INTEGER NOT NULL,
    title TEXT NOT NULL,
    summary TEXT NOT NULL,
    node_type TEXT NOT NULL,
    tags_json TEXT NOT NULL,
    x REAL NOT NULL,
    y REAL NOT NULL,
    path TEXT NOT NULL,
    sha TEXT NOT NULL,
    is_virtual INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (target_id, node_id),
    FOREIGN KEY(target_id) REFERENCES targets(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_nodes_target_path ON nodes(target_id, path);

CREATE TABLE IF NOT EXISTS edges (
    target_id INTEGER NOT NULL,
    from_id INTEGER NOT NULL,
    to_id INTEGER NOT NULL,
    PRIMARY KEY (target_id, from_id, to_id),
    FOREIGN KEY(target_id) REFERENCES targets(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_edges_target_from ON edges(target_id, from_id);

CREATE TABLE IF NOT EXISTS backlinks (
    target_id INTEGER NOT NULL,
    source_path TEXT NOT NULL,
    target_path TEXT NOT NULL,
    PRIMARY KEY (target_id, source_path, target_path),
    FOREIGN KEY(target_id) REFERENCES targets(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_backlinks_target_target_path
    ON backlinks(target_id, target_path);

CREATE TABLE IF NOT EXISTS work_items (
    target_id INTEGER NOT NULL,
    brain_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    title TEXT NOT NULL,
    state TEXT NOT NULL,
    labels_json TEXT NOT NULL DEFAULT '[]',
    assignees_json TEXT NOT NULL DEFAULT '[]',
    content_path TEXT,
    system_of_record TEXT NOT NULL DEFAULT 'brain',
    PRIMARY KEY (target_id, brain_id),
    FOREIGN KEY(target_id) REFERENCES targets(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_work_items_target_content_path
    ON work_items(target_id, content_path);

CREATE TABLE IF NOT EXISTS work_item_bindings (
    target_id INTEGER NOT NULL,
    brain_id TEXT NOT NULL,
    system TEXT NOT NULL,
    project TEXT NOT NULL,
    item_key TEXT NOT NULL,
    provider_id TEXT,
    url TEXT,
    PRIMARY KEY (target_id, brain_id),
    FOREIGN KEY(target_id) REFERENCES targets(id) ON DELETE CASCADE,
    FOREIGN KEY(target_id, brain_id) REFERENCES work_items(target_id, brain_id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS pending_provider_sync (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    target_id INTEGER NOT NULL,
    brain_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    attempts INTEGER NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    last_attempt_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    last_error TEXT,
    UNIQUE(target_id, brain_id, kind),
    FOREIGN KEY(target_id) REFERENCES targets(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS audit_events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    ts TEXT NOT NULL DEFAULT (datetime('now')),
    kind TEXT NOT NULL,
    actor TEXT,
    detail TEXT
);

CREATE INDEX IF NOT EXISTS idx_audit_ts ON audit_events(ts DESC);

-- Heal legacy rows where the registration-metadata ALTER landed without a
-- backfill. No-op on fresh DBs (column defaults to NULL on new rows but
-- inserts always supply branch).
UPDATE targets SET default_branch = branch WHERE default_branch IS NULL;
