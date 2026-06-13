//! Pure graph construction from parsed Brain markdown files.
//!
//! Input: the raw contents of markdown files fetched from the Brain repo.
//! Output: the same `(Vec<Node>, Vec<Edge>)` the UI consumes, with positions
//! from a lightweight force-directed layout.
//!
//! This crate performs no I/O. Fetching and caching live in `gitnodes-storage`.

use std::collections::{BTreeMap, HashSet};

use gitnodes_domain::{BrainConfig, Edge, EdgeKind, Node};

mod layout;
mod parse;

pub use layout::{edge_attraction_multiplier, layout};
pub use parse::{Parsed, is_included_md, parse_file};

/// A file fetched from the Brain repo, fed into `build_graph`.
pub struct RawFile {
    pub path: String,
    pub sha: String,
    pub content: String,
}

/// Build the full graph (nodes + edges) from a set of raw markdown files.
///
/// Files that don't pass `is_included_md` or don't parse as a typed Brain doc
/// are silently skipped (caller has already filtered the tree, but we re-check
/// defensively).
pub fn build_graph(files: &[RawFile], config: &BrainConfig) -> (Vec<Node>, Vec<Edge>) {
    // 1. Parse each file; skip anything without a valid Brain `type:` frontmatter.
    let mut parsed: Vec<Parsed> = files
        .iter()
        .filter(|f| is_included_md(&f.path))
        .filter_map(|f| parse_file(&f.content, &f.path, &f.sha, config))
        .collect();
    parsed.sort_by(|a, b| {
        let label_a = config
            .lookup(&a.node_type)
            .unwrap_or_else(|| config.default_spec())
            .label
            .as_str();
        let label_b = config
            .lookup(&b.node_type)
            .unwrap_or_else(|| config.default_spec())
            .label
            .as_str();
        (label_a, &a.rel).cmp(&(label_b, &b.rel))
    });

    // 2. Assign doc IDs in sorted order (stable for a given input set).
    let path_to_id: BTreeMap<String, u32> = parsed
        .iter()
        .enumerate()
        .map(|(i, p)| (p.rel.clone(), (i as u32) + 1))
        .collect();

    // 3. Resolve inter-doc links into edges (deduplicated).
    let mut edge_pairs: Vec<EdgeDraft> = Vec::new();
    let mut seen: HashSet<EdgeKey> = HashSet::new();
    for p in &parsed {
        let from = path_to_id[&p.rel];
        let dir = std::path::Path::new(&p.rel)
            .parent()
            .unwrap_or(std::path::Path::new(""));
        for link in &p.links {
            let resolved = match link.kind {
                EdgeKind::Body => resolve_link(dir, &link.target),
                EdgeKind::Frontmatter(_) | EdgeKind::Tag => Some(link.target.clone()),
            };
            if let Some(resolved) = resolved
                && let Some(&to) = path_to_id.get(&resolved)
                && from != to
            {
                let key = EdgeKey::new(from, to, &link.kind);
                if seen.insert(key) {
                    edge_pairs.push(EdgeDraft {
                        from,
                        to,
                        kind: link.kind.clone(),
                    });
                }
            }
        }
    }

    // 4. Virtual tag nodes: one per tag that connects ≥2 docs.
    let mut tag_to_docs: BTreeMap<String, Vec<u32>> = BTreeMap::new();
    for p in &parsed {
        let doc_id = path_to_id[&p.rel];
        for t in &p.tags {
            let key = t.to_lowercase();
            tag_to_docs.entry(key).or_default().push(doc_id);
        }
    }
    tag_to_docs.retain(|_, docs| docs.len() >= 2);

    let mut tag_nodes: Vec<(String, u32, Vec<u32>)> = Vec::new();
    let next_id_start = (parsed.len() as u32) + 1;
    for (i, (tag, docs)) in tag_to_docs.iter().enumerate() {
        tag_nodes.push((tag.clone(), next_id_start + (i as u32), docs.clone()));
    }
    for (_, tid, docs) in &tag_nodes {
        for d in docs {
            let key = EdgeKey::new(*d, *tid, &EdgeKind::Tag);
            if seen.insert(key) {
                edge_pairs.push(EdgeDraft {
                    from: *d,
                    to: *tid,
                    kind: EdgeKind::Tag,
                });
            }
        }
    }

    // 5. Layout. Precompute the per-kind attraction multiplier so the
    // force-directed loop in `layout.rs` operates on plain `f32`s and we
    // don't clone the `String` inside every `EdgeKind::Frontmatter`.
    let layout_edges: Vec<(u32, u32, f32)> = edge_pairs
        .iter()
        .map(|edge| {
            (
                edge.from,
                edge.to,
                layout::edge_attraction_multiplier(&edge.kind),
            )
        })
        .collect();
    let positions = layout(&parsed, &tag_nodes, &layout_edges, config);

    // 6. Materialize into public Node/Edge shapes.
    let mut nodes: Vec<Node> = Vec::with_capacity(parsed.len() + tag_nodes.len());
    for p in &parsed {
        let id = path_to_id[&p.rel];
        let (x, y) = positions[&id];
        nodes.push(Node {
            id,
            title: p.title.clone(),
            summary: p.summary.clone(),
            node_type: p.node_type.clone(),
            tags: p.tags.clone(),
            x,
            y,
            path: p.rel.clone(),
            sha: p.sha.clone(),
        });
    }
    if let Some(tag_spec) = config.synthetic_tag_spec() {
        for (tag, id, docs) in &tag_nodes {
            let (x, y) = positions[id];
            nodes.push(Node {
                id: *id,
                title: format!("#{tag}"),
                summary: format!("Tag connecting {} docs.", docs.len()),
                node_type: tag_spec.name.clone(),
                tags: vec![tag.clone()],
                x,
                y,
                path: String::new(),
                sha: String::new(),
            });
        }
    }

    let edges: Vec<Edge> = edge_pairs
        .into_iter()
        .map(|edge| Edge {
            from: edge.from,
            to: edge.to,
            kind: edge.kind,
        })
        .collect();

    (nodes, edges)
}

