mod error;
pub use error::ApiError;

mod files;
pub use files::{
    BrainFile, FileQueryFilters, RepoFile, WriteCapabilities, WriteMode, WriteResult,
    delete_brain_file, get_write_capabilities, list_brain_files, read_brain_file, save_brain_file,
};
#[cfg(feature = "ssr")]
pub use files::{
    DeleteBrainFile, GetWriteCapabilities, ListBrainFiles, ReadBrainFile, SaveBrainFile,
};

mod file_ops;
#[cfg(feature = "ssr")]
pub use file_ops::{ListBrainFolders, RenameBrainFile, UploadAsset};
pub use file_ops::{RenameResult, list_brain_folders, rename_brain_file, upload_asset};
#[cfg(feature = "ssr")]
pub(crate) use file_ops::{relativize, slugify, validate_markdown_path};

mod config_admin;
pub use config_admin::{
    AppConfig, AuditEntry, ConfigLoadDiagnostic, ConfigLoadStatus, PendingSyncEntry,
    ProjectionStatus, ProjectionStatusEntry, SessionEntry, ViewsPreview, get_app_config,
    get_current_user, get_projection_status, list_pending_sync, list_sessions, list_views,
    load_audit_log, load_brain_config, load_brain_config_status,
    load_brain_config_status_for_target, load_brain_template, preview_views, revoke_session,
    save_views,
};
#[cfg(feature = "ssr")]
pub use config_admin::{
    GetAppConfig, GetCurrentUser, GetProjectionStatus, ListPendingSync, ListSessions, ListViews,
    LoadAuditLog, LoadBrainConfig, LoadBrainConfigStatus, LoadBrainConfigStatusForTarget,
    LoadBrainTemplate, PreviewViews, RevokeSession, SaveViews,
};

mod graph;
pub use graph::{
    AccessibleTarget, AccessibleTargetState, NodeQueryFilters, list_accessible_targets, list_nodes,
    load_brain_config_for_target, load_gitnodes_graph, load_gitnodes_graph_for_target, read_node,
    refresh_gitnodes_graph, resolve_legacy_target,
};
#[cfg(feature = "ssr")]
pub use graph::{
    ListAccessibleTargets, ListNodes, LoadBrainConfigForTarget, LoadBrainGraph,
    LoadBrainGraphForTarget, ReadNode, RefreshBrainGraph, ResolveLegacyTarget,
};

mod search;
#[cfg(feature = "ssr")]
pub use search::SearchBrain;
pub use search::{SearchBrainQuery, SearchHit, search_brain};

mod pull_requests;
#[cfg(feature = "ssr")]
pub use pull_requests::{ListOpenPrs, MergePullRequest};
pub use pull_requests::{MergePrResult, PrSummary, list_open_prs, merge_pull_request};

mod work_items;
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
pub(crate) use work_items::{apply_provider_work_item_update, reconcile_provider_sync};

#[cfg(feature = "ssr")]
mod write_orchestrator;

#[cfg(feature = "ssr")]
use gitnodes_domain::BrainError;

/// Bridge a server-side `BrainError` to the typed boundary error. Kept as a
/// short alias over `ApiError::from` so the ~120 existing `.map_err(sfe)` call
/// sites need no churn — only the server-fn return types change from
/// `ServerFnError` to `ApiError` (the forward-compatible leptos 0.9 shape).
#[cfg(feature = "ssr")]
fn sfe(e: BrainError) -> ApiError {
    ApiError::from(e)
}

/// Server-side input size caps applied to mutating server fns. These guard
/// against CPU/memory DoS from arbitrarily large payloads (e.g. `pulldown_cmark`
/// on a huge body) and keep repo writes sane. Tuned generously so legitimate
/// docs are never rejected; `MAX_ASSET_BYTES` (in `file_ops`) is the analogous
/// cap for binary uploads.
#[cfg(feature = "ssr")]
pub(crate) mod limits {
    use gitnodes_domain::BrainError;

    /// Markdown body of a single Brain file. 1 MiB is far above any real note.
    pub const MAX_MARKDOWN_BYTES: usize = 1024 * 1024;
    /// YAML frontmatter block of a single Brain file.
    pub const MAX_FRONTMATTER_BYTES: usize = 64 * 1024;
    /// Repo-relative path length for any file operation.
    pub const MAX_PATH_LEN: usize = 1024;
    /// Serialized saved-views YAML payload.
    pub const MAX_VIEWS_BYTES: usize = 256 * 1024;
    /// Free-text fields on work item mutations (assignee, label, etc.).
    pub const MAX_FIELD_LEN: usize = 4096;

