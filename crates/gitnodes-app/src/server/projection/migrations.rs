use sqlx::SqlitePool;
use sqlx::migrate::Migrator;

static MIGRATOR: Migrator = sqlx::migrate!("./migrations");

pub async fn migrate(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    sqlx::query("PRAGMA foreign_keys = ON")
        .execute(pool)
        .await?;
    prepare_legacy_targets(pool).await?;
    MIGRATOR.run(pool).await?;
    Ok(())
}

async fn prepare_legacy_targets(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    let targets_exists: Option<(i64,)> =
        sqlx::query_as("SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'targets'")
            .fetch_optional(pool)
            .await?;
    if targets_exists.is_none() {
        return Ok(());
    }

    add_column_if_missing(
        pool,
        ColumnSpec {
            name: "registered_at",
            spec: "TEXT",
        },
    )
    .await?;
    add_column_if_missing(
        pool,
        ColumnSpec {
            name: "registered_by",
            spec: "TEXT",
        },
    )
    .await?;
    add_column_if_missing(
        pool,
        ColumnSpec {
            name: "source",
            spec: "TEXT NOT NULL DEFAULT 'env_default'",
        },
    )
    .await?;
    add_column_if_missing(
        pool,
        ColumnSpec {
            name: "default_branch",
            spec: "TEXT",
        },
    )
    .await?;

    sqlx::query("UPDATE targets SET registered_at = CURRENT_TIMESTAMP WHERE registered_at IS NULL")
        .execute(pool)
        .await?;
    sqlx::query("UPDATE targets SET source = 'env_default' WHERE source IS NULL")
        .execute(pool)
        .await?;
    sqlx::query("UPDATE targets SET default_branch = branch WHERE default_branch IS NULL")
        .execute(pool)
        .await?;

    Ok(())
}

#[derive(Clone, Copy)]
struct ColumnSpec {
    name: &'static str,
    spec: &'static str,
}

async fn add_column_if_missing(pool: &SqlitePool, column: ColumnSpec) -> Result<(), sqlx::Error> {
    let existing: Vec<(String,)> = sqlx::query_as("SELECT name FROM pragma_table_info('targets')")
        .fetch_all(pool)
        .await?;
    if existing.iter().any(|(name,)| name == column.name) {
        return Ok(());
    }
    let stmt = format!(
        "ALTER TABLE targets ADD COLUMN {} {}",
        column.name, column.spec
    );
    sqlx::query(&stmt).execute(pool).await?;
    Ok(())
}
