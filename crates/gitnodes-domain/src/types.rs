use crate::config::{BrainConfig, TargetRef};
use crate::frontmatter::split_frontmatter;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::collections::BTreeMap;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Node {
    pub id: u32,
    pub title: String,
    pub summary: String,
    pub node_type: String,
    pub tags: Vec<String>,
    pub x: f32,
    pub y: f32,
    /// Relative path in the Brain repo (e.g. "concepts/Foo.md").
    #[serde(default)]
    pub path: String,
    /// GitHub file SHA for optimistic concurrency on updates.
    #[serde(default)]
    pub sha: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EdgeKind {
    #[default]
    Body,
    Frontmatter(String),
    Tag,
}

impl EdgeKind {
    pub fn storage_key(&self) -> Cow<'_, str> {
        match self {
            Self::Body => Cow::Borrowed("body"),
            Self::Frontmatter(field) => Cow::Owned(format!("frontmatter:{field}")),
            Self::Tag => Cow::Borrowed("tag"),
        }
    }

    pub fn from_storage_key(value: &str) -> Self {
        if let Some(field) = value.strip_prefix("frontmatter:") {
            Self::Frontmatter(field.to_string())
        } else if value == "tag" {
            Self::Tag
        } else {
            Self::Body
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Edge {
    pub from: u32,
    pub to: u32,
    #[serde(default)]
    pub kind: EdgeKind,
}

/// Snapshot of an existing doc's structured fields, used to prefill the editor on edit.
#[derive(Clone, Debug, Default)]
pub struct EditPrefill {
    pub path: String,
    pub sha: String,
    pub node_type: Option<String>,
    pub title: String,
    pub author: String,
    pub tags: Vec<String>,
    /// Full body (everything after the frontmatter), preserved verbatim.
    pub body: String,
    pub related: Vec<String>,
    /// Full parsed frontmatter, preserved so `save_brain_file` can merge
    /// form-controlled fields without wiping custom keys (status, severity,
    /// owner, etc.). Empty map if the file has no frontmatter.
    pub frontmatter: BTreeMap<String, serde_yaml::Value>,
    /// True when the source file had a frontmatter block that failed YAML
    /// parsing. The editor must surface this and prevent saves to avoid
    /// silently rewriting the file from defaults and dropping the original
    /// keys.
    pub frontmatter_malformed: bool,
}

/// What the editor panel should do when open.
#[derive(Clone, Debug, Default)]
pub enum EditMode {
    #[default]
    Closed,
    New,
    Edit(Box<EditPrefill>),
}

impl EditPrefill {
    /// Parse a raw markdown file (with YAML frontmatter) from the Brain repo
    /// into the structured fields the editor expects. Body is preserved verbatim.
    pub fn from_raw(path: &str, sha: &str, raw: &str, config: &BrainConfig) -> Self {
        let (front, body) = split_frontmatter(raw);
        let mut out = EditPrefill {
            path: path.to_string(),
            sha: sha.to_string(),
            body: body.to_string(),
            ..Default::default()
        };

        if !front.trim().is_empty() {
            match serde_yaml::from_str::<BTreeMap<String, serde_yaml::Value>>(front) {
                Ok(map) => out.frontmatter = map,
                Err(_) => {
                    out.frontmatter_malformed = true;
                }
            }
        }

        if let Some(v) = out.frontmatter.get("type").and_then(|v| v.as_str()) {
            out.node_type = Some(v.to_string());
        }
        // Title key is per-type (declared in NodeTypeSpec). Custom types that
        // omit `title_key` fall back to `"topic"` so legacy/unconfigured files
        // don't silently drop the title.
        let title_key = out
            .node_type
            .as_deref()
            .and_then(|t| config.lookup(t))
            .and_then(|s| s.title_key.as_deref())
            .unwrap_or("topic");
        if let Some(v) = out.frontmatter.get(title_key).and_then(|v| v.as_str()) {
            out.title = v.to_string();
        }
        if let Some(v) = out.frontmatter.get("author").and_then(|v| v.as_str()) {
            out.author = v.to_string();
        }
        if let Some(seq) = out.frontmatter.get("tags").and_then(|v| v.as_sequence()) {
            out.tags = seq
                .iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .filter(|s| !s.is_empty())
                .collect();
        }

        // If no `topic:` field, derive title from the first heading.
        if out.title.is_empty() {
            for line in body.lines() {
                let l = line.trim_start();
                if let Some(rest) = l.strip_prefix("# ") {
                    let mut t = rest.trim().to_string();
                    for prefix in ["Concept: ", "ADR: ", "Meeting: "] {
                        if t.starts_with(prefix) {
                            t = t.trim_start_matches(prefix).to_string();
                        }
                    }
                    out.title = t;
                    break;
                }
            }
        }

        // Extract related links under "## Related / See also".
        let mut in_related = false;
        for line in body.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("## ") {
                in_related = trimmed.to_lowercase().contains("related")
                    || trimmed.to_lowercase().contains("see also");
                continue;
            }
            if in_related
                && let Some(rest) = trimmed.strip_prefix("- ")
                && let Some(open) = rest.find("](")
                && let Some(close) = rest[open + 2..].find(')')
            {
                let url = &rest[open + 2..open + 2 + close];
                let cleaned = url
                    .trim_start_matches("../")
                    .split('#')
                    .next()
                    .unwrap_or(url);
                if cleaned.ends_with(".md") && !cleaned.starts_with("http") {
                    out.related.push(cleaned.to_string());
                }
            }
        }

        out
    }
}

/// How the author wants a write committed. Decouples *posture* (do I want
/// review?) from *capability* (am I allowed to push?). The default preserves
/// the legacy behaviour: commit directly when the user can push, fall back to a
/// PR otherwise.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WriteIntent {
    /// Direct commit if the user can push, PR fallback otherwise.
    #[default]
    Direct,
    /// Open a pull request even when the user could commit directly (opt-in
    /// review flow). Routes through the existing PR orchestrator.
    ProposeViaPr,
}

/// Payload sent from the editor form to create/update a Brain file.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BrainFilePayload {
    /// Explicit target identity for this write. Legacy clients may omit this
    /// while the single-target `/knowledge` compat page exists, but all
    /// multi-tenant UI paths must send it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<TargetRef>,
    pub node_type: String,
    pub title: String,
    pub author: String,
    pub tags: Vec<String>,
    pub body: String,
    /// Related file paths chosen via forced-linking.
    pub related: Vec<String>,
    pub folder: Option<String>,
    /// For updates: the file path and sha.
    pub path: Option<String>,
    pub sha: Option<String>,
    /// Optional user-supplied commit message. Empty/None falls back to the
    /// auto-generated "Update/Create X via Brain UI" message.
    #[serde(default)]
    pub commit_message: Option<String>,
    /// Frontmatter parsed from the original file on update. `save_brain_file`
    /// merges form-controlled fields on top of this map so custom keys
    /// (status, severity, owner, etc.) survive the round-trip. `None` on
    /// create; old clients without the field still deserialize via serde
    /// default.
    #[serde(default)]
    pub preserved_frontmatter: Option<BTreeMap<String, serde_yaml::Value>>,
    /// True when the source file's frontmatter failed to parse. Blocks save
    /// server-side so we don't silently rewrite the file from defaults.
    #[serde(default)]
    pub frontmatter_malformed: bool,
    /// Author-chosen write posture. `#[serde(default)]` → `Direct` for old
    /// clients, preserving current behaviour.
    #[serde(default)]
    pub write_intent: WriteIntent,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefill_parses_topic_and_tags() {
        let raw = "---\ntype: adr\ntopic: Foo Bar\ntags: [\"a\", b, 'c']\n---\nbody\n";
        let p = EditPrefill::from_raw("adrs/Foo.md", "abc", raw, &BrainConfig::default());
        assert_eq!(p.title, "Foo Bar");
        assert_eq!(p.node_type, Some("adr".to_string()));
        assert_eq!(p.tags, vec!["a", "b", "c"]);
        assert_eq!(p.body, "body\n");
    }

    #[test]
    fn prefill_falls_back_to_h1_title() {
        let raw = "---\ntype: concept\n---\n# Concept: Alpha\n\ncontent";
        let p = EditPrefill::from_raw("concepts/Alpha.md", "sha", raw, &BrainConfig::default());
        assert_eq!(p.title, "Alpha");
    }

    #[test]
    fn prefill_extracts_related_links() {
        let raw = "---\ntype: concept\n---\n# X\n\n## Related\n- [Foo](../concepts/Foo.md)\n- [ext](https://example.com)\n";
        let p = EditPrefill::from_raw("concepts/X.md", "s", raw, &BrainConfig::default());
        assert_eq!(p.related, vec!["concepts/Foo.md"]);
    }

    #[test]
    fn prefill_preserves_custom_frontmatter_fields() {
        // Caveat #5: a doc with custom fields (status: accepted on a non-draft
        // ADR, severity on a post-mortem) must round-trip the full map so
        // save_brain_file can merge instead of regenerating.
        let raw = "---\ntype: adr\nstatus: accepted\ndate: 2026-03-01\nauthor: alice\ntags: [\"x\"]\n---\nbody\n";
        let p = EditPrefill::from_raw("adrs/F.md", "sha", raw, &BrainConfig::default());
        assert_eq!(p.node_type, Some("adr".to_string()));
        assert_eq!(
            p.frontmatter.get("status").and_then(|v| v.as_str()),
            Some("accepted")
        );
        assert_eq!(
            p.frontmatter.get("date").and_then(|v| v.as_str()),
            Some("2026-03-01")
        );
    }

    #[test]
    fn prefill_malformed_yaml_flags_without_panicking() {
        // Malformed YAML frontmatter must not panic, must be flagged so the
        // server can refuse to silently rewrite the file from defaults even
        // if the UI block is bypassed.
        let raw = "---\ntype: concept\n: not valid :\n---\nbody";
        let p = EditPrefill::from_raw("concepts/X.md", "s", raw, &BrainConfig::default());
        assert!(p.frontmatter.is_empty());
        assert!(p.frontmatter_malformed);
        assert_eq!(p.body, "body");

        let payload = BrainFilePayload {
            target: None,
            node_type: "concept".to_string(),
            title: "X".into(),
            author: "alice".into(),
            tags: vec![],
            body: p.body.clone(),
            related: p.related.clone(),
            folder: None,
            path: Some(p.path.clone()),
            sha: Some(p.sha.clone()),
            commit_message: None,
            preserved_frontmatter: if p.frontmatter.is_empty() {
                None
            } else {
                Some(p.frontmatter.clone())
            },
            frontmatter_malformed: p.frontmatter_malformed,
            write_intent: WriteIntent::Direct,
        };
        assert!(payload.frontmatter_malformed);
    }

    #[test]
    fn prefill_custom_type_reads_title_from_configured_key() {
        use crate::config::NodeTypeSpec;
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
            date_update_field: None,
            body_label: Some("Corpo".into()),
            work_item_kind: None,
            link_fields: BTreeMap::new(),
        });
        let raw = "---\ntype: articolo\ntitolo: \"Il Mio Pezzo\"\n---\n";
        let p = EditPrefill::from_raw("articoli/Foo.md", "s", raw, &cfg);
        assert_eq!(p.node_type, Some("articolo".to_string()));
        assert_eq!(p.title, "Il Mio Pezzo");
    }

    #[test]
    fn prefill_project_title_from_name() {
        // Project stores its title under `name:`, not `topic:`. Without this
        // fallback the editor would open with an empty title and a save would
        // blank out name.
        let raw = "---\ntype: project\nname: \"Rewrite search\"\nstatus: active\n---\n";
        let p = EditPrefill::from_raw("projects/Foo.md", "s", raw, &BrainConfig::default());
        assert_eq!(p.node_type, Some("project".to_string()));
        assert_eq!(p.title, "Rewrite search");
    }
}
