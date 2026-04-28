#[cfg(feature = "ssr")]
use leptos::prelude::*;

mod files;
pub use files::{
    BrainFile, WriteCapabilities, WriteMode, WriteResult, delete_brain_file,
    get_write_capabilities, read_brain_file, save_brain_file,
};
#[cfg(feature = "ssr")]
pub use files::{DeleteBrainFile, GetWriteCapabilities, ReadBrainFile, SaveBrainFile};

mod file_ops;
#[cfg(feature = "ssr")]
pub use file_ops::{ListBrainFolders, RenameBrainFile, UploadAsset};
pub use file_ops::{RenameResult, list_brain_folders, rename_brain_file, upload_asset};
#[cfg(feature = "ssr")]
pub(crate) use file_ops::{relativize, slugify};

mod config_admin;
pub use config_admin::{
    AppConfig, AuditEntry, SessionEntry, get_app_config, get_current_user, list_sessions,
    list_views, load_audit_log, load_brain_config, load_brain_template, revoke_session, save_views,
};
#[cfg(feature = "ssr")]
pub use config_admin::{
    GetAppConfig, GetCurrentUser, ListSessions, ListViews, LoadAuditLog, LoadBrainConfig,
    LoadBrainTemplate, RevokeSession, SaveViews,
};

mod graph;
pub use graph::{
    AccessibleTarget, NodeQueryFilters, list_accessible_targets, list_nodes,
    load_brain_config_for_target, load_brain_graph, load_brain_graph_for_target, read_node,
    refresh_brain_graph,
};
#[cfg(feature = "ssr")]
pub use graph::{
    ListAccessibleTargets, ListNodes, LoadBrainConfigForTarget, LoadBrainGraph,
    LoadBrainGraphForTarget, ReadNode, RefreshBrainGraph,
};

mod work_items;
#[cfg(feature = "ssr")]
pub(crate) use work_items::apply_provider_work_item_update;
#[cfg(feature = "ssr")]
pub use work_items::{
    AssignWorkItem, BindWorkItem, ListWorkItems, LoadWorkItemByPath, LoadWorkItemComments,
    TransitionWorkItem,
};
pub use work_items::{
    WorkItemComment, WorkItemMutationResult, WorkItemQueryFilters, assign_work_item,
    bind_work_item, list_work_items, load_work_item_by_path, load_work_item_comments,
    transition_work_item,
};

#[cfg(feature = "ssr")]
mod write_orchestrator;

#[cfg(feature = "ssr")]
use brain_domain::BrainError;

#[cfg(feature = "ssr")]
fn sfe(e: BrainError) -> ServerFnError {
    ServerFnError::new(e.to_string())
}

/// Accept a user-supplied commit message only if it's non-empty after trim and
/// free of control characters (tabs, CR, LF, etc.). Cap at 200 chars to keep
/// subject lines sane. Returns `None` to signal "fall back to auto-message".
#[cfg(feature = "ssr")]
fn sanitize_commit_message(raw: Option<&str>) -> Option<String> {
    let trimmed = raw?.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.chars().any(|c| c.is_control() && c != ' ') {
        return None;
    }
    let mut out = trimmed.to_string();
    if out.chars().count() > 200 {
        out = out.chars().take(200).collect();
    }
    Some(out)
}

/// Canonical list of every `#[server]` fn in this crate. Single source of
/// truth for both `register_server_functions` (runtime registration) and the
/// `server_fns_registered_match_attributes` test (build-time guardrail).
///
/// **Adding a new `#[server]` fn requires adding its struct name here.** The
/// regression test fails the build otherwise, preventing the silent
/// release-mode 404 documented in caveat #9.
#[cfg(feature = "ssr")]
#[cfg_attr(not(test), allow(dead_code))]
const SERVER_FNS: &[&str] = &[
    "GetAppConfig",
    "LoadBrainConfig",
    "LoadAuditLog",
    "ListSessions",
    "RevokeSession",
    "GetCurrentUser",
    "LoadBrainTemplate",
    "ListNodes",
    "ListWorkItems",
    "ReadNode",
    "LoadBrainGraph",
    "LoadWorkItemByPath",
    "LoadWorkItemComments",
    "ReadBrainFile",
    "SaveBrainFile",
    "DeleteBrainFile",
    "RenameBrainFile",
    "UploadAsset",
    "ListBrainFolders",
    "RefreshBrainGraph",
    "GetWriteCapabilities",
    "ListAccessibleTargets",
    "LoadBrainGraphForTarget",
    "LoadBrainConfigForTarget",
    "TransitionWorkItem",
    "AssignWorkItem",
    "BindWorkItem",
    "ListViews",
    "SaveViews",
];

