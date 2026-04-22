//! Runtime configuration loaded from the process environment.
//!
//! These values are read once at server startup (see `brain-app/src/main.rs`),
//! then passed explicitly through constructors or provided via Leptos context.
//! No code outside of `main.rs` should call `std::env::var` for these keys.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use thiserror::Error;

/// The GitHub repository the app reads from and writes to.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TargetConfig {
    pub org: String,
    pub repo: String,
    pub branch: String,
}

impl TargetConfig {
    pub fn contents_url(&self, path: &str) -> String {
        format!(
            "https://api.github.com/repos/{}/{}/contents/{}",
            self.org, self.repo, path
        )
    }

    pub fn tree_url(&self) -> String {
        format!(
            "https://api.github.com/repos/{}/{}/git/trees/{}?recursive=1",
            self.org, self.repo, self.branch
        )
    }

    pub fn raw_base(&self) -> String {
        format!(
            "https://raw.githubusercontent.com/{}/{}/{}",
            self.org, self.repo, self.branch
        )
    }

    pub fn blob_base(&self) -> String {
        format!(
            "https://github.com/{}/{}/blob/{}",
            self.org, self.repo, self.branch
        )
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
    /// CSS custom-property reference string (e.g. `"var(--accent-concept)"`).
    /// Derivable from `name` but kept explicit so the `:root` palette and the
    /// config stay decoupled.
    pub accent_var: String,
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
}

fn default_true() -> bool {
    true
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
}

/// Reserved names that clash with known repo paths. A node type cannot use
/// these as its `name` or `directory`.
const RESERVED: &[&str] = &["tags", "templates"];

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
}

fn is_valid_hex_color(s: &str) -> bool {
    let bytes = s.as_bytes();
    bytes.len() == 7 && bytes[0] == b'#' && bytes[1..].iter().all(|b| b.is_ascii_hexdigit())
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
                    accent_var: "var(--accent-concept)".into(),
                    template_filename: Some("ConceptNote.md".into()),
                    creatable: true,
                    frontmatter_seed: BTreeMap::new(),
                },
                NodeTypeSpec {
                    name: "adr".into(),
                    label: "ADR".into(),
                    directory: "adrs".into(),
                    accent: "#f59e0b".into(),
                    accent_var: "var(--accent-decision)".into(),
                    template_filename: Some("ADR.md".into()),
                    creatable: true,
                    frontmatter_seed: seed(&[("status", s("draft"))]),
                },
                NodeTypeSpec {
                    name: "meeting".into(),
                    label: "Meeting".into(),
                    directory: "meetings".into(),
                    accent: "#a78bfa".into(),
                    accent_var: "var(--accent-meeting)".into(),
                    template_filename: None,
                    creatable: true,
                    frontmatter_seed: BTreeMap::new(),
                },
                NodeTypeSpec {
                    name: "post-mortem".into(),
                    label: "Post-mortem".into(),
                    directory: "post-mortems".into(),
                    accent: "#f87171".into(),
                    accent_var: "var(--accent-postmortem)".into(),
                    template_filename: Some("PostMortem.md".into()),
                    creatable: true,
                    frontmatter_seed: seed(&[("severity", s(""))]),
                },
                NodeTypeSpec {
                    name: "preventivo".into(),
                    label: "Preventivo".into(),
                    directory: "preventivi".into(),
                    accent: "#38bdf8".into(),
                    accent_var: "var(--accent-preventivo)".into(),
                    template_filename: Some("Preventivo.md".into()),
                    creatable: true,
                    frontmatter_seed: seed(&[
                        ("status", s("draft")),
                        ("cliente", s("")),
                        ("modello", s("T&M")),
                    ]),
                },
                NodeTypeSpec {
                    name: "runbook".into(),
                    label: "Runbook".into(),
                    directory: "runbooks".into(),
                    accent: "#4ade80".into(),
                    accent_var: "var(--accent-runbook)".into(),
                    template_filename: Some("Runbook.md".into()),
                    creatable: true,
                    frontmatter_seed: seed(&[("service", s(""))]),
                },
                NodeTypeSpec {
                    name: "tag".into(),
                    label: "Tag".into(),
                    directory: String::new(),
                    accent: "#64748b".into(),
                    accent_var: "var(--accent-tag)".into(),
                    template_filename: None,
                    creatable: false,
                    frontmatter_seed: BTreeMap::new(),
                },
            ],
            default_type: "concept".into(),
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
  - { name: a, label: A, directory: x, accent: "#112233", accent_var: "v" }
  - { name: b, label: B, directory: x, accent: "#445566", accent_var: "v" }
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
  - { name: a, label: A, directory: x, accent: "nope", accent_var: "v" }
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
  - { name: a, label: A, directory: x, accent: "#112233", accent_var: "v" }
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
  - { name: tags, label: X, directory: x, accent: "#112233", accent_var: "v" }
"##;
        assert!(matches!(
            BrainConfig::parse(yaml),
            Err(ConfigError::ReservedName(_))
        ));
    }

    #[test]
    fn roundtrip_yaml() {
        let cfg = BrainConfig::default();
        let yaml = serde_yaml::to_string(&cfg).unwrap();
        let parsed = BrainConfig::parse(&yaml).unwrap();
        assert_eq!(cfg, parsed);
    }
}
