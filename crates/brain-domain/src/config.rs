//! Runtime configuration loaded from the process environment.
//!
//! These values are loaded at server startup, then passed explicitly through
//! constructors or provided through the application context.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use thiserror::Error;

use crate::work_items::{WorkItemKind, WorkItemState};

/// The GitHub repository the app reads from and writes to.
///
/// This is the runtime *config wrapper* — what `GithubStorage` and the config
/// loader consume. It is built from env on boot and then propagated via Leptos
/// context. For the *identity* contract that travels in URLs, server fn
/// payloads, audit logs, and webhook payloads use [`TargetRef`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TargetConfig {
    pub org: String,
    pub repo: String,
    pub branch: String,
}

/// Canonical *identity* of a forge target. The narrow contract: org + repo +
/// branch, hashable, serde-roundtrippable, validated. This is the type that
/// belongs in URLs (`/{org}/{repo}/{branch}/...`), in mutation server fn
/// payloads, in audit-log entries, and in webhook routing tables. Keep
/// [`TargetConfig`] for runtime config consumed by storage clients; reach for
/// `TargetRef` whenever the value crosses a trust boundary.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TargetRef {
    pub org: String,
    pub repo: String,
    pub branch: String,
}

/// Validation errors for [`TargetRef`]. Exposed so callers can surface a
/// precise `BrainError` at HTTP boundaries instead of a generic 400.
#[derive(Debug, Error)]
pub enum TargetRefError {
    #[error("target {field} is empty")]
    Empty { field: &'static str },
    #[error("target {field} is too long ({len} bytes, max {max})")]
    TooLong {
        field: &'static str,
        len: usize,
        max: usize,
    },
    #[error("target {field} contains forbidden segment {value:?}")]
    PathTraversal { field: &'static str, value: String },
    #[error("target {field} {value:?} contains a non-printable character")]
    NonPrintable { field: &'static str, value: String },
    #[error("target {field} {value:?} starts with a slash")]
    LeadingSlash { field: &'static str, value: String },
}

const MAX_TARGET_COMPONENT_BYTES: usize = 1024;

impl TargetRef {
    pub fn new(org: impl Into<String>, repo: impl Into<String>, branch: impl Into<String>) -> Self {
        Self {
            org: org.into(),
            repo: repo.into(),
            branch: branch.into(),
        }
    }

    /// Reject empty/`..`/leading-slash/non-printable values across all three
    /// components. The branch may contain `/` (e.g. `feature/foo`) — that's
    /// valid on the forge — but `..` is rejected outright because every code
    /// path that interpolates the branch into a URL or filesystem path would
    /// otherwise be a traversal sink.
    pub fn validate(&self) -> Result<(), TargetRefError> {
        for (field, value) in [
            ("org", &self.org),
            ("repo", &self.repo),
            ("branch", &self.branch),
        ] {
            if value.is_empty() {
                return Err(TargetRefError::Empty { field });
            }
            if value.len() > MAX_TARGET_COMPONENT_BYTES {
                return Err(TargetRefError::TooLong {
                    field,
                    len: value.len(),
                    max: MAX_TARGET_COMPONENT_BYTES,
                });
            }
            if value.starts_with('/') {
                return Err(TargetRefError::LeadingSlash {
                    field,
                    value: value.clone(),
                });
            }
            if value.chars().any(|c| c.is_control()) {
                return Err(TargetRefError::NonPrintable {
                    field,
                    value: value.clone(),
                });
            }
            for segment in value.split('/') {
                if segment == ".." || segment == "." {
                    return Err(TargetRefError::PathTraversal {
                        field,
                        value: value.clone(),
                    });
                }
            }
            // org and repo cannot contain `/` at all — they are single
            // segments on every forge we plan to support. Branch is allowed
            // to be multi-segment (e.g. `feature/foo`).
            if field != "branch" && value.contains('/') {
                return Err(TargetRefError::PathTraversal {
                    field,
                    value: value.clone(),
                });
            }
        }
        Ok(())
    }

    /// Parse and validate an `org/repo/branch` key string. Branch names may
    /// contain `/`, so only the first two separators delimit owner and repo.
    pub fn try_from_key_string(key: &str) -> Result<Self, TargetRefError> {
        let mut parts = key.splitn(3, '/');
        let target = Self::new(
            parts.next().unwrap_or_default(),
            parts.next().unwrap_or_default(),
            parts.next().unwrap_or_default(),
        );
        target.validate()?;
        Ok(target)
    }
}

impl std::fmt::Display for TargetRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}/{}", self.org, self.repo, self.branch)
    }
}

impl From<TargetRef> for TargetConfig {
    fn from(r: TargetRef) -> Self {
        Self {
            org: r.org,
            repo: r.repo,
            branch: r.branch,
        }
    }
}

impl From<&TargetRef> for TargetConfig {
    fn from(r: &TargetRef) -> Self {
        Self {
            org: r.org.clone(),
            repo: r.repo.clone(),
            branch: r.branch.clone(),
        }
    }
}

impl From<&TargetConfig> for TargetRef {
    fn from(c: &TargetConfig) -> Self {
        Self {
            org: c.org.clone(),
            repo: c.repo.clone(),
            branch: c.branch.clone(),
        }
    }
}

impl From<TargetConfig> for TargetRef {
    fn from(c: TargetConfig) -> Self {
        Self {
            org: c.org,
            repo: c.repo,
            branch: c.branch,
        }
    }
}

/// Stable, hashable identity for a target repo. Used as the cache key in
/// brain-storage and config_loader so a future multi-target deployment cannot
/// cross-contaminate a graph or template cache between repositories.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TargetKey(String);

impl TargetKey {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Build a `TargetKey` directly from its three parts. Both
    /// `From<&TargetConfig>` and `From<&TargetRef>` route through here so the
    /// format string lives in exactly one place.
    pub fn from_parts(org: &str, repo: &str, branch: &str) -> Self {
        Self(format!("{org}/{repo}/{branch}"))
    }

