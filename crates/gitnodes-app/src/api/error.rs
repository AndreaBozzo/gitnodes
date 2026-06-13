//! Typed error at the server-fn boundary.
//!
//! Server functions return `Result<T, ApiError>` directly (not the deprecated
//! `ServerFnError<E>` wrapper — leptos 0.8 deprecates the generic and 0.9
//! standardizes on the bare form, so this shape survives the upgrade). The UI
//! deserializes `ApiError` and matches on the variant — a stale write becomes
//! "reload and retry", a 403 becomes a PR hint — instead of `String`-parsing a
//! flattened message.
//!
//! `ServerFn` folds in the framework's own transport/serialization errors so
//! nothing is lost when `from_server_fn_error` is the only thing the runtime
//! can produce.

use leptos::prelude::{FromServerFnError, ServerFnErrorErr};
use leptos::server_fn::codec::JsonEncoding;
use serde::{Deserialize, Serialize};

use gitnodes_domain::ConflictKind;

/// Application error surfaced to the client. Variants map to the failure modes
/// the UI is expected to react to differently. The `String` payloads carry a
/// human-readable detail for display/logging; UI branching keys off the variant.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ApiError {
    /// Framework/transport error (deserialization, request, etc.). Produced by
    /// `from_server_fn_error`; not constructed by application code directly.
    ServerFn(ServerFnErrorErr),
    /// No authenticated session / token. UI → prompt re-login.
    Unauthenticated,
    /// GitHub 403 / protected branch / "resource not accessible". UI → offer
    /// the PR-fallback path or explain the permission gap.
    PermissionDenied(String),
    /// GitHub 404 — file/issue/repo not found. UI → "it may have moved; reload".
    NotFound(String),
    /// Optimistic-concurrency conflict (stale sha / precondition). UI → "reload
    /// and retry"; never a transparent retry.
    Conflict { kind: ConflictKind, message: String },
    /// GitHub 429 / secondary rate limit. UI → "slow down / retry shortly".
    RateLimited(String),
    /// Input rejected before any side effect (size cap, invalid path/target).
    BadInput(String),
    /// Anything else, including unclassified GitHub/parse/io errors.
    Internal(String),
}

impl FromServerFnError for ApiError {
    type Encoder = JsonEncoding;

    fn from_server_fn_error(value: ServerFnErrorErr) -> Self {
        ApiError::ServerFn(value)
    }
}

impl ApiError {
    /// Operator-facing, actionable message for UI display. Unlike `Display`
    /// (which is diagnostic), this tells the user what to *do*: reload on a
    /// stale conflict, retry shortly on rate-limit, re-login when the session
    /// lapsed. The UI matches on the variant; this is the shared phrasing.
    pub fn actionable_message(&self) -> String {
        match self {
            ApiError::Unauthenticated => {
                "Your session expired. Please sign in again to continue.".into()
            }
            ApiError::PermissionDenied(message) if message.contains("read access") => {
                "You don't have permission to read this repository.".into()
            }
            ApiError::PermissionDenied(_) => "GitHub denied this operation. Check your repository permissions or propose the change through a pull request.".into(),
            ApiError::NotFound(_) => {
                "That file or item could not be found — it may have moved. Try reloading.".into()
            }
            ApiError::Conflict { kind, .. } => match kind {
                ConflictKind::PathTaken => {
                    "That destination already exists. Choose another path or reload before retrying."
                        .into()
                }
                ConflictKind::RemotePathDeletedUnderUs => {
                    "That file was deleted remotely. Reload to get the latest version.".into()
                }
                ConflictKind::RefNonFastForward | ConflictKind::BlobShaMoved => {
                    "This was changed by someone else since you opened it. Reload to get the \
                     latest version, then reapply your edit."
                        .into()
                }
            },
            ApiError::RateLimited(_) => {
                "GitHub is rate-limiting requests right now. Wait a few seconds and try again."
                    .into()
            }
            ApiError::BadInput(m) => m.clone(),
            ApiError::ServerFn(_) | ApiError::Internal(_) => {
                format!("Something went wrong: {self}")
            }
        }
    }
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ApiError::ServerFn(e) => write!(f, "{e:?}"),
            ApiError::Unauthenticated => write!(f, "not authenticated"),
            ApiError::PermissionDenied(m) => write!(f, "permission denied: {m}"),
            ApiError::NotFound(m) => write!(f, "not found: {m}"),
            ApiError::Conflict { kind, message } => write!(f, "conflict ({kind}): {message}"),
            ApiError::RateLimited(m) => write!(f, "rate limited: {m}"),
            ApiError::BadInput(m) => write!(f, "{m}"),
            ApiError::Internal(m) => write!(f, "{m}"),
        }
    }
}

