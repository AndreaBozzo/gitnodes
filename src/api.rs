use leptos::prelude::*;
use serde::{Deserialize, Serialize};

use crate::knowledge::types::BrainFilePayload;
#[cfg(feature = "ssr")]
use crate::knowledge::types::NodeType;

#[cfg(feature = "ssr")]
const OWNER: &str = "Dritara-Digital";
#[cfg(feature = "ssr")]
const REPO: &str = "Brain";

/// Result of reading a file from the Brain repo.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BrainFile {
    pub path: String,
    pub sha: String,
    pub content: String,
}

/// Get the current user's GitHub login (or None if not logged in).
#[server(GetCurrentUser, "/api")]
pub async fn get_current_user() -> Result<Option<String>, ServerFnError> {
    use tower_sessions::Session;
    let session =
        use_context::<Session>().ok_or_else(|| ServerFnError::new("No session available"))?;
    Ok(crate::server::auth::get_session_user(&session).await)
}

/// Read a single file from the Brain repo.
#[server(ReadBrainFile, "/api")]
pub async fn read_brain_file(path: String) -> Result<BrainFile, ServerFnError> {
    use tower_sessions::Session;
    let session =
        use_context::<Session>().ok_or_else(|| ServerFnError::new("No session available"))?;
    let token = crate::server::auth::get_session_token(&session)
        .await
        .ok_or_else(|| ServerFnError::new("Not authenticated"))?;

    let crab = octocrab::Octocrab::builder()
        .personal_token(token)
        .build()
        .map_err(|e| ServerFnError::new(format!("Octocrab init: {e}")))?;

    let content = crab
        .repos(OWNER, REPO)
        .get_content()
        .path(&path)
        .r#ref("main")
        .send()
        .await
        .map_err(|e| ServerFnError::new(format!("GitHub API: {e}")))?;

    let item = content
        .items
        .into_iter()
        .next()
        .ok_or_else(|| ServerFnError::new("File not found"))?;

    let sha = item.sha.clone();
    let decoded = item
        .decoded_content()
        .ok_or_else(|| ServerFnError::new("Cannot decode file content"))?;

    Ok(BrainFile {
        path,
        sha,
        content: decoded,
    })
}

/// Create or update a file in the Brain repo.
#[server(SaveBrainFile, "/api")]
pub async fn save_brain_file(payload: BrainFilePayload) -> Result<String, ServerFnError> {
    use tower_sessions::Session;
    let session =
        use_context::<Session>().ok_or_else(|| ServerFnError::new("No session available"))?;
    let token = crate::server::auth::get_session_token(&session)
        .await
        .ok_or_else(|| ServerFnError::new("Not authenticated"))?;
    let user = crate::server::auth::get_session_user(&session)
        .await
        .unwrap_or_else(|| "brain_ui".to_string());

    let crab = octocrab::Octocrab::builder()
        .personal_token(token)
        .build()
        .map_err(|e| ServerFnError::new(format!("Octocrab init: {e}")))?;

    // Build the markdown content with auto-generated frontmatter
    let markdown = generate_markdown(&payload, &user);

    // Determine the file path
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

    // Build the PUT request body
    let mut body = serde_json::json!({
        "message": commit_msg,
        "content": encoded,
        "branch": "main",
        "committer": {
            "name": user,
            "email": format!("{}@users.noreply.github.com", user),
        }
    });

    if let Some(sha) = &payload.sha {
        if !sha.is_empty() {
            body["sha"] = serde_json::json!(sha);
        }
    }

    let url = format!("https://api.github.com/repos/{OWNER}/{REPO}/contents/{file_path}");

    let response = crab
        ._put(url, Some(&body))
        .await
        .map_err(|e| ServerFnError::new(format!("GitHub PUT: {e}")))?;

    if response.status().is_success() {
        Ok(file_path)
    } else {
        let status = response.status();
        Err(ServerFnError::new(format!("GitHub API error {status}")))
    }
}

/// Delete a file from the Brain repo.
#[server(DeleteBrainFile, "/api")]
pub async fn delete_brain_file(path: String, sha: String) -> Result<(), ServerFnError> {
    use tower_sessions::Session;
    let session =
        use_context::<Session>().ok_or_else(|| ServerFnError::new("No session available"))?;
    let token = crate::server::auth::get_session_token(&session)
        .await
        .ok_or_else(|| ServerFnError::new("Not authenticated"))?;
    let user = crate::server::auth::get_session_user(&session)
        .await
        .unwrap_or_else(|| "brain_ui".to_string());

    let crab = octocrab::Octocrab::builder()
        .personal_token(token)
        .build()
        .map_err(|e| ServerFnError::new(format!("Octocrab init: {e}")))?;

    let body = serde_json::json!({
        "message": format!("Delete {} via Brain UI", path),
        "sha": sha,
        "branch": "main",
        "committer": {
            "name": user,
            "email": format!("{}@users.noreply.github.com", user),
        }
    });

    let url = format!("https://api.github.com/repos/{OWNER}/{REPO}/contents/{path}");

    let response = crab
        ._delete(url, Some(&body))
        .await
        .map_err(|e| ServerFnError::new(format!("GitHub DELETE: {e}")))?;

    if response.status().is_success() {
        Ok(())
    } else {
        let status = response.status();
        Err(ServerFnError::new(format!("GitHub API error {status}")))
    }
}

/// Generate the full markdown (frontmatter + body) from a payload.
/// Enforces the Brain templates programmatically.
#[cfg(feature = "ssr")]
fn generate_markdown(payload: &BrainFilePayload, author: &str) -> String {
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

    let related_section = if payload.related.is_empty() {
        "- (none yet)".to_string()
    } else {
        payload
            .related
            .iter()
            .map(|r| format!("- [{}](../{})", r, r))
            .collect::<Vec<_>>()
            .join("\n")
    };

    match payload.node_type {
        NodeType::Concept => {
            format!(
                r#"---
type: concept
topic: "{title}"
date_created: {date}
author: {author}
tags: {tags}
---

# Concept: {title}

## Summary
{body}

## Detailed Explanation
(To be expanded.)

## Related / See also
{related}
"#,
                title = payload.title,
                date = date,
                author = author,
                tags = tags_str,
                body = payload.body,
                related = related_section,
            )
        }
        NodeType::Decision => {
            format!(
                r#"---
type: adr
status: draft
date: {date}
author: {author}
tags: {tags}
---

# ADR: {title}

## Context
{body}

## Decision
(To be documented.)

## Consequences
(To be documented.)

## Related / See also
{related}
"#,
                title = payload.title,
                date = date,
                author = author,
                tags = tags_str,
                body = payload.body,
                related = related_section,
            )
        }
        NodeType::Meeting => {
            format!(
                r#"---
type: meeting
date: {date}
author: {author}
tags: {tags}
---

# Meeting: {title}

## Summary
{body}

## Action Items
- [ ] (To be added)

## Related / See also
{related}
"#,
                title = payload.title,
                date = date,
                author = author,
                tags = tags_str,
                body = payload.body,
                related = related_section,
            )
        }
        NodeType::Tag => {
            // Tags are virtual nodes, not files
            String::new()
        }
    }
}