    /// Parse and validate an `org/repo/branch` key string before turning it
    /// into a cache/broadcast key. Branch names may contain `/`, so the parser
    /// splits only the first two separators and treats the rest as the branch.
    pub fn try_from_key_string(key: &str) -> Result<Self, TargetRefError> {
        let target = TargetRef::try_from_key_string(key)?;
        Ok(Self::from(&target))
    }
}

impl From<&TargetConfig> for TargetKey {
    fn from(t: &TargetConfig) -> Self {
        Self::from_parts(&t.org, &t.repo, &t.branch)
    }
}

impl From<&TargetRef> for TargetKey {
    fn from(t: &TargetRef) -> Self {
        Self::from_parts(&t.org, &t.repo, &t.branch)
    }
}

impl std::fmt::Display for TargetKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Percent-encode a value for use as a single URL path segment.
pub fn encode_path_segment(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Percent-encode a repository path for use under GitHub's `/contents/{path}`.
/// Slash remains a path separator; every segment is encoded independently.
/// Dot-only segments are double-encoded because URL parsers normalize both
/// literal `..` and `%2E%2E` before the request is sent.
pub fn encode_repo_path(path: &str) -> String {
    path.split('/')
        .map(|segment| match segment {
            "." => "%252E".to_string(),
            ".." => "%252E%252E".to_string(),
            _ => encode_path_segment(segment),
        })
        .collect::<Vec<_>>()
        .join("/")
}

/// Decode one URL path segment, leaving invalid escape sequences untouched.
pub fn decode_path_segment(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = &s[i + 1..i + 3];
            if let Ok(v) = u8::from_str_radix(hex, 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| s.to_string())
}

/// Builds GitHub REST + raw + blob URLs for a target repository. This is the
/// single seam Phase 4's forge abstraction will retarget — keep direct
/// `https://api.github.com/...` format strings out of the rest of the codebase.
#[derive(Clone, Debug)]
pub struct GithubClient {
    target: TargetConfig,
    api_base: String,
}

const DEFAULT_API_BASE: &str = "https://api.github.com";

impl GithubClient {
    pub fn new(target: TargetConfig) -> Self {
        Self {
            target,
            api_base: DEFAULT_API_BASE.to_string(),
        }
    }

    /// Override the REST API base URL. Intended for tests against a mock
    /// server; production code should use `new` and accept the default
    /// `https://api.github.com`.
    pub fn with_api_base(mut self, base: impl Into<String>) -> Self {
        self.api_base = normalize_api_base(base.into());
        self
    }

    /// Derive a client for another branch in the same repository while
    /// preserving the configured API base. Branch transactions use this to
    /// target an ephemeral ref without rebuilding transport configuration.
    pub fn with_branch(&self, branch: impl Into<String>) -> Self {
        let mut target = self.target.clone();
        target.branch = branch.into();
        Self {
            target,
            api_base: self.api_base.clone(),
        }
    }

    pub fn default_api_base() -> &'static str {
        DEFAULT_API_BASE
    }

    pub fn app_installation_access_tokens_url(api_base: &str, installation_id: &str) -> String {
        format!(
            "{}/app/installations/{}/access_tokens",
            normalize_api_base(api_base.to_string()),
            encode_path_segment(installation_id)
        )
    }

    pub fn target(&self) -> &TargetConfig {
        &self.target
    }

    pub fn contents_url(&self, path: &str) -> String {
        format!(
            "{}/repos/{}/{}/contents/{}",
            self.api_base,
            self.target.org,
            self.target.repo,
            encode_repo_path(path)
        )
    }

    pub fn repo_url(&self, owner: &str, repo: &str) -> String {
        format!("{}/repos/{owner}/{repo}", self.api_base)
    }

    pub fn target_repo_url(&self) -> String {
        self.repo_url(&self.target.org, &self.target.repo)
    }

    pub fn branch_url(&self, branch: &str) -> String {
        format!(
            "{}/repos/{}/{}/branches/{}",
            self.api_base,
            self.target.org,
            self.target.repo,
            encode_path_segment(branch)
        )
    }

    pub fn user_repos_url(&self) -> String {
        format!(
            "{}/user/repos?per_page=100&sort=pushed&affiliation=owner,collaborator,organization_member",
            self.api_base
        )
    }

    pub fn tree_url(&self) -> String {
        format!(
            "{}/repos/{}/{}/git/trees/{}?recursive=1",
            self.api_base, self.target.org, self.target.repo, self.target.branch
        )
    }

    pub fn raw_base(&self) -> String {
        format!(
            "https://raw.githubusercontent.com/{}/{}/{}",
            self.target.org, self.target.repo, self.target.branch
        )
    }

    pub fn blob_base(&self) -> String {
        format!(
            "https://github.com/{}/{}/blob/{}",
            self.target.org, self.target.repo, self.target.branch
        )
    }

    /// Direct link to `.brain-config.yml` in the target repo. Used by the
    /// orphan-type banner CTA.
    pub fn config_blob_url(&self) -> String {
        format!("{}/.brain-config.yml", self.blob_base())
    }

    pub fn git_blobs_url(&self) -> String {
        format!(
            "{}/repos/{}/{}/git/blobs",
            self.api_base, self.target.org, self.target.repo
        )
    }

    pub fn git_trees_url(&self) -> String {
        format!(
            "{}/repos/{}/{}/git/trees",
            self.api_base, self.target.org, self.target.repo
        )
    }

    /// Recursive tree read by tree SHA (not by branch). Used to verify
    /// optimistic-concurrency preconditions against the exact base_tree we are
    /// about to commit on top of.
    pub fn git_tree_by_sha_url(&self, tree_sha: &str) -> String {
        format!(
            "{}/repos/{}/{}/git/trees/{}?recursive=1",
            self.api_base, self.target.org, self.target.repo, tree_sha
        )
    }

    pub fn git_commits_url(&self) -> String {
        format!(
            "{}/repos/{}/{}/git/commits",
            self.api_base, self.target.org, self.target.repo
        )
    }

    pub fn git_commit_url(&self, sha: &str) -> String {
        format!(
            "{}/repos/{}/{}/git/commits/{}",
            self.api_base, self.target.org, self.target.repo, sha
        )
    }

    pub fn git_ref_url(&self) -> String {
        format!(
            "{}/repos/{}/{}/git/refs/heads/{}",
            self.api_base, self.target.org, self.target.repo, self.target.branch
        )
    }

    pub fn git_refs_url(&self) -> String {
        format!(
            "{}/repos/{}/{}/git/refs",
            self.api_base, self.target.org, self.target.repo
        )
    }

    pub fn forks_url(&self) -> String {
        format!(
            "{}/repos/{}/{}/forks",
            self.api_base, self.target.org, self.target.repo
        )
    }

    pub fn pulls_url(&self) -> String {
        format!(
            "{}/repos/{}/{}/pulls",
            self.api_base, self.target.org, self.target.repo
        )
    }

    pub fn issue_url(&self, project: &str, item_key: &str) -> Result<String, crate::BrainError> {
        let (owner, repo) = project.split_once('/').ok_or_else(|| {
            crate::BrainError::parse(format!("invalid GitHub project: {project}"))
        })?;
        if owner.trim().is_empty() || repo.trim().is_empty() || repo.contains('/') {
            return Err(crate::BrainError::parse(format!(
                "invalid GitHub project: {project}"
            )));
        }
        let number = item_key.parse::<u64>().map_err(|_| {
            crate::BrainError::parse(format!("invalid GitHub issue number: {item_key}"))
        })?;
        Ok(format!(
            "{}/repos/{owner}/{repo}/issues/{number}",
            self.api_base
        ))
    }

    pub fn issue_comments_url(
        &self,
        project: &str,
        item_key: &str,
    ) -> Result<String, crate::BrainError> {
        Ok(format!(
            "{}/comments?per_page=100",
            self.issue_url(project, item_key)?
        ))
    }
}

fn normalize_api_base(mut base: String) -> String {
    while base.ends_with('/') {
        base.pop();
    }
    base
}

impl From<TargetConfig> for GithubClient {
    fn from(target: TargetConfig) -> Self {
        Self::new(target)
    }
}

/// User-facing branding copy (landing page title, access-denied messages).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BrandConfig {
    /// Display name shown in the header, e.g. "Dritara Brain".
    pub name: String,
    /// Organisation label shown in access-denied copy, e.g. "Dritara-Digital".
    /// In practice this matches `TargetConfig::org` but kept separate to allow
    /// prettier display casing if ever needed.
    pub org_label: String,
}

/// Declaration of a single node type — the dynamic, config-driven replacement
/// for the hardcoded `NodeType` enum. Loaded from `.brain-config.yml` at the
/// root of the target repo, or from the built-in default.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct NodeTypeSpec {
    /// Canonical key; what ends up in the `type:` field of each doc's
    /// frontmatter (e.g. `"concept"`, `"adr"`).
    pub name: String,
    /// UI label (e.g. `"Concept"`, `"ADR"`).
    pub label: String,
    /// Canonical directory under the repo root where docs of this type live
    /// (e.g. `"concepts"`). Empty string for virtual/synthetic types like Tag.
    pub directory: String,
    /// Accent color as `#RRGGBB` hex. Used for SVG fills and CSS variables.
    pub accent: String,
    /// Optional template filename under `templates/` (e.g. `"ConceptNote.md"`).
    #[serde(default)]
    pub template_filename: Option<String>,
    /// Whether this type should appear in the "New Node" menu. Synthetic types
    /// like Tag set this to false.
    #[serde(default = "default_true")]
    pub creatable: bool,
    /// Seed values inserted into the frontmatter on create (e.g. for ADR:
    /// `{status: "draft"}`). Merged with form-controlled fields at save time.
    #[serde(default)]
    pub frontmatter_seed: BTreeMap<String, serde_yaml::Value>,
    /// Frontmatter key that stores the human title for this type (e.g.
    /// `"topic"` for concepts, `"progetto"` for preventivi). When `None`,
    /// the title is not injected into frontmatter on save — callers that
    /// need to read a title from frontmatter should fall back to `"topic"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title_key: Option<String>,
    /// Frontmatter field to populate with today's date when a new doc of this
    /// type is created. `None` = no create-time date injection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date_create_field: Option<String>,
    /// Frontmatter field to refresh with today's date on every update. `None`
    /// = no update-time date injection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date_update_field: Option<String>,
    /// UI label used above the markdown body textarea in the editor. Falls
    /// back to `"Description"` when `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body_label: Option<String>,
    /// Optional classification for node types that participate in the
    /// operational model. This stays provider-agnostic: a `task` node type can
    /// later bind 1:1 to a GitHub Issue, GitLab Issue, or remain local/offline
    /// without the domain model changing shape.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub work_item_kind: Option<WorkItemKind>,
    /// Frontmatter fields whose scalar or sequence values are slugs pointing
    /// to nodes of the configured target type.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub link_fields: BTreeMap<String, String>,
}

