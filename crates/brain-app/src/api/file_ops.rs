use leptos::prelude::*;
use serde::{Deserialize, Serialize};

use super::ApiError;
use super::WriteResult;
#[cfg(feature = "ssr")]
use super::sanitize_commit_message;
#[cfg(feature = "ssr")]
use super::sfe;
#[cfg(feature = "ssr")]
use super::write_orchestrator::{
    propose_transaction, rebuild_projection_after_write, should_fallback_to_pr,
};
use brain_domain::TargetRef;
#[cfg(feature = "ssr")]
use brain_domain::{BrainError, ConflictKind};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RenameResult {
    pub new_path: String,
    /// Paths of files whose links were rewritten to point at `new_path`.
    pub updated_referrers: Vec<String>,
    pub write: WriteResult,
}

/// Move a file to a new path and rewrite every markdown link that pointed at
/// the old path. Issues one commit per touched file (referrers, then the move
/// itself); we accept the commit churn to stay on the simple Contents API
/// rather than assembling a Git Data tree.
#[server(RenameBrainFile, "/api", endpoint = "rename_brain_file")]
pub async fn rename_brain_file(
    target: TargetRef,
    old_path: String,
    new_path: String,
    old_sha: String,
    commit_message: Option<String>,
) -> Result<RenameResult, ApiError> {
    use crate::server::session;

    let old_path = old_path.trim().trim_matches('/').to_string();
    let new_path = new_path.trim().trim_matches('/').to_string();

    if new_path.is_empty() || old_path.is_empty() {
        return Err(sfe(BrainError::parse("Empty path")));
    }
    if new_path == old_path {
        return Err(sfe(BrainError::parse("New path matches old path")));
    }
    validate_markdown_path(&old_path).map_err(sfe)?;
    validate_markdown_path(&new_path).map_err(sfe)?;

    let target = super::target_from_ref(target).map_err(sfe)?;
    let (s, token, permissions) = session::require_target_read(&target).await.map_err(sfe)?;
    let user = session::session_user_or_fallback(&s).await;
    let author_email = format!("{}@users.noreply.github.com", user);
    let storage = session::storage_for(target.clone()).map_err(sfe)?;

    let user_msg = sanitize_commit_message(commit_message.as_deref());
    let (transaction, updated_referrers) = prepare_rename_transaction(
        &storage,
        &token,
        &old_path,
        &new_path,
        &old_sha,
        user_msg,
        &user,
        &author_email,
    )
    .await
    .map_err(sfe)?;

    if permissions.push {
        match storage
            .commit_transaction(&token, transaction.clone())
            .await
        {
            Ok(_) => {
                crate::server::audit::log(
                    "rename",
                    Some(&user),
                    &format!(
                        "{old_path} -> {new_path} ({} referrers)",
                        updated_referrers.len()
                    ),
                )
                .await;
                rebuild_projection_after_write(
                    &storage,
                    &target,
                    &token,
                    &user,
                    &format!("rename:{old_path}->{new_path}"),
                )
                .await;
                return Ok(RenameResult {
                    new_path: new_path.clone(),
                    updated_referrers,
                    write: WriteResult::direct(new_path),
                });
            }
            Err(error) if should_fallback_to_pr(&error) => {}
            Err(error) => return Err(sfe(error)),
        }
    }

    let write = propose_transaction(
        &storage,
        &token,
        &user,
        &target,
        "rename",
        &old_path,
        permissions.push,
        transaction,
        &format!("Propose rename {old_path} to {new_path} via Brain UI"),
        &format!("Brain UI could not rename `{old_path}` directly on `{}` and proposed the rename through a pull request instead.\n\nNew path: `{new_path}`\nRewritten referrers: {}", target.branch, updated_referrers.len()),
    )
    .await
    .map_err(sfe)?;
    let pr_number = write.pr_number.unwrap_or_default();
    crate::server::audit::log(
        "propose_rename",
        Some(&user),
        &format!("{old_path} -> {new_path} via PR #{pr_number}"),
    )
    .await;

    Ok(RenameResult {
        new_path: new_path.clone(),
        updated_referrers,
        write,
    })
}

