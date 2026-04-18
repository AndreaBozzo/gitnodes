use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
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

}

impl fmt::Display for NodeType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

#[derive(Clone, Debug)]
pub struct Node {
    pub id: u32,
    pub title: &'static str,
    pub summary: &'static str,
    pub node_type: NodeType,
    pub tags: &'static [&'static str],
    pub x: f32,
    pub y: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct Edge {
    pub from: u32,
    pub to: u32,
}
