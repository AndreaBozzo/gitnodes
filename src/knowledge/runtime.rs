//! Runtime graph loader: walks the Brain repo via the GitHub API using the
//! caller's session token and returns the same `(Vec<Node>, Vec<Edge>)` shape
//! that `build.rs` produces at compile time.
//!
//! This replaces the compile-time bake so the graph reflects the live repo on
//! every page load, not just what was present at `cargo build` time.

use std::collections::{BTreeMap, HashSet};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use base64::Engine;
use serde::Deserialize;

use super::types::{Edge, Node, NodeType};

const OWNER: &str = "Dritara-Digital";
const REPO: &str = "Brain";

/// In-memory TTL cache for the full graph. The repo contents are identical for
/// every authed org member, so a process-wide cache is safe — no need to key
/// by user. Kept short (30s) so edits made outside the UI still surface quickly.
const CACHE_TTL: Duration = Duration::from_secs(30);

struct CacheEntry {
    stored_at: Instant,
    nodes: Vec<Node>,
    edges: Vec<Edge>,
}

static CACHE: Mutex<Option<CacheEntry>> = Mutex::new(None);

/// Drop any cached graph. Called after a successful write (save/delete) so the
/// next `/knowledge` render picks up the change immediately instead of waiting
/// for the TTL.
pub fn invalidate() {
    if let Ok(mut guard) = CACHE.lock() {
        *guard = None;
    }
}

fn cache_get() -> Option<(Vec<Node>, Vec<Edge>)> {
    let guard = CACHE.lock().ok()?;
    let entry = guard.as_ref()?;
    if entry.stored_at.elapsed() > CACHE_TTL {
        return None;
    }
    Some((entry.nodes.clone(), entry.edges.clone()))
}

fn cache_store(nodes: &[Node], edges: &[Edge]) {
    if let Ok(mut guard) = CACHE.lock() {
        *guard = Some(CacheEntry {
            stored_at: Instant::now(),
            nodes: nodes.to_vec(),
            edges: edges.to_vec(),
        });
    }
}

#[derive(Deserialize)]
struct TreeResponse {
    tree: Vec<TreeEntry>,
}

#[derive(Deserialize)]
struct TreeEntry {
    path: String,
    #[serde(rename = "type")]
    kind: String,
    sha: String,
}

#[derive(Deserialize)]
struct ContentResponse {
    content: String,
    sha: String,
}

struct Parsed {
    rel: String,
    sha: String,
    title: String,
    summary: String,
    node_type: NodeTypeTag,
    tags: Vec<String>,
    links: Vec<String>,
}

#[derive(Copy, Clone, PartialEq, Eq)]
enum NodeTypeTag {
    Concept,
    Decision,
    Meeting,
}

impl NodeTypeTag {
    fn as_str(self) -> &'static str {
        match self {
            NodeTypeTag::Concept => "Concept",
            NodeTypeTag::Decision => "Decision",
            NodeTypeTag::Meeting => "Meeting",
        }
    }
    fn to_node_type(self) -> NodeType {
        match self {
            NodeTypeTag::Concept => NodeType::Concept,
            NodeTypeTag::Decision => NodeType::Decision,
            NodeTypeTag::Meeting => NodeType::Meeting,
        }
    }
}

