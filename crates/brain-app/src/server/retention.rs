//! Periodic data retention for the shared SQLite store (Projection Schema v2).
//!
//! Three policies, all idempotent:
//! - **Audit log**: rows older than `AUDIT_RETENTION_DAYS` (default 90) are
//!   deleted on each tick. Operators retain ~3 months of trail by default.
//! - **Expired sessions**: `tower-sessions-sqlx-store` doesn't wire its own
//!   sweeper at boot, so rows with `expiry_date < now` are deleted here as a
//!   fallback. Cheap belt-and-braces against the cookie store.
//! - **Stuck `pending_provider_sync` rows**: not deleted — operators still
//!   need them visible in admin — but counted and logged each tick so the
//!   number doesn't silently grow.
//!
//! Editing locks and watch notifications are intentionally out of scope: the
//! tables don't exist yet. When those features land they'll add their own
//! DELETE statements here.

use std::time::Duration;

use sqlx::SqlitePool;

/// Default tick interval. Daily is enough — none of these are urgent.
const DEFAULT_INTERVAL_SECS: u64 = 24 * 3600;
/// Default audit-log retention window.
const DEFAULT_AUDIT_RETENTION_DAYS: i64 = 90;

pub fn spawn(pool: SqlitePool) {
    let interval_secs = std::env::var("RETENTION_INTERVAL_SECS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .filter(|v| *v > 0)
        .unwrap_or(DEFAULT_INTERVAL_SECS);
    let audit_days = std::env::var("AUDIT_RETENTION_DAYS")
        .ok()
        .and_then(|v| v.parse::<i64>().ok())
        .filter(|v| *v >= 0)
        .unwrap_or(DEFAULT_AUDIT_RETENTION_DAYS);

    let task = tokio::spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_secs(interval_secs));
        loop {
            ticker.tick().await;
            if let Err(error) = run_once(&pool, audit_days).await {
                tracing::warn!(%error, "retention tick failed");
            }
        }
    });

    tokio::spawn(async move {
        match task.await {
            Ok(()) => {}
            Err(join_err) if join_err.is_panic() => {
                tracing::error!("retention loop panicked");
            }
            Err(_) => {
                tracing::warn!("retention loop cancelled");
            }
        }
    });
}

/// One retention pass. Public for testing.
pub async fn run_once(pool: &SqlitePool, audit_days: i64) -> Result<(), sqlx::Error> {
    // `datetime('now', ?)` accepts modifiers like '-90 days'.
    let cutoff = format!("-{} days", audit_days.max(0));
    let audit = sqlx::query("DELETE FROM audit_events WHERE ts < datetime('now', ?)")
        .bind(&cutoff)
        .execute(pool)
        .await?;

    // `tower-sessions-sqlx-store` declares this column as INTEGER but sqlx
    // stores `OffsetDateTime` as RFC3339 text; tolerate either shape.
    let sessions = sqlx::query(
        "DELETE FROM tower_sessions
         WHERE COALESCE(
            CAST(strftime('%s', expiry_date) AS INTEGER),
            CAST(strftime('%s', expiry_date, 'unixepoch') AS INTEGER)
         ) < CAST(strftime('%s', 'now') AS INTEGER)",
    )
    .execute(pool)
    .await?;

    // Stuck outbox rows: count, don't delete. Operators need them in admin.
    // Threshold shared with the retry job so the two stay in lockstep.
    let stuck: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM pending_provider_sync WHERE attempts >= ?")
            .bind(super::pending_sync_job::MAX_ATTEMPTS)
            .fetch_one(pool)
            .await?;

    if audit.rows_affected() > 0 || sessions.rows_affected() > 0 || stuck.0 > 0 {
        tracing::info!(
            audit_deleted = audit.rows_affected(),
            sessions_deleted = sessions.rows_affected(),
            pending_sync_stuck = stuck.0,
            audit_retention_days = audit_days,
            "retention tick"
        );
    }

    Ok(())
}
