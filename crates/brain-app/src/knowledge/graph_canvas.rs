use std::collections::{HashMap, HashSet};

use leptos::prelude::*;

use super::types::{Edge, Node};

#[component]
pub fn GraphCanvas(
    nodes: StoredValue<Vec<Node>>,
    edges: StoredValue<Vec<Edge>>,
    visible_ids: Signal<HashSet<u32>>,
    hovered: RwSignal<Option<u32>>,
    selected: RwSignal<Option<u32>>,
    config: brain_domain::BrainConfig,
) -> impl IntoView {
    let adjacency: StoredValue<HashMap<u32, HashSet<u32>>> = StoredValue::new({
        let mut m: HashMap<u32, HashSet<u32>> = HashMap::new();
        edges.with_value(|es| {
            for e in es {
                m.entry(e.from).or_default().insert(e.to);
                m.entry(e.to).or_default().insert(e.from);
            }
        });
        m
    });

    let focus = Memo::new(move |_| selected.get().or_else(|| hovered.get()));

    let positions: StoredValue<HashMap<u32, (f32, f32)>> =
        StoredValue::new(nodes.with_value(|ns| ns.iter().map(|n| (n.id, (n.x, n.y))).collect()));

    let view_box = Memo::new(move |_| match selected.get() {
        Some(id) => positions
            .with_value(|p| p.get(&id).copied())
            .map(|(x, y)| format!("{:.3} {:.3} 100 100", x - 50.0, y - 50.0))
            .unwrap_or_else(|| "0 0 100 100".to_string()),
        None => "0 0 100 100".to_string(),
    });

    let degrees: StoredValue<HashMap<u32, usize>> =
        StoredValue::new(adjacency.with_value(|a| a.iter().map(|(k, v)| (*k, v.len())).collect()));

    let edges_view = move || {
        let vis = visible_ids.get();
        let f = focus.get();
        edges.with_value(|es| {
            positions.with_value(|pos| {
                es.iter()
                    .filter(|e| vis.contains(&e.from) && vis.contains(&e.to))
                    .map(|e| {
                        let (x1, y1) = pos[&e.from];
                        let (x2, y2) = pos[&e.to];
                        let mx = (x1 + x2) / 2.0;
                        let my = (y1 + y2) / 2.0;
                        let dx = x2 - x1;
                        let dy = y2 - y1;
                        let len = (dx * dx + dy * dy).sqrt().max(0.001);
                        let ox = -dy / len;
                        let oy = dx / len;
                        let bow = (len * 0.12).min(6.0);
                        let cx = mx + ox * bow;
                        let cy = my + oy * bow;
                        let touches = match f {
                            Some(id) => id == e.from || id == e.to,
                            None => false,
                        };
                        let opacity = if f.is_none() {
                            0.50
                        } else if touches {
                            0.95
                        } else {
                            0.05
                        };
                        let stroke = if touches { "#5eead4" } else { "#334155" };
                        let width = if touches { "0.35" } else { "0.18" };
                        view! {
                            <path
                                d=format!("M{:.3},{:.3} Q{:.3},{:.3} {:.3},{:.3}", x1, y1, cx, cy, x2, y2)
                                fill="none"
                                stroke=stroke
                                stroke-width=width
                                stroke-linecap="round"
                                stroke-opacity=opacity
                            />
                        }
                    })
                    .collect_view()
            })
        })
    };

    let config_for_nodes = config.clone();
    let nodes_view = move || {
        let vis = visible_ids.get();
        nodes.with_value(|ns| {
            ns.iter()
                .filter(|n| vis.contains(&n.id))
                .map(|n| {
                    let id = n.id;
                    let spec = config_for_nodes.lookup(&n.node_type).unwrap_or_else(|| config_for_nodes.default_spec());
                    let accent = spec.accent_var.clone();
                    let is_tag = config_for_nodes.by_directory("").map(|s| s.name.as_str()) == Some(n.node_type.as_str()) || n.node_type == "tag";
                    let title = n.title.clone();
                    let x = n.x;
                    let y = n.y;
                    let deg = degrees.with_value(|d| *d.get(&id).unwrap_or(&0));
                    let base_r = if is_tag {
                        0.9_f32 + (deg as f32).min(4.0) * 0.12
                    } else {
                        1.5_f32 + (deg as f32).min(6.0) * 0.18
                    };

                    let bright = Memo::new(move |_| match focus.get() {
                        None => true,
                        Some(f) if f == id => true,
                        Some(f) => adjacency
                            .with_value(|a| a.get(&f).map(|s| s.contains(&id)).unwrap_or(false)),
                    });
                    let is_selected = Memo::new(move |_| selected.get() == Some(id));
                    let is_hovered = Memo::new(move |_| hovered.get() == Some(id));

                    let label_size = if is_tag { 1.1 } else { 1.55 };
                    let label_offset = base_r + 2.4;
                    let label_fill = if is_tag { "#cbd5e1" } else { "#e2e8f0" };

                    view! {
                        <g
                            class="cursor-pointer"
                            style=move || format!("opacity:{}; transition: opacity 200ms ease;", if bright.get() { 1.0 } else { 0.15 })
                            on:mouseenter=move |_| hovered.set(Some(id))
                            on:mouseleave=move |_| hovered.update(|h| if *h == Some(id) { *h = None; })
                            on:click=move |_| selected.update(|s| { *s = if *s == Some(id) { None } else { Some(id) }; })
                        >
                            <circle
                                cx=format!("{:.3}", x)
                                cy=format!("{:.3}", y)
                                r=move || {
                                    let bump = if is_selected.get() { 0.8 }
                                        else if is_hovered.get() { 0.5 }
                                        else { 0.0 };
                                    format!("{:.3}", base_r + bump)
                                }
                                fill=accent.clone()
                                fill-opacity=if is_tag { "0.55" } else { "0.92" }
                                stroke={
                                    let accent = accent.clone();
                                    move || if is_selected.get() { "#f8fafc".to_string() } else { accent.clone() }
                                }
                                stroke-width=move || if is_selected.get() { "0.5" } else { "0.18" }
                                style={
                                    let accent = accent.clone();
                                    move || {
                                        if is_selected.get() {
                                            format!("filter: drop-shadow(0 0 2.4px {}); animation: brain-pulse 2.4s ease-in-out infinite;", accent)
                                        } else if is_hovered.get() {
                                            format!("filter: drop-shadow(0 0 1.8px {});", accent)
                                        } else {
                                            String::new()
                                        }
                                    }
                                }
                            />
                            <text
                                x=format!("{:.3}", x)
                                y=format!("{:.3}", y + label_offset)
                                text-anchor="middle"
                                font-size=label_size
                                fill=label_fill
                                style="pointer-events:none; font-weight:500;"
                            >
                                {title}
                            </text>
                        </g>
                    }
                })
                .collect_view()
        })
    };

    view! {
        <div class="flex-1 relative bg-gradient-to-br from-slate-950 via-slate-900 to-slate-950 overflow-hidden">
            <svg
                viewBox=move || view_box.get()
                preserveAspectRatio="xMidYMid meet"
                class="absolute inset-0 w-full h-full"
            >
                <defs>
                    <radialGradient id="bg-glow" cx="50%" cy="50%" r="65%">
                        <stop offset="0%" stop-color="#0ea5e9" stop-opacity="0.10"/>
                        <stop offset="100%" stop-color="#020617" stop-opacity="0"/>
                    </radialGradient>
                    <style>
                        {"@keyframes brain-pulse { 0%,100% { opacity: 1; } 50% { opacity: 0.55; } }"}
                    </style>
                </defs>
                <rect width="100" height="100" fill="url(#bg-glow)" pointer-events="none"/>
                <g>{edges_view}</g>
                <g>{nodes_view}</g>
            </svg>

            <div class="pointer-events-none absolute top-3 right-4 flex items-center gap-3 text-[10px] uppercase tracking-widest text-slate-500 bg-slate-900/60 border border-slate-800 rounded-md px-3 py-1.5 backdrop-blur">
                {config.node_types.iter().map(|spec| {
                    view! {
                        <span class="flex items-center gap-1.5">
                            <span class="inline-block w-1.5 h-1.5 rounded-full" style=format!("background:{}", spec.accent_var)></span>
                            <span>{spec.label.clone()}</span>
                        </span>
                    }
                }).collect_view()}
            </div>

            <div class="pointer-events-none absolute bottom-3 right-4 text-[10px] uppercase tracking-widest text-slate-600">
                {move || format!("graph · {} nodes", visible_ids.get().len())}
            </div>
        </div>
    }
}