#[cfg(feature = "ssr")]
#[allow(clippy::too_many_arguments)]
async fn prepare_rename_transaction(
    storage: &brain_storage::GithubStorage,
    token: &str,
    old_path: &str,
    new_path: &str,
    old_sha: &str,
    user_msg: Option<String>,
    user: &str,
    author_email: &str,
) -> Result<(brain_storage::GitTransaction, Vec<String>), BrainError> {
    use brain_storage::Storage;

    // Sanity: the source file still exists at the sha the client saw.
    let (old_content, live_sha) = storage.read_file(token, old_path).await?;
    if live_sha != old_sha {
        return Err(BrainError::conflict(
            ConflictKind::BlobShaMoved,
            "File was modified since you opened it; reload and retry",
        ));
    }

    let config = crate::knowledge::config_loader::load(storage.target(), token).await;

    // Find every file that links to old_path. Walk the tree once, read each
    // candidate, and string-scan for link targets that resolve to old_path.
    let (_nodes, _edges) = storage.load_graph(token, &config).await?;
    let all_paths = collect_repo_md_paths(token, storage).await?;

    // Collect every referrer that needs a link rewrite together with the
    // renamed file's new path. They will be committed together via the Git
    // Data API instead of one Contents API commit per file.
    let mut upserts: Vec<(String, String)> = Vec::new();
    let mut expected_shas: Vec<(String, String)> = vec![(old_path.to_string(), live_sha.clone())];
    let mut updated_referrers = Vec::<String>::new();
    for candidate in &all_paths {
        if candidate == old_path {
            continue;
        }
        let (content, sha) = match storage.read_file(token, candidate).await {
            Ok(v) => v,
            Err(_) => continue,
        };
        let Some(rewritten) = rewrite_links(&content, candidate, old_path, new_path) else {
            continue;
        };
        upserts.push((candidate.clone(), rewritten));
        expected_shas.push((candidate.clone(), sha));
        updated_referrers.push(candidate.clone());
    }
    upserts.push((new_path.to_string(), old_content));

    let referrer_count = updated_referrers.len();
    let message = user_msg.unwrap_or_else(|| {
        if referrer_count == 0 {
            format!("Rename {old_path} -> {new_path} via Brain UI")
        } else {
            format!("Rename {old_path} -> {new_path} via Brain UI ({referrer_count} referrers)")
        }
    });

    let transaction = brain_storage::GitTransaction::new(message, user, author_email)
        .expect_absent(new_path)
        .delete(old_path);
    let transaction = upserts
        .into_iter()
        .fold(transaction, |tx, (path, content)| {
            tx.upsert_text(path, content)
        });
    let transaction = expected_shas
        .into_iter()
        .fold(transaction, |tx, (path, sha)| tx.expect_sha(path, sha));

    Ok((transaction, updated_referrers))
}