pub async fn load_graph(token: &str) -> Result<(Vec<Node>, Vec<Edge>), String> {
    if let Some(hit) = cache_get() {
        return Ok(hit);
    }

    let client = reqwest::Client::builder()
        .user_agent("brain_ui")
        .build()
        .map_err(|e| format!("http client: {e}"))?;

    // 1. One recursive tree call — all paths + blob SHAs in a single request.
    let tree_url =
        format!("https://api.github.com/repos/{OWNER}/{REPO}/git/trees/main?recursive=1");
    let tree: TreeResponse = client
        .get(&tree_url)
        .bearer_auth(token)
        .send()
        .await
        .map_err(|e| format!("tree fetch: {e}"))?
        .error_for_status()
        .map_err(|e| format!("tree status: {e}"))?
        .json()
        .await
        .map_err(|e| format!("tree parse: {e}"))?;

    // 2. Filter to the set of markdown files the compile-time walker would have kept.
    let mut candidates: Vec<(String, String)> = tree
        .tree
        .into_iter()
        .filter(|e| e.kind == "blob")
        .filter(|e| is_included_md(&e.path))
        .map(|e| (e.path, e.sha))
        .collect();
    candidates.sort_by(|a, b| a.0.cmp(&b.0));

    // 3. Fetch each file's content and parse it.
    let mut parsed: Vec<Parsed> = Vec::with_capacity(candidates.len());
    for (path, _tree_sha) in &candidates {
        let url = format!("https://api.github.com/repos/{OWNER}/{REPO}/contents/{path}?ref=main");
        let resp = match client.get(&url).bearer_auth(token).send().await {
            Ok(r) => r,
            Err(_) => continue,
        };
        if !resp.status().is_success() {
            continue;
        }
        let body: ContentResponse = match resp.json().await {
            Ok(b) => b,
            Err(_) => continue,
        };
        let cleaned: String = body
            .content
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect();
        let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(cleaned) else {
            continue;
        };
        let Ok(text) = String::from_utf8(bytes) else {
            continue;
        };
        if let Some(p) = parse_file(&text, path, &body.sha) {
            parsed.push(p);
        }
    }
    parsed.sort_by(|a, b| (a.node_type.as_str(), &a.rel).cmp(&(b.node_type.as_str(), &b.rel)));

    // 4. Build the graph using the same rules as build.rs.
    let path_to_id: BTreeMap<String, u32> = parsed
        .iter()
        .enumerate()
        .map(|(i, p)| (p.rel.clone(), (i as u32) + 1))
        .collect();

    let mut edges: Vec<(u32, u32)> = Vec::new();
    let mut seen: HashSet<(u32, u32)> = HashSet::new();
    for p in &parsed {
        let from = path_to_id[&p.rel];
        let dir = std::path::Path::new(&p.rel)
            .parent()
            .unwrap_or(std::path::Path::new(""));
        for link in &p.links {
            if let Some(resolved) = resolve_link(dir, link)
                && let Some(&to) = path_to_id.get(&resolved)
                && from != to
            {
                let key = if from < to { (from, to) } else { (to, from) };
                if seen.insert(key) {
                    edges.push((from, to));
                }
            }
        }
    }

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
            let key = if *d < *tid { (*d, *tid) } else { (*tid, *d) };
            if seen.insert(key) {
                edges.push((*d, *tid));
            }
        }
    }

    let positions = layout(&parsed, &tag_nodes, &edges);

    let mut nodes: Vec<Node> = Vec::with_capacity(parsed.len() + tag_nodes.len());
    for p in &parsed {
        let id = path_to_id[&p.rel];
        let (x, y) = positions[&id];
        nodes.push(Node {
            id,
            title: p.title.clone(),
            summary: p.summary.clone(),
            node_type: p.node_type.to_node_type(),
            tags: p.tags.clone(),
            x,
            y,
            path: p.rel.clone(),
            sha: p.sha.clone(),
        });
    }
    for (tag, id, docs) in &tag_nodes {
        let (x, y) = positions[id];
        nodes.push(Node {
            id: *id,
            title: format!("#{tag}"),
            summary: format!("Tag connecting {} docs.", docs.len()),
            node_type: NodeType::Tag,
            tags: vec![tag.clone()],
            x,
            y,
            path: String::new(),
            sha: String::new(),
        });
    }

    let edges: Vec<Edge> = edges
        .into_iter()
        .map(|(from, to)| Edge { from, to })
        .collect();
    cache_store(&nodes, &edges);
    Ok((nodes, edges))
}

fn is_included_md(path: &str) -> bool {
    if !path.ends_with(".md") {
        return false;
    }
    for seg in path.split('/') {
        if seg.starts_with('.') || seg == "templates" {
            return false;
        }
    }
    let file = path.rsplit('/').next().unwrap_or(path);
    if file.eq_ignore_ascii_case("README.md") || file.eq_ignore_ascii_case("AGENTS.md") {
        return false;
    }
    true
}

