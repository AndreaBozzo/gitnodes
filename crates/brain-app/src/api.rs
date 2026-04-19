use leptos::prelude::*;
use serde::{Deserialize, Serialize};

#[cfg(feature = "ssr")]
use crate::knowledge::types::NodeType;
use crate::knowledge::types::{BrainFilePayload, Edge, Node};

#[cfg(feature = "ssr")]
use brain_domain::BrainError;

#[cfg(feature = "ssr")]
fn sfe(e: BrainError) -> ServerFnError {
    ServerFnError::new(e.to_string())
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
    register_explicit::<CreateFolder>();
    register_explicit::<ListBrainFolders>();
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
pub async fn load_brain_template(
    node_type: crate::knowledge::types::NodeType,
) -> Result<String, ServerFnError> {
    use crate::server::session;
    use brain_storage::{GithubStorage, Storage};
    let Some(filename) = node_type.template_filename() else {
        return Ok(String::new());
    };
    let (_s, token) = session::require_session_and_token().await.map_err(sfe)?;
    let storage = GithubStorage::new();
    let raw = storage.load_template(&token, filename).await.map_err(sfe)?;
    let (body, _front) = crate::markdown::split_frontmatter(&raw);
    Ok(body.trim_start_matches('\n').to_string())
}

#[server(LoadBrainGraph, "/api", endpoint = "load_brain_graph")]
pub async fn load_brain_graph() -> Result<(Vec<Node>, Vec<Edge>), ServerFnError> {
    use crate::server::session;
    use brain_storage::{GithubStorage, Storage};
    let (_s, token) = session::require_session_and_token().await.map_err(sfe)?;
    let storage = GithubStorage::new();
    storage.load_graph(&token).await.map_err(sfe)
}

#[server(ReadBrainFile, "/api", endpoint = "read_brain_file")]
pub async fn read_brain_file(path: String) -> Result<BrainFile, ServerFnError> {
    use crate::server::session;
    use brain_storage::{GithubStorage, Storage};

    let (_s, token) = session::require_session_and_token().await.map_err(sfe)?;
    let storage = GithubStorage::new();
    let (content, sha) = storage.read_file(&token, &path).await.map_err(sfe)?;

    let (body, _fm) = crate::markdown::split_frontmatter(&content);
    let rendered_html = crate::markdown::render_for_file(body, &path);

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
        generate_frontmatter(&payload, &user),
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
            format!("{}/{}.md", payload.node_type.directory(), slug)
        }
    };

    let commit_msg = if payload.sha.is_some() {
        format!("Update {} via Brain UI", file_path)
    } else {
        format!("Create {} via Brain UI", file_path)
    };

    let storage = GithubStorage::new();
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
pub async fn delete_brain_file(path: String, sha: String) -> Result<(), ServerFnError> {
    use crate::server::session;
    use brain_storage::{GithubStorage, Storage};

    let (s, token) = session::require_session_and_token().await.map_err(sfe)?;
    let user = session::session_user_or_fallback(&s).await;
    let author_email = format!("{}@users.noreply.github.com", user);
    let commit_msg = format!("Delete {} via Brain UI", path);

    let storage = GithubStorage::new();
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

#[server(CreateFolder, "/api", endpoint = "create_folder")]
pub async fn create_folder(folder_path: String) -> Result<String, ServerFnError> {
    use crate::server::session;
    use brain_storage::{GithubStorage, Storage};

    let sanitized = folder_path.trim().trim_matches('/');
    if sanitized.is_empty()
        || sanitized.contains("..")
        || !sanitized
            .chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == '_' || c == '/')
    {
        return Err(sfe(BrainError::parse("Invalid folder name")));
    }

    let (s, token) = session::require_session_and_token().await.map_err(sfe)?;
    let user = session::session_user_or_fallback(&s).await;
    let commit_msg = format!("Create section {sanitized}/ via Brain UI");
    let author_email = format!("{}@users.noreply.github.com", user);

    let storage = GithubStorage::new();
    match storage
        .create_folder(&token, sanitized, &commit_msg, &user, &author_email)
        .await
    {
        Ok(path) => {
            crate::server::audit::log("create_folder", Some(&user), sanitized).await;
            Ok(path)
        }
        Err(e) => {
            crate::server::audit::log(
                "api_error",
                Some(&user),
                &format!("create_folder {sanitized}: {e}"),
            )
            .await;
            Err(sfe(e))
        }
    }
}

#[server(ListBrainFolders, "/api", endpoint = "list_brain_folders")]
pub async fn list_brain_folders() -> Result<Vec<String>, ServerFnError> {
    use crate::server::session;
    use brain_storage::{GithubStorage, Storage};

    let (_s, token) = session::require_session_and_token().await.map_err(sfe)?;
    let storage = GithubStorage::new();
    storage.list_folders(&token).await.map_err(sfe)
}

#[cfg(feature = "ssr")]
fn generate_frontmatter(payload: &BrainFilePayload, author: &str) -> String {
    let today = time::OffsetDateTime::now_utc();
    let date = format!(
        "{:04}-{:02}-{:02}",
        today.year(),
        today.month() as u8,
        today.day()
    );
    let tags_str = format!(
        "[{}]",
        payload
            .tags
            .iter()
            .map(|t| format!("\"{}\"", t))
            .collect::<Vec<_>>()
            .join(", ")
    );

    match payload.node_type {
        NodeType::Concept => format!(
            "---\ntype: concept\ntopic: \"{title}\"\ndate_created: {date}\nauthor: {author}\ntags: {tags}\n---\n",
            title = payload.title,
            tags = tags_str,
        ),
        NodeType::Decision => format!(
            "---\ntype: adr\nstatus: draft\ndate: {date}\nauthor: {author}\ntags: {tags}\n---\n",
            tags = tags_str,
        ),
        NodeType::Meeting => format!(
            "---\ntype: meeting\ndate: {date}\nauthor: {author}\ntags: {tags}\n---\n",
            tags = tags_str,
        ),
        NodeType::PostMortem => format!(
            "---\ntype: post-mortem\nincident_date: {date}\nseverity: \nauthor: {author}\ntags: {tags}\n---\n",
            tags = tags_str,
        ),
        NodeType::Preventivo => format!(
            "---\ntype: preventivo\nstatus: draft\ndate: {date}\nauthor: {author}\ncliente: \nprogetto: \"{title}\"\nmodello: T&M\ntags: {tags}\n---\n",
            title = payload.title,
            tags = tags_str,
        ),
        NodeType::Runbook => format!(
            "---\ntype: runbook\nservice: \nlast_updated: {date}\nauthor: {author}\ntags: {tags}\n---\n",
            tags = tags_str,
        ),
        NodeType::Tag => String::new(),
    }
}