#[cfg(feature = "ssr")]
async fn collect_repo_md_paths(
    token: &str,
    storage: &brain_storage::GithubStorage,
) -> Result<Vec<String>, BrainError> {
    use brain_domain::GithubClient;
    use brain_graph::is_included_md;
    use brain_storage::GithubHttp;
    // Reuse graph load's internal logic by re-reading the tree directly. Keep
    // this narrow — we only need paths, not parsed docs. Build the URL from
    // the storage's actual target so a rename always reads the tree of the
    // repo it's modifying, never the process-default target.
    let url = GithubClient::new(storage.target().clone()).tree_url();
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
    let resp: Tree = GithubHttp::send_json(storage.http().get(&url, token), "tree").await?;
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
    let mut copied_until = 0;
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
                out.push_str(&content[copied_until..i]);
                out.push_str("](");
                out.push_str(&new_rel);
                if let Some(f) = fragment {
                    out.push('#');
                    out.push_str(f);
                }
                out.push(')');
                i = i + 2 + end + 1;
                copied_until = i;
                changed = true;
                continue;
            }
        }
        i += 1;
    }
    if changed {
        out.push_str(&content[copied_until..]);
        Some(out)
    } else {
        None
    }
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
pub(crate) fn relativize(from_dir: &std::path::Path, target: &str) -> String {
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
pub async fn upload_asset(
    target: TargetRef,
    filename: String,
    bytes: Vec<u8>,
) -> Result<String, ApiError> {
    use crate::server::session;
    use brain_storage::Storage;

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
    // Magic-bytes check: the real content must be an image whose type matches
    // the declared extension. Stops a script/HTML payload wearing a `.png`
    // extension from being committed and later served by the asset proxy.
    if !content_matches_ext(&bytes, &ext) {
        return Err(sfe(BrainError::parse(format!(
            "File content does not match .{ext} (failed magic-byte check)"
        ))));
    }

    let target = super::target_from_ref(target).map_err(sfe)?;
    let (s, token, permissions) = session::require_target_read(&target).await.map_err(sfe)?;
    crate::server::access::ensure_write(&target, &permissions).map_err(sfe)?;
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
    let storage = session::storage_for(target).map_err(sfe)?;
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
    matches!(ext, "png" | "jpg" | "jpeg" | "gif" | "webp")
}

/// Sniff the leading bytes and confirm they describe an image of the type the
/// declared extension claims. `jpg`/`jpeg` are aliases for the same magic.
#[cfg(feature = "ssr")]
fn content_matches_ext(bytes: &[u8], ext: &str) -> bool {
    let Some(kind) = infer::get(bytes) else {
        return false;
    };
    let detected = kind.extension();
    match ext {
        "jpg" | "jpeg" => matches!(detected, "jpg" | "jpeg"),
        other => detected == other,
    }
}

#[cfg(feature = "ssr")]
pub(crate) fn validate_markdown_path(path: &str) -> Result<(), BrainError> {
    let path = path.trim();
    if path.is_empty() {
        return Err(BrainError::parse("Path is required"));
    }
    if path.len() > super::limits::MAX_PATH_LEN {
        return Err(BrainError::parse("Path too long"));
    }
    if !path.ends_with(".md") {
        return Err(BrainError::parse("Path must end in .md"));
    }
    if path.starts_with('/') || path.contains('\\') || path.contains("..") {
        return Err(BrainError::parse("Invalid path"));
    }
    if path.chars().any(char::is_control) {
        return Err(BrainError::parse("Invalid path"));
    }
    if path
        .split('/')
        .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(BrainError::parse("Invalid path"));
    }
    let filename = path.rsplit('/').next().unwrap_or(path);
    if filename.trim_end_matches(".md").is_empty() {
        return Err(BrainError::parse("Filename is required"));
    }
    Ok(())
}

