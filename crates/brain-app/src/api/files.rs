use leptos::prelude::*;
use serde::{Deserialize, Serialize};

use crate::knowledge::types::BrainFilePayload;
#[cfg(feature = "ssr")]
use brain_domain::BrainConfig;
use brain_domain::TargetRef;

use super::ApiError;
#[cfg(feature = "ssr")]
use super::sanitize_commit_message;
#[cfg(feature = "ssr")]
use super::sfe;
#[cfg(feature = "ssr")]
use super::validate_markdown_path;
#[cfg(feature = "ssr")]
use super::write_orchestrator::{
    delete_file_permission_aware, rebuild_projection_after_write, save_file_permission_aware,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BrainFile {
    pub path: String,
    pub sha: String,
    pub content: String,
    #[serde(default)]
    pub rendered_html: String,
    /// Optional hero image URL parsed from the frontmatter `cover:` field, with
    /// repo-relative paths already resolved (via the same rules used for
    /// inline markdown images). `None` when the file has no `cover:` value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cover_url: Option<String>,
    /// Optional `alt` text override for the hero image, taken verbatim from
    /// `cover_alt:` when present. The UI falls back to the document title when
    /// this is `None`; it is never rendered when `cover_url` is `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cover_alt: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct RepoFile {
    pub path: String,
    pub sha: String,
    pub size_bytes: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub is_work_item: bool,
    pub is_orphan_in_graph: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct FileQueryFilters {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path_prefix: Option<String>,
    #[serde(default)]
    pub orphan_only: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub org: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WriteMode {
    Direct,
    PullRequest,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WriteResult {
    pub path: String,
    pub mode: WriteMode,
    /// Fresh blob SHA for direct file writes. The editor uses this to keep an
    /// open edit session committable after a successful save.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pr_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pr_number: Option<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WriteCapabilities {
    pub can_read: bool,
    pub can_write_default_branch: bool,
    pub can_review_via_pr: bool,
    pub can_admin_config: bool,
}

#[server(GetWriteCapabilities, "/api", endpoint = "get_write_capabilities")]
pub async fn get_write_capabilities(target: TargetRef) -> Result<WriteCapabilities, ApiError> {
    use crate::server::session;

    let (_s, token) = session::require_session_and_token().await.map_err(sfe)?;
    let target = super::target_from_ref(target).map_err(sfe)?;
    let storage = session::storage_for(target).map_err(sfe)?;
    let permissions = storage.repository_permissions(&token).await.map_err(sfe)?;
    Ok(WriteCapabilities {
        can_read: permissions.pull,
        can_write_default_branch: permissions.push,
        can_review_via_pr: permissions.pull,
        can_admin_config: permissions.admin || permissions.maintain,
    })
}

#[cfg(feature = "ssr")]
impl WriteResult {
    pub(super) fn direct(path: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            mode: WriteMode::Direct,
            sha: None,
            branch: None,
            pr_url: None,
            pr_number: None,
        }
    }

    pub(super) fn pull_request(
        path: impl Into<String>,
        branch: impl Into<String>,
        pr_number: u64,
        pr_url: impl Into<String>,
    ) -> Self {
        Self {
            path: path.into(),
            mode: WriteMode::PullRequest,
            sha: None,
            branch: Some(branch.into()),
            pr_url: Some(pr_url.into()),
            pr_number: Some(pr_number),
        }
    }
}

#[server(ReadBrainFile, "/api", endpoint = "read_brain_file")]
pub async fn read_brain_file(target: TargetRef, path: String) -> Result<BrainFile, ApiError> {
    use crate::server::session;
    use brain_storage::Storage;

    let (_s, token) = session::require_session_and_token().await.map_err(sfe)?;
    let cfg = super::target_from_ref(target).map_err(sfe)?;
    let storage = session::storage_for(cfg.clone()).map_err(sfe)?;
    let (content, sha) = storage.read_file(&token, &path).await.map_err(sfe)?;

    let (body, fm) = crate::markdown::split_frontmatter(&content);
    let rendered_html = crate::markdown::render_for_file(body, &path, &cfg);
    let (cover_url, cover_alt) = extract_cover(fm, &path, &cfg);

    Ok(BrainFile {
        path,
        sha,
        content,
        rendered_html,
        cover_url,
        cover_alt,
    })
}

/// Pull `cover:` + optional `cover_alt:` out of the frontmatter, then resolve
/// the path through the same image-URL rewriter the body uses. Returns
/// `(None, None)` if the file has no frontmatter, no `cover:`, or the value is
/// non-string / dangerous.
#[cfg(feature = "ssr")]
fn extract_cover(
    frontmatter: Option<&str>,
    file_path: &str,
    cfg: &brain_domain::TargetConfig,
) -> (Option<String>, Option<String>) {
    let Some(raw) = frontmatter else {
        return (None, None);
    };
    let Ok(value) = serde_yaml::from_str::<serde_yaml::Value>(raw) else {
        return (None, None);
    };
    let Some(map) = value.as_mapping() else {
        return (None, None);
    };
    let cover = map
        .get(serde_yaml::Value::String("cover".to_string()))
        .and_then(serde_yaml::Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let Some(cover) = cover else {
        return (None, None);
    };
    let Some(url) = crate::markdown::resolve_cover_url(cover, file_path, cfg) else {
        return (None, None);
    };
    let alt = map
        .get(serde_yaml::Value::String("cover_alt".to_string()))
        .and_then(serde_yaml::Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    (Some(url), alt)
}

#[server(
    ListBrainFiles,
    "/api",
    input = server_fn::codec::Json,
    endpoint = "list_brain_files",
)]
pub async fn list_brain_files(filters: FileQueryFilters) -> Result<Vec<RepoFile>, ApiError> {
    use crate::server::session;

    let _ = session::require_authenticated().await.map_err(sfe)?;
    let fallback = session::target_cfg().map_err(sfe)?;
    let target = match (filters.org.clone(), filters.repo.clone()) {
        (Some(org), Some(repo)) if !org.is_empty() && !repo.is_empty() => {
            brain_domain::TargetConfig {
                org,
                repo,
                branch: filters.branch.clone().unwrap_or(fallback.branch),
            }
        }
        _ => fallback,
    };
    crate::server::projection::list_files(
        &target,
        &crate::server::projection::FileFilters {
            path_prefix: filters.path_prefix,
            orphan_only: filters.orphan_only,
        },
    )
    .await
    .map(|files| {
        files
            .into_iter()
            .map(|file| RepoFile {
                path: file.path,
                sha: file.sha,
                size_bytes: file.size_bytes,
                node_type: file.node_type,
                title: file.title,
                is_work_item: file.is_work_item,
                is_orphan_in_graph: file.is_orphan_in_graph,
            })
            .collect()
    })
    .map_err(sfe)
}

#[server(
    SaveBrainFile,
    "/api",
    input = server_fn::codec::Json,
    endpoint = "save_brain_file",
)]
pub async fn save_brain_file(payload: BrainFilePayload) -> Result<WriteResult, ApiError> {
    use crate::server::session;
    use brain_storage::Storage;

    use super::limits;
    limits::check_len("Document body", &payload.body, limits::MAX_MARKDOWN_BYTES).map_err(sfe)?;
    // Path length is enforced by `validate_markdown_path` below (covers the
    // derived path too); body/frontmatter caps guard against pulldown-cmark DoS.

    let (s, token) = session::require_session_and_token().await.map_err(sfe)?;
    let user = session::session_user_or_fallback(&s).await;

    let target = match payload.target.clone() {
        Some(target) => super::target_from_ref(target).map_err(sfe)?,
        None => session::target_cfg().map_err(sfe)?,
    };
    let config = crate::knowledge::config_loader::load(&target, &token).await;

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
    validate_markdown_path(&file_path).map_err(sfe)?;

    let related_section = build_related_section(&file_path, &payload.related);
    let body_without_related = strip_related_section(&payload.body);

    let frontmatter = merge_frontmatter(&payload, &user, &config);
    limits::check_len("Frontmatter", &frontmatter, limits::MAX_FRONTMATTER_BYTES).map_err(sfe)?;

    let markdown = format!(
        "{}\n{}{}",
        frontmatter, body_without_related, related_section
    );

    let auto_msg = if payload.sha.is_some() {
        format!("Update {} via Brain UI", file_path)
    } else {
        format!("Create {} via Brain UI", file_path)
    };
    let commit_msg = sanitize_commit_message(payload.commit_message.as_deref()).unwrap_or(auto_msg);

    let storage = session::storage_for(target.clone()).map_err(sfe)?;
    let author_email = format!("{}@users.noreply.github.com", user);

    match save_file_permission_aware(
        &storage,
        &token,
        &file_path,
        &markdown,
        payload.sha.as_deref(),
        &commit_msg,
        &user,
        &author_email,
        &target,
    )
    .await
    {
        Ok(mut result) => {
            let kind = if payload.sha.is_some() {
                "update"
            } else {
                "create"
            };
            match result.mode.clone() {
                WriteMode::Direct => {
                    match storage.read_file(&token, &file_path).await {
                        Ok((_content, fresh_sha)) => {
                            result.sha = Some(fresh_sha);
                        }
                        Err(error) => {
                            crate::server::audit::log(
                                "post_save_sha_error",
                                Some(&user),
                                &format!("{file_path}: {error}"),
                            )
                            .await;
                        }
                    }
                    crate::server::audit::log(kind, Some(&user), &file_path).await;
                    rebuild_projection_after_write(
                        &storage,
                        &target,
                        &token,
                        &user,
                        &format!("write:{file_path}"),
                    )
                    .await;
                }
                WriteMode::PullRequest => {
                    crate::server::audit::log(
                        "propose_write",
                        Some(&user),
                        &format!(
                            "{} via PR #{}",
                            file_path,
                            result
                                .pr_number
                                .map(|n| n.to_string())
                                .unwrap_or_else(|| "?".to_string())
                        ),
                    )
                    .await;
                }
            }
            Ok(result)
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
    target: TargetRef,
    path: String,
    sha: String,
    commit_message: Option<String>,
) -> Result<WriteResult, ApiError> {
    use crate::server::session;

    let (s, token) = session::require_session_and_token().await.map_err(sfe)?;
    let user = session::session_user_or_fallback(&s).await;
    let target = super::target_from_ref(target).map_err(sfe)?;
    validate_markdown_path(&path).map_err(sfe)?;
    let author_email = format!("{}@users.noreply.github.com", user);
    let commit_msg = sanitize_commit_message(commit_message.as_deref())
        .unwrap_or_else(|| format!("Delete {} via Brain UI", path));

    let storage = session::storage_for(target.clone()).map_err(sfe)?;
    match delete_file_permission_aware(
        &storage,
        &token,
        &path,
        &sha,
        &commit_msg,
        &user,
        &author_email,
        &target,
    )
    .await
    {
        Ok(result) => {
            match result.mode {
                WriteMode::Direct => {
                    crate::server::audit::log("delete", Some(&user), &path).await;
                    rebuild_projection_after_write(
                        &storage,
                        &target,
                        &token,
                        &user,
                        &format!("delete:{path}"),
                    )
                    .await;
                }
                WriteMode::PullRequest => {
                    crate::server::audit::log(
                        "propose_delete",
                        Some(&user),
                        &format!(
                            "{} via PR #{}",
                            path,
                            result
                                .pr_number
                                .map(|n| n.to_string())
                                .unwrap_or_else(|| "?".to_string())
                        ),
                    )
                    .await;
                }
            }
            Ok(result)
        }
        Err(e) => {
            crate::server::audit::log("api_error", Some(&user), &format!("delete {path}: {e}"))
                .await;
            Err(sfe(e))
        }
    }
}

/// Remove any trailing "## Related / See also" section from the body so the
/// rebuilt section (from the picker) doesn't duplicate links already present.
#[cfg(feature = "ssr")]
pub(super) fn strip_related_section(body: &str) -> &str {
    let mut last_related_start: Option<usize> = None;
    let mut search_start = 0;
    while let Some(pos) = body[search_start..].find("## ") {
        let abs = search_start + pos;
        let line_end = body[abs..]
            .find('\n')
            .map(|n| abs + n)
            .unwrap_or(body.len());
        let heading = body[abs..line_end].to_lowercase();
        if heading.contains("related") || heading.contains("see also") {
            last_related_start = Some(abs);
        }
        search_start = line_end + 1;
        if search_start >= body.len() {
            break;
        }
    }
    match last_related_start {
        Some(pos) => body[..pos].trim_end_matches('\n'),
        None => body,
    }
}

#[cfg(feature = "ssr")]
pub(super) fn build_related_section(file_path: &str, related: &[String]) -> String {
    use std::path::Path;

    if related.is_empty() {
        return String::new();
    }

    let from_dir = Path::new(file_path).parent().unwrap_or(Path::new(""));
    let links: Vec<String> = related
        .iter()
        .map(|path| {
            let label = path
                .rsplit('/')
                .next()
                .unwrap_or(path)
                .trim_end_matches(".md");
            let relative = super::relativize(from_dir, path);
            format!("- [{}]({})", label, relative)
        })
        .collect();
    format!("\n## Related / See also\n\n{}\n", links.join("\n"))
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
pub(super) fn merge_frontmatter(
    payload: &BrainFilePayload,
    author: &str,
    config: &BrainConfig,
) -> String {
    use rand::{Rng, distributions::Alphanumeric};
    use serde_yaml::Value;

    if config.synthetic_tag_spec().map(|s| s.name.as_str()) == Some(payload.node_type.as_str()) {
        return String::new();
    }

    let date = today_iso();
    let is_update = payload.sha.is_some();
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
    // Title is controlled by the form for types that declare a title_key.
    if let Some(key) = spec.title_key.as_deref() {
        map.insert(key.into(), Value::String(payload.title.clone()));
    }

    if !is_update {
        if let Some(field) = spec.date_create_field.as_deref() {
            map.insert(field.into(), Value::String(date));
        }
    } else if let Some(field) = spec.date_update_field.as_deref() {
        map.insert(field.into(), Value::String(date));
    }

    if spec.is_work_item() {
        let needs_brain_id = map
            .get("brain_id")
            .and_then(|value| value.as_str())
            .is_none_or(|value| value.trim().is_empty());
        if needs_brain_id {
            let suffix = rand::thread_rng()
                .sample_iter(&Alphanumeric)
                .take(6)
                .map(char::from)
                .collect::<String>()
                .to_ascii_lowercase();
            let timestamp = time::OffsetDateTime::now_utc().unix_timestamp();
            map.insert(
                "brain_id".into(),
                Value::String(format!("{}-{timestamp}-{suffix}", spec.name)),
            );
        }
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
            target: None,
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
        let mut payload = base_payload("adr".to_string());
        payload.path = None;
        payload.sha = None;
        let out = merge_frontmatter(&payload, "alice", &BrainConfig::default());
        assert!(out.contains("type: adr"));
        assert!(out.contains("status: draft"));
        assert!(out.starts_with("---\n"));
        assert!(out.ends_with("---\n"));
    }

    #[test]
    fn create_with_form_managed_extra_field_keeps_create_semantics() {
        let mut payload = base_payload("adr".to_string());
        payload.sha = None;
        payload.path = None;
        let mut preserved = BTreeMap::new();
        preserved.insert(
            "status".into(),
            serde_yaml::Value::String("accepted".into()),
        );
        payload.preserved_frontmatter = Some(preserved);

        let out = merge_frontmatter(&payload, "alice", &BrainConfig::default());
        assert!(out.contains("status: accepted"), "out was: {out}");
        assert!(out.contains("date:"), "out was: {out}");
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
    fn custom_type_respects_spec_title_and_date_fields() {
        use brain_domain::NodeTypeSpec;
        let mut cfg = BrainConfig::default();
        cfg.node_types.push(NodeTypeSpec {
            name: "articolo".into(),
            label: "Articolo".into(),
            directory: "articoli".into(),
            accent: "#abcdef".into(),
            template_filename: None,
            creatable: true,
            frontmatter_seed: BTreeMap::new(),
            title_key: Some("titolo".into()),
            date_create_field: Some("creato_il".into()),
            date_update_field: Some("aggiornato_il".into()),
            body_label: Some("Corpo".into()),
            work_item_kind: None,
            link_fields: BTreeMap::new(),
        });

        // Create path: title_key and date_create_field both get injected.
        let mut payload = base_payload("articolo".to_string());
        payload.title = "Il Mio Articolo".into();
        payload.path = None;
        payload.sha = None;
        let out = merge_frontmatter(&payload, "me", &cfg);
        assert!(out.contains("titolo: Il Mio Articolo"), "out was: {out}");
        assert!(out.contains("creato_il:"), "out was: {out}");
        assert!(
            !out.contains("aggiornato_il:"),
            "update field must not appear on create: {out}"
        );

        // Update path: date_update_field is used instead.
        let mut payload = base_payload("articolo".to_string());
        payload.title = "Il Mio Articolo".into();
        payload.preserved_frontmatter = Some(BTreeMap::new());
        let out = merge_frontmatter(&payload, "me", &cfg);
        assert!(out.contains("titolo: Il Mio Articolo"));
        assert!(out.contains("aggiornato_il:"), "out was: {out}");
        assert!(
            !out.contains("creato_il:"),
            "create field must not appear on update: {out}"
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

    #[test]
    fn work_item_create_injects_brain_id_once() {
        use brain_domain::{NodeTypeSpec, WorkItemKind};

        let mut cfg = BrainConfig::default();
        cfg.node_types.push(NodeTypeSpec {
            name: "task".into(),
            label: "Task".into(),
            directory: "tasks".into(),
            accent: "#fb7185".into(),
            template_filename: Some("Task.md".into()),
            creatable: true,
            frontmatter_seed: BTreeMap::new(),
            title_key: Some("topic".into()),
            date_create_field: Some("date_created".into()),
            date_update_field: Some("last_updated".into()),
            body_label: Some("Description".into()),
            work_item_kind: Some(WorkItemKind::Task),
            link_fields: BTreeMap::new(),
        });

        let mut payload = base_payload("task".to_string());
        payload.path = None;
        payload.sha = None;
        let out = merge_frontmatter(&payload, "alice", &cfg);
        assert!(out.contains("type: task"), "out was: {out}");
        assert!(out.contains("brain_id: task-"), "out was: {out}");
        assert!(out.contains("date_created:"), "out was: {out}");
    }

    #[test]
    fn work_item_update_preserves_existing_brain_id() {
        use brain_domain::{NodeTypeSpec, WorkItemKind};

        let mut cfg = BrainConfig::default();
        cfg.node_types.push(NodeTypeSpec {
            name: "task".into(),
            label: "Task".into(),
            directory: "tasks".into(),
            accent: "#fb7185".into(),
            template_filename: Some("Task.md".into()),
            creatable: true,
            frontmatter_seed: BTreeMap::new(),
            title_key: Some("topic".into()),
            date_create_field: Some("date_created".into()),
            date_update_field: Some("last_updated".into()),
            body_label: Some("Description".into()),
            work_item_kind: Some(WorkItemKind::Task),
            link_fields: BTreeMap::new(),
        });

        let mut payload = base_payload("task".to_string());
        let mut preserved = BTreeMap::new();
        preserved.insert(
            "brain_id".into(),
            serde_yaml::Value::String("task-existing-123".into()),
        );
        payload.preserved_frontmatter = Some(preserved);
        let out = merge_frontmatter(&payload, "alice", &cfg);
        assert!(
            out.contains("brain_id: task-existing-123"),
            "out was: {out}"
        );
        assert!(
            !out.contains("brain_id: task-task-existing-123"),
            "out was: {out}"
        );
    }

    #[test]
    fn related_section_uses_relative_links_from_nested_destination() {
        let out = build_related_section(
            "concepts/sub_folder_test_brain_UI/README.md",
            &[
                "runbooks/uso-brain-ui.md".to_string(),
                "concepts/TestbrainUI.md".to_string(),
            ],
        );

        assert!(out.contains("- [uso-brain-ui](../../runbooks/uso-brain-ui.md)"));
        assert!(out.contains("- [TestbrainUI](../TestbrainUI.md)"));
    }

    #[test]
    fn related_section_uses_same_directory_relative_links() {
        let out = build_related_section(
            "runbooks/uso-brain-ui.md",
            &["runbooks/another-runbook.md".to_string()],
        );

        assert!(out.contains("- [another-runbook](./another-runbook.md)"));
    }

    #[test]
    fn strip_related_section_removes_trailing_related_block() {
        let body = "## Description\nSome content.\n\n## Related / See also\n\n- [Foo](../concepts/Foo.md)\n";
        assert_eq!(strip_related_section(body), "## Description\nSome content.");
    }

    #[test]
    fn strip_related_section_leaves_body_without_related() {
        let body = "## Description\nSome content.\n";
        assert_eq!(strip_related_section(body), body);
    }

    #[test]
    fn strip_related_section_removes_last_of_multiple_related_blocks() {
        let body = "## Related / See also\n\n- [A](../a.md)\n\n## Other\n\n## Related / See also\n\n- [B](../b.md)\n";
        let stripped = strip_related_section(body);
        assert!(!stripped.contains("- [B]"), "second block must be stripped");
        assert!(stripped.contains("## Other"), "other sections must be kept");
    }
}
