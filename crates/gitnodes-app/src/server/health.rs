// Copyright (C) 2026 Andrea Bozzo
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU Affero General Public License for more details.
//
// You should have received a copy of the GNU Affero General Public License
// along with this program.  If not, see <https://www.gnu.org/licenses/>.

//! Operational health probes.
//!
//! Two unauthenticated endpoints an orchestrator (Railway healthcheck, uptime
//! probe) can poll. They live outside the auth / CSRF / session middleware so a
//! probe never needs a cookie and never touches the session store.
//!
//! - `GET /healthz` — *liveness*. The process is up and the HTTP stack serves.
//!   Returns `200` unconditionally, no I/O. Failing liveness means "restart me".
//! - `GET /readyz` — *readiness*. The process can actually serve traffic. Runs
//!   cheap checks and returns `200` when all pass, `503` when any fails. Failing
//!   readiness means "stop routing traffic to me", not "restart me".
//!
//! Several boot invariants (app boot, session-store migration) are fail-fast at
//! startup via `expect`/`process::exit`, so by the time the router answers a
//! request they are definitionally true. We report them as static `true` rather
//! than re-probing. The only live check worth doing is a `SELECT 1` against the
//! single shared SQLite pool — which also backs sessions and audit, so one probe
//! covers all three.

use std::collections::BTreeMap;

use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Serialize;
use serde_json::json;

use super::projection;

/// One readiness check result. `ok` drives the aggregate readiness; `detail`
/// carries an optional human-readable note (an error message, or a benign state
/// such as "no_sync_yet") for operators.
#[derive(Debug, Serialize)]
pub struct CheckStatus {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

impl CheckStatus {
    fn ok() -> Self {
        Self {
            ok: true,
            detail: None,
        }
    }

    fn ok_with(detail: impl Into<String>) -> Self {
        Self {
            ok: true,
            detail: Some(detail.into()),
        }
    }

    fn fail(detail: impl Into<String>) -> Self {
        Self {
            ok: false,
            detail: Some(detail.into()),
        }
    }
}

/// Aggregate readiness report. `ready` is the AND of every check's `ok`.
#[derive(Debug, Serialize)]
pub struct ReadyzReport {
    pub ready: bool,
    pub checks: BTreeMap<String, CheckStatus>,
}

impl ReadyzReport {
    /// Run the readiness checks and assemble the report.
    ///
    /// `sqlite` is the only live I/O check: a `SELECT 1` against the shared
    /// pool. `projection_pool` reports whether `projection::init` has run.
    /// `session_store_migrated` is a static `true` — the migration is fail-fast
    /// at boot, so reaching this handler proves it succeeded.
    async fn collect() -> Self {
        let mut checks = BTreeMap::new();

        let sqlite = match projection::pool_handle() {
            Some(pool) => match sqlx::query("SELECT 1").execute(pool).await {
                Ok(_) => CheckStatus::ok(),
                Err(e) => CheckStatus::fail(format!("select failed: {e}")),
            },
            None => CheckStatus::fail("projection pool not initialized".to_string()),
        };
        checks.insert("sqlite".to_string(), sqlite);

        let projection_pool = if projection::pool_handle().is_some() {
            CheckStatus::ok()
        } else {
            CheckStatus::fail("init not run".to_string())
        };
        checks.insert("projection_pool".to_string(), projection_pool);

        // Boot invariant: the session store migration at startup is fail-fast
        // (`expect`). If it had failed the process would have exited before
        // binding the listener, so a served request implies success.
        checks.insert(
            "session_store_migrated".to_string(),
            CheckStatus::ok_with("boot-guaranteed"),
        );

        let ready = checks.values().all(|c| c.ok);
        Self { ready, checks }
    }
}

/// GET /healthz — liveness. Always 200, no I/O.
pub async fn healthz() -> impl IntoResponse {
    (StatusCode::OK, Json(json!({ "status": "ok" })))
}

/// GET /readyz — readiness. 200 when every check passes, else 503. The JSON body
/// is identical in both cases so an operator sees which check flipped.
pub async fn readyz() -> Response {
    let report = ReadyzReport::collect().await;
    let status = if report.ready {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    (status, Json(report)).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Router, body::Body, http::Request, routing::get};
    use http_body_util::BodyExt;
    use sqlx::sqlite::SqlitePoolOptions;
    use tower::ServiceExt; // for `oneshot`

    #[tokio::test]
    async fn healthz_is_always_ok() {
        let resp = healthz().await.into_response();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn readyz_ready_when_pool_initialized() {
        // An in-memory pool stands in for the shared SQLite pool. `init` is a
        // process-global `OnceLock::set`; if another test in this binary already
        // initialized it the ping still succeeds, so this stays robust.
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("open in-memory sqlite");
        projection::init(pool);

        let report = ReadyzReport::collect().await;
        assert!(report.ready, "expected ready, got {report:?}");
        assert!(report.checks["sqlite"].ok);
        assert!(report.checks["projection_pool"].ok);
        assert!(report.checks["session_store_migrated"].ok);
    }

    /// Drive the real routes through a router to confirm wiring, status codes,
    /// and body shape end-to-end without booting the full app.
    #[tokio::test]
    async fn routes_respond_without_auth() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("open in-memory sqlite");
        projection::init(pool);

        let app = Router::new()
            .route("/healthz", get(healthz))
            .route("/readyz", get(readyz));

        let healthz_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/healthz")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(healthz_resp.status(), StatusCode::OK);
        let body = healthz_resp.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(&body[..], br#"{"status":"ok"}"#);

        let readyz_resp = app
            .oneshot(
                Request::builder()
                    .uri("/readyz")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        // Pool is initialized, so readiness is 200 with a per-check body.
        assert_eq!(readyz_resp.status(), StatusCode::OK);
        let body = readyz_resp.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["ready"], true);
        assert_eq!(json["checks"]["sqlite"]["ok"], true);
    }
}
