use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum NodeType {
    Concept,
    Decision,
    Meeting,
    PostMortem,
    Preventivo,
    Runbook,
    Tag,
}

impl NodeType {
    pub const ALL: [NodeType; 7] = [
        NodeType::Concept,
        NodeType::Decision,
        NodeType::Meeting,
        NodeType::PostMortem,
        NodeType::Preventivo,
        NodeType::Runbook,
        NodeType::Tag,
    ];

    /// Types a user can create via the editor (excludes Tag, which is virtual).
    pub const CREATABLE: [NodeType; 6] = [
        NodeType::Concept,
        NodeType::Decision,
        NodeType::Meeting,
        NodeType::PostMortem,
        NodeType::Preventivo,
        NodeType::Runbook,
    ];

    pub fn label(self) -> &'static str {
        match self {
            NodeType::Concept => "Concept",
            NodeType::Decision => "ADR",
            NodeType::Meeting => "Meeting",
            NodeType::PostMortem => "Post-mortem",
            NodeType::Preventivo => "Preventivo",
            NodeType::Runbook => "Runbook",
            NodeType::Tag => "Tag",
        }
    }

    pub fn accent(self) -> &'static str {
        match self {
            NodeType::Concept => "#2dd4bf",
            NodeType::Decision => "#f59e0b",
            NodeType::Meeting => "#a78bfa",
            NodeType::PostMortem => "#f87171",
            NodeType::Preventivo => "#38bdf8",
            NodeType::Runbook => "#4ade80",
            NodeType::Tag => "#64748b",
        }
    }

    /// Returns the Brain repo directory for this type.
    pub fn directory(self) -> &'static str {
        match self {
            NodeType::Concept => "concepts",
            NodeType::Decision => "adrs",
            NodeType::Meeting => "meetings",
            NodeType::PostMortem => "post-mortems",
            NodeType::Preventivo => "preventivi",
            NodeType::Runbook => "runbooks",
            NodeType::Tag => "",
        }
    }

    /// Returns the Brain template frontmatter type value.
    pub fn frontmatter_type(self) -> &'static str {
        match self {
            NodeType::Concept => "concept",
            NodeType::Decision => "adr",
            NodeType::Meeting => "meeting",
            NodeType::PostMortem => "post-mortem",
            NodeType::Preventivo => "preventivo",
            NodeType::Runbook => "runbook",
            NodeType::Tag => "",
        }
    }

    /// Filename under `templates/` in the Brain repo. None = no template (e.g. Meeting, Tag).
    pub fn template_filename(self) -> Option<&'static str> {
        match self {
            NodeType::Concept => Some("ConceptNote.md"),
            NodeType::Decision => Some("ADR.md"),
            NodeType::PostMortem => Some("PostMortem.md"),
            NodeType::Preventivo => Some("Preventivo.md"),
            NodeType::Runbook => Some("Runbook.md"),
            NodeType::Meeting | NodeType::Tag => None,
        }
    }
}

impl fmt::Display for NodeType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Node {
    pub id: u32,
    pub title: String,
    pub summary: String,
    pub node_type: NodeType,
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

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Edge {
    pub from: u32,
    pub to: u32,
}

/// Snapshot of an existing doc's structured fields, used to prefill the editor on edit.
#[derive(Clone, Debug, Default)]
pub struct EditPrefill {
    pub path: String,
    pub sha: String,
    pub node_type: Option<NodeType>,
    pub title: String,
    pub author: String,
    pub tags: Vec<String>,
    /// Full body (everything after the frontmatter), preserved verbatim.
    pub body: String,
    pub related: Vec<String>,
}

/// What the editor panel should do when open.
#[derive(Clone, Debug, Default)]
pub enum EditMode {
    #[default]
    Closed,
    New,
    Edit(EditPrefill),
}

impl EditPrefill {
    /// Parse a raw markdown file (with YAML frontmatter) from the Brain repo
    /// into the structured fields the editor expects. Body is preserved verbatim.
    pub fn from_raw(path: &str, sha: &str, raw: &str) -> Self {
        let (front, body) = split_frontmatter(raw);
        let mut out = EditPrefill {
            path: path.to_string(),
            sha: sha.to_string(),
            body: body.to_string(),
            ..Default::default()
        };

        for line in front.lines() {
            let line = line.trim_end();
            if let Some(rest) = line.strip_prefix("type:") {
                out.node_type = match rest.trim().trim_matches('"') {
                    "concept" => Some(NodeType::Concept),
                    "adr" => Some(NodeType::Decision),
                    "meeting" => Some(NodeType::Meeting),
                    "post-mortem" => Some(NodeType::PostMortem),
                    "preventivo" => Some(NodeType::Preventivo),
                    "runbook" => Some(NodeType::Runbook),
                    _ => None,
                };
            } else if let Some(rest) = line.strip_prefix("topic:") {
                out.title = rest.trim().trim_matches('"').to_string();
            } else if let Some(rest) = line.strip_prefix("author:") {
                out.author = rest.trim().trim_matches('"').to_string();
            } else if let Some(rest) = line.strip_prefix("tags:") {
                let v = rest.trim();
                if let Some(inner) = v.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
                    out.tags = inner
                        .split(',')
                        .map(|t| t.trim().trim_matches('"').trim_matches('\'').to_string())
                        .filter(|t| !t.is_empty())
                        .collect();
                }
            }
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

fn split_frontmatter(raw: &str) -> (&str, &str) {
    let Some(rest) = raw
        .strip_prefix("---\n")
        .or_else(|| raw.strip_prefix("---\r\n"))
    else {
        return ("", raw);
    };
    let Some(end) = rest.find("\n---") else {
        return ("", raw);
    };
    let front = &rest[..end];
    let after = &rest[end..];
    let body = after
        .strip_prefix("\n---\n")
        .or_else(|| after.strip_prefix("\n---\r\n"))
        .unwrap_or("");
    (front, body)
}

/// Payload sent from the editor form to create/update a Brain file.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BrainFilePayload {
    pub node_type: NodeType,
    pub title: String,
    pub author: String,
    pub tags: Vec<String>,
    pub body: String,
    /// Related file paths chosen via forced-linking.
    pub related: Vec<String>,
    /// For updates: the file path and sha.
    pub path: Option<String>,
    pub sha: Option<String>,
}
