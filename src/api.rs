use leptos::prelude::*;
use serde::{Deserialize, Serialize};

#[cfg(feature = "ssr")]
use crate::knowledge::types::NodeType;
use crate::knowledge::types::{BrainFilePayload, Edge, Node};

#[cfg(feature = "ssr")]
use brain_domain::BrainError;

/// Convert domain errors to the stringly `ServerFnError` that Leptos speaks on
/// the wire. Typed matching happens inside the server fn; the client sees a
/// `Display` representation.
#[cfg(feature = "ssr")]
fn sfe(e: BrainError) -> ServerFnError {
    ServerFnError::new(e.to_string())
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

#[server(LoadAuditLog, "/api")]
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

#[server(ListSessions, "/api")]
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

#[server(RevokeSession, "/api")]
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

/// Result of reading a file from the Brain repo.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BrainFile {
    pub path: String,
    pub sha: String,
    pub content: String,
    /// Sanitized HTML rendered from the markdown body (frontmatter stripped).
    #[serde(default)]
    pub rendered_html: String,
}

/// Get the current user's GitHub login (or None if not logged in).
#[server(GetCurrentUser, "/api")]
pub async fn get_current_user() -> Result<Option<String>, ServerFnError> {
    use crate::server::session;
    let s = session::session().map_err(sfe)?;
    Ok(crate::server::auth::get_session_user(&s).await)
}

/// Fetch the template body for a given NodeType. Returns the markdown body
/// (frontmatter stripped) so the editor can prefill the textarea with the scaffold.
#[server(LoadBrainTemplate, "/api")]
pub async fn load_brain_template(
    node_type: crate::knowledge::types::NodeType,
) -> Result<String, ServerFnError> {
    use crate::server::session;
    let Some(filename) = node_type.template_filename() else {
        return Ok(String::new());
    };
    let (_s, token) = session::require_session_and_token().await.map_err(sfe)?;
    let raw = crate::knowledge::runtime::load_template(&token, filename)
        .await
        .map_err(sfe)?;
    let (body, _front) = crate::markdown::split_frontmatter(&raw);
    Ok(body.trim_start_matches('\n').to_string())
}

/// Load the full knowledge graph (nodes + edges) live from the Brain repo.
/// Runs on every `/knowledge` render — replaces the compile-time bake from `build.rs`.
#[server(LoadBrainGraph, "/api")]
pub async fn load_brain_graph() -> Result<(Vec<Node>, Vec<Edge>), ServerFnError> {
    use crate::server::session;
    let (_s, token) = session::require_session_and_token().await.map_err(sfe)?;
    crate::knowledge::runtime::load_graph(&token)
        .await
        .map_err(sfe)
}

/// Read a single file from the Brain repo.
#[server(ReadBrainFile, "/api")]
pub async fn read_brain_file(path: String) -> Result<BrainFile, ServerFnError> {
    use crate::server::session;
    use brain_storage as github;

    let (_s, token) = session::require_session_and_token().await.map_err(sfe)?;
    let crab = github::client(token).map_err(sfe)?;

    let content = crab
        .repos(github::OWNER, github::REPO)
        .get_content()
        .path(&path)
        .r#ref("main")
        .send()
        .await
        .map_err(|e| sfe(BrainError::github(format!("get_content: {e}"))))?;

    let item = content
        .items
        .into_iter()
        .next()
        .ok_or_else(|| sfe(BrainError::NotFound(path.clone())))?;

    let sha = item.sha.clone();
    let decoded = item
        .decoded_content()
        .ok_or_else(|| sfe(BrainError::parse("cannot decode file content")))?;

    let (body, _fm) = crate::markdown::split_frontmatter(&decoded);
    let rendered_html = crate::markdown::render(body);

    Ok(BrainFile {
        path,
        sha,
        content: decoded,
        rendered_html,
    })
}