/// Server-side bridge: classify a `BrainError` into the typed boundary error.
///
/// `BrainError::GitHub` carries a status + body snippet string (built by
/// `GithubHttp::send_json`); we sniff the well-known statuses so the client gets
/// a typed variant instead of a flattened message. The substring sniff mirrors
/// the existing `should_fallback_to_pr` matcher in `write_orchestrator` — when
/// β lands fully that matcher should key off `ApiError::PermissionDenied`
/// instead of re-sniffing.
#[cfg(feature = "ssr")]
impl From<gitnodes_domain::BrainError> for ApiError {
    fn from(e: gitnodes_domain::BrainError) -> Self {
        use gitnodes_domain::BrainError as B;
        match e {
            B::Unauthenticated => ApiError::Unauthenticated,
            B::PermissionDenied(m) => ApiError::PermissionDenied(m),
            B::NotFound(m) => ApiError::NotFound(m),
            B::Conflict { kind, message } => ApiError::Conflict { kind, message },
            B::Parse(m) => ApiError::BadInput(m),
            B::GitHub(m) => classify_github(m),
            B::Io(m) => ApiError::Internal(m),
            B::Other(m) => ApiError::Internal(m),
        }
    }
}

/// Sniff a GitHub error message (status + body snippet) into a typed variant.
#[cfg(feature = "ssr")]
fn classify_github(msg: String) -> ApiError {
    let lower = msg.to_lowercase();
    if lower.contains("status 403")
        || lower.contains("protected")
        || lower.contains("resource not accessible")
    {
        ApiError::PermissionDenied(msg)
    } else if lower.contains("status 404") {
        ApiError::NotFound(msg)
    } else if lower.contains("status 409") || lower.contains("not a fast forward") {
        ApiError::Conflict {
            kind: gitnodes_domain::ConflictKind::RefNonFastForward,
            message: msg,
        }
    } else if lower.contains("status 429")
        || lower.contains("rate limit")
        || lower.contains("secondary rate")
    {
        ApiError::RateLimited(msg)
    } else {
        ApiError::Internal(msg)
    }
}

#[cfg(all(test, feature = "ssr"))]
mod tests {
    use super::*;
    use gitnodes_domain::BrainError;

    #[test]
    fn github_status_classification() {
        assert!(matches!(
            ApiError::from(BrainError::github("contents status 404: not found")),
            ApiError::NotFound(_)
        ));
        assert!(matches!(
            ApiError::from(BrainError::github("save status 403: forbidden")),
            ApiError::PermissionDenied(_)
        ));
        assert!(matches!(
            ApiError::from(BrainError::github("patch status 409: not a fast forward")),
            ApiError::Conflict { .. }
        ));
        assert!(matches!(
            ApiError::from(BrainError::github("read status 429: secondary rate limit")),
            ApiError::RateLimited(_)
        ));
        assert!(matches!(
            ApiError::from(BrainError::github("status 500: boom")),
            ApiError::Internal(_)
        ));
    }

    #[test]
    fn domain_variants_map_directly() {
        assert_eq!(
            ApiError::from(BrainError::Unauthenticated),
            ApiError::Unauthenticated
        );
        assert!(matches!(
            ApiError::from(BrainError::conflict(
                ConflictKind::BlobShaMoved,
                "stale sha"
            )),
            ApiError::Conflict {
                kind: ConflictKind::BlobShaMoved,
                ..
            }
        ));
        assert!(matches!(
            ApiError::from(BrainError::parse("too large")),
            ApiError::BadInput(_)
        ));
        assert!(matches!(
            ApiError::from(BrainError::NotFound("x".into())),
            ApiError::NotFound(_)
        ));
        assert!(matches!(
            ApiError::from(BrainError::permission_denied("read access required")),
            ApiError::PermissionDenied(_)
        ));
    }

    #[test]
    fn permission_messages_distinguish_read_from_write_operations() {
        assert_eq!(
            ApiError::PermissionDenied("read access required".into()).actionable_message(),
            "You don't have permission to read this repository."
        );
        assert!(
            ApiError::PermissionDenied("write access required".into())
                .actionable_message()
                .contains("GitHub denied this operation")
        );
    }

    #[test]
    fn roundtrips_as_json() {
        let e = ApiError::Conflict {
            kind: ConflictKind::BlobShaMoved,
            message: "stale".into(),
        };
        let json = serde_json::to_string(&e).unwrap();
        let back: ApiError = serde_json::from_str(&json).unwrap();
        assert_eq!(e, back);
    }
}