fn default_true() -> bool {
    true
}

impl NodeTypeSpec {
    /// CSS color reference. Emits `var(--accent-{name}, {hex})` so a type
    /// defined in `.brain-config.yml` renders its configured hex even when
    /// no matching `:root` custom property exists. Declared `:root` vars
    /// still win because CSS only uses the fallback when the var is unset.
    pub fn accent_var(&self) -> String {
        format!("var(--accent-{}, {})", self.name, self.accent)
    }

    pub fn is_work_item(&self) -> bool {
        self.work_item_kind.is_some()
    }
}

/// Machine-readable mapping from a `WorkItemKind` + `WorkItemState` pair to the
/// provider labels that represent it. This lets the UI and sync layer translate
/// between Brain's internal state model and the label set on the external forge
/// without hardcoding GitHub-specific strings in application code.
///
/// The `state_labels` map is optional. When omitted, state is managed only via
/// Brain frontmatter and is not projected onto the external issue.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct WorkItemLabelSpec {
    /// The internal kind this entry describes (e.g. `task`, `incident`).
    pub kind: WorkItemKind,
    /// Canonical label for this kind on the external forge (e.g. `"brain:task"`).
    /// Used when creating or filtering issues on the provider.
    pub kind_label: String,
    /// Optional state → label mapping. When present, transitioning a WorkItem
    /// to a given state will apply/remove the corresponding forge label.
    /// States not listed here are managed only in Brain frontmatter.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub state_labels: BTreeMap<WorkItemState, String>,
}

/// Top-level config parsed from `.brain-config.yml` at the repo root, or the
/// built-in default when the file is absent.
///
/// The default is guaranteed to be equivalent to the hardcoded `NodeType`
/// enum so repos without a config file keep working unchanged.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct BrainConfig {
    /// Order-preserving list of node types. UI iteration respects this order.
    pub node_types: Vec<NodeTypeSpec>,
    /// Name of the spec used when a doc references an unknown type.
    pub default_type: String,
    /// Machine-readable taxonomy of Work Item labels. Drives forge-label sync
    /// and UI filters without hardcoding provider strings anywhere else.
    /// Defaults to the built-in kinds with `brain:*` label names.
    #[serde(
        default = "default_label_taxonomy",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub label_taxonomy: Vec<WorkItemLabelSpec>,
    /// Saved filter sets for the Knowledge sidebar. Each view is a named tuple
    /// of existing URL-persisted filters (`?tags=`, `?types=`); no new filter
    /// dimensions are introduced.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub views: Vec<ViewSpec>,
}

/// A named, target-scoped saved filter set. Each view maps to the same
/// URL-persisted filter parameters used by the Knowledge sidebar (`?tags=`,
/// `?types=`), so a view click is equivalent to navigating to that URL.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ViewSpec {
    /// Human-readable label rendered in the sidebar.
    pub name: String,
    /// URL-safe identifier, unique within the target. Auto-derived from
    /// `name` when the GUI omits it.
    pub slug: String,
    /// Tag values to apply (lowercase). Empty = no tag filter.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    /// Type names to apply. Each must reference an existing `node_types[].name`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub types: Vec<String>,
    /// Optional sort weight. Lower = earlier in the sidebar; ties (and views
    /// without a weight) keep the order they appear in YAML. `None` is treated
    /// as `0`, so a single pinned view with `weight: -10` floats to the top
    /// without forcing the author to weight everything else.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weight: Option<i32>,
}

/// Reserved names that clash with known repo paths. A node type cannot use
/// these as its `name` or `directory`.
const RESERVED: &[&str] = &["tags", "templates"];
const RESERVED_FRONTMATTER_FIELDS: &[&str] = &["type", "author", "tags"];

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("yaml parse: {0}")]
    Parse(String),
    #[error("duplicate node type name: {0}")]
    DuplicateName(String),
    #[error("duplicate directory: {0}")]
    DuplicateDirectory(String),
    #[error("invalid accent color {accent:?} for type {name:?} (expected #RRGGBB)")]
    InvalidAccent { name: String, accent: String },
    #[error("default_type {0:?} not present in node_types")]
    UnknownDefault(String),
    #[error("reserved name {0:?} cannot be used as a node type name or directory")]
    ReservedName(String),
    #[error("invalid {field} {key:?} for type {name:?}")]
    InvalidFrontmatterField {
        name: String,
        field: &'static str,
        key: String,
    },
    #[error("node_types is empty")]
    Empty,
    #[error("duplicate view slug: {0}")]
    DuplicateViewSlug(String),
    #[error("view {view:?} references unknown type {type_name:?}")]
    UnknownViewType { view: String, type_name: String },
    #[error("view name is empty")]
    EmptyViewName,
    #[error("invalid view slug {0:?} (expected lowercase alnum, '-', '_')")]
    InvalidViewSlug(String),
    #[error("view tag {tag:?} in view {view:?} must be lowercase")]
    NonLowercaseViewTag { view: String, tag: String },
    #[error(
        "link field {field:?} for type {name:?} references unknown target type {target_type:?}"
    )]
    UnknownLinkFieldTarget {
        name: String,
        field: String,
        target_type: String,
    },
}

impl BrainConfig {
    /// Parse YAML source into a validated `BrainConfig`.
    pub fn parse(yaml: &str) -> Result<Self, ConfigError> {
        let cfg: BrainConfig =
            serde_yaml::from_str(yaml).map_err(|e| ConfigError::Parse(e.to_string()))?;
        cfg.validate()?;
        Ok(cfg)
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.node_types.is_empty() {
            return Err(ConfigError::Empty);
        }
        let mut names = std::collections::HashSet::new();
        let mut dirs = std::collections::HashSet::new();
        for spec in &self.node_types {
            if RESERVED.contains(&spec.name.as_str()) {
                return Err(ConfigError::ReservedName(spec.name.clone()));
            }
            if !spec.directory.is_empty() && RESERVED.contains(&spec.directory.as_str()) {
                return Err(ConfigError::ReservedName(spec.directory.clone()));
            }
            if !names.insert(spec.name.clone()) {
                return Err(ConfigError::DuplicateName(spec.name.clone()));
            }
            if !spec.directory.is_empty() && !dirs.insert(spec.directory.clone()) {
                return Err(ConfigError::DuplicateDirectory(spec.directory.clone()));
            }
            if !is_valid_hex_color(&spec.accent) {
                return Err(ConfigError::InvalidAccent {
                    name: spec.name.clone(),
                    accent: spec.accent.clone(),
                });
            }
            validate_frontmatter_fields(spec)?;
        }
        if !names.contains(&self.default_type) {
            return Err(ConfigError::UnknownDefault(self.default_type.clone()));
        }
        validate_link_fields(&self.node_types, &names)?;
        validate_views(&self.views, &names)?;
        Ok(())
    }

    pub fn lookup(&self, name: &str) -> Option<&NodeTypeSpec> {
        self.node_types.iter().find(|s| s.name == name)
    }

    /// The spec used as fallback for unknown types. Safe to unwrap because
    /// `validate()` enforces that `default_type` is in the list.
    pub fn default_spec(&self) -> &NodeTypeSpec {
        self.lookup(&self.default_type)
            .expect("validated: default_type exists in node_types")
    }