/// Create or update a file in the Brain repo.
#[server(
    SaveBrainFile,
    "/api",
    input = server_fn::codec::Json,
)]
pub async fn save_brain_file(payload: BrainFilePayload) -> Result<String, ServerFnError> {
    use crate::server::session;
    use brain_storage as github;

    let (s, token) = session::require_session_and_token().await.map_err(sfe)?;
    let user = session::session_user_or_fallback(&s).await;
    let crab = github::client(token).map_err(sfe)?;

    let markdown = format!(
        "{}\n{}",
        generate_frontmatter(&payload, &user),
        payload.body
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

    let encoded = base64::Engine::encode(
        &base64::engine::general_purpose::STANDARD,
        markdown.as_bytes(),
    );

    let mut body = serde_json::json!({
        "message": commit_msg,
        "content": encoded,
        "branch": "main",
        "committer": {
            "name": user,
            "email": format!("{}@users.noreply.github.com", user),
        }
    });

    if let Some(sha) = &payload.sha
        && !sha.is_empty()
    {
        body["sha"] = serde_json::json!(sha);
    }

    let url = github::contents_url(&file_path);

    let response = crab
        ._put(url, Some(&body))
        .await
        .map_err(|e| sfe(BrainError::github(format!("PUT: {e}"))))?;

    if response.status().is_success() {
        crate::knowledge::runtime::invalidate();
        let kind = if payload.sha.is_some() {
            "update"
        } else {
            "create"
        };
        crate::server::audit::log(kind, Some(&user), &file_path).await;
        Ok(file_path)
    } else {
        let status = response.status();
        crate::server::audit::log(
            "api_error",
            Some(&user),
            &format!("save {file_path}: {status}"),
        )
        .await;
        Err(sfe(BrainError::github(format!("API error {status}"))))
    }
}

/// Delete a file from the Brain repo.
#[server(DeleteBrainFile, "/api")]
pub async fn delete_brain_file(path: String, sha: String) -> Result<(), ServerFnError> {
    use crate::server::session;
    use brain_storage as github;

    let (s, token) = session::require_session_and_token().await.map_err(sfe)?;
    let user = session::session_user_or_fallback(&s).await;
    let crab = github::client(token).map_err(sfe)?;

    let body = serde_json::json!({
        "message": format!("Delete {} via Brain UI", path),
        "sha": sha,
        "branch": "main",
        "committer": {
            "name": user,
            "email": format!("{}@users.noreply.github.com", user),
        }
    });

    let url = github::contents_url(&path);

    let response = crab
        ._delete(url, Some(&body))
        .await
        .map_err(|e| sfe(BrainError::github(format!("DELETE: {e}"))))?;

    if response.status().is_success() {
        crate::knowledge::runtime::invalidate();
        crate::server::audit::log("delete", Some(&user), &path).await;
        Ok(())
    } else {
        let status = response.status();
        crate::server::audit::log(
            "api_error",
            Some(&user),
            &format!("delete {path}: {status}"),
        )
        .await;
        Err(sfe(BrainError::github(format!("API error {status}"))))
    }
}

/// Create a folder (section) in the Brain repo.
/// GitHub doesn't support empty directories; we create a README.md placeholder.
#[server(CreateFolder, "/api")]
pub async fn create_folder(folder_path: String) -> Result<String, ServerFnError> {
    use crate::server::session;
    use brain_storage as github;

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
    let crab = github::client(token).map_err(sfe)?;

    let folder_title = sanitized.rsplit('/').next().unwrap_or(sanitized);

    let readme_content = format!("# {folder_title}\n\n(Section created via Brain UI)\n");
    let encoded = base64::Engine::encode(
        &base64::engine::general_purpose::STANDARD,
        readme_content.as_bytes(),
    );

    let file_path = format!("{sanitized}/README.md");
    let body = serde_json::json!({
        "message": format!("Create section {sanitized}/ via Brain UI"),
        "content": encoded,
        "branch": "main",
        "committer": {
            "name": user,
            "email": format!("{}@users.noreply.github.com", user),
        }
    });

    let url = github::contents_url(&file_path);

    let response = crab
        ._put(url, Some(&body))
        .await
        .map_err(|e| sfe(BrainError::github(format!("PUT: {e}"))))?;

    if response.status().is_success() {
        crate::knowledge::runtime::invalidate();
        crate::server::audit::log("create_folder", Some(&user), sanitized).await;
        Ok(file_path)
    } else {
        let status = response.status();
        crate::server::audit::log(
            "api_error",
            Some(&user),
            &format!("create_folder {sanitized}: {status}"),
        )
        .await;
        Err(sfe(BrainError::github(format!("API error {status}"))))
    }
}

/// List top-level directories in the Brain repo (for the folder picker).
#[server(ListBrainFolders, "/api")]
pub async fn list_brain_folders() -> Result<Vec<String>, ServerFnError> {
    use crate::server::session;
    use brain_storage as github;

    let (_s, token) = session::require_session_and_token().await.map_err(sfe)?;
    let crab = github::client(token).map_err(sfe)?;

    let content = crab
        .repos(github::OWNER, github::REPO)
        .get_content()
        .path("")
        .r#ref("main")
        .send()
        .await
        .map_err(|e| sfe(BrainError::github(format!("get_content: {e}"))))?;

    let folders: Vec<String> = content
        .items
        .iter()
        .filter(|item| item.r#type == "dir")
        .map(|item| item.path.clone())
        .collect();

    Ok(folders)
}

/// Generate just the YAML frontmatter block (including the trailing `---\n`).
/// Each NodeType gets the frontmatter fields its template in the Brain repo uses.
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
