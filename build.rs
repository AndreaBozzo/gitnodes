use std::collections::{BTreeMap, HashSet};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

fn main() {
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let brain = env::var("BRAIN_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| manifest.join("../Brain"));
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let out = out_dir.join("brain_data.rs");

    println!("cargo:rerun-if-env-changed=BRAIN_DIR");
    println!("cargo:rerun-if-changed=build.rs");

    if !brain.exists() {
        fs::write(&out, stub()).unwrap();
        println!(
            "cargo:warning=Brain dir not found at {:?}; generated empty graph",
            brain
        );
        return;
    }

    println!("cargo:rerun-if-changed={}", brain.display());

    let mut rel_files: Vec<String> = Vec::new();
    collect_md(&brain, &brain, &mut rel_files);
    rel_files.sort();

    let mut parsed: Vec<Parsed> = rel_files
        .iter()
        .filter_map(|rel| parse_file(&brain.join(rel), rel))
        .filter(|p| matches!(p.node_type.as_str(), "Concept" | "Decision" | "Meeting"))
        .collect();
    parsed.sort_by(|a, b| (&a.node_type, &a.rel).cmp(&(&b.node_type, &b.rel)));

    let path_to_id: BTreeMap<String, u32> = parsed
        .iter()
        .enumerate()
        .map(|(i, p)| (p.rel.clone(), (i as u32) + 1))
        .collect();

    // --- Doc-to-doc edges from "Related / See also" links ---
    let mut edges: Vec<(u32, u32)> = Vec::new();
    let mut seen: HashSet<(u32, u32)> = HashSet::new();
    for p in &parsed {
        let from = path_to_id[&p.rel];
        let dir = Path::new(&p.rel).parent().unwrap_or(Path::new(""));
        for link in &p.links {
            if let Some(resolved) = resolve_link(dir, link) {
                if let Some(&to) = path_to_id.get(&resolved) {
                    if from != to {
                        let key = if from < to { (from, to) } else { (to, from) };
                        if seen.insert(key) {
                            edges.push((from, to));
                        }
                    }
                }
            }
        }
    }

    // --- Tag nodes (one per unique tag, connected to every doc that carries it) ---
    let mut tag_to_docs: BTreeMap<String, Vec<u32>> = BTreeMap::new();
    for p in &parsed {
        let doc_id = path_to_id[&p.rel];
        for t in &p.tags {
            let key = t.to_lowercase();
            tag_to_docs.entry(key).or_default().push(doc_id);
        }
    }
    // Drop orphan tags that only touch one doc — keeps the graph meaningful.
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

    // --- Layout: cluster centroids + tag attraction + relaxation ---
    let positions = layout(&parsed, &tag_nodes, &edges);

    // --- Emit ---
    let mut s = String::new();
    s.push_str("use super::types::{Edge, Node, NodeType};\n\n");
    s.push_str("pub fn nodes() -> Vec<Node> {\n    vec![\n");
    for p in &parsed {
        let id = path_to_id[&p.rel];
        let (x, y) = positions[&id];
        let tags_lit = p
            .tags
            .iter()
            .map(|t| format!("{:?}", t))
            .collect::<Vec<_>>()
            .join(", ");
        s.push_str(&format!(
            "        Node {{ id: {id}, title: {title:?}, summary: {summary:?}, node_type: NodeType::{nt}, tags: &[{tags}], x: {x:.3}, y: {y:.3} }},\n",
            id = id,
            title = p.title,
            summary = p.summary,
            nt = p.node_type,
            tags = tags_lit,
            x = x,
            y = y,
        ));
    }
    for (tag, id, docs) in &tag_nodes {
        let (x, y) = positions[id];
        let title = format!("#{tag}");
        let summary = format!("Tag connecting {} docs.", docs.len());
        s.push_str(&format!(
            "        Node {{ id: {id}, title: {title:?}, summary: {summary:?}, node_type: NodeType::Tag, tags: &[{tag:?}], x: {x:.3}, y: {y:.3} }},\n",
            id = id,
            title = title,
            summary = summary,
            tag = tag,
            x = x,
            y = y,
        ));
    }
    s.push_str("    ]\n}\n\n");
    s.push_str("pub fn edges() -> Vec<Edge> {\n    vec![\n");
    for (from, to) in &edges {
        s.push_str(&format!("        Edge {{ from: {from}, to: {to} }},\n"));
    }
    s.push_str("    ]\n}\n");

    fs::write(&out, s).unwrap();
}

fn stub() -> String {
    "use super::types::{Edge, Node};\n\npub fn nodes() -> Vec<Node> { Vec::new() }\npub fn edges() -> Vec<Edge> { Vec::new() }\n".into()
}

struct Parsed {
    rel: String,
    title: String,
    summary: String,
    node_type: String,
    tags: Vec<String>,
    links: Vec<String>,
}

fn collect_md(base: &Path, dir: &Path, out: &mut Vec<String>) {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('.') {
            continue;
        }
        if path.is_dir() {
            if name == "templates" || name == ".git" || name == ".github" {
                continue;
            }
            collect_md(base, &path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("md") {
            if name.eq_ignore_ascii_case("README.md") || name.eq_ignore_ascii_case("AGENTS.md") {
                continue;
            }
            if let Ok(rel) = path.strip_prefix(base) {
                out.push(rel.to_string_lossy().replace('\\', "/"));
            }
        }
    }
}

fn parse_file(path: &Path, rel: &str) -> Option<Parsed> {
    let raw = fs::read_to_string(path).ok()?;
    let (front, body) = split_frontmatter(&raw)?;

    let mut node_type = String::new();
    let mut topic = String::new();
    let mut tags: Vec<String> = Vec::new();

    for line in front.lines() {
        let line = line.trim_end();
        if let Some(rest) = line.strip_prefix("type:") {
            let v = rest.trim().trim_matches('"').to_string();
            node_type = match v.as_str() {
                "concept" => "Concept".into(),
                "adr" => "Decision".into(),
                "meeting" => "Meeting".into(),
                _ => String::new(),
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

    let title = if !topic.is_empty() {
        title_case(&topic)
    } else {
        first_heading(body).unwrap_or_else(|| {
            Path::new(rel)
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| rel.to_string())
        })
    };

    let summary = extract_summary(body);
    let links = extract_links(body);

    Some(Parsed {
        rel: rel.to_string(),
        title,
        summary,
        node_type,
        tags,
        links,
    })
}

fn split_frontmatter(raw: &str) -> Option<(&str, &str)> {
    let rest = raw.strip_prefix("---\n").or_else(|| raw.strip_prefix("---\r\n"))?;
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
        s.push_str("…");
    }
    s
}

fn title_case(s: &str) -> String {
    let s = s.replace('-', " ").replace('_', " ");
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
        if c == '[' {
            if let Some(close) = s[i..].find("](") {
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
        if bytes[i] == b']' && i + 1 < bytes.len() && bytes[i + 1] == b'(' {
            if let Some(end) = body[i + 2..].find(')') {
                let url = &body[i + 2..i + 2 + end];
                if url.ends_with(".md") && !url.starts_with("http") {
                    let clean = url.split('#').next().unwrap_or(url).to_string();
                    out.push(clean);
                }
                i = i + 2 + end + 1;
                continue;
            }
        }
        i += 1;
    }
    out
}

fn resolve_link(from_dir: &Path, link: &str) -> Option<String> {
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

    // Seed doc positions in type clusters.
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

    // Seed tag positions as weighted centroid of their connected docs, pulled toward (55, 55).
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
        let (mut x, mut y) = if n > 0.0 {
            (sx / n, sy / n)
        } else {
            center
        };
        // Bias toward canvas center, then jitter deterministically by tag hash.
        x = x * 0.55 + center.0 * 0.45;
        y = y * 0.55 + center.1 * 0.45;
        let h = small_hash(tag);
        x += ((h % 97) as f32 / 97.0 - 0.5) * 10.0;
        y += (((h / 97) % 97) as f32 / 97.0 - 0.5) * 10.0;
        pos.insert(*id, (x, y));
    }

    // Force relax: edges attract, all-pairs repel. Deterministic.
    let ids: Vec<u32> = pos.keys().copied().collect();
    let min_dist = 6.0_f32;
    let ideal_edge = 10.0_f32;
    for _ in 0..120 {
        let mut delta: BTreeMap<u32, (f32, f32)> = ids.iter().map(|i| (*i, (0.0, 0.0))).collect();
        // Repulsion
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
        // Attraction along edges
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
        // Apply + clamp to canvas
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
