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

//! Single-user Personal Access Token mode.
//!
//! When `GITHUB_PAT` is set, GitNodes runs for a single operator instead of the
//! multi-user OAuth flow: the PAT is the token for every GitHub call and the
//! session is auto-established as the PAT's owner, so there is no OAuth App to
//! create. Authorization is unchanged — it still flows through live
//! `repository_permissions`, so a read-only fine-grained PAT lands in the
//! PR-fallback path exactly like a low-privilege OAuth user.
//!
//! Security boundary: in this mode anyone who can reach the HTTP port acts as
//! the PAT owner. Server startup therefore refuses a non-loopback bind unless
//! `GITNODES_ALLOW_REMOTE_PAT` is explicitly set, so remote exposure is a
//! deliberate choice rather than a silent default.

use std::net::SocketAddr;
use std::sync::OnceLock;

/// The resolved single-user identity when PAT mode is active.
#[derive(Clone)]
pub struct PatIdentity {
    pub token: String,
    pub login: String,
}

static PAT: OnceLock<Option<PatIdentity>> = OnceLock::new();

/// True when GitNodes is running in single-user PAT mode.
pub fn is_enabled() -> bool {
    identity().is_some()
}

/// The resolved PAT identity, or `None` in normal OAuth mode.
pub fn identity() -> Option<&'static PatIdentity> {
    PAT.get().and_then(|slot| slot.as_ref())
}

fn remote_pat_allowed() -> bool {
    matches!(
        std::env::var("GITNODES_ALLOW_REMOTE_PAT").as_deref(),
        Ok("1") | Ok("true") | Ok("TRUE")
    )
}

/// Resolve PAT mode at startup. No-op (and `Ok`) when `GITHUB_PAT` is unset.
///
/// When set, validates the token via `GET /user` to resolve the owner login and
/// enforces the loopback guardrail against `addr`. Any failure here is fatal:
/// the operator asked for PAT mode, so we refuse to fall back to an unexpected
/// auth posture.
pub async fn init(addr: &SocketAddr) {
    let token = match std::env::var("GITHUB_PAT") {
        Ok(t) if !t.trim().is_empty() => t.trim().to_string(),
        _ => {
            let _ = PAT.set(None);
            return;
        }
    };

    if !addr.ip().is_loopback() && !remote_pat_allowed() {
        tracing::error!(
            %addr,
            "GITHUB_PAT is set but the server binds a non-loopback address. In PAT \
             mode anyone who can reach this port acts as the token owner. Bind to \
             127.0.0.1, or set GITNODES_ALLOW_REMOTE_PAT=1 if it sits behind your \
             own auth/proxy."
        );
        std::process::exit(1);
    }

    let client = reqwest::Client::new();
    let login = match gitnodes_auth::fetch_user_login(&client, &token).await {
        Ok(login) => login,
        Err(error) => {
            tracing::error!(%error, "GITHUB_PAT rejected by GitHub (GET /user failed)");
            std::process::exit(1);
        }
    };

    tracing::info!(%login, "single-user PAT mode active");
    let _ = PAT.set(Some(PatIdentity { token, login }));
}
