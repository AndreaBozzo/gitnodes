//! Runtime configuration loaded from the process environment.
//!
//! These values are read once at server startup (see `brain-app/src/main.rs`),
//! then passed explicitly through constructors or provided via Leptos context.
//! No code outside of `main.rs` should call `std::env::var` for these keys.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use thiserror::Error;

use crate::work_items::{WorkItemKind, WorkItemState};

/// The GitHub repository the app reads from and writes to.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TargetConfig {
    pub org: String,
    pub repo: String,
    pub branch: String,
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
}

impl From<&TargetConfig> for TargetKey {
    fn from(t: &TargetConfig) -> Self {
        Self(format!("{}/{}/{}", t.org, t.repo, t.branch))
    }
}

impl std::fmt::Display for TargetKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
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
        let mut base = base.into();
        while base.ends_with('/') {
            base.pop();
        }
        self.api_base = base;
        self
    }

    pub fn target(&self) -> &TargetConfig {
        &self.target
    }

    pub fn contents_url(&self, path: &str) -> String {
        format!(
            "{}/repos/{}/{}/contents/{}",
            self.api_base, self.target.org, self.target.repo, path
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
}

impl From<TargetConfig> for GithubClient {
    fn from(target: TargetConfig) -> Self {
        Self::new(target)
    }
}

/// User-facing branding copy (landing page title, access-denied messages).
#[derive(Clone, Debug, Serialize, Deserialize)]
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
    /// Defaults to the five built-in kinds with `brain:*` label names.
    #[serde(
        default = "default_label_taxonomy",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub label_taxonomy: Vec<WorkItemLabelSpec>,
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
    ]
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
                },
            ],
            default_type: "concept".into(),
            label_taxonomy: default_label_taxonomy(),
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
    fn default_label_taxonomy_has_five_kinds() {
        let cfg = BrainConfig::default();
        assert_eq!(cfg.label_taxonomy.len(), 5);
        let task_spec = cfg.labels_for_kind(&WorkItemKind::Task).unwrap();
        assert_eq!(task_spec.kind_label, "brain:task");
        assert!(
            task_spec
                .state_labels
                .contains_key(&WorkItemState::InProgress)
        );
        let all: Vec<&str> = cfg.all_kind_labels().collect();
        assert!(all.contains(&"brain:task"));
        assert!(all.contains(&"brain:incident"));
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
        assert_eq!(cfg.label_taxonomy.len(), 5);
    }

    #[test]
    fn roundtrip_yaml() {
        let cfg = BrainConfig::default();
        let yaml = serde_yaml::to_string(&cfg).unwrap();
        let parsed = BrainConfig::parse(&yaml).unwrap();
        assert_eq!(cfg, parsed);
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
}
