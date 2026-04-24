//! Force-directed layout over clustered initial positions. Pure; deterministic.

use std::collections::BTreeMap;

use crate::parse::Parsed;

pub fn layout(
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
        ("concept", 22.0, 38.0, 14.0),
        ("adr", 52.0, 20.0, 10.0),
        ("meeting", 80.0, 74.0, 9.0),
        ("post-mortem", 78.0, 28.0, 9.0),
        ("preventivo", 25.0, 78.0, 9.0),
        ("runbook", 50.0, 80.0, 9.0),
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

    let mut unseen: Vec<u32> = Vec::new();
    for (name, ids) in &by_type {
        if !clusters.iter().any(|c| c.0 == *name) {
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
        for (a, b) in edges {
            let (ax, ay) = pos[a];
            let (bx, by) = pos[b];
            let dx = bx - ax;
            let dy = by - ay;
            let d = (dx * dx + dy * dy).sqrt().max(0.5);
            let diff = (d - ideal_edge) * 0.05;
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

fn small_hash(s: &str) -> u32 {
    let mut h: u32 = 2166136261;
    for b in s.bytes() {
        h ^= b as u32;
        h = h.wrapping_mul(16777619);
    }
    h
}
