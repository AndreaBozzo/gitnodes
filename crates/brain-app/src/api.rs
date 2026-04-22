use leptos::prelude::*;
use serde::{Deserialize, Serialize};

use crate::knowledge::types::{BrainFilePayload, Edge, Node};
use brain_domain::{BrainConfig, BrandConfig, TargetConfig};

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

#[cfg(feature = "ssr")]
pub fn register_server_functions() {
    // LTO (`lto = true` in [profile.release]) strips the `inventory::submit!`
    // entries that `#[server]` relies on for automatic registration. Calling
    // `register_explicit` bypasses inventory and directly inserts each server
    // function into the global handler map.
    use leptos::server_fn::axum::register_explicit;
    register_explicit::<LoadAuditLog>();
    register_explicit::<ListSessions>();
    register_explicit::<RevokeSession>();
    register_explicit::<GetCurrentUser>();
    register_explicit::<LoadBrainTemplate>();
    register_explicit::<LoadBrainGraph>();
    register_explicit::<ReadBrainFile>();
    register_explicit::<SaveBrainFile>();
    register_explicit::<DeleteBrainFile>();
    register_explicit::<ListBrainFolders>();
    register_explicit::<GetAppConfig>();
    register_explicit::<UploadAsset>();
    register_explicit::<RenameBrainFile>();
    register_explicit::<LoadBrainConfig>();
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AppConfig {
    pub target: TargetConfig,
    pub brand: BrandConfig,
}

#[server(GetAppConfig, "/api", endpoint = "get_app_config")]
pub async fn get_app_config() -> Result<AppConfig, ServerFnError> {
    use crate::server::session;
    let target = session::target_cfg().map_err(sfe)?;
    let brand = use_context::<BrandConfig>()
        .ok_or_else(|| sfe(BrainError::other("No brand config available")))?;
    Ok(AppConfig { target, brand })
}

#[server(LoadBrainConfig, "/api", endpoint = "load_brain_config")]
pub async fn load_brain_config() -> Result<BrainConfig, ServerFnError> {
    use crate::knowledge::config_loader;
    use crate::server::session;
    let (_s, token) = session::require_session_and_token().await.map_err(sfe)?;
    let target = session::target_cfg().map_err(sfe)?;
    let cfg = config_loader::load(&target, &token).await;
    Ok((*cfg).clone())
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AuditEntry {
    pub id: i64,
    pub ts: String,
    pub kind: String,
    pub actor: Option<String>,
    pub detail: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SessionEntry {
    pub id: String,
    pub expiry_date: String,
}

#[server(LoadAuditLog, "/api", endpoint = "load_audit_log")]
pub async fn load_audit_log(
    kind: Option<String>,
    limit: Option<i64>,
) -> Result<Vec<AuditEntry>, ServerFnError> {
    use crate::server::session;
    let _ = session::require_authenticated().await.map_err(sfe)?;
    let rows = crate::server::audit::recent(limit.unwrap_or(200), kind.as_deref())
        .await
        .map_err(|e| sfe(BrainError::other(format!("DB: {e}"))))?;
    Ok(rows
        .into_iter()
        .map(|r| AuditEntry {
            id: r.id,
            ts: r.ts,
            kind: r.kind,
            actor: r.actor,
            detail: r.detail,
        })
        .collect())
}

#[server(ListSessions, "/api", endpoint = "list_sessions")]
pub async fn list_sessions() -> Result<Vec<SessionEntry>, ServerFnError> {
    use crate::server::session;
    let _ = session::require_authenticated().await.map_err(sfe)?;
    let rows = crate::server::audit::list_sessions(100)
        .await
        .map_err(|e| sfe(BrainError::other(format!("DB: {e}"))))?;
    Ok(rows
        .into_iter()
        .map(|r| SessionEntry {
            id: r.id,
            expiry_date: r.expiry_date,
        })
        .collect())
}

#[server(RevokeSession, "/api", endpoint = "revoke_session")]
pub async fn revoke_session(id: String) -> Result<u64, ServerFnError> {
    use crate::server::session;
    let s = session::require_authenticated().await.map_err(sfe)?;
    let actor = crate::server::auth::get_session_user(&s).await;
    let n = crate::server::audit::revoke_session(&id)
        .await
        .map_err(|e| sfe(BrainError::other(format!("DB: {e}"))))?;
    crate::server::audit::log("revoke_session", actor.as_deref(), &id).await;
    Ok(n)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BrainFile {
    pub path: String,
    pub sha: String,
    pub content: String,
    #[serde(default)]
    pub rendered_html: String,
}

#[server(GetCurrentUser, "/api", endpoint = "get_current_user")]
pub async fn get_current_user() -> Result<Option<String>, ServerFnError> {
    use crate::server::session;
    let s = session::session().map_err(sfe)?;
    Ok(crate::server::auth::get_session_user(&s).await)
}

#[server(LoadBrainTemplate, "/api", endpoint = "load_brain_template")]
pub async fn load_brain_template(node_type: String) -> Result<String, ServerFnError> {
    use crate::server::session;
    use brain_storage::{GithubStorage, Storage};
    let target = session::target_cfg().map_err(sfe)?;
    let (_s, token) = session::require_session_and_token().await.map_err(sfe)?;
    let config = crate::knowledge::config_loader::load(&target, &token).await;
    let Some(filename) = config
        .lookup(&node_type)
        .and_then(|s| s.template_filename.as_deref())
    else {
        return Ok(String::new());
    };
    let storage = GithubStorage::new(target);
    let raw = storage.load_template(&token, filename).await.map_err(sfe)?;
    let (body, _front) = crate::markdown::split_frontmatter(&raw);
    Ok(body.trim_start_matches('\n').to_string())
}

#[server(LoadBrainGraph, "/api", endpoint = "load_brain_graph")]
pub async fn load_brain_graph() -> Result<(Vec<Node>, Vec<Edge>), ServerFnError> {
    use crate::server::session;
    use brain_storage::{GithubStorage, Storage};
    let (_s, token) = session::require_session_and_token().await.map_err(sfe)?;
    let target = session::target_cfg().map_err(sfe)?;
    let config = crate::knowledge::config_loader::load(&target, &token).await;
    let storage = GithubStorage::new(target);
    storage.load_graph(&token, &config).await.map_err(sfe)
}

#[server(ReadBrainFile, "/api", endpoint = "read_brain_file")]
pub async fn read_brain_file(path: String) -> Result<BrainFile, ServerFnError> {
    use crate::server::session;
    use brain_storage::{GithubStorage, Storage};

    let (_s, token) = session::require_session_and_token().await.map_err(sfe)?;
    let cfg = session::target_cfg().map_err(sfe)?;
    let storage = GithubStorage::new(cfg.clone());
    let (content, sha) = storage.read_file(&token, &path).await.map_err(sfe)?;

    let (body, _fm) = crate::markdown::split_frontmatter(&content);
    let rendered_html = crate::markdown::render_for_file(body, &path, &cfg);

    Ok(BrainFile {
        path,
        sha,
        content,
        rendered_html,
    })
}

#[server(
    SaveBrainFile,
    "/api",
    input = server_fn::codec::Json,
    endpoint = "save_brain_file",
)]
pub async fn save_brain_file(payload: BrainFilePayload) -> Result<String, ServerFnError> {
    use crate::server::session;
    use brain_storage::{GithubStorage, Storage};

    let (s, token) = session::require_session_and_token().await.map_err(sfe)?;
    let user = session::session_user_or_fallback(&s).await;

    let target = session::target_cfg().map_err(sfe)?;
    let config = crate::knowledge::config_loader::load(&target, &token).await;

    let related_section = if payload.related.is_empty() {
        String::new()
    } else {
        let links: Vec<String> = payload
            .related
            .iter()
            .map(|path| {
                let label = path
                    .rsplit('/')
                    .next()
                    .unwrap_or(path)
                    .trim_end_matches(".md");
                format!("- [{}](../{})", label, path)
            })
            .collect();
        format!("\n## Related / See also\n\n{}\n", links.join("\n"))
    };

    let markdown = format!(
        "{}\n{}{}",
        merge_frontmatter(&payload, &user, &config),
        payload.body,
        related_section,
    );

    let file_path = match &payload.path {
        Some(p) if !p.is_empty() => p.clone(),
        _ => {
            let slug = payload
                .title
                .replace(' ', "-")
                .chars()
                .filter(|c| c.is_alphanumeric() || *c == '-')
                .collect::<String>();
            let dir = payload
                .folder
                .as_deref()
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(|| {
                    config
                        .lookup(&payload.node_type)
                        .map(|s| s.directory.as_str())
                        .unwrap_or("")
                })
                .trim_matches('/');
            if dir.is_empty() {
                format!("{}.md", slug)
            } else {
                format!("{}/{}.md", dir, slug)
            }
        }
    };

    let auto_msg = if payload.sha.is_some() {
        format!("Update {} via Brain UI", file_path)
    } else {
        format!("Create {} via Brain UI", file_path)
    };
    let commit_msg = sanitize_commit_message(payload.commit_message.as_deref()).unwrap_or(auto_msg);

    let storage = GithubStorage::new(target);
    let author_email = format!("{}@users.noreply.github.com", user);

    match storage
        .save_file(
            &token,
            &file_path,
            &markdown,
            payload.sha.as_deref(),
            &commit_msg,
            &user,
            &author_email,
        )
        .await
    {
        Ok(path) => {
            let kind = if payload.sha.is_some() {
                "update"
            } else {
                "create"
            };
            crate::server::audit::log(kind, Some(&user), &file_path).await;
            Ok(path)
        }
        Err(e) => {
            crate::server::audit::log("api_error", Some(&user), &format!("save {file_path}: {e}"))
                .await;
            Err(sfe(e))
        }
    }
}

#[server(DeleteBrainFile, "/api", endpoint = "delete_brain_file")]
pub async fn delete_brain_file(
    path: String,
    sha: String,
    commit_message: Option<String>,
) -> Result<(), ServerFnError> {
    use crate::server::session;
    use brain_storage::{GithubStorage, Storage};

    let (s, token) = session::require_session_and_token().await.map_err(sfe)?;
    let user = session::session_user_or_fallback(&s).await;
    let author_email = format!("{}@users.noreply.github.com", user);
    let commit_msg = sanitize_commit_message(commit_message.as_deref())
        .unwrap_or_else(|| format!("Delete {} via Brain UI", path));

    let storage = GithubStorage::new(session::target_cfg().map_err(sfe)?);
    match storage
        .delete_file(&token, &path, &sha, &commit_msg, &user, &author_email)
        .await
    {
        Ok(_) => {
            crate::server::audit::log("delete", Some(&user), &path).await;
            Ok(())
        }
        Err(e) => {
            crate::server::audit::log("api_error", Some(&user), &format!("delete {path}: {e}"))
                .await;
            Err(sfe(e))
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RenameResult {
    pub new_path: String,
    /// Paths of files whose links were rewritten to point at `new_path`.
    pub updated_referrers: Vec<String>,
}

/// Move a file to a new path and rewrite every markdown link that pointed at
/// the old path. Issues one commit per touched file (referrers, then the move
/// itself); we accept the commit churn to stay on the simple Contents API
/// rather than assembling a Git Data tree.
#[server(RenameBrainFile, "/api", endpoint = "rename_brain_file")]
pub async fn rename_brain_file(
    old_path: String,
    new_path: String,
    old_sha: String,
    commit_message: Option<String>,
) -> Result<RenameResult, ServerFnError> {
    use crate::server::session;
    use brain_storage::{GithubStorage, Storage};

    let old_path = old_path.trim().trim_matches('/').to_string();
    let new_path = new_path.trim().trim_matches('/').to_string();

    if new_path.is_empty() || old_path.is_empty() {
        return Err(sfe(BrainError::parse("Empty path")));
    }
    if new_path == old_path {
        return Err(sfe(BrainError::parse("New path matches old path")));
    }
    if !new_path.ends_with(".md") {
        return Err(sfe(BrainError::parse("New path must end in .md")));
    }
    if new_path.contains("..") || new_path.starts_with('/') {
        return Err(sfe(BrainError::parse("Invalid new path")));
    }

    let (s, token) = session::require_session_and_token().await.map_err(sfe)?;
    let user = session::session_user_or_fallback(&s).await;
    let author_email = format!("{}@users.noreply.github.com", user);
    let cfg = session::target_cfg().map_err(sfe)?;
    let storage = GithubStorage::new(cfg.clone());

    // Sanity: the source file still exists at the sha the client saw.
    let (old_content, live_sha) = storage.read_file(&token, &old_path).await.map_err(sfe)?;
    if live_sha != old_sha {
        return Err(sfe(BrainError::other(
            "File was modified since you opened it; reload and retry",
        )));
    }

    let config = crate::knowledge::config_loader::load(&cfg, &token).await;

    // Find every file that links to old_path. Walk the tree once, read each
    // candidate, and string-scan for link targets that resolve to old_path.
    let (_nodes, _edges) = storage.load_graph(&token, &config).await.map_err(sfe)?;
    let all_paths = collect_repo_md_paths(&token, &storage).await.map_err(sfe)?;

    let mut updated_referrers = Vec::<String>::new();
    for candidate in &all_paths {
        if candidate == &old_path {
            continue;
        }
        let (content, sha) = match storage.read_file(&token, candidate).await {
            Ok(v) => v,
            Err(_) => continue,
        };
        let Some(rewritten) = rewrite_links(&content, candidate, &old_path, &new_path) else {
            continue;
        };
        let msg = format!("Update links in {candidate} after rename via Brain UI");
        storage
            .save_file(
                &token,
                candidate,
                &rewritten,
                Some(&sha),
                &msg,
                &user,
                &author_email,
            )
            .await
            .map_err(sfe)?;
        updated_referrers.push(candidate.clone());
    }

    // Create the file at the new path, then delete the old one. A user-
    // supplied message only applies to these two "main" commits — the
    // "Update links in X" referrer commits keep their auto-generated msg
    // since they're side-effects of the move, not the user's stated intent.
    let user_msg = sanitize_commit_message(commit_message.as_deref());
    let create_msg = user_msg
        .clone()
        .unwrap_or_else(|| format!("Rename {old_path} → {new_path} via Brain UI"));
    storage
        .save_file(
            &token,
            &new_path,
            &old_content,
            None,
            &create_msg,
            &user,
            &author_email,
        )
        .await
        .map_err(sfe)?;

    let delete_msg = user_msg
        .unwrap_or_else(|| format!("Delete {old_path} (renamed to {new_path}) via Brain UI"));
    storage
        .delete_file(
            &token,
            &old_path,
            &old_sha,
            &delete_msg,
            &user,
            &author_email,
        )
        .await
        .map_err(sfe)?;

    crate::server::audit::log(
        "rename",
        Some(&user),
        &format!(
            "{old_path} -> {new_path} ({} referrers)",
            updated_referrers.len()
        ),
    )
    .await;

    Ok(RenameResult {
        new_path,
        updated_referrers,
    })
}

#[cfg(feature = "ssr")]
async fn collect_repo_md_paths(
    token: &str,
    storage: &brain_storage::GithubStorage,
) -> Result<Vec<String>, BrainError> {
    use brain_graph::is_included_md;
    // Reuse graph load's internal logic by re-reading the tree directly. Keep
    // this narrow — we only need paths, not parsed docs.
    let client = brain_storage::http_client()?;
    let _ = storage; // kept for future use if we swap to a storage method
    let cfg = crate::server::session::target_cfg()?;
    let url = brain_domain::GithubClient::new(cfg).tree_url();
    #[derive(serde::Deserialize)]
    struct Tree {
        tree: Vec<Entry>,
    }
    #[derive(serde::Deserialize)]
    struct Entry {
        path: String,
        #[serde(rename = "type")]
        kind: String,
    }
    let resp: Tree = client
        .get(&url)
        .bearer_auth(token)
        .send()
        .await
        .map_err(|e| BrainError::github(format!("tree fetch: {e}")))?
        .error_for_status()
        .map_err(|e| BrainError::github(format!("tree status: {e}")))?
        .json()
        .await
        .map_err(|e| BrainError::github(format!("tree parse: {e}")))?;
    Ok(resp
        .tree
        .into_iter()
        .filter(|e| e.kind == "blob" && is_included_md(&e.path))
        .map(|e| e.path)
        .collect())
}

/// Given the content of `file_path`, rewrite any `](X)` whose X resolves to
/// `old_target` so X becomes the correct relative path to `new_target`.
/// Returns `None` if nothing changed.
#[cfg(feature = "ssr")]
fn rewrite_links(
    content: &str,
    file_path: &str,
    old_target: &str,
    new_target: &str,
) -> Option<String> {
    use std::path::Path;

    let from_dir = Path::new(file_path).parent().unwrap_or(Path::new(""));
    let new_rel = relativize(from_dir, new_target);

    let mut out = String::with_capacity(content.len());
    let mut i = 0;
    let bytes = content.as_bytes();
    let mut changed = false;
    while i < bytes.len() {
        if bytes[i] == b']'
            && i + 1 < bytes.len()
            && bytes[i + 1] == b'('
            && let Some(end) = content[i + 2..].find(')')
        {
            let url = &content[i + 2..i + 2 + end];
            let (path_part, fragment) = match url.split_once('#') {
                Some((p, f)) => (p, Some(f)),
                None => (url, None),
            };
            if !path_part.starts_with("http")
                && path_part.ends_with(".md")
                && resolve_link_path(from_dir, path_part) == old_target
            {
                out.push_str("](");
                out.push_str(&new_rel);
                if let Some(f) = fragment {
                    out.push('#');
                    out.push_str(f);
                }
                out.push(')');
                i = i + 2 + end + 1;
                changed = true;
                continue;
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    changed.then_some(out)
}

#[cfg(feature = "ssr")]
fn resolve_link_path(from_dir: &std::path::Path, link: &str) -> String {
    use std::path::Path;
    let joined = from_dir.join(link);
    let mut parts: Vec<&str> = Vec::new();
    for comp in Path::new(&joined).iter() {
        let Some(s) = comp.to_str() else {
            return String::new();
        };
        if s == "." {
            continue;
        } else if s == ".." {
            parts.pop();
        } else {
            parts.push(s);
        }
    }
    parts.join("/")
}

/// Shortest relative path from `from_dir` to `target` (both repo-rooted).
#[cfg(feature = "ssr")]
fn relativize(from_dir: &std::path::Path, target: &str) -> String {
    let from_parts: Vec<&str> = from_dir
        .to_str()
        .unwrap_or("")
        .split('/')
        .filter(|s| !s.is_empty())
        .collect();
    let target_parts: Vec<&str> = target.split('/').filter(|s| !s.is_empty()).collect();

    let mut common = 0;
    while common < from_parts.len()
        && common < target_parts.len() - 1
        && from_parts[common] == target_parts[common]
    {
        common += 1;
    }

    let ups = from_parts.len() - common;
    let mut out = String::new();
    for _ in 0..ups {
        out.push_str("../");
    }
    if ups == 0 {
        out.push_str("./");
    }
    out.push_str(&target_parts[common..].join("/"));
    out
}

/// Max size for a single asset upload. GitHub Contents API accepts larger, but
/// we keep it modest to stay responsive and avoid ballooning the repo.
#[cfg(feature = "ssr")]
const MAX_ASSET_BYTES: usize = 2 * 1024 * 1024;

#[server(
    UploadAsset,
    "/api",
    input = server_fn::codec::Json,
    endpoint = "upload_asset",
)]
pub async fn upload_asset(filename: String, bytes: Vec<u8>) -> Result<String, ServerFnError> {
    use crate::server::session;
    use brain_storage::{GithubStorage, Storage};

    if bytes.is_empty() {
        return Err(sfe(BrainError::parse("Empty upload")));
    }
    if bytes.len() > MAX_ASSET_BYTES {
        return Err(sfe(BrainError::parse(format!(
            "Upload too large ({} bytes; max {})",
            bytes.len(),
            MAX_ASSET_BYTES
        ))));
    }

    let (stem, ext) = split_filename(&filename);
    if !is_allowed_image_ext(&ext) {
        return Err(sfe(BrainError::parse(format!(
            "Unsupported file extension: .{ext}"
        ))));
    }

    let (s, token) = session::require_session_and_token().await.map_err(sfe)?;
    let user = session::session_user_or_fallback(&s).await;
    let author_email = format!("{}@users.noreply.github.com", user);

    let today = time::OffsetDateTime::now_utc();
    let short_hash = short_content_hash(&bytes);
    let slug = slugify(&stem);
    let asset_path = format!(
        "assets/{:04}/{:02}/{}-{}.{}",
        today.year(),
        today.month() as u8,
        slug,
        short_hash,
        ext,
    );

    let commit_msg = format!("Upload {asset_path} via Brain UI");
    let storage = GithubStorage::new(session::target_cfg().map_err(sfe)?);
    match storage
        .upload_binary(
            &token,
            &asset_path,
            &bytes,
            &commit_msg,
            &user,
            &author_email,
        )
        .await
    {
        Ok(path) => {
            crate::server::audit::log("upload_asset", Some(&user), &asset_path).await;
            Ok(path)
        }
        Err(e) => {
            crate::server::audit::log(
                "api_error",
                Some(&user),
                &format!("upload_asset {asset_path}: {e}"),
            )
            .await;
            Err(sfe(e))
        }
    }
}

#[cfg(feature = "ssr")]
fn split_filename(filename: &str) -> (String, String) {
    let name = filename.rsplit('/').next().unwrap_or(filename);
    match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => (stem.to_string(), ext.to_lowercase()),
        _ => (name.to_string(), String::new()),
    }
}

#[cfg(feature = "ssr")]
fn is_allowed_image_ext(ext: &str) -> bool {
    matches!(ext, "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg")
}

#[cfg(feature = "ssr")]
fn slugify(stem: &str) -> String {
    let mut out = String::with_capacity(stem.len());
    let mut prev_dash = false;
    for c in stem.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
            prev_dash = false;
        } else if !prev_dash && !out.is_empty() {
            out.push('-');
            prev_dash = true;
        }
    }
    let trimmed = out.trim_matches('-').to_string();
    if trimmed.is_empty() {
        "asset".to_string()
    } else if trimmed.len() > 40 {
        trimmed.chars().take(40).collect()
    } else {
        trimmed
    }
}

/// Short content-derived suffix so two uploads with the same slug don't collide.
/// Not cryptographic — just needs to be stable and short.
#[cfg(feature = "ssr")]
fn short_content_hash(bytes: &[u8]) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    bytes.hash(&mut h);
    format!("{:x}", h.finish()).chars().take(8).collect()
}

#[server(ListBrainFolders, "/api", endpoint = "list_brain_folders")]
pub async fn list_brain_folders() -> Result<Vec<String>, ServerFnError> {
    use crate::server::session;
    use brain_storage::{GithubStorage, Storage};

    let (_s, token) = session::require_session_and_token().await.map_err(sfe)?;
    let storage = GithubStorage::new(session::target_cfg().map_err(sfe)?);
    storage.list_folders(&token).await.map_err(sfe)
}

#[cfg(feature = "ssr")]
fn today_iso() -> String {
    let today = time::OffsetDateTime::now_utc();
    format!(
        "{:04}-{:02}-{:02}",
        today.year(),
        today.month() as u8,
        today.day()
    )
}

/// Build the final frontmatter block by merging the form's authoritative
/// fields onto the document's preserved map (update) or onto a seeded
/// template (create). Preserves custom keys (status, severity, cliente,
/// etc.) that the form doesn't manage, per the fix for caveat #5.
#[cfg(feature = "ssr")]
fn merge_frontmatter(payload: &BrainFilePayload, author: &str, config: &BrainConfig) -> String {
    use serde_yaml::Value;

    if payload.node_type == "tag"
        || config.synthetic_tag_spec().map(|s| s.name.as_str()) == Some(payload.node_type.as_str())
    {
        return String::new();
    }

    let date = today_iso();
    let is_update = payload.preserved_frontmatter.is_some();
    let spec = config
        .lookup(&payload.node_type)
        .unwrap_or_else(|| config.default_spec());

    let mut map = payload
        .preserved_frontmatter
        .clone()
        .unwrap_or_else(|| spec.frontmatter_seed.clone());

    // Form-authoritative fields: always overwrite.
    map.insert("type".into(), Value::String(spec.name.clone()));
    map.insert("author".into(), Value::String(author.to_string()));
    map.insert(
        "tags".into(),
        Value::Sequence(
            payload
                .tags
                .iter()
                .map(|t| Value::String(t.clone()))
                .collect(),
        ),
    );
    // `topic` is controlled by the `title` form field for the types that use it.
    if payload.node_type == "concept" || payload.node_type == "preventivo" {
        let key = if payload.node_type == "preventivo" {
            "progetto"
        } else {
            "topic"
        };
        map.insert(key.into(), Value::String(payload.title.clone()));
    }

    if !is_update {
        // Replicate legacy date injections for default types to avoid breaking existing workflows,
        // but only if they are the default types.
        match payload.node_type.as_str() {
            "concept" => {
                map.insert("date_created".into(), Value::String(date.clone()));
            }
            "adr" | "meeting" | "preventivo" => {
                map.insert("date".into(), Value::String(date.clone()));
            }
            "post-mortem" => {
                map.insert("incident_date".into(), Value::String(date.clone()));
            }
            _ => {}
        }
    } else if payload.node_type == "runbook" {
        map.insert("last_updated".into(), Value::String(date));
    }

    match serde_yaml::to_string(&map) {
        Ok(yaml) => format!("---\n{}---\n", yaml),
        Err(_) => String::new(),
    }
}

#[cfg(all(test, feature = "ssr"))]
mod merge_frontmatter_tests {
    use super::*;
    use std::collections::BTreeMap;

    fn base_payload(node_type: String) -> BrainFilePayload {
        BrainFilePayload {
            node_type,
            title: "T".into(),
            author: "alice".into(),
            tags: vec!["x".into()],
            body: String::new(),
            related: vec![],
            folder: None,
            path: Some("adrs/F.md".into()),
            sha: Some("sha".into()),
            commit_message: None,
            preserved_frontmatter: None,
            frontmatter_malformed: false,
        }
    }

    #[test]
    fn update_preserves_custom_fields() {
        let mut preserved = BTreeMap::new();
        preserved.insert(
            "status".into(),
            serde_yaml::Value::String("accepted".into()),
        );
        preserved.insert(
            "date".into(),
            serde_yaml::Value::String("2026-03-01".into()),
        );
        let mut payload = base_payload("adr".to_string());
        payload.preserved_frontmatter = Some(preserved);

        let out = merge_frontmatter(&payload, "bob", &BrainConfig::default());
        assert!(out.contains("status: accepted"), "out was: {out}");
        assert!(out.contains("date: 2026-03-01"), "out was: {out}");
        assert!(out.contains("author: bob"), "out was: {out}");
        assert!(out.contains("type: adr"), "out was: {out}");
    }

    #[test]
    fn create_seeds_defaults() {
        let payload = base_payload("adr".to_string());
        let out = merge_frontmatter(&payload, "alice", &BrainConfig::default());
        assert!(out.contains("type: adr"));
        assert!(out.contains("status: draft"));
        assert!(out.starts_with("---\n"));
        assert!(out.ends_with("---\n"));
    }

    #[test]
    fn tag_type_emits_empty() {
        let payload = base_payload("tag".to_string());
        assert_eq!(
            merge_frontmatter(&payload, "x", &BrainConfig::default()),
            ""
        );
    }

    #[test]
    fn form_fields_win_over_preserved() {
        let mut preserved = BTreeMap::new();
        preserved.insert("author".into(), serde_yaml::Value::String("old".into()));
        preserved.insert(
            "tags".into(),
            serde_yaml::Value::Sequence(vec![serde_yaml::Value::String("stale".into())]),
        );
        let mut payload = base_payload("adr".to_string());
        payload.preserved_frontmatter = Some(preserved);
        payload.tags = vec!["new".into()];

        let out = merge_frontmatter(&payload, "bob", &BrainConfig::default());
        assert!(out.contains("author: bob"));
        assert!(out.contains("- new"));
        assert!(!out.contains("old"));
        assert!(!out.contains("stale"));
    }
}
