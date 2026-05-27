//! Force-directed layout over clustered initial positions. Pure; deterministic.

use std::collections::BTreeMap;

use crate::parse::Parsed;
use brain_domain::{BrainConfig, EdgeKind};

/// Per-kind attraction multiplier applied to edge spring forces. Body links
/// shape distances at full strength because they encode the narrative
/// neighborhood; frontmatter edges and tag membership are progressively
/// damped so structural metadata and tag bundles don't dissolve the
/// type-based clusters that intra-cluster gravity is trying to hold.
///
/// Exposed so the caller can precompute the multiplier once per edge and
/// pass it as a plain `f32` into `layout`, avoiding a `String` clone per
/// `EdgeKind::Frontmatter` on every rebuild.
pub fn edge_attraction_multiplier(kind: &EdgeKind) -> f32 {
    match kind {
        EdgeKind::Body => 1.0,
        EdgeKind::Frontmatter(_) => 0.5,
        EdgeKind::Tag => 0.25,
    }
}

pub fn layout(
    parsed: &[Parsed],
    tag_nodes: &[(String, u32, Vec<u32>)],
    edges: &[(u32, u32, f32)],
    config: &BrainConfig,
) -> BTreeMap<u32, (f32, f32)> {
    let mut pos: BTreeMap<u32, (f32, f32)> = BTreeMap::new();
    let mut cluster_centers: BTreeMap<u32, (f32, f32)> = BTreeMap::new();

    let mut by_type: BTreeMap<&str, Vec<u32>> = BTreeMap::new();
    for (i, p) in parsed.iter().enumerate() {
        by_type
            .entry(p.node_type.as_str())
            .or_default()
            .push((i as u32) + 1);
    }
    let configured_types: Vec<&str> = config
        .node_types
        .iter()
        .filter(|spec| spec.creatable || !spec.directory.is_empty())
        .map(|spec| spec.name.as_str())
        .collect();
    let cluster_count = configured_types.len().max(1);
    let graph_center = (50.0_f32, 50.0_f32);
    // Cluster ring radius pushes cluster centers toward the canvas bounds so
    // the graph has visual breathing room around each cluster instead of
    // piling them in the center 40% of the canvas. The post-relaxation
    // clamp is asymmetric — `x ∈ [6, 94]` (88-unit usable width) and
    // `y ∈ [8, 92]` (84-unit usable height) — so the binding dimension is
    // the vertical one. A ring of 40 puts cluster centers near the edge
    // but still leaves room for the in-cluster spread (`cluster_inner_radius`
    // caps at 14) plus a small border before the clamp kicks in on the
    // tighter axis.
    let cluster_ring_radius = if cluster_count <= 1 { 0.0 } else { 40.0 };
    for (index, name) in configured_types.iter().enumerate() {
        if let Some(ids) = by_type.get(name) {
            let theta = (index as f32) / (cluster_count as f32) * std::f32::consts::TAU - 1.35;
            let cx = graph_center.0 + cluster_ring_radius * theta.cos();
            let cy = graph_center.1 + cluster_ring_radius * theta.sin();
            let n = ids.len().max(1) as f32;
            let radius = if ids.len() <= 1 {
                0.0
            } else {
                cluster_inner_radius(ids.len())
            };
            for (k, id) in ids.iter().enumerate() {
                let theta = (k as f32) / n * std::f32::consts::TAU + 0.7;
                pos.insert(*id, (cx + radius * theta.cos(), cy + radius * theta.sin()));
                cluster_centers.insert(*id, (cx, cy));
            }
        }
    }

    let mut unseen: Vec<u32> = Vec::new();
    for (name, ids) in &by_type {
        if !configured_types.iter().any(|configured| configured == name) {
            unseen.extend(ids.iter().copied());
        }
    }
    if !unseen.is_empty() {
        let n = unseen.len().max(1) as f32;
        let cx = 50.0;
        let cy = 50.0;
        let r = 12.0;
        let radius = if unseen.len() <= 1 { 0.0 } else { r };
        for (k, id) in unseen.iter().enumerate() {
            let theta = (k as f32) / n * std::f32::consts::TAU;
            pos.insert(*id, (cx + radius * theta.cos(), cy + radius * theta.sin()));
            cluster_centers.insert(*id, (cx, cy));
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
    // Ideal edge length: target distance the spring pulls connected pairs to.
    // Shorter values compact the graph; on dense brains (Pokémon mock with
    // 213 typed edges) values below ~12 stack hubs on top of their neighbors.
    // 13 leaves the body link spine readable without pushing leaf nodes
    // against the canvas clamp.
    let ideal_edge = 13.0_f32;
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
                    if let Some(e) = delta.get_mut(&a) {
                        e.0 += ux * force;
                        e.1 += uy * force;
                    }
                    if let Some(e) = delta.get_mut(&b) {
                        e.0 -= ux * force;
                        e.1 -= uy * force;
                    }
                }
            }
        }
        for (a, b, multiplier) in edges {
            let (ax, ay) = pos[a];
            let (bx, by) = pos[b];
            let dx = bx - ax;
            let dy = by - ay;
            let d = (dx * dx + dy * dy).sqrt().max(0.5);
            let diff = (d - ideal_edge) * 0.05 * multiplier;
            let ux = dx / d;
            let uy = dy / d;
            if let Some(e) = delta.get_mut(a) {
                e.0 += ux * diff;
                e.1 += uy * diff;
            }
            if let Some(e) = delta.get_mut(b) {
                e.0 -= ux * diff;
                e.1 -= uy * diff;
            }
        }
        // Intra-cluster gravity: pulls each node toward its type's cluster
        // centroid. Tuned to ~match the edge-attraction coefficient (0.05) so
        // typed-edge attraction across cluster boundaries doesn't dissolve the
        // type-based grouping. Lower values (e.g. 0.025) let cross-cluster
        // edges drag nodes out of their cluster on dense graphs like the
        // Pokémon mock; higher values risk overlapping cluster cores when
        // many types share the ring.
        for id in &ids {
            if let Some(&(cx, cy)) = cluster_centers.get(id) {
                let (x, y) = pos[id];
                if let Some(e) = delta.get_mut(id) {
                    e.0 += (cx - x) * 0.06;
                    e.1 += (cy - y) * 0.06;
                }
            }
        }
        for id in &ids {
            let d = delta[id];
            if let Some(p) = pos.get_mut(id) {
                p.0 = (p.0 + d.0).clamp(6.0, 94.0);
                p.1 = (p.1 + d.1).clamp(8.0, 92.0);
            }
        }
    }

    pos
}

