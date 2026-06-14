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
        Some(p) => format!("gitnodes:draft:v{DRAFT_SCHEMA_VERSION}:{repo_scope}:{p}"),
        None => format!("gitnodes:draft:v{DRAFT_SCHEMA_VERSION}:{repo_scope}:new"),
    }
}

#[cfg(any(feature = "hydrate", test))]
fn legacy_storage_key(key: &str) -> Option<String> {
    key.strip_prefix("gitnodes:")
        .map(|suffix| format!("brain-ui:{suffix}"))
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
    let raw = store.get_item(key).ok().flatten().or_else(|| {
        legacy_storage_key(key).and_then(|legacy| store.get_item(&legacy).ok().flatten())
    })?;
    serde_json::from_str(&raw).ok()
}

#[cfg(feature = "hydrate")]
pub fn clear(key: &str) {
    let Some(store) = local_storage() else {
        return;
    };
    let _ = store.remove_item(key);
    if let Some(legacy) = legacy_storage_key(key) {
        let _ = store.remove_item(&legacy);
    }
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
            "gitnodes:draft:v2:Dritara-Digital/Brain:new"
        );
        assert_eq!(
            storage_key("Dritara-Digital/Brain", Some("concepts/foo.md")),
            "gitnodes:draft:v2:Dritara-Digital/Brain:concepts/foo.md"
        );
        assert_eq!(
            legacy_storage_key("gitnodes:draft:v2:org/repo:new").as_deref(),
            Some("brain-ui:draft:v2:org/repo:new")
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