#[derive(Clone, Debug)]
struct EdgeDraft {
    from: u32,
    to: u32,
    kind: EdgeKind,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct EdgeKey {
    low: u32,
    high: u32,
    kind: EdgeKind,
}

impl EdgeKey {
    fn new(from: u32, to: u32, kind: &EdgeKind) -> Self {
        let (low, high) = if from < to { (from, to) } else { (to, from) };
        Self {
            low,
            high,
            kind: kind.clone(),
        }
    }
}

fn resolve_link(from_dir: &std::path::Path, link: &str) -> Option<String> {
    let joined = from_dir.join(link);
    let mut parts: Vec<&str> = Vec::new();
    for comp in joined.iter() {
        let s = comp.to_str()?;
        if s == "." {
            continue;
        } else if s == ".." {
            parts.pop();
        } else {
            parts.push(s);
        }
    }
    Some(parts.join("/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(path: &str, content: &str) -> RawFile {
        RawFile {
            path: path.to_string(),
            sha: "sha".to_string(),
            content: content.to_string(),
        }
    }

    #[test]
    fn empty_input_empty_graph() {
        let config = BrainConfig::default();
        let (n, e) = build_graph(&[], &config);
        assert!(n.is_empty());
        assert!(e.is_empty());
    }

    #[test]
    fn single_concept_no_edges() {
        let config = BrainConfig::default();
        let raw = "---\ntype: concept\ntopic: Alpha\ntags: [x]\n---\nbody\n";
        let (n, e) = build_graph(&[f("concepts/Alpha.md", raw)], &config);
        assert_eq!(n.len(), 1);
        assert!(e.is_empty());
        assert_eq!(n[0].title, "Alpha");
        assert_eq!(n[0].node_type, "concept");
    }

    #[test]
    fn two_docs_linked_produces_edge() {
        let config = BrainConfig::default();
        let a = "---\ntype: concept\ntopic: A\n---\nsee [B](../concepts/B.md)\n";
        let b = "---\ntype: concept\ntopic: B\n---\nhi\n";
        let (nodes, edges) = build_graph(&[f("concepts/A.md", a), f("concepts/B.md", b)], &config);
        assert_eq!(nodes.len(), 2);
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].kind, EdgeKind::Body);
    }

