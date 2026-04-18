use std::fmt;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum NodeType {
    Concept,
    Decision,
    Meeting,
    Tag,
}

impl NodeType {
    pub const ALL: [NodeType; 4] = [
        NodeType::Concept,
        NodeType::Decision,
        NodeType::Meeting,
        NodeType::Tag,
    ];

    pub fn label(self) -> &'static str {
        match self {
            NodeType::Concept => "Concept",
            NodeType::Decision => "ADR",
            NodeType::Meeting => "Meeting",
            NodeType::Tag => "Tag",
        }
    }

    pub fn accent(self) -> &'static str {
        match self {
            NodeType::Concept => "#2dd4bf",
            NodeType::Decision => "#f59e0b",
            NodeType::Meeting => "#a78bfa",
            NodeType::Tag => "#64748b",
        }
    }

    /// Returns the Brain repo directory for this type.
    pub fn directory(self) -> &'static str {
        match self {
            NodeType::Concept => "concepts",
            NodeType::Decision => "adrs",
            NodeType::Meeting => "meetings",
            NodeType::Tag => "",
        }
    }

    /// Returns the Brain template frontmatter type value.
    pub fn frontmatter_type(self) -> &'static str {
        match self {
            NodeType::Concept => "concept",
            NodeType::Decision => "adr",
            NodeType::Meeting => "meeting",
            NodeType::Tag => "",
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