    pub fn check_len(label: &str, value: &str, max: usize) -> Result<(), BrainError> {
        if value.len() > max {
            return Err(BrainError::parse(format!(
                "{label} too large ({} bytes; max {max})",
                value.len()
            )));
        }
        Ok(())
    }
}

#[cfg(feature = "ssr")]
fn target_from_ref(
    target: gitnodes_domain::TargetRef,
) -> Result<gitnodes_domain::TargetConfig, BrainError> {
    target
        .validate()
        .map_err(|e| BrainError::parse(format!("invalid target: {e}")))?;
    Ok(target.into())
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
    "LoadBrainConfigStatus",
    "LoadBrainConfigStatusForTarget",
    "LoadAuditLog",
    "ListSessions",
    "ListPendingSync",
    "RevokeSession",
    "GetCurrentUser",
    "GetProjectionStatus",
    "LoadBrainTemplate",
    "ListNodes",
    "ListWorkItems",
    "ReadNode",
    "LoadBrainGraph",
    "LoadWorkItemByPath",
    "LoadWorkItemComments",
    "ReadBrainFile",
    "ListBrainFiles",
    "SaveBrainFile",
    "DeleteBrainFile",
    "RenameBrainFile",
    "UploadAsset",
    "ListBrainFolders",
    "RefreshBrainGraph",
    "GetWriteCapabilities",
    "ListAccessibleTargets",
    "ResolveLegacyTarget",
    "LoadBrainGraphForTarget",
    "LoadBrainConfigForTarget",
    "TransitionWorkItem",
    "AssignWorkItem",
    "BindWorkItem",
    "ListViews",
    "PreviewViews",
    "SaveViews",
    "SearchBrain",
    "ListOpenPrs",
    "MergePullRequest",
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
    register_explicit::<LoadBrainConfigStatus>();
    register_explicit::<LoadBrainConfigStatusForTarget>();
    register_explicit::<LoadAuditLog>();
    register_explicit::<ListSessions>();
    register_explicit::<ListPendingSync>();
    register_explicit::<RevokeSession>();
    register_explicit::<GetCurrentUser>();
    register_explicit::<GetProjectionStatus>();
    register_explicit::<LoadBrainTemplate>();
    register_explicit::<ListNodes>();
    register_explicit::<ListWorkItems>();
    register_explicit::<ReadNode>();
    register_explicit::<LoadBrainGraph>();
    register_explicit::<LoadWorkItemByPath>();
    register_explicit::<LoadWorkItemComments>();
    register_explicit::<ReadBrainFile>();
    register_explicit::<ListBrainFiles>();
    register_explicit::<SaveBrainFile>();
    register_explicit::<DeleteBrainFile>();
    register_explicit::<RenameBrainFile>();
    register_explicit::<UploadAsset>();
    register_explicit::<ListBrainFolders>();
    register_explicit::<RefreshBrainGraph>();
    register_explicit::<GetWriteCapabilities>();
    register_explicit::<ListAccessibleTargets>();
    register_explicit::<ResolveLegacyTarget>();
    register_explicit::<LoadBrainGraphForTarget>();
    register_explicit::<LoadBrainConfigForTarget>();
    register_explicit::<TransitionWorkItem>();
    register_explicit::<AssignWorkItem>();
    register_explicit::<BindWorkItem>();
    register_explicit::<ListViews>();
    register_explicit::<PreviewViews>();
    register_explicit::<SaveViews>();
    register_explicit::<SearchBrain>();
    register_explicit::<ListOpenPrs>();
    register_explicit::<MergePullRequest>();
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
        include_str!("api/pull_requests.rs"),
        include_str!("api/search.rs"),
        include_str!("api/work_items.rs"),
    ];

    const PUBLIC_SERVER_FNS: &[&str] = &["GetAppConfig"];

    #[derive(Debug, PartialEq, Eq)]
    struct ServerFnSource {
        name: String,
        body: String,
    }

    /// Pull the struct name out of `#[server(Name, ...)]` or `#[server(\n    Name,\n ...`).
    fn extract_server_fn_names(src: &str) -> Vec<String> {
        extract_server_fns(src)
            .into_iter()
            .map(|server_fn| server_fn.name)
            .collect()
    }

    fn extract_server_fns(src: &str) -> Vec<ServerFnSource> {
        let mut server_fns = Vec::new();
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
            if name.is_empty() {
                continue;
            }

            let Some(fn_idx) = after
                .find("pub async fn")
                .map(|offset| idx + needle.len() + offset)
            else {
                continue;
            };
            let Some(open_idx) = src[fn_idx..].find('{').map(|offset| fn_idx + offset) else {
                continue;
            };
            let Some(close_idx) = find_matching_brace(src, open_idx) else {
                continue;
            };
            server_fns.push(ServerFnSource {
                name,
                body: src[open_idx + 1..close_idx].to_string(),
            });
        }
        server_fns
    }

    fn find_matching_brace(src: &str, open_idx: usize) -> Option<usize> {
        #[derive(Clone, Copy)]
        enum State {
            Normal,
            String { escaped: bool },
            Char { escaped: bool },
            LineComment,
            BlockComment,
        }

        let bytes = src.as_bytes();
        let mut state = State::Normal;
        let mut depth = 0usize;
        let mut i = open_idx;
        while i < bytes.len() {
            match state {
                State::Normal => match bytes[i] {
                    b'/' if bytes.get(i + 1) == Some(&b'/') => {
                        state = State::LineComment;
                        i += 1;
                    }
                    b'/' if bytes.get(i + 1) == Some(&b'*') => {
                        state = State::BlockComment;
                        i += 1;
                    }
                    b'"' => state = State::String { escaped: false },
                    b'\'' => state = State::Char { escaped: false },
                    b'{' => depth += 1,
                    b'}' => {
                        depth = depth.checked_sub(1)?;
                        if depth == 0 {
                            return Some(i);
                        }
                    }
                    _ => {}
                },
                State::String { escaped } => {
                    state = match (bytes[i], escaped) {
                        (_, true) => State::String { escaped: false },
                        (b'\\', false) => State::String { escaped: true },
                        (b'"', false) => State::Normal,
                        _ => State::String { escaped: false },
                    };
                }
                State::Char { escaped } => {
                    state = match (bytes[i], escaped) {
                        (_, true) => State::Char { escaped: false },
                        (b'\\', false) => State::Char { escaped: true },
                        (b'\'', false) => State::Normal,
                        _ => State::Char { escaped: false },
                    };
                }
                State::LineComment => {
                    if bytes[i] == b'\n' {
                        state = State::Normal;
                    }
                }
                State::BlockComment => {
                    if bytes[i] == b'*' && bytes.get(i + 1) == Some(&b'/') {
                        state = State::Normal;
                        i += 1;
                    }
                }
            }
            i += 1;
        }
        None
    }

    fn push_masked(out: &mut String, bytes: &[u8]) {
        for byte in bytes {
            out.push(if *byte == b'\n' { '\n' } else { ' ' });
        }
    }

    fn raw_string_bounds(bytes: &[u8], start: usize) -> Option<(usize, usize)> {
        let mut quote_idx = start.checked_add(1)?;
        if bytes.get(start) == Some(&b'b') && bytes.get(start + 1) == Some(&b'r') {
            quote_idx += 1;
        } else if bytes.get(start) != Some(&b'r') {
            return None;
        }

        let mut hashes = 0usize;
        while bytes.get(quote_idx) == Some(&b'#') {
            quote_idx += 1;
            hashes += 1;
        }
        if bytes.get(quote_idx) != Some(&b'"') {
            return None;
        }

        let mut i = quote_idx + 1;
        while i < bytes.len() {
            if bytes[i] == b'"'
                && (0..hashes).all(|offset| bytes.get(i + 1 + offset) == Some(&b'#'))
            {
                return Some((quote_idx, i + hashes));
            }
            i += 1;
        }
        Some((quote_idx, bytes.len().saturating_sub(1)))
    }

    fn code_without_comments_and_literals(src: &str) -> String {
        #[derive(Clone, Copy)]
        enum State {
            Normal,
            String { escaped: bool },
            LineComment,
            BlockComment { depth: usize },
        }

        let bytes = src.as_bytes();
        let mut out = String::with_capacity(src.len());
        let mut state = State::Normal;
        let mut i = 0usize;
        while i < bytes.len() {
            match state {
                State::Normal => {
                    if let Some((_quote_idx, end_idx)) = raw_string_bounds(bytes, i) {
                        push_masked(&mut out, &bytes[i..=end_idx]);
                        i = end_idx + 1;
                        continue;
                    }

                    match bytes[i] {
                        b'/' if bytes.get(i + 1) == Some(&b'/') => {
                            push_masked(&mut out, &bytes[i..i + 2]);
                            state = State::LineComment;
                            i += 2;
                            continue;
                        }
                        b'/' if bytes.get(i + 1) == Some(&b'*') => {
                            push_masked(&mut out, &bytes[i..i + 2]);
                            state = State::BlockComment { depth: 1 };
                            i += 2;
                            continue;
                        }
                        b'b' if bytes.get(i + 1) == Some(&b'"') => {
                            push_masked(&mut out, &bytes[i..i + 2]);
                            state = State::String { escaped: false };
                            i += 2;
                            continue;
                        }
                        b'"' => {
                            push_masked(&mut out, &bytes[i..=i]);
                            state = State::String { escaped: false };
                        }
                        byte => out.push(byte as char),
                    }
                }
                State::String { escaped } => {
                    push_masked(&mut out, &bytes[i..=i]);
                    state = match (bytes[i], escaped) {
                        (_, true) => State::String { escaped: false },
                        (b'\\', false) => State::String { escaped: true },
                        (b'"', false) => State::Normal,
                        _ => State::String { escaped: false },
                    };
                }
                State::LineComment => {
                    push_masked(&mut out, &bytes[i..=i]);
                    if bytes[i] == b'\n' {
                        state = State::Normal;
                    }
                }
                State::BlockComment { depth } => {
                    if bytes[i] == b'/' && bytes.get(i + 1) == Some(&b'*') {
                        push_masked(&mut out, &bytes[i..i + 2]);
                        state = State::BlockComment { depth: depth + 1 };
                        i += 2;
                        continue;
                    }
                    if bytes[i] == b'*' && bytes.get(i + 1) == Some(&b'/') {
                        push_masked(&mut out, &bytes[i..i + 2]);
                        state = if depth == 1 {
                            State::Normal
                        } else {
                            State::BlockComment { depth: depth - 1 }
                        };
                        i += 2;
                        continue;
                    }
                    push_masked(&mut out, &bytes[i..=i]);
                }
            }
            i += 1;
        }
        out
    }

    fn body_has_auth_gate(body: &str) -> bool {
        let code = code_without_comments_and_literals(body);
        [
            "session::require_session_and_token",
            "session::require_authenticated",
            "session::require_target_read",
            "session::require_current_target_read",
            "session::require_target_admin_session",
            "session::__assert_gated",
        ]
        .iter()
        .any(|needle| code.contains(needle))
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
    fn non_public_server_fns_have_auth_gate() {
        let server_fns = API_SOURCES
            .iter()
            .flat_map(|src| extract_server_fns(src))
            .collect::<Vec<_>>();
        let mut offenders = server_fns
            .iter()
            .filter(|server_fn| !PUBLIC_SERVER_FNS.contains(&server_fn.name.as_str()))
            .filter(|server_fn| !body_has_auth_gate(&server_fn.body))
            .map(|server_fn| server_fn.name.clone())
            .collect::<Vec<_>>();
        offenders.sort();

        assert!(
            offenders.is_empty(),
            "every non-public #[server(...)] fn must call a session auth gate \
             or session::__assert_gated() when the gate is delegated to an inner helper. \
             Intentionally public server fns belong in PUBLIC_SERVER_FNS. Missing: {offenders:?}"
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

    #[test]
    fn body_has_auth_gate_accepts_direct_gate_or_marker() {
        assert!(body_has_auth_gate(
            "let _ = session::require_authenticated().await?;"
        ));
        assert!(body_has_auth_gate(
            "let _ = session::require_target_read(&target).await?;"
        ));
        assert!(body_has_auth_gate("session::__assert_gated();"));
        assert!(!body_has_auth_gate("let target = session::target_cfg()?;"));
    }

    #[test]
    fn body_has_auth_gate_ignores_comments_and_literals() {
        assert!(!body_has_auth_gate(
            "// session::require_authenticated().await?;\nlet x = 1;"
        ));
        assert!(!body_has_auth_gate(
            "let s = \"session::require_authenticated\";"
        ));
        assert!(!body_has_auth_gate(
            "let s = r#\"session::require_session_and_token\"#;"
        ));
        assert!(!body_has_auth_gate(
            "/* session::__assert_gated(); */\nlet target = session::target_cfg()?;"
        ));
    }
}