    #[test]
    fn configured_frontmatter_link_produces_typed_edge() {
        let mut config = BrainConfig::default();
        config
            .node_types
            .iter_mut()
            .find(|spec| spec.name == "concept")
            .unwrap()
            .link_fields
            .insert("trainer".to_string(), "runbook".to_string());
        let a = "---\ntype: concept\ntopic: A\ntrainer: owner\n---\n";
        let b = "---\ntype: runbook\ntopic: Owner\n---\n";
        let (_nodes, edges) =
            build_graph(&[f("concepts/A.md", a), f("runbooks/owner.md", b)], &config);

        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].kind, EdgeKind::Frontmatter("trainer".to_string()));
    }

    #[test]
    fn shared_tag_creates_virtual_tag_node() {
        let config = BrainConfig::default();
        let a = "---\ntype: concept\ntopic: A\ntags: [shared]\n---\n";
        let b = "---\ntype: concept\ntopic: B\ntags: [shared]\n---\n";
        let (nodes, edges) = build_graph(&[f("concepts/A.md", a), f("concepts/B.md", b)], &config);
        // 2 docs + 1 tag node
        assert_eq!(nodes.len(), 3);
        assert!(nodes.iter().any(|n| n.node_type == "tag"));
        // 2 doc↔tag edges
        assert_eq!(edges.len(), 2);
        assert!(edges.iter().all(|edge| edge.kind == EdgeKind::Tag));
    }

    #[test]
    fn singleton_tag_has_no_virtual_node() {
        let config = BrainConfig::default();
        let a = "---\ntype: concept\ntopic: A\ntags: [only]\n---\n";
        let (nodes, _) = build_graph(&[f("concepts/A.md", a)], &config);
        assert_eq!(nodes.len(), 1);
        assert!(!nodes.iter().any(|n| n.node_type == "tag"));
    }

    #[test]
    fn shared_tag_uses_custom_synthetic_tag_spec_name() {
        let mut config = BrainConfig::default();
        let tag_spec = config
            .node_types
            .iter_mut()
            .find(|spec| spec.name == "tag")
            .expect("default tag spec exists");
        tag_spec.name = "keyword".to_string();
        tag_spec.label = "Keyword".to_string();

        let a = "---\ntype: concept\ntopic: A\ntags: [shared]\n---\n";
        let b = "---\ntype: concept\ntopic: B\ntags: [shared]\n---\n";
        let (nodes, _) = build_graph(&[f("concepts/A.md", a), f("concepts/B.md", b)], &config);

        assert!(nodes.iter().any(|n| n.node_type == "keyword"));
    }

    #[test]
    fn readme_and_templates_are_skipped() {
        let config = BrainConfig::default();
        let readme = "---\ntype: concept\ntopic: X\n---\n";
        let tmpl = "---\ntype: concept\ntopic: Y\n---\n";
        let hidden = "---\ntype: concept\ntopic: Z\n---\n";
        let (nodes, _) = build_graph(
            &[
                f("README.md", readme),
                f("templates/Foo.md", tmpl),
                f(".hidden/Bar.md", hidden),
            ],
            &config,
        );
        assert!(nodes.is_empty());
    }

    #[test]
    fn untyped_doc_is_skipped() {
        let config = BrainConfig::default();
        let raw = "---\ntopic: NoType\n---\nhi\n";
        let (n, _) = build_graph(&[f("concepts/NoType.md", raw)], &config);
        assert!(n.is_empty());
    }
}