#[cfg(feature = "ssr")]
pub(crate) fn slugify(stem: &str) -> String {
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
pub async fn list_brain_folders(target: TargetRef) -> Result<Vec<String>, ApiError> {
    use crate::server::session;
    use brain_storage::Storage;

    let target = super::target_from_ref(target).map_err(sfe)?;
    let (_s, token, _permissions) = session::require_target_read(&target).await.map_err(sfe)?;
    let storage = session::storage_for(target).map_err(sfe)?;
    storage.list_folders(&token).await.map_err(sfe)
}

/// Tests for `rewrite_links` covering the rename path. Codifies the
/// invariants that Phase 2A's "rename safety" deliverable must keep:
/// fragments preserved, non-`.md` links left alone, every prefix variant
/// (`./`, `../`, bare) handled, external URLs untouched.
#[cfg(all(test, feature = "ssr"))]
mod rewrite_links_tests {
    use super::*;

    #[test]
    fn rewrites_bare_link_in_same_directory() {
        let body = "see [old](old.md) for details";
        let out = rewrite_links(
            body,
            "concepts/host.md",
            "concepts/old.md",
            "concepts/new.md",
        );
        assert_eq!(out.as_deref(), Some("see [old](./new.md) for details"));
    }

    #[test]
    fn rewrites_dot_slash_prefixed_link() {
        let body = "see [old](./old.md)";
        let out = rewrite_links(
            body,
            "concepts/host.md",
            "concepts/old.md",
            "concepts/new.md",
        );
        assert_eq!(out.as_deref(), Some("see [old](./new.md)"));
    }

    #[test]
    fn rewrites_parent_relative_link() {
        let body = "see [x](../adrs/old.md)";
        let out = rewrite_links(body, "concepts/host.md", "adrs/old.md", "adrs/new.md");
        assert_eq!(out.as_deref(), Some("see [x](../adrs/new.md)"));
    }

    #[test]
    fn preserves_fragment_after_rename() {
        let body = "jump to [section](./old.md#deep-section)";
        let out = rewrite_links(
            body,
            "concepts/host.md",
            "concepts/old.md",
            "concepts/new.md",
        );
        assert_eq!(
            out.as_deref(),
            Some("jump to [section](./new.md#deep-section)")
        );
    }

    #[test]
    fn preserves_fragment_with_parent_relative_link() {
        let body = "[a](../sub/old.md#h2)";
        let out = rewrite_links(body, "host/x.md", "sub/old.md", "sub/new.md");
        assert_eq!(out.as_deref(), Some("[a](../sub/new.md#h2)"));
    }

    #[test]
    fn ignores_image_links_with_md_lookalike_in_alt() {
        // The link target is an image, not a markdown doc. The matcher checks
        // `.md` on `path_part`, so `image.png` must not be touched even if
        // surrounding markdown looks similar.
        let body = "![ConceptNote](./img.png) and [doc](./old.md)";
        let out = rewrite_links(
            body,
            "concepts/host.md",
            "concepts/old.md",
            "concepts/new.md",
        );
        let updated = out.expect("at least the .md link should rewrite");
        assert!(
            updated.contains("![ConceptNote](./img.png)"),
            "image untouched: {updated}"
        );
        assert!(
            updated.contains("[doc](./new.md)"),
            "doc rewritten: {updated}"
        );
    }

    #[test]
    fn leaves_external_http_links_alone() {
        let body = "[home](https://example.com/old.md)";
        let out = rewrite_links(
            body,
            "concepts/host.md",
            "concepts/old.md",
            "concepts/new.md",
        );
        assert!(out.is_none(), "external URL must not match");
    }

    #[test]
    fn returns_none_when_no_link_matches() {
        let body = "[other](./other.md)";
        let out = rewrite_links(
            body,
            "concepts/host.md",
            "concepts/old.md",
            "concepts/new.md",
        );
        assert!(out.is_none(), "non-matching link returns None");
    }

    #[test]
    fn rewrites_nested_to_nested_across_depths() {
        // Host file in `notes/x.md` links to `concepts/a.md`; rename target
        // lives at `adrs/deep/b.md`. Output must use the correct relative
        // path from the host's directory.
        let body = "[a](../concepts/a.md)";
        let out = rewrite_links(body, "notes/x.md", "concepts/a.md", "adrs/deep/b.md");
        assert_eq!(out.as_deref(), Some("[a](../adrs/deep/b.md)"));
    }

    #[test]
    fn case_only_rename_is_treated_as_distinct_path() {
        // GitHub Contents API is case-sensitive; a rename Foo.md -> foo.md
        // must rewrite links pointing at `Foo.md`. The match is exact-string
        // on the resolved path, so this confirms case-sensitivity isn't
        // accidentally normalised away.
        let body = "[x](./Foo.md)";
        let out = rewrite_links(body, "host.md", "Foo.md", "foo.md");
        assert_eq!(out.as_deref(), Some("[x](./foo.md)"));

        // Inverse: a link to `foo.md` must NOT be rewritten when only `Foo.md`
        // was renamed.
        let body = "[x](./foo.md)";
        let out = rewrite_links(body, "host.md", "Foo.md", "foo.md");
        assert!(out.is_none(), "case-different link must not match");
    }

    #[test]
    fn rewrites_multiple_links_in_one_document() {
        let body = "first [a](./old.md) then [b](./old.md#anchor) and [c](./other.md)";
        let out = rewrite_links(body, "host.md", "old.md", "new.md");
        assert_eq!(
            out.as_deref(),
            Some("first [a](./new.md) then [b](./new.md#anchor) and [c](./other.md)")
        );
    }

    #[test]
    fn rewrites_into_new_directory() {
        // Renaming into a previously-nonexistent directory must produce a valid
        // relative path (the `save_file` Contents API call creates intermediate
        // dirs; rewrite_links itself just needs to emit the right string).
        let body = "[x](./old.md)";
        let out = rewrite_links(body, "host.md", "old.md", "fresh/new.md");
        assert_eq!(out.as_deref(), Some("[x](./fresh/new.md)"));
    }

    #[test]
    fn preserves_non_ascii_text_when_rewriting() {
        let body = "Caffe corretto: caffè, déjà vu, 東京 [old](./old.md) è già qui";
        let out = rewrite_links(body, "host.md", "old.md", "new.md");
        assert_eq!(
            out.as_deref(),
            Some("Caffe corretto: caffè, déjà vu, 東京 [old](./new.md) è già qui")
        );
    }

    #[test]
    fn does_not_rewrite_link_pointing_at_host_file_itself() {
        // A self-referential link (e.g. a doc linking back to its own anchor
        // via `[x](./host.md#section)`) should not match a rename of a
        // different file.
        let body = "[self](./host.md#top)";
        let out = rewrite_links(
            body,
            "concepts/host.md",
            "concepts/old.md",
            "concepts/new.md",
        );
        assert!(out.is_none());
    }

    #[test]
    fn rejects_unsafe_markdown_paths() {
        for path in [
            "",
            ".md",
            "/notes/a.md",
            "../a.md",
            "notes/../a.md",
            "notes\\a.md",
        ] {
            assert!(
                validate_markdown_path(path).is_err(),
                "path should be rejected: {path:?}"
            );
        }
        assert!(validate_markdown_path("notes/a.md").is_ok());
        assert!(validate_markdown_path("notes/deep/a-b.md").is_ok());
    }

    #[test]
    fn upload_extensions_exclude_svg_until_sanitizer_exists() {
        assert!(is_allowed_image_ext("png"));
        assert!(is_allowed_image_ext("webp"));
        assert!(!is_allowed_image_ext("svg"));
    }

    #[test]
    fn magic_bytes_reject_mismatched_extension() {
        // HTML/script payload renamed to .png must be rejected.
        let html = b"<script>alert(1)</script>";
        assert!(!content_matches_ext(html, "png"));
        // Genuine PNG signature passes for png.
        let png = b"\x89PNG\r\n\x1a\n\x00\x00\x00\x0dIHDR";
        assert!(content_matches_ext(png, "png"));
        // PNG bytes claiming to be a jpg must be rejected.
        assert!(!content_matches_ext(png, "jpg"));
        // Empty content is not a valid image.
        assert!(!content_matches_ext(b"", "png"));
    }

    #[test]
    fn magic_bytes_accept_jpeg_alias() {
        let jpeg = b"\xff\xd8\xff\xe0\x00\x10JFIF";
        assert!(content_matches_ext(jpeg, "jpg"));
        assert!(content_matches_ext(jpeg, "jpeg"));
    }
}