    pub fn creatable(&self) -> impl Iterator<Item = &NodeTypeSpec> {
        self.node_types.iter().filter(|s| s.creatable)
    }

    /// Spec used for virtual tag nodes in the graph. Synthetic types are the
    /// non-creatable entries with no backing directory.
    pub fn synthetic_tag_spec(&self) -> Option<&NodeTypeSpec> {
        self.node_types
            .iter()
            .find(|s| !s.creatable && s.directory.is_empty())
    }

    pub fn by_directory(&self, dir: &str) -> Option<&NodeTypeSpec> {
        let dir = dir.trim_matches('/');
        if dir.is_empty() {
            return None;
        }
        self.node_types.iter().find(|s| s.directory == dir)
    }

    /// Label spec for the given `WorkItemKind`, or `None` if the taxonomy does
    /// not include that kind (custom config may intentionally omit some).
    pub fn labels_for_kind(&self, kind: &WorkItemKind) -> Option<&WorkItemLabelSpec> {
        self.label_taxonomy.iter().find(|e| &e.kind == kind)
    }

    /// All `kind_label` strings defined in the taxonomy. Useful for seeding the
    /// label set on a forge project or building a filter predicate.
    pub fn all_kind_labels(&self) -> impl Iterator<Item = &str> {
        self.label_taxonomy.iter().map(|e| e.kind_label.as_str())
    }

    /// Views in display order: sorted by `weight` ascending (defaulting to 0),
    /// with the original YAML order preserved as the tie-breaker. Callers that
    /// just iterate `self.views` get the raw YAML order — use this whenever the
    /// result is shown to a human, so authors can pin a view by giving it a
    /// negative weight without re-ordering the whole list.
    pub fn sorted_views(&self) -> Vec<&ViewSpec> {
        let mut indexed: Vec<(usize, &ViewSpec)> = self.views.iter().enumerate().collect();
        indexed.sort_by_key(|(idx, v)| (v.weight.unwrap_or(0), *idx));
        indexed.into_iter().map(|(_, v)| v).collect()
    }
}

fn default_label_taxonomy() -> Vec<WorkItemLabelSpec> {
    use WorkItemKind::*;
    use WorkItemState::*;
    let entry = |kind: WorkItemKind, kind_label: &str, states: &[(WorkItemState, &str)]| {
        WorkItemLabelSpec {
            kind,
            kind_label: kind_label.into(),
            state_labels: states
                .iter()
                .map(|(s, l)| (s.clone(), l.to_string()))
                .collect(),
        }
    };
    vec![
        entry(
            Task,
            "brain:task",
            &[
                (InProgress, "brain:in-progress"),
                (Blocked, "brain:blocked"),
                (Done, "brain:done"),
            ],
        ),
        entry(Discussion, "brain:discussion", &[]),
        entry(Decision, "brain:decision", &[(Done, "brain:done")]),
        entry(
            Incident,
            "brain:incident",
            &[(InProgress, "brain:in-progress"), (Done, "brain:done")],
        ),
        entry(Change, "brain:change", &[(Done, "brain:done")]),
        entry(
            Quote,
            "brain:quote",
            &[
                (InProgress, "brain:in-progress"),
                (Blocked, "brain:blocked"),
                (Done, "brain:done"),
            ],
        ),
    ]
}

fn validate_views(
    views: &[ViewSpec],
    type_names: &std::collections::HashSet<String>,
) -> Result<(), ConfigError> {
    let mut slugs = std::collections::HashSet::new();
    for view in views {
        if view.name.trim().is_empty() {
            return Err(ConfigError::EmptyViewName);
        }
        if !is_valid_view_slug(&view.slug) {
            return Err(ConfigError::InvalidViewSlug(view.slug.clone()));
        }
        if !slugs.insert(view.slug.clone()) {
            return Err(ConfigError::DuplicateViewSlug(view.slug.clone()));
        }
        for tag in &view.tags {
            if tag.chars().any(|c| c.is_uppercase()) {
                return Err(ConfigError::NonLowercaseViewTag {
                    view: view.slug.clone(),
                    tag: tag.clone(),
                });
            }
        }
        for type_name in &view.types {
            if !type_names.contains(type_name) {
                return Err(ConfigError::UnknownViewType {
                    view: view.slug.clone(),
                    type_name: type_name.clone(),
                });
            }
        }
    }
    Ok(())
}

fn is_valid_view_slug(slug: &str) -> bool {
    !slug.is_empty()
        && slug
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
}