fn parse_file(raw: &str, rel: &str, sha: &str) -> Option<Parsed> {
    let (front, body) = split_frontmatter(raw)?;

    let mut node_type: Option<NodeTypeTag> = None;
    let mut topic = String::new();
    let mut tags: Vec<String> = Vec::new();

    for line in front.lines() {
        let line = line.trim_end();
        if let Some(rest) = line.strip_prefix("type:") {
            let v = rest.trim().trim_matches('"');
            node_type = match v {
                "concept" => Some(NodeTypeTag::Concept),
                "adr" => Some(NodeTypeTag::Decision),
                "meeting" => Some(NodeTypeTag::Meeting),
                _ => None,
            };
        } else if let Some(rest) = line.strip_prefix("topic:") {
            topic = rest.trim().trim_matches('"').to_string();
        } else if let Some(rest) = line.strip_prefix("tags:") {
            let v = rest.trim();
            if let Some(inner) = v.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
                tags = inner
                    .split(',')
                    .map(|t| t.trim().trim_matches('"').trim_matches('\'').to_string())
                    .filter(|t| !t.is_empty())
                    .collect();
            }
        }
    }

    let node_type = node_type?;

    let title = if !topic.is_empty() {
        title_case(&topic)
    } else {
        first_heading(body).unwrap_or_else(|| {
            std::path::Path::new(rel)
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| rel.to_string())
        })
    };

    let summary = extract_summary(body);
    let links = extract_links(body);

    Some(Parsed {
        rel: rel.to_string(),
        sha: sha.to_string(),
        title,
        summary,
        node_type,
        tags,
        links,
    })
}

fn split_frontmatter(raw: &str) -> Option<(&str, &str)> {
    let rest = raw
        .strip_prefix("---\n")
        .or_else(|| raw.strip_prefix("---\r\n"))?;
    let end = rest.find("\n---")?;
    let front = &rest[..end];
    let after = &rest[end..];
    let body = after
        .strip_prefix("\n---\n")
        .or_else(|| after.strip_prefix("\n---\r\n"))
        .unwrap_or("");
    Some((front, body))
}

fn first_heading(body: &str) -> Option<String> {
    for line in body.lines() {
        let l = line.trim_start();
        if let Some(rest) = l.strip_prefix("# ") {
            return Some(clean_heading(rest));
        }
    }
    None
}

fn clean_heading(h: &str) -> String {
    let mut s = h.trim().to_string();
    for prefix in ["Concept: ", "ADR: ", "Meeting: ", "ADR 001: ", "ADR 002: "] {
        if s.starts_with(prefix) {
            s = s.trim_start_matches(prefix).to_string();
        }
    }
    if s.len() > 60 {
        s.truncate(57);
        s.push('…');
    }
    s
}

fn title_case(s: &str) -> String {
    let s = s.replace(['-', '_'], " ");
    s.split_whitespace()
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                Some(first) => first.to_uppercase().collect::<String>() + c.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn extract_summary(body: &str) -> String {
    let mut in_summary = false;
    let mut buf = String::new();
    for line in body.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("## ") {
            if in_summary {
                break;
            }
            if trimmed.to_lowercase().contains("summary")
                || trimmed.to_lowercase().contains("riepilogo")
            {
                in_summary = true;
                continue;
            }
        }
        if in_summary {
            if trimmed.is_empty() {
                if !buf.is_empty() {
                    break;
                }
                continue;
            }
            if trimmed.starts_with('>') || trimmed.starts_with('*') && trimmed.ends_with('*') {
                continue;
            }
            if !buf.is_empty() {
                buf.push(' ');
            }
            buf.push_str(trimmed);
        }
    }
    if buf.is_empty() {
        for line in body.lines() {
            let t = line.trim();
            if t.is_empty() || t.starts_with('#') || t.starts_with('>') {
                continue;
            }
            buf = t.to_string();
            break;
        }
    }
    let cleaned = strip_md(&buf);
    truncate(&cleaned, 180)
}

fn strip_md(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        if c == '['
            && let Some(close) = s[i..].find("](")
        {
            let text_end = i + close;
            out.push_str(&s[i + 1..text_end]);
            if let Some(paren) = s[text_end..].find(')') {
                let skip_to = text_end + paren + 1;
                while let Some(&(j, _)) = chars.peek() {
                    if j >= skip_to {
                        break;
                    }
                    chars.next();
                }
                continue;
            }
        }
        if c == '*' || c == '_' || c == '`' {
            continue;
        }
        out.push(c);
    }
    out
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

fn extract_links(body: &str) -> Vec<String> {
    let mut out = Vec::new();
    let bytes = body.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b']'
            && i + 1 < bytes.len()
            && bytes[i + 1] == b'('
            && let Some(end) = body[i + 2..].find(')')
        {
            let url = &body[i + 2..i + 2 + end];
            if url.ends_with(".md") && !url.starts_with("http") {
                let clean = url.split('#').next().unwrap_or(url).to_string();
                out.push(clean);
            }
            i = i + 2 + end + 1;
            continue;
        }
        i += 1;
    }
    out
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