#[cfg(feature = "ssr")]
pub fn register_server_functions() {
    // LTO (`lto = true` in [profile.release]) strips the `inventory::submit!`
    // entries that `#[server]` relies on for automatic registration. Calling
    // `register_explicit` bypasses inventory and directly inserts each server
    // function into the global handler map.
    use leptos::server_fn::axum::register_explicit;
    register_explicit::<GetAppConfig>();
    register_explicit::<LoadBrainConfig>();
    register_explicit::<LoadAuditLog>();
    register_explicit::<ListSessions>();
    register_explicit::<RevokeSession>();
    register_explicit::<GetCurrentUser>();
    register_explicit::<LoadBrainTemplate>();
    register_explicit::<ListNodes>();
    register_explicit::<ListWorkItems>();
    register_explicit::<ReadNode>();
    register_explicit::<LoadBrainGraph>();
    register_explicit::<LoadWorkItemByPath>();
    register_explicit::<LoadWorkItemComments>();
    register_explicit::<ReadBrainFile>();
    register_explicit::<SaveBrainFile>();
    register_explicit::<DeleteBrainFile>();
    register_explicit::<RenameBrainFile>();
    register_explicit::<UploadAsset>();
    register_explicit::<ListBrainFolders>();
    register_explicit::<RefreshBrainGraph>();
    register_explicit::<GetWriteCapabilities>();
    register_explicit::<ListAccessibleTargets>();
    register_explicit::<LoadBrainGraphForTarget>();
    register_explicit::<LoadBrainConfigForTarget>();
    register_explicit::<TransitionWorkItem>();
    register_explicit::<AssignWorkItem>();
    register_explicit::<BindWorkItem>();
    register_explicit::<ListViews>();
    register_explicit::<SaveViews>();
}

/// Regression guard for caveat #9: `lto = true` strips Leptos's
/// `inventory::submit!` entries, so every `#[server]` fn must be listed in
/// `SERVER_FNS` and registered explicitly. Without this test, adding a server
/// fn without registering it silently 404s in release builds.
#[cfg(all(test, feature = "ssr"))]
mod server_fn_registration_tests {
    use super::SERVER_FNS;

    /// Sources that can define `#[server]` functions in this module tree.
    /// Embedding them here keeps the test independent of runtime filesystem
    /// access while still allowing `api.rs` to be split into focused modules.
    const API_SOURCES: &[&str] = &[
        include_str!("api.rs"),
        include_str!("api/config_admin.rs"),
        include_str!("api/file_ops.rs"),
        include_str!("api/files.rs"),
        include_str!("api/graph.rs"),
        include_str!("api/work_items.rs"),
    ];

    /// Pull the struct name out of `#[server(Name, ...)]` or `#[server(\n    Name,\n ...`).
    fn extract_server_fn_names(src: &str) -> Vec<String> {
        let mut names = Vec::new();
        let needle = "#[server(";
        for (idx, _) in src.match_indices(needle) {
            // Skip occurrences that are inside a string literal in this very
            // file (the `let needle = "#[server(";` line above) by requiring
            // the match to be at the start of a line modulo whitespace.
            let line_start = src[..idx].rfind('\n').map(|n| n + 1).unwrap_or(0);
            let prefix = &src[line_start..idx];
            if !prefix.chars().all(|c| c.is_whitespace()) {
                continue;
            }
            let after = &src[idx + needle.len()..];
            // Skip whitespace and commas; first ident is the struct name.
            let trimmed = after.trim_start_matches(|c: char| c.is_whitespace() || c == ',');
            let name: String = trimmed
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect();
            if !name.is_empty() {
                names.push(name);
            }
        }
        names
    }

    #[test]
    fn server_fns_registered_match_attributes() {
        let mut found = API_SOURCES
            .iter()
            .flat_map(|src| extract_server_fn_names(src))
            .collect::<Vec<_>>();
        found.sort();
        found.dedup();

        let mut declared: Vec<String> = SERVER_FNS.iter().map(|s| (*s).to_string()).collect();
        declared.sort();
        declared.dedup();

        assert_eq!(
            found, declared,
            "every #[server(...)] fn in the api module tree must appear in SERVER_FNS \
             (and register_server_functions). Found in source: {found:?}; \
             declared in SERVER_FNS: {declared:?}"
        );
    }

    #[test]
    fn extract_server_fn_names_ignores_string_literal_occurrences() {
        // Sanity: the literal needle in this test file should not be picked up
        // because it's not at start-of-line.
        let sample = "fn x() { let needle = \"#[server(Bogus,\"; }";
        let names = extract_server_fn_names(sample);
        assert!(
            names.is_empty(),
            "string-literal #[server( must be ignored: {names:?}"
        );
    }

    #[test]
    fn extract_server_fn_names_finds_real_attributes() {
        let sample = "#[server(Foo, \"/api\")]\npub async fn foo() {}\n\
                       #[server(\n    Bar,\n    \"/api\",\n)]\npub async fn bar() {}\n";
        let mut names = extract_server_fn_names(sample);
        names.sort();
        assert_eq!(names, vec!["Bar".to_string(), "Foo".to_string()]);
    }
}