/// Auto-derive a URL-safe slug from a human-readable view name. The GUI uses
/// this as the default value of the `slug` field; users can override it
/// explicitly when they want to control the URL or resolve a collision.
///
/// Behavior: lowercase, ASCII alphanumerics + `-`/`_` kept, every other run of
/// characters collapses to a single `-`. Leading/trailing `-` stripped.
/// Empty or all-non-alnum input yields an empty string — callers should treat
/// that as "user must enter an explicit slug".
pub fn slugify_view_name(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut prev_dash = true;
    for c in name.chars() {
        let lc = c.to_ascii_lowercase();
        if lc.is_ascii_alphanumeric() || lc == '_' {
            out.push(lc);
            prev_dash = false;
        } else if !prev_dash {
            out.push('-');
            prev_dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    out
}

fn is_valid_hex_color(s: &str) -> bool {
    let bytes = s.as_bytes();
    bytes.len() == 7 && bytes[0] == b'#' && bytes[1..].iter().all(|b| b.is_ascii_hexdigit())
}

fn validate_frontmatter_fields(spec: &NodeTypeSpec) -> Result<(), ConfigError> {
    for (field, key) in [
        ("title_key", spec.title_key.as_deref()),
        ("date_create_field", spec.date_create_field.as_deref()),
        ("date_update_field", spec.date_update_field.as_deref()),
    ] {
        let Some(key) = key else {
            continue;
        };
        if key.trim().is_empty() || RESERVED_FRONTMATTER_FIELDS.contains(&key) {
            return Err(ConfigError::InvalidFrontmatterField {
                name: spec.name.clone(),
                field,
                key: key.to_string(),
            });
        }
    }

    if let (Some(title_key), Some(date_create_field)) =
        (spec.title_key.as_deref(), spec.date_create_field.as_deref())
        && title_key == date_create_field
    {
        return Err(ConfigError::InvalidFrontmatterField {
            name: spec.name.clone(),
            field: "date_create_field",
            key: date_create_field.to_string(),
        });
    }

    if let (Some(title_key), Some(date_update_field)) =
        (spec.title_key.as_deref(), spec.date_update_field.as_deref())
        && title_key == date_update_field
    {
        return Err(ConfigError::InvalidFrontmatterField {
            name: spec.name.clone(),
            field: "date_update_field",
            key: date_update_field.to_string(),
        });
    }

    Ok(())
}

fn validate_link_fields(
    specs: &[NodeTypeSpec],
    type_names: &std::collections::HashSet<String>,
) -> Result<(), ConfigError> {
    for spec in specs {
        for (field, target_type) in &spec.link_fields {
            if field.trim().is_empty() || RESERVED_FRONTMATTER_FIELDS.contains(&field.as_str()) {
                return Err(ConfigError::InvalidFrontmatterField {
                    name: spec.name.clone(),
                    field: "link_fields",
                    key: field.clone(),
                });
            }
            if !type_names.contains(target_type) {
                return Err(ConfigError::UnknownLinkFieldTarget {
                    name: spec.name.clone(),
                    field: field.clone(),
                    target_type: target_type.clone(),
                });
            }
        }
    }
    Ok(())
}

impl Default for BrainConfig {
    /// Equivalent to the hardcoded `NodeType` enum — repos without a
    /// `.brain-config.yml` must behave identically to pre-Phase-1 installs.
    fn default() -> Self {
        use serde_yaml::Value;
        let s = |v: &str| Value::String(v.to_string());
        let seed = |pairs: &[(&str, Value)]| -> BTreeMap<String, Value> {
            pairs
                .iter()
                .map(|(k, v)| (k.to_string(), v.clone()))
                .collect()
        };

        BrainConfig {
            node_types: vec![
                NodeTypeSpec {
                    name: "concept".into(),
                    label: "Concept".into(),
                    directory: "concepts".into(),
                    accent: "#2dd4bf".into(),
                    template_filename: Some("ConceptNote.md".into()),
                    creatable: true,
                    frontmatter_seed: BTreeMap::new(),
                    title_key: Some("topic".into()),
                    date_create_field: Some("date_created".into()),
                    date_update_field: None,
                    body_label: Some("Summary".into()),
                    work_item_kind: None,
                    link_fields: BTreeMap::new(),
                },
                NodeTypeSpec {
                    name: "adr".into(),
                    label: "ADR".into(),
                    directory: "adrs".into(),
                    accent: "#f59e0b".into(),
                    template_filename: Some("ADR.md".into()),
                    creatable: true,
                    frontmatter_seed: seed(&[("status", s("draft"))]),
                    title_key: None,
                    date_create_field: Some("date".into()),
                    date_update_field: None,
                    body_label: Some("Context".into()),
                    work_item_kind: None,
                    link_fields: BTreeMap::new(),
                },
                NodeTypeSpec {
                    name: "meeting".into(),
                    label: "Meeting".into(),
                    directory: "meetings".into(),
                    accent: "#a78bfa".into(),
                    template_filename: None,
                    creatable: true,
                    frontmatter_seed: BTreeMap::new(),
                    title_key: None,
                    date_create_field: Some("date".into()),
                    date_update_field: None,
                    body_label: Some("Summary / Notes".into()),
                    work_item_kind: None,
                    link_fields: BTreeMap::new(),
                },
                NodeTypeSpec {
                    name: "post-mortem".into(),
                    label: "Post-mortem".into(),
                    directory: "post-mortems".into(),
                    accent: "#f87171".into(),
                    template_filename: Some("PostMortem.md".into()),
                    creatable: true,
                    frontmatter_seed: seed(&[("severity", s(""))]),
                    title_key: None,
                    date_create_field: Some("incident_date".into()),
                    date_update_field: None,
                    body_label: Some("Incident Summary".into()),
                    work_item_kind: None,
                    link_fields: BTreeMap::new(),
                },
                NodeTypeSpec {
                    name: "preventivo".into(),
                    label: "Preventivo".into(),
                    directory: "preventivi".into(),
                    accent: "#38bdf8".into(),
                    template_filename: Some("Preventivo.md".into()),
                    creatable: true,
                    frontmatter_seed: seed(&[
                        ("status", s("draft")),
                        ("cliente", s("")),
                        ("modello", s("T&M")),
                    ]),
                    title_key: Some("progetto".into()),
                    date_create_field: Some("date".into()),
                    date_update_field: None,
                    body_label: Some("Riepilogo".into()),
                    work_item_kind: None,
                    link_fields: BTreeMap::new(),
                },
                NodeTypeSpec {
                    name: "runbook".into(),
                    label: "Runbook".into(),
                    directory: "runbooks".into(),
                    accent: "#4ade80".into(),
                    template_filename: Some("Runbook.md".into()),
                    creatable: true,
                    frontmatter_seed: seed(&[("service", s(""))]),
                    title_key: None,
                    date_create_field: None,
                    date_update_field: Some("last_updated".into()),
                    body_label: Some("Description".into()),
                    work_item_kind: None,
                    link_fields: BTreeMap::new(),
                },
                NodeTypeSpec {
                    name: "tag".into(),
                    label: "Tag".into(),
                    directory: String::new(),
                    accent: "#64748b".into(),
                    template_filename: None,
                    creatable: false,
                    frontmatter_seed: BTreeMap::new(),
                    title_key: None,
                    date_create_field: None,
                    date_update_field: None,
                    body_label: Some("Body".into()),
                    work_item_kind: None,
                    link_fields: BTreeMap::new(),
                },
            ],
            default_type: "concept".into(),
            label_taxonomy: default_label_taxonomy(),
            views: Vec::new(),
        }
    }
}

#[cfg(test)]
mod config_tests {
    use super::*;

    #[test]
    fn default_has_seven_types_and_validates() {
        let cfg = BrainConfig::default();
        assert_eq!(cfg.node_types.len(), 7);
        cfg.validate().expect("default must validate");
        assert_eq!(cfg.default_spec().name, "concept");
        assert_eq!(cfg.creatable().count(), 6);
    }

    #[test]
    fn default_directories_match_enum() {
        let cfg = BrainConfig::default();
        let dirs: Vec<&str> = cfg
            .node_types
            .iter()
            .map(|s| s.directory.as_str())
            .collect();
        assert_eq!(
            dirs,
            vec![
                "concepts",
                "adrs",
                "meetings",
                "post-mortems",
                "preventivi",
                "runbooks",
                "",
            ]
        );
    }

    #[test]
    fn rejects_duplicate_directory() {
        let yaml = r##"
default_type: a
node_types:
  - { name: a, label: A, directory: x, accent: "#112233" }
  - { name: b, label: B, directory: x, accent: "#445566" }
"##;
        assert!(matches!(
            BrainConfig::parse(yaml),
            Err(ConfigError::DuplicateDirectory(_))
        ));
    }

    #[test]
    fn rejects_invalid_accent() {
        let yaml = r##"
default_type: a
node_types:
  - { name: a, label: A, directory: x, accent: "nope" }
"##;
        assert!(matches!(
            BrainConfig::parse(yaml),
            Err(ConfigError::InvalidAccent { .. })
        ));
    }

    #[test]
    fn rejects_unknown_default_type() {
        let yaml = r##"
default_type: ghost
node_types:
  - { name: a, label: A, directory: x, accent: "#112233" }
"##;
        assert!(matches!(
            BrainConfig::parse(yaml),
            Err(ConfigError::UnknownDefault(_))
        ));
    }

    #[test]
    fn rejects_reserved_name() {
        let yaml = r##"
default_type: tags
node_types:
  - { name: tags, label: X, directory: x, accent: "#112233" }
"##;
        assert!(matches!(
            BrainConfig::parse(yaml),
            Err(ConfigError::ReservedName(_))
        ));
    }

    #[test]
    fn rejects_reserved_frontmatter_field_mapping() {
        let yaml = r##"
default_type: concept
node_types:
  - { name: concept, label: Concept, directory: concepts, accent: "#112233", title_key: tags }
"##;
        assert!(matches!(
            BrainConfig::parse(yaml),
            Err(ConfigError::InvalidFrontmatterField {
                field: "title_key",
                ..
            })
        ));
    }

    #[test]
    fn rejects_title_key_collision_with_date_field() {
        let yaml = r##"
default_type: concept
node_types:
  - { name: concept, label: Concept, directory: concepts, accent: "#112233", title_key: topic, date_create_field: topic }
"##;
        assert!(matches!(
            BrainConfig::parse(yaml),
            Err(ConfigError::InvalidFrontmatterField {
                field: "date_create_field",
                ..
            })
        ));
    }

    #[test]
    fn default_label_taxonomy_has_builtin_kinds() {
        let cfg = BrainConfig::default();
        assert_eq!(cfg.label_taxonomy.len(), 6);
        let task_spec = cfg.labels_for_kind(&WorkItemKind::Task).unwrap();
        assert_eq!(task_spec.kind_label, "brain:task");
        assert!(
            task_spec
                .state_labels
                .contains_key(&WorkItemState::InProgress)
        );
        let quote_spec = cfg.labels_for_kind(&WorkItemKind::Quote).unwrap();
        assert_eq!(quote_spec.kind_label, "brain:quote");
        assert!(
            quote_spec
                .state_labels
                .contains_key(&WorkItemState::InProgress)
        );
        let all: Vec<&str> = cfg.all_kind_labels().collect();
        assert!(all.contains(&"brain:task"));
        assert!(all.contains(&"brain:incident"));
        assert!(all.contains(&"brain:quote"));
    }

    #[test]
    fn label_taxonomy_roundtrips_yaml() {
        let cfg = BrainConfig::default();
        let yaml = serde_yaml::to_string(&cfg).unwrap();
        let parsed = BrainConfig::parse(&yaml).unwrap();
        assert_eq!(cfg.label_taxonomy, parsed.label_taxonomy);
    }

    #[test]
    fn config_without_label_taxonomy_uses_default() {
        let yaml = r##"
default_type: concept
node_types:
  - { name: concept, label: Concept, directory: concepts, accent: "#112233" }
"##;
        let cfg = BrainConfig::parse(yaml).unwrap();
        assert_eq!(cfg.label_taxonomy.len(), 6);
    }

    #[test]
    fn link_fields_roundtrip_yaml() {
        let yaml = r##"
default_type: pokemon
node_types:
  - name: pokemon
    label: Pokemon
    directory: pokemon
    accent: "#ef4444"
    link_fields:
      trainer: trainer
  - { name: trainer, label: Trainer, directory: trainers, accent: "#2563eb" }
"##;
        let cfg = BrainConfig::parse(yaml).unwrap();
        assert_eq!(
            cfg.lookup("pokemon")
                .unwrap()
                .link_fields
                .get("trainer")
                .map(String::as_str),
            Some("trainer")
        );

        let yaml = serde_yaml::to_string(&cfg).unwrap();
        assert!(yaml.contains("link_fields:"));
    }

    #[test]
    fn rejects_link_field_unknown_target_type() {
        let yaml = r##"
default_type: pokemon
node_types:
  - name: pokemon
    label: Pokemon
    directory: pokemon
    accent: "#ef4444"
    link_fields:
      trainer: trainer
"##;
        assert!(matches!(
            BrainConfig::parse(yaml),
            Err(ConfigError::UnknownLinkFieldTarget { .. })
        ));
    }

    #[test]
    fn roundtrip_yaml() {
        let cfg = BrainConfig::default();
        let yaml = serde_yaml::to_string(&cfg).unwrap();
        let parsed = BrainConfig::parse(&yaml).unwrap();
        assert_eq!(cfg, parsed);
    }

    #[test]
    fn views_default_is_empty_and_omitted_from_yaml() {
        let cfg = BrainConfig::default();
        assert!(cfg.views.is_empty());
        let yaml = serde_yaml::to_string(&cfg).unwrap();
        assert!(
            !yaml.contains("views:"),
            "empty views should not be serialized:\n{yaml}"
        );
    }

    #[test]
    fn views_roundtrip_yaml_preserves_order_and_fields() {
        let cfg = BrainConfig {
            views: vec![
                ViewSpec {
                    name: "Open tasks".into(),
                    slug: "open-tasks".into(),
                    tags: vec!["urgent".into()],
                    types: vec!["concept".into()],
                    weight: None,
                },
                ViewSpec {
                    name: "All ADRs".into(),
                    slug: "adrs".into(),
                    tags: vec![],
                    types: vec!["adr".into()],
                    weight: None,
                },
            ],
            ..Default::default()
        };
        let yaml = serde_yaml::to_string(&cfg).unwrap();
        let parsed = BrainConfig::parse(&yaml).unwrap();
        assert_eq!(cfg.views, parsed.views);
    }

    /// Simulates the `SaveViews` server fn round-trip: start from an existing
    /// non-trivial config, mutate ONLY the `views` block, re-serialize, reparse,
    /// and verify everything else came back identical. Guards against the most
    /// likely regression mode — a non-views field silently dropped or
    /// reshaped during the save path.
    #[test]
    fn save_views_roundtrip_preserves_other_fields() {
        let original = BrainConfig::default();
        let yaml_before = serde_yaml::to_string(&original).unwrap();

        let mut cfg = BrainConfig::parse(&yaml_before).unwrap();
        cfg.views = vec![
            ViewSpec {
                name: "Active tasks".into(),
                slug: "active-tasks".into(),
                tags: vec!["urgent".into(), "blocked".into()],
                types: vec!["concept".into(), "adr".into()],
                weight: None,
            },
            ViewSpec {
                name: "Recent decisions".into(),
                slug: "recent-decisions".into(),
                tags: vec![],
                types: vec!["adr".into()],
                weight: None,
            },
        ];
        cfg.validate().expect("post-mutation config must validate");

        let yaml_after = serde_yaml::to_string(&cfg).unwrap();
        let reparsed = BrainConfig::parse(&yaml_after).unwrap();

        assert_eq!(reparsed.node_types, original.node_types);
        assert_eq!(reparsed.default_type, original.default_type);
        assert_eq!(reparsed.label_taxonomy, original.label_taxonomy);
        assert_eq!(reparsed.views, cfg.views);

        // Clearing views again must drop the block from YAML (so configs that
        // never had views go back to byte-equivalent with the original).
        let mut cleared = reparsed.clone();
        cleared.views.clear();
        let yaml_cleared = serde_yaml::to_string(&cleared).unwrap();
        assert!(
            !yaml_cleared.contains("views:"),
            "cleared views should not appear in YAML:\n{yaml_cleared}"
        );
        assert_eq!(yaml_cleared, yaml_before);
    }

    /// Real-world shape closer to what a SaveViews call sees: a target with a
    /// custom `node_types` list (not the built-in default), a non-default
    /// `label_taxonomy`, AND existing views that get extended. Ensures the
    /// round-trip works against a config a target author actually wrote.
    #[test]
    fn save_views_roundtrip_with_custom_config_and_existing_views() {
        let yaml_before = r##"
default_type: note
node_types:
  - name: note
    label: Note
    directory: notes
    accent: "#abcdef"
    creatable: true
  - name: task
    label: Task
    directory: tasks
    accent: "#123456"
    creatable: true
    work_item_kind: task
label_taxonomy:
  - kind: task
    kind_label: "team:task"
    state_labels:
      done: "team:done"
views:
  - { name: Pinned, slug: pinned, tags: [pinned] }
"##;
        let original = BrainConfig::parse(yaml_before).unwrap();
        assert_eq!(original.views.len(), 1);

        let mut cfg = original.clone();
        cfg.views.push(ViewSpec {
            name: "Open tasks".into(),
            slug: "open-tasks".into(),
            tags: vec![],
            types: vec!["task".into()],
            weight: None,
        });
        cfg.validate().unwrap();

        let yaml_after = serde_yaml::to_string(&cfg).unwrap();
        let reparsed = BrainConfig::parse(&yaml_after).unwrap();

        assert_eq!(reparsed.node_types, original.node_types);
        assert_eq!(reparsed.default_type, original.default_type);
        assert_eq!(reparsed.label_taxonomy, original.label_taxonomy);
        assert_eq!(reparsed.views.len(), 2);
        assert_eq!(reparsed.views[0].slug, "pinned");
        assert_eq!(reparsed.views[1].slug, "open-tasks");
        assert_eq!(reparsed.views[1].types, vec!["task"]);
    }

    #[test]
    fn sorted_views_falls_back_to_yaml_order_when_no_weights() {
        let cfg = BrainConfig {
            views: vec![
                ViewSpec {
                    name: "A".into(),
                    slug: "a".into(),
                    tags: vec![],
                    types: vec![],
                    weight: None,
                },
                ViewSpec {
                    name: "B".into(),
                    slug: "b".into(),
                    tags: vec![],
                    types: vec![],
                    weight: None,
                },
            ],
            ..Default::default()
        };
        let order: Vec<&str> = cfg.sorted_views().iter().map(|v| v.slug.as_str()).collect();
        assert_eq!(order, vec!["a", "b"]);
    }

    #[test]
    fn sorted_views_floats_negative_weight_to_top() {
        let cfg = BrainConfig {
            views: vec![
                ViewSpec {
                    name: "First in YAML".into(),
                    slug: "first".into(),
                    tags: vec![],
                    types: vec![],
                    weight: None,
                },
                ViewSpec {
                    name: "Pinned".into(),
                    slug: "pinned".into(),
                    tags: vec![],
                    types: vec![],
                    weight: Some(-10),
                },
                ViewSpec {
                    name: "Third".into(),
                    slug: "third".into(),
                    tags: vec![],
                    types: vec![],
                    weight: None,
                },
            ],
            ..Default::default()
        };
        // Pinned (-10) jumps to the front; the others keep their relative YAML
        // order (both default to weight 0).
        let order: Vec<&str> = cfg.sorted_views().iter().map(|v| v.slug.as_str()).collect();
        assert_eq!(order, vec!["pinned", "first", "third"]);
    }

    #[test]
    fn sorted_views_is_stable_on_equal_weights() {
        let cfg = BrainConfig {
            views: vec![
                ViewSpec {
                    name: "A".into(),
                    slug: "a".into(),
                    tags: vec![],
                    types: vec![],
                    weight: Some(5),
                },
                ViewSpec {
                    name: "B".into(),
                    slug: "b".into(),
                    tags: vec![],
                    types: vec![],
                    weight: Some(5),
                },
            ],
            ..Default::default()
        };
        let order: Vec<&str> = cfg.sorted_views().iter().map(|v| v.slug.as_str()).collect();
        assert_eq!(order, vec!["a", "b"]);
    }

    #[test]
    fn view_weight_roundtrips_yaml_and_omits_default() {
        let cfg = BrainConfig {
            views: vec![
                ViewSpec {
                    name: "Pinned".into(),
                    slug: "pinned".into(),
                    tags: vec![],
                    types: vec![],
                    weight: Some(-1),
                },
                ViewSpec {
                    name: "Plain".into(),
                    slug: "plain".into(),
                    tags: vec![],
                    types: vec![],
                    weight: None,
                },
            ],
            ..Default::default()
        };
        let yaml = serde_yaml::to_string(&cfg).unwrap();
        // Weight is emitted for the pinned view…
        assert!(yaml.contains("weight: -1"), "yaml: {yaml}");
        // …and omitted entirely for the one that uses the default. This keeps
        // existing configs byte-equivalent after a roundtrip.
        let plain_block_has_weight = yaml
            .lines()
            .skip_while(|l| !l.contains("slug: plain"))
            .take(4)
            .any(|l| l.contains("weight:"));
        assert!(!plain_block_has_weight, "yaml: {yaml}");
        let parsed = BrainConfig::parse(&yaml).unwrap();
        assert_eq!(parsed.views[0].weight, Some(-1));
        assert_eq!(parsed.views[1].weight, None);
    }

    #[test]
    fn views_validation_rejects_duplicate_slug() {
        let yaml = r##"
default_type: concept
node_types:
  - { name: concept, label: Concept, directory: concepts, accent: "#112233" }
views:
  - { name: One, slug: same }
  - { name: Two, slug: same }
"##;
        assert!(matches!(
            BrainConfig::parse(yaml),
            Err(ConfigError::DuplicateViewSlug(_))
        ));
    }

    #[test]
    fn views_validation_rejects_unknown_type_reference() {
        let yaml = r##"
default_type: concept
node_types:
  - { name: concept, label: Concept, directory: concepts, accent: "#112233" }
views:
  - { name: Ghosts, slug: ghosts, types: [ghost] }
"##;
        assert!(matches!(
            BrainConfig::parse(yaml),
            Err(ConfigError::UnknownViewType { .. })
        ));
    }

    #[test]
    fn views_validation_rejects_uppercase_tag() {
        let yaml = r##"
default_type: concept
node_types:
  - { name: concept, label: Concept, directory: concepts, accent: "#112233" }
views:
  - { name: Bad, slug: bad, tags: [URGENT] }
"##;
        assert!(matches!(
            BrainConfig::parse(yaml),
            Err(ConfigError::NonLowercaseViewTag { .. })
        ));
    }

    #[test]
    fn views_validation_rejects_invalid_slug() {
        let yaml = r##"
default_type: concept
node_types:
  - { name: concept, label: Concept, directory: concepts, accent: "#112233" }
views:
  - { name: Bad, slug: "Has Space" }
"##;
        assert!(matches!(
            BrainConfig::parse(yaml),
            Err(ConfigError::InvalidViewSlug(_))
        ));
    }

    #[test]
    fn views_validation_rejects_empty_name() {
        let yaml = r##"
default_type: concept
node_types:
  - { name: concept, label: Concept, directory: concepts, accent: "#112233" }
views:
  - { name: "", slug: empty }
"##;
        assert!(matches!(
            BrainConfig::parse(yaml),
            Err(ConfigError::EmptyViewName)
        ));
    }

    #[test]
    fn slugify_handles_common_inputs() {
        assert_eq!(slugify_view_name("Open Tasks"), "open-tasks");
        assert_eq!(slugify_view_name("  Trim & Split  "), "trim-split");
        assert_eq!(slugify_view_name("ADRs"), "adrs");
        assert_eq!(slugify_view_name("Café résumé"), "caf-r-sum");
        assert_eq!(slugify_view_name("under_score"), "under_score");
        assert_eq!(slugify_view_name("---"), "");
        assert_eq!(slugify_view_name(""), "");
    }

    #[test]
    fn parses_optional_work_item_kind() {
        let yaml = r##"
default_type: task
node_types:
  - name: task
    label: Task
    directory: tasks
    accent: "#112233"
    work_item_kind: task
"##;
        let cfg = BrainConfig::parse(yaml).unwrap();
        assert_eq!(
            cfg.lookup("task").and_then(|s| s.work_item_kind.clone()),
            Some(WorkItemKind::Task)
        );
        assert!(cfg.lookup("task").is_some_and(|s| s.is_work_item()));
    }

    #[test]
    fn parses_quote_work_item_kind() {
        let yaml = r##"
default_type: preventivo
node_types:
  - name: preventivo
    label: Preventivo
    directory: preventivi
    accent: "#38bdf8"
    work_item_kind: quote
"##;
        let cfg = BrainConfig::parse(yaml).unwrap();
        assert_eq!(
            cfg.lookup("preventivo")
                .and_then(|s| s.work_item_kind.clone()),
            Some(WorkItemKind::Quote)
        );
    }

    fn gh(org: &str, repo: &str, branch: &str) -> GithubClient {
        GithubClient::new(TargetConfig {
            org: org.into(),
            repo: repo.into(),
            branch: branch.into(),
        })
    }

    #[test]
    fn git_data_api_urls_target_repo() {
        let c = gh("acme", "kb", "main");
        assert_eq!(
            c.git_blobs_url(),
            "https://api.github.com/repos/acme/kb/git/blobs"
        );
        assert_eq!(
            c.git_trees_url(),
            "https://api.github.com/repos/acme/kb/git/trees"
        );
        assert_eq!(
            c.git_commits_url(),
            "https://api.github.com/repos/acme/kb/git/commits"
        );
        assert_eq!(
            c.git_commit_url("abc"),
            "https://api.github.com/repos/acme/kb/git/commits/abc"
        );
        assert_eq!(
            c.git_ref_url(),
            "https://api.github.com/repos/acme/kb/git/refs/heads/main"
        );
    }

    #[test]
    fn git_tree_by_sha_url_is_recursive() {
        let c = gh("acme", "kb", "main");
        assert_eq!(
            c.git_tree_by_sha_url("ABC123"),
            "https://api.github.com/repos/acme/kb/git/trees/ABC123?recursive=1"
        );
    }

    #[test]
    fn git_ref_url_uses_branch() {
        let c = gh("o", "r", "feat/x");
        assert_eq!(
            c.git_ref_url(),
            "https://api.github.com/repos/o/r/git/refs/heads/feat/x"
        );
    }

    #[test]
    fn with_branch_preserves_repository_and_api_base() {
        let c = gh("acme", "kb", "main").with_api_base("https://example.test/api/");
        let branch = c.with_branch("patch/alice/change");

        assert_eq!(branch.target().org, "acme");
        assert_eq!(branch.target().repo, "kb");
        assert_eq!(branch.target().branch, "patch/alice/change");
        assert_eq!(
            branch.git_ref_url(),
            "https://example.test/api/repos/acme/kb/git/refs/heads/patch/alice/change"
        );
        assert_eq!(c.target().branch, "main");
    }

    #[test]
    fn issue_comments_url_targets_external_project() {
        let c = gh("app", "runtime", "main").with_api_base("https://example.test/api/");
        assert_eq!(
            c.issue_comments_url("acme/kb", "42").unwrap(),
            "https://example.test/api/repos/acme/kb/issues/42/comments?per_page=100"
        );
    }

    #[test]
    fn user_repos_url_uses_configured_api_base() {
        let c = gh("app", "runtime", "main").with_api_base("https://example.test/api/");
        assert_eq!(
            c.user_repos_url(),
            "https://example.test/api/user/repos?per_page=100&sort=pushed&affiliation=owner,collaborator,organization_member"
        );
    }

    #[test]
    fn app_installation_token_url_uses_configured_api_base() {
        assert_eq!(
            GithubClient::app_installation_access_tokens_url("https://example.test/api/", "12345"),
            "https://example.test/api/app/installations/12345/access_tokens"
        );
    }

    #[test]
    fn branch_url_encodes_branch_as_single_segment() {
        let c = gh("acme", "kb", "main").with_api_base("https://example.test/api/");
        assert_eq!(
            c.branch_url("feature/foo"),
            "https://example.test/api/repos/acme/kb/branches/feature%2Ffoo"
        );
    }

    #[test]
    fn contents_url_encodes_path_segments_but_preserves_slashes() {
        let c = gh("acme", "kb", "main").with_api_base("https://example.test/api/");
        assert_eq!(
            c.contents_url("notes/space name/#draft?.md"),
            "https://example.test/api/repos/acme/kb/contents/notes/space%20name/%23draft%3F.md"
        );
    }

    #[test]
    fn contents_url_double_encodes_dot_only_segments() {
        let c = gh("acme", "kb", "main").with_api_base("https://example.test/api/");
        assert_eq!(
            c.contents_url("../branches/main"),
            "https://example.test/api/repos/acme/kb/contents/%252E%252E/branches/main"
        );
        assert_eq!(
            c.contents_url("./branches/main"),
            "https://example.test/api/repos/acme/kb/contents/%252E/branches/main"
        );
        assert_eq!(
            c.contents_url("notes/a.b.md"),
            "https://example.test/api/repos/acme/kb/contents/notes/a.b.md"
        );
    }

    #[test]
    fn path_segment_helpers_roundtrip_branch_values() {
        let branch = "feature/foo";
        assert_eq!(decode_path_segment(&encode_path_segment(branch)), branch);
    }

    // ----- TargetRef -----

    #[test]
    fn target_ref_validate_accepts_canonical_inputs() {
        TargetRef::new("acme", "kb", "main").validate().unwrap();
        // Branch with `/` is valid (e.g. feature branches).
        TargetRef::new("acme", "kb", "feature/foo")
            .validate()
            .unwrap();
    }

    #[test]
    fn target_ref_validate_rejects_empty_components() {
        let e = TargetRef::new("", "kb", "main").validate().unwrap_err();
        assert!(matches!(e, TargetRefError::Empty { field: "org" }));
        let e = TargetRef::new("acme", "", "main").validate().unwrap_err();
        assert!(matches!(e, TargetRefError::Empty { field: "repo" }));
        let e = TargetRef::new("acme", "kb", "").validate().unwrap_err();
        assert!(matches!(e, TargetRefError::Empty { field: "branch" }));
    }

    #[test]
    fn target_ref_validate_rejects_path_traversal() {
        let e = TargetRef::new("acme", "kb", "..").validate().unwrap_err();
        assert!(matches!(
            e,
            TargetRefError::PathTraversal {
                field: "branch",
                ..
            }
        ));
        let e = TargetRef::new("acme", "kb", "feature/../escape")
            .validate()
            .unwrap_err();
        assert!(matches!(e, TargetRefError::PathTraversal { .. }));
        // Single-dot segment also forbidden in branch — leaks `current dir`
        // semantics into URL/path interpolation.
        let e = TargetRef::new("acme", "kb", ".").validate().unwrap_err();
        assert!(matches!(e, TargetRefError::PathTraversal { .. }));
    }

    #[test]
    fn target_ref_validate_rejects_slash_in_org_or_repo() {
        // org and repo are single segments on every supported forge.
        let e = TargetRef::new("ac/me", "kb", "main")
            .validate()
            .unwrap_err();
        assert!(matches!(
            e,
            TargetRefError::PathTraversal { field: "org", .. }
        ));
        let e = TargetRef::new("acme", "k/b", "main")
            .validate()
            .unwrap_err();
        assert!(matches!(
            e,
            TargetRefError::PathTraversal { field: "repo", .. }
        ));
    }

    #[test]
    fn target_ref_validate_rejects_leading_slash() {
        let e = TargetRef::new("/acme", "kb", "main")
            .validate()
            .unwrap_err();
        assert!(matches!(
            e,
            TargetRefError::LeadingSlash { field: "org", .. }
        ));
    }

    #[test]
    fn target_ref_validate_rejects_control_chars() {
        let e = TargetRef::new("acme", "kb", "main\n")
            .validate()
            .unwrap_err();
        assert!(matches!(
            e,
            TargetRefError::NonPrintable {
                field: "branch",
                ..
            }
        ));
    }

    #[test]
    fn target_ref_validate_rejects_oversized_components() {
        let long = "x".repeat(super::MAX_TARGET_COMPONENT_BYTES + 1);
        let e = TargetRef::new("acme", "kb", long).validate().unwrap_err();
        assert!(matches!(
            e,
            TargetRefError::TooLong {
                field: "branch",
                ..
            }
        ));
    }

    #[test]
    fn target_ref_and_target_config_produce_same_key() {
        let r = TargetRef::new("acme", "kb", "main");
        let c = TargetConfig {
            org: "acme".into(),
            repo: "kb".into(),
            branch: "main".into(),
        };
        assert_eq!(TargetKey::from(&r), TargetKey::from(&c));
        assert_eq!(TargetKey::from(&r).as_str(), "acme/kb/main");
    }

    #[test]
    fn target_key_parses_validated_key_string() {
        let key = TargetKey::try_from_key_string("acme/kb/feature/foo").unwrap();
        assert_eq!(key.as_str(), "acme/kb/feature/foo");

        let e = TargetKey::try_from_key_string("acme/kb/../escape").unwrap_err();
        assert!(matches!(
            e,
            TargetRefError::PathTraversal {
                field: "branch",
                ..
            }
        ));
        let e = TargetKey::try_from_key_string("acme/kb").unwrap_err();
        assert!(matches!(e, TargetRefError::Empty { field: "branch" }));
    }

    #[test]
    fn target_ref_parses_validated_key_string() {
        let target = TargetRef::try_from_key_string("acme/kb/feature/foo").unwrap();
        assert_eq!(target, TargetRef::new("acme", "kb", "feature/foo"));

        let e = TargetRef::try_from_key_string("acme/kb/../escape").unwrap_err();
        assert!(matches!(e, TargetRefError::PathTraversal { .. }));
    }

    #[test]
    fn target_ref_roundtrips_through_target_config() {
        let original = TargetRef::new("acme", "kb", "feature/foo");
        let cfg: TargetConfig = (&original).into();
        let back: TargetRef = (&cfg).into();
        assert_eq!(original, back);
    }

    #[test]
    fn target_ref_serde_roundtrip() {
        let r = TargetRef::new("acme", "kb", "main");
        let json = serde_json::to_string(&r).unwrap();
        let back: TargetRef = serde_json::from_str(&json).unwrap();
        assert_eq!(r, back);
    }

    #[test]
    fn target_ref_display_matches_target_key() {
        let r = TargetRef::new("acme", "kb", "main");
        assert_eq!(format!("{r}"), TargetKey::from(&r).as_str());
    }
}