fn layout(
    parsed: &[Parsed],
    tag_nodes: &[(String, u32, Vec<u32>)],
    edges: &[(u32, u32)],
) -> BTreeMap<u32, (f32, f32)> {
    let mut pos: BTreeMap<u32, (f32, f32)> = BTreeMap::new();

    let mut by_type: BTreeMap<&str, Vec<u32>> = BTreeMap::new();
    for (i, p) in parsed.iter().enumerate() {
        by_type
            .entry(p.node_type.as_str())
            .or_default()
            .push((i as u32) + 1);
    }
    let clusters: &[(&str, f32, f32, f32)] = &[
        ("Concept", 22.0, 38.0, 14.0),
        ("Decision", 52.0, 20.0, 10.0),
        ("Meeting", 80.0, 74.0, 9.0),
    ];
    for (name, cx, cy, r) in clusters {
        if let Some(ids) = by_type.get(name) {
            let n = ids.len().max(1) as f32;
            let radius = if ids.len() <= 1 { 0.0 } else { *r };
            for (k, id) in ids.iter().enumerate() {
                let theta = (k as f32) / n * std::f32::consts::TAU + 0.7;
                pos.insert(*id, (cx + radius * theta.cos(), cy + radius * theta.sin()));
            }
        }
    }

    let center = (55.0_f32, 55.0_f32);
    for (tag, id, docs) in tag_nodes {
        let mut sx = 0.0_f32;
        let mut sy = 0.0_f32;
        let mut n = 0.0_f32;
        for d in docs {
            if let Some(&(x, y)) = pos.get(d) {
                sx += x;
                sy += y;
                n += 1.0;
            }
        }
        let (mut x, mut y) = if n > 0.0 { (sx / n, sy / n) } else { center };
        x = x * 0.55 + center.0 * 0.45;
        y = y * 0.55 + center.1 * 0.45;
        let h = small_hash(tag);
        x += ((h % 97) as f32 / 97.0 - 0.5) * 10.0;
        y += (((h / 97) % 97) as f32 / 97.0 - 0.5) * 10.0;
        pos.insert(*id, (x, y));
    }

    let ids: Vec<u32> = pos.keys().copied().collect();
    let min_dist = 6.0_f32;
    let ideal_edge = 10.0_f32;
    for _ in 0..120 {
        let mut delta: BTreeMap<u32, (f32, f32)> = ids.iter().map(|i| (*i, (0.0, 0.0))).collect();
        for i in 0..ids.len() {
            for j in (i + 1)..ids.len() {
                let a = ids[i];
                let b = ids[j];
                let (ax, ay) = pos[&a];
                let (bx, by) = pos[&b];
                let dx = ax - bx;
                let dy = ay - by;
                let d2 = (dx * dx + dy * dy).max(0.25);
                let d = d2.sqrt();
                if d < min_dist * 2.5 {
                    let force = (min_dist * 2.5 - d) * 0.18;
                    let ux = dx / d;
                    let uy = dy / d;
                    let e = delta.get_mut(&a).unwrap();
                    e.0 += ux * force;
                    e.1 += uy * force;
                    let e = delta.get_mut(&b).unwrap();
                    e.0 -= ux * force;
                    e.1 -= uy * force;
                }
            }
        }
        for (a, b) in edges {
            let (ax, ay) = pos[a];
            let (bx, by) = pos[b];
            let dx = bx - ax;
            let dy = by - ay;
            let d = (dx * dx + dy * dy).sqrt().max(0.5);
            let diff = (d - ideal_edge) * 0.05;
            let ux = dx / d;
            let uy = dy / d;
            let e = delta.get_mut(a).unwrap();
            e.0 += ux * diff;
            e.1 += uy * diff;
            let e = delta.get_mut(b).unwrap();
            e.0 -= ux * diff;
            e.1 -= uy * diff;
        }
        for id in &ids {
            let d = delta[id];
            let p = pos.get_mut(id).unwrap();
            p.0 = (p.0 + d.0).clamp(6.0, 94.0);
            p.1 = (p.1 + d.1).clamp(8.0, 92.0);
        }
    }

    pos
}

fn small_hash(s: &str) -> u32 {
    let mut h: u32 = 2166136261;
    for b in s.bytes() {
        h ^= b as u32;
        h = h.wrapping_mul(16777619);
    }
    h
}
