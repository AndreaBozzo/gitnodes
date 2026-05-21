use sqlx::SqlitePool;

pub async fn migrate(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    sqlx::query("PRAGMA foreign_keys = ON")
        .execute(pool)
        .await?;
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS targets (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            key TEXT NOT NULL UNIQUE,
            org TEXT NOT NULL,
            repo TEXT NOT NULL,
            branch TEXT NOT NULL
        )",
    )
    .execute(pool)
    .await?;
    // 3.7B-alpha: extend `targets` with registration metadata so a
    // deterministic sticky branch can be persisted at first sighting.
    add_column_if_missing(
        pool,
        MigratableTable::Targets,
        ColumnSpec {
            name: "registered_at",
            spec: "TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP",
        },
    )
    .await?;
    add_column_if_missing(
        pool,
        MigratableTable::Targets,
        ColumnSpec {
            name: "registered_by",
            spec: "TEXT",
        },
    )
    .await?;
    add_column_if_missing(
        pool,
        MigratableTable::Targets,
        ColumnSpec {
            name: "source",
            spec: "TEXT NOT NULL DEFAULT 'env_default'",
        },
    )
    .await?;
    add_column_if_missing(
        pool,
        MigratableTable::Targets,
        ColumnSpec {
            name: "default_branch",
            spec: "TEXT",
        },
    )
    .await?;
    sqlx::query("UPDATE targets SET default_branch = branch WHERE default_branch IS NULL")
        .execute(pool)
        .await?;
    sqlx::query(
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_targets_org_repo
            ON targets(org, repo)",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS projection_sync_state (
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
        )",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS files (
            target_id INTEGER NOT NULL,
            path TEXT NOT NULL,
            sha TEXT NOT NULL,
            size_bytes INTEGER NOT NULL,
            updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            PRIMARY KEY (target_id, path),
            FOREIGN KEY(target_id) REFERENCES targets(id) ON DELETE CASCADE
        )",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS nodes (
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
        )",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS edges (
            target_id INTEGER NOT NULL,
            from_id INTEGER NOT NULL,
            to_id INTEGER NOT NULL,
            PRIMARY KEY (target_id, from_id, to_id),
            FOREIGN KEY(target_id) REFERENCES targets(id) ON DELETE CASCADE
        )",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS backlinks (
            target_id INTEGER NOT NULL,
            source_path TEXT NOT NULL,
            target_path TEXT NOT NULL,
            PRIMARY KEY (target_id, source_path, target_path),
            FOREIGN KEY(target_id) REFERENCES targets(id) ON DELETE CASCADE
        )",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS work_items (
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
        )",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS work_item_bindings (
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
        )",
    )
    .execute(pool)
    .await?;
    // Failure-Mode Matrix slice γ: outbox for best-effort provider pushes.
    // When an editorial save propagates to the forge (`system_of_record =
    // split|external`) and the push fails, the editorial save is NOT rolled
    // back — instead a row lands here so a background job can retry and
    // operators can see what hasn't propagated. `kind` records which mutation
    // (state/assignees/binding) so the retry re-drives the right push.
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS pending_provider_sync (
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
        )",
    )
    .execute(pool)
    .await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_targets_key ON targets(key)")
        .execute(pool)
        .await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_files_target_path ON files(target_id, path)")
        .execute(pool)
        .await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_nodes_target_path ON nodes(target_id, path)")
        .execute(pool)
        .await?;
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_backlinks_target_target_path ON backlinks(target_id, target_path)",
    )
    .execute(pool)
    .await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_edges_target_from ON edges(target_id, from_id)")
        .execute(pool)
        .await?;
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_work_items_target_content_path ON work_items(target_id, content_path)",
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// Tables the migration helper is allowed to alter. Using an enum (rather than a
/// `&str`) means `add_column_if_missing` can only ever interpolate an identifier
/// from this closed set into DDL — no caller can pass an unvalidated string.
#[derive(Clone, Copy)]
enum MigratableTable {
    Targets,
}

impl MigratableTable {
    fn name(self) -> &'static str {
        match self {
            MigratableTable::Targets => "targets",
        }
    }
}

/// A column to add, with its SQLite column type/constraints. Both fields are
/// `&'static str` so only compile-time literals reach the DDL string.
#[derive(Clone, Copy)]
struct ColumnSpec {
    name: &'static str,
    spec: &'static str,
}

/// Idempotent `ALTER TABLE ADD COLUMN`. SQLite does not support
/// `ADD COLUMN IF NOT EXISTS`, so we probe `pragma_table_info` first and
/// skip when the column already exists. Table and column come from typed
/// allowlists, so the `format!` interpolation can't carry injected SQL.
async fn add_column_if_missing(
    pool: &SqlitePool,
    table: MigratableTable,
    column: ColumnSpec,
) -> Result<(), sqlx::Error> {
    let table = table.name();
    let existing: Vec<(String,)> =
        sqlx::query_as(&format!("SELECT name FROM pragma_table_info('{table}')"))
            .fetch_all(pool)
            .await?;
    if existing.iter().any(|(name,)| name == column.name) {
        return Ok(());
    }
    let stmt = format!(
        "ALTER TABLE {table} ADD COLUMN {} {}",
        column.name, column.spec
    );
    sqlx::query(&stmt).execute(pool).await?;
    Ok(())
}
