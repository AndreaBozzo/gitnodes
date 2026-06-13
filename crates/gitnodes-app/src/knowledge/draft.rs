//! Client-side draft persistence for the editor.
//!
//! Serializes the in-progress form state to `window.localStorage` on a debounce
//! so a tab crash or accidental close doesn't lose 30 minutes of writing. Keyed
//! by `<org>/<repo>` + path (or `:new`) so drafts from a different deployment
//! target don't collide and per-file drafts stay isolated.
//!
//! SSR-safe stubs keep this module compilable on the server build; all real
//! work happens under `cfg(not(feature = "ssr"))`.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

const DRAFT_SCHEMA_VERSION: u8 = 2;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Draft {
    pub node_type: String,
    pub title: String,
    pub author: String,
    pub tags: Vec<String>,
    pub body: String,
    pub related: Vec<String>,
    pub folder: Option<String>,
    /// Unix seconds. Used by the restore banner to show "saved N minutes ago".
    pub saved_at: i64,
    /// Sha of the file the draft was forked from (edit mode only). Lets us
    /// detect a stale draft: if the live file sha doesn't match, upstream
    /// moved on and restoring would silently revert their changes.
    pub base_sha: Option<String>,
    /// Frontmatter from the original file on edit drafts, preserved so save
    /// can merge it with form fields instead of regenerating from template.
    /// `serde(default)` keeps pre-existing drafts deserializable with `None`.
    #[serde(default)]
    pub preserved_frontmatter: Option<BTreeMap<String, serde_yaml::Value>>,
    /// UI-managed extra frontmatter string fields, such as ADR `status`.
    #[serde(default)]
    pub extra_frontmatter: BTreeMap<String, String>,
    /// Keeps the malformed-frontmatter guard active across draft restore.
    #[serde(default)]
    pub frontmatter_malformed: bool,
}

/// Build the localStorage key for a given repo scope and file path.
///
/// - `repo_scope` = `"<org>/<repo>"` — isolates drafts across deployments.
/// - `path` = `Some("...")` for edit mode, `None` for new-doc mode.
pub fn storage_key(repo_scope: &str, path: Option<&str>) -> String {
    match path {
        Some(p) => format!("brain-ui:draft:v{DRAFT_SCHEMA_VERSION}:{repo_scope}:{p}"),
        None => format!("brain-ui:draft:v{DRAFT_SCHEMA_VERSION}:{repo_scope}:new"),
    }
}

#[cfg(feature = "hydrate")]
fn local_storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok().flatten()
}

#[cfg(feature = "hydrate")]
pub fn save(key: &str, draft: &Draft) {
    let Some(store) = local_storage() else {
        return;
    };
    let Ok(json) = serde_json::to_string(draft) else {
        return;
    };
    let _ = store.set_item(key, &json);
}

#[cfg(feature = "hydrate")]
pub fn load(key: &str) -> Option<Draft> {
    let store = local_storage()?;
    let raw = store.get_item(key).ok().flatten()?;
    serde_json::from_str(&raw).ok()
}

#[cfg(feature = "hydrate")]
pub fn clear(key: &str) {
    let Some(store) = local_storage() else {
        return;
    };
    let _ = store.remove_item(key);
}

#[cfg(feature = "hydrate")]
pub fn now_secs() -> i64 {
    (js_sys::Date::now() / 1000.0) as i64
}

// --- SSR stubs ------------------------------------------------------------

#[cfg(not(feature = "hydrate"))]
#[allow(dead_code)]
pub fn save(_key: &str, _draft: &Draft) {}

#[cfg(not(feature = "hydrate"))]
pub fn load(_key: &str) -> Option<Draft> {
    None
}

#[cfg(not(feature = "hydrate"))]
pub fn clear(_key: &str) {}

#[cfg(not(feature = "hydrate"))]
pub fn now_secs() -> i64 {
    0
}

/// Human-readable "N minutes ago" from a unix-seconds timestamp.
pub fn relative_time(saved_at: i64, now: i64) -> String {
    let delta = (now - saved_at).max(0);
    if delta < 60 {
        "just now".to_string()
    } else if delta < 3600 {
        let m = delta / 60;
        format!("{m} minute{} ago", if m == 1 { "" } else { "s" })
    } else if delta < 86400 {
        let h = delta / 3600;
        format!("{h} hour{} ago", if h == 1 { "" } else { "s" })
    } else {
        let d = delta / 86400;
        format!("{d} day{} ago", if d == 1 { "" } else { "s" })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_scoping() {
        assert_eq!(
            storage_key("Dritara-Digital/Brain", None),
            "brain-ui:draft:v2:Dritara-Digital/Brain:new"
        );
        assert_eq!(
            storage_key("Dritara-Digital/Brain", Some("concepts/foo.md")),
            "brain-ui:draft:v2:Dritara-Digital/Brain:concepts/foo.md"
        );
    }

    #[test]
    fn relative_time_buckets() {
        assert_eq!(relative_time(100, 130), "just now");
        assert_eq!(relative_time(100, 160), "1 minute ago");
        assert_eq!(relative_time(100, 700), "10 minutes ago");
        assert_eq!(relative_time(0, 7200), "2 hours ago");
        assert_eq!(relative_time(0, 86_400 * 3), "3 days ago");
    }
}