fn cluster_inner_radius(count: usize) -> f32 {
    (4.5 + (count as f32).sqrt() * 1.65).clamp(6.0, 14.0)
}

fn small_hash(s: &str) -> u32 {
    let mut h: u32 = 2166136261;
    for b in s.bytes() {
        h ^= b as u32;
        h = h.wrapping_mul(16777619);
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edge_attraction_hierarchy_holds() {
        // Layout invariant: body links shape the strongest distance signal
        // (they encode the narrative neighborhood); frontmatter edges are
        // dampened so structural metadata doesn't drag nodes across type
        // clusters; tag membership is the weakest because virtual tag hubs
        // would otherwise pull every doc sharing a tag toward the same
        // spot. If this ordering is ever inverted, type clusters dissolve
        // under typed-edge attraction on dense brains like the Pokémon mock.
        let body = edge_attraction_multiplier(&EdgeKind::Body);
        let fm = edge_attraction_multiplier(&EdgeKind::Frontmatter("trainer".to_string()));
        let tag = edge_attraction_multiplier(&EdgeKind::Tag);
        assert!(
            body > fm,
            "body multiplier {body} must exceed frontmatter {fm}"
        );
        assert!(
            fm > tag,
            "frontmatter multiplier {fm} must exceed tag {tag}"
        );
        assert!(
            tag > 0.0,
            "tag multiplier {tag} must stay positive so tag hubs still cluster their docs"
        );
    }
}
