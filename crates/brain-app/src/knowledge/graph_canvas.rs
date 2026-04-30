use std::collections::{HashMap, HashSet};
#[cfg(feature = "hydrate")]
use std::{cell::Cell, cell::RefCell, rc::Rc};

use leptos::prelude::*;
#[cfg(feature = "hydrate")]
use wasm_bindgen::{JsCast, closure::Closure};

use super::types::{Edge, Node};

#[cfg(feature = "hydrate")]
type RafClosure = Closure<dyn FnMut(f64)>;

const MIN_SCALE: f32 = 0.25;
const MAX_SCALE: f32 = 4.0;
const BASE_VIEW_SIZE: f32 = 100.0;
const NODE_HOVER_BUMP: f32 = 0.5;
const NODE_SELECTED_BUMP: f32 = 0.8;
const NODE_HIT_TARGET_BUFFER: f32 = 0.35;
#[cfg(feature = "hydrate")]
const VIEWPORT_TWEEN_MS: f64 = 300.0;

#[derive(Clone, Copy, Debug, PartialEq)]
struct GraphBounds {
    min_x: f32,
    min_y: f32,
    max_x: f32,
    max_y: f32,
}

impl GraphBounds {
    fn from_nodes(nodes: &[Node]) -> Self {
        let mut bounds = Self {
            min_x: 0.0,
            min_y: 0.0,
            max_x: 100.0,
            max_y: 100.0,
        };

        for node in nodes {
            bounds.min_x = bounds.min_x.min(node.x);
            bounds.min_y = bounds.min_y.min(node.y);
            bounds.max_x = bounds.max_x.max(node.x);
            bounds.max_y = bounds.max_y.max(node.y);
        }

        bounds
    }

    fn center(self) -> (f32, f32) {
        (
            (self.min_x + self.max_x) * 0.5,
            (self.min_y + self.max_y) * 0.5,
        )
    }

    fn width(self) -> f32 {
        (self.max_x - self.min_x).max(1.0)
    }

    fn height(self) -> f32 {
        (self.max_y - self.min_y).max(1.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Viewport {
    cx: f32,
    cy: f32,
    scale: f32,
}

impl Viewport {
    fn overview(bounds: GraphBounds) -> Self {
        let (cx, cy) = bounds.center();
        let graph_span = bounds.width().max(bounds.height());
        let scale = (BASE_VIEW_SIZE / graph_span)
            .min(1.0)
            .clamp(MIN_SCALE, MAX_SCALE);
        clamp_viewport(Self { cx, cy, scale }, bounds)
    }

    fn view_size(self) -> f32 {
        BASE_VIEW_SIZE / self.scale
    }

    fn view_box(self) -> String {
        let size = self.view_size();
        format!(
            "{:.3} {:.3} {:.3} {:.3}",
            self.cx - size * 0.5,
            self.cy - size * 0.5,
            size,
            size
        )
    }

    fn rect(self) -> (f32, f32, f32, f32) {
        let size = self.view_size();
        (self.cx - size * 0.5, self.cy - size * 0.5, size, size)
    }

    #[cfg(feature = "hydrate")]
    fn lerp(self, to: Self, t: f32) -> Self {
        Self {
            cx: self.cx + (to.cx - self.cx) * t,
            cy: self.cy + (to.cy - self.cy) * t,
            scale: self.scale + (to.scale - self.scale) * t,
        }
    }
}

fn node_visual_radius(base_r: f32, is_selected: bool, is_hovered: bool) -> f32 {
    let bump = if is_selected {
        NODE_SELECTED_BUMP
    } else if is_hovered {
        NODE_HOVER_BUMP
    } else {
        0.0
    };

    base_r + bump
}

fn node_hit_radius(base_r: f32) -> f32 {
    base_r + NODE_SELECTED_BUMP + NODE_HIT_TARGET_BUFFER
}

fn clamp_viewport(viewport: Viewport, bounds: GraphBounds) -> Viewport {
    let scale = viewport.scale.clamp(MIN_SCALE, MAX_SCALE);
    let size = BASE_VIEW_SIZE / scale;
    let (bounds_cx, bounds_cy) = bounds.center();
    let cx = if size <= bounds.width() {
        viewport
            .cx
            .clamp(bounds.min_x + size * 0.5, bounds.max_x - size * 0.5)
    } else {
        bounds_cx
    };
    let cy = if size <= bounds.height() {
        viewport
            .cy
            .clamp(bounds.min_y + size * 0.5, bounds.max_y - size * 0.5)
    } else {
        bounds_cy
    };
    Viewport { cx, cy, scale }
}

#[cfg(feature = "hydrate")]
fn eased(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[allow(clippy::too_many_arguments)]
fn set_viewport(
    rendered: RwSignal<Viewport>,
    target: RwSignal<Viewport>,
    animation_epoch: RwSignal<u32>,
    bounds: GraphBounds,
    next: Viewport,
    animated: bool,
) {
    let next = clamp_viewport(next, bounds);
    target.set(next);
    if !animated || rendered.get_untracked() == next {
        animation_epoch.update(|epoch| *epoch = epoch.wrapping_add(1));
        rendered.set(next);
        return;
    }

    #[cfg(not(feature = "hydrate"))]
    {
        animation_epoch.update(|epoch| *epoch = epoch.wrapping_add(1));
        rendered.set(next);
    }

    #[cfg(feature = "hydrate")]
    {
        let Some(window) = web_sys::window() else {
            rendered.set(next);
            return;
        };
        let from = rendered.get_untracked();
        animation_epoch.update(|epoch| *epoch = epoch.wrapping_add(1));
        let epoch = animation_epoch.get_untracked();
        let start_at = Rc::new(Cell::new(None::<f64>));
        let frame: Rc<RefCell<Option<RafClosure>>> = Rc::new(RefCell::new(None));
        let frame_for_cb = Rc::clone(&frame);
        let start_for_cb = Rc::clone(&start_at);
        let window_for_cb = window.clone();

        *frame.borrow_mut() = Some(Closure::<dyn FnMut(f64)>::new(move |ts| {
            if animation_epoch.get_untracked() != epoch {
                frame_for_cb.borrow_mut().take();
                return;
            }
            let start = start_for_cb.get().unwrap_or_else(|| {
                start_for_cb.set(Some(ts));
                ts
            });
            let progress = ((ts - start) / VIEWPORT_TWEEN_MS).clamp(0.0, 1.0) as f32;
            rendered.set(from.lerp(next, eased(progress)));
            if progress < 1.0 {
                if let Some(cb) = frame_for_cb.borrow().as_ref() {
                    let _ = window_for_cb.request_animation_frame(cb.as_ref().unchecked_ref());
                }
            } else {
                rendered.set(next);
                frame_for_cb.borrow_mut().take();
            }
        }));

        if let Some(cb) = frame.borrow().as_ref() {
            let _ = window.request_animation_frame(cb.as_ref().unchecked_ref());
        }
    }
}

fn touch_distance(ev: &web_sys::TouchEvent) -> Option<f64> {
    let touches = ev.touches();
    let a = touches.item(0)?;
    let b = touches.item(1)?;
    let dx = f64::from(a.client_x() - b.client_x());
    let dy = f64::from(a.client_y() - b.client_y());
    Some((dx * dx + dy * dy).sqrt())
}

#[derive(Clone, Copy)]
struct LabelCandidate {
    id: u32,
    x: f32,
    y: f32,
    width: f32,
    priority: i32,
}

fn label_budget(visible_count: usize, has_focus: bool) -> usize {
    if has_focus {
        18
    } else if visible_count > 90 {
        12
    } else if visible_count > 55 {
        16
    } else {
        24
    }
}

fn compact_label(title: &str, is_tag: bool, is_focus: bool) -> String {
    let limit = if is_focus {
        34
    } else if is_tag {
        18
    } else {
        24
    };

    if title.chars().count() <= limit {
        return title.to_string();
    }

    let mut out: String = title.chars().take(limit.saturating_sub(3)).collect();
    out.push_str("...");
    out
}

fn estimated_label_width(label: &str, font_size: f32) -> f32 {
    label.chars().count() as f32 * font_size * 0.58 + 1.0
}

fn labels_overlap(a: LabelCandidate, b: LabelCandidate) -> bool {
    let horizontal_gap = (a.width + b.width) * 0.5 + 1.2;
    let vertical_gap = 3.2;
    (a.x - b.x).abs() < horizontal_gap && (a.y - b.y).abs() < vertical_gap
}

fn visible_label_ids(
    nodes: &[Node],
    visible_ids: &HashSet<u32>,
    degrees: &HashMap<u32, usize>,
    adjacency: &HashMap<u32, HashSet<u32>>,
    focus: Option<u32>,
    tag_type: Option<&str>,
) -> HashSet<u32> {
    let mut candidates: Vec<LabelCandidate> = nodes
        .iter()
        .filter(|n| visible_ids.contains(&n.id))
        .filter_map(|n| {
            let deg = *degrees.get(&n.id).unwrap_or(&0);
            let is_tag = tag_type == Some(n.node_type.as_str());
            let in_focus_neighbourhood = focus.is_some_and(|f| {
                f == n.id
                    || adjacency
                        .get(&f)
                        .map(|s| s.contains(&n.id))
                        .unwrap_or(false)
            });

            if focus.is_some() && !in_focus_neighbourhood {
                return None;
            }

            if focus.is_none() && is_tag && deg < 4 {
                return None;
            }

            let base_r = if is_tag {
                0.9_f32 + (deg as f32).min(4.0) * 0.12
            } else {
                1.5_f32 + (deg as f32).min(6.0) * 0.18
            };
            let label_size = if is_tag { 1.1 } else { 1.55 };
            let is_focus = focus == Some(n.id);
            let label = compact_label(&n.title, is_tag, is_focus);

            let mut priority = (deg as i32) * 8;
            if !is_tag {
                priority += 24;
            }
            if in_focus_neighbourhood {
                priority += 60;
            }
            if is_focus {
                priority += 1_000;
            }

            Some(LabelCandidate {
                id: n.id,
                x: n.x,
                y: n.y + base_r + 2.4,
                width: estimated_label_width(&label, label_size),
                priority,
            })
        })
        .collect();

    candidates.sort_by(|a, b| {
        b.priority
            .cmp(&a.priority)
            .then_with(|| a.y.total_cmp(&b.y))
            .then_with(|| a.x.total_cmp(&b.x))
    });

    let budget = label_budget(visible_ids.len(), focus.is_some());
    let mut placed: Vec<LabelCandidate> = Vec::new();
    let mut ids = HashSet::new();

    for candidate in candidates {
        let forced = focus == Some(candidate.id);
        if !forced && placed.len() >= budget {
            break;
        }
        if forced || placed.iter().all(|p| !labels_overlap(candidate, *p)) {
            ids.insert(candidate.id);
            placed.push(candidate);
        }
    }

    ids
}

#[component]
pub fn GraphCanvas(
    nodes: StoredValue<Vec<Node>>,
    edges: StoredValue<Vec<Edge>>,
    visible_ids: Signal<HashSet<u32>>,
    hovered: RwSignal<Option<u32>>,
    selected: RwSignal<Option<u32>>,
    selected_path: RwSignal<Option<String>>,
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

    let bounds = StoredValue::new(nodes.with_value(|ns| GraphBounds::from_nodes(ns)));
    let initial_viewport = bounds.with_value(|b| Viewport::overview(*b));
    let rendered_viewport = RwSignal::new(initial_viewport);
    let target_viewport = RwSignal::new(initial_viewport);
    let animation_epoch = RwSignal::new(0_u32);

    let view_box = Memo::new(move |_| rendered_viewport.get().view_box());
    let bg_rect = Memo::new(move |_| rendered_viewport.get().rect());

    Effect::new(move |_| {
        let current_scale = target_viewport.get_untracked().scale;
        let next = match focus.get() {
            Some(id) => positions
                .with_value(|p| p.get(&id).copied())
                .map(|(cx, cy)| Viewport {
                    cx,
                    cy,
                    scale: current_scale,
                })
                .unwrap_or_else(|| bounds.with_value(|b| Viewport::overview(*b))),
            None => bounds.with_value(|b| Viewport::overview(*b)),
        };
        let graph_bounds = bounds.get_value();
        set_viewport(
            rendered_viewport,
            target_viewport,
            animation_epoch,
            graph_bounds,
            next,
            true,
        );
    });

    let zoom_by = move |factor: f32, animated: bool| {
        let current = target_viewport.get_untracked();
        let graph_bounds = bounds.get_value();
        set_viewport(
            rendered_viewport,
            target_viewport,
            animation_epoch,
            graph_bounds,
            Viewport {
                scale: current.scale * factor,
                ..current
            },
            animated,
        );
    };

    let reset_view = move || {
        let graph_bounds = bounds.get_value();
        set_viewport(
            rendered_viewport,
            target_viewport,
            animation_epoch,
            graph_bounds,
            Viewport::overview(graph_bounds),
            true,
        );
    };

    let last_pinch_distance = RwSignal::new(None::<f64>);

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
                                style="transition: stroke 200ms ease, stroke-width 200ms ease, stroke-opacity 200ms ease;"
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
        let f = focus.get();
        let tag_type = config_for_nodes
            .synthetic_tag_spec()
            .map(|s| s.name.clone());
        let label_ids = nodes.with_value(|ns| {
            degrees.with_value(|d| {
                adjacency.with_value(|a| visible_label_ids(ns, &vis, d, a, f, tag_type.as_deref()))
            })
        });
        nodes.with_value(|ns| {
            ns.iter()
                .filter(|n| vis.contains(&n.id))
                .map(|n| {
                    let id = n.id;
                    let spec = config_for_nodes.lookup(&n.node_type).unwrap_or_else(|| config_for_nodes.default_spec());
                    let accent = spec.accent_var();
                    let is_tag = config_for_nodes
                        .synthetic_tag_spec()
                        .map(|s| s.name.as_str())
                        == Some(n.node_type.as_str());
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
                    let is_label_visible = label_ids.contains(&id);
                    let label = compact_label(&title, is_tag, f == Some(id));

                    view! {
                        <g
                            class="cursor-pointer"
                            style=move || format!("opacity:{}; transition: opacity 200ms ease;", if bright.get() { 1.0 } else { 0.15 })
                            on:mouseenter=move |_| hovered.set(Some(id))
                            on:mouseleave=move |_| hovered.update(|h| if *h == Some(id) { *h = None; })
                            on:click={
                                let path = n.path.clone();
                                move |_| {
                                    if path.is_empty() {
                                        return;
                                    }
                                    selected_path.update(|current| {
                                        *current = if current.as_deref() == Some(path.as_str()) {
                                            None
                                        } else {
                                            Some(path.clone())
                                        };
                                    });
                                }
                            }
                        >
                            <title>{title.clone()}</title>
                            <circle
                                cx=format!("{:.3}", x)
                                cy=format!("{:.3}", y)
                                r=move || {
                                    format!(
                                        "{:.3}",
                                        node_visual_radius(base_r, is_selected.get(), is_hovered.get())
                                    )
                                }
                                fill=accent.clone()
                                fill-opacity=if is_tag { "0.55" } else { "0.92" }
                                stroke={
                                    let accent = accent.clone();
                                    move || {
                                        if is_selected.get() {
                                            "#f8fafc".to_string()
                                        } else {
                                            accent.clone()
                                        }
                                    }
                                }
                                stroke-width=move || if is_selected.get() { "0.5" } else { "0.18" }
                                style={
                                    let accent = accent.clone();
                                    // SVG presentation attributes (r, stroke, stroke-width, filter)
                                    // are CSS-mapped, so a `transition` here crossfades hover/select
                                    // states without any JS animation loop.
                                    const TRANSITION: &str = "transition: r 200ms ease, stroke 200ms ease, stroke-width 200ms ease, filter 200ms ease;";
                                    move || {
                                        if is_selected.get() {
                                            format!("{TRANSITION} filter: drop-shadow(0 0 2.4px {accent}); animation: brain-pulse 2.4s ease-in-out infinite;")
                                        } else if is_hovered.get() {
                                            format!("{TRANSITION} filter: drop-shadow(0 0 1.8px {accent});")
                                        } else {
                                            TRANSITION.to_string()
                                        }
                                    }
                                }
                                pointer-events="none"
                            />
                            <circle
                                cx=format!("{:.3}", x)
                                cy=format!("{:.3}", y)
                                r=format!("{:.3}", node_hit_radius(base_r))
                                fill="transparent"
                                pointer-events="all"
                            />
                            {is_label_visible.then(|| view! {
                                <text
                                    x=format!("{:.3}", x)
                                    y=format!("{:.3}", y + label_offset)
                                    text-anchor="middle"
                                    font-size=label_size
                                    fill=label_fill
                                    style="pointer-events:none; font-weight:500; paint-order:stroke; stroke:#020617; stroke-width:0.55; stroke-linejoin:round;"
                                >
                                    {label}
                                </text>
                            })}
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
                on:wheel=move |ev: web_sys::WheelEvent| {
                    ev.prevent_default();
                    let factor = if ev.delta_y() < 0.0 { 1.12 } else { 1.0 / 1.12 };
                    zoom_by(factor, false);
                }
                on:touchstart=move |ev: web_sys::TouchEvent| {
                    if ev.touches().length() == 2 {
                        last_pinch_distance.set(touch_distance(&ev));
                    }
                }
                on:touchmove=move |ev: web_sys::TouchEvent| {
                    if ev.touches().length() != 2 {
                        last_pinch_distance.set(None);
                        return;
                    }
                    ev.prevent_default();
                    let Some(distance) = touch_distance(&ev) else {
                        return;
                    };
                    if let Some(previous) = last_pinch_distance.get_untracked().filter(|d| *d > 0.0) {
                        zoom_by((distance / previous) as f32, false);
                    }
                    last_pinch_distance.set(Some(distance));
                }
                on:touchend=move |_| last_pinch_distance.set(None)
                on:touchcancel=move |_| last_pinch_distance.set(None)
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
                <rect
                    x=move || format!("{:.3}", bg_rect.get().0)
                    y=move || format!("{:.3}", bg_rect.get().1)
                    width=move || format!("{:.3}", bg_rect.get().2)
                    height=move || format!("{:.3}", bg_rect.get().3)
                    fill="url(#bg-glow)"
                    pointer-events="none"
                />
                <g>{edges_view}</g>
                <g>{nodes_view}</g>
            </svg>

            <div class="pointer-events-none absolute top-3 right-4 flex items-center gap-3 text-[10px] uppercase tracking-widest text-slate-500 bg-slate-900/60 border border-slate-800 rounded-md px-3 py-1.5 backdrop-blur">
                {config.node_types.iter().map(|spec| {
                    view! {
                        <span class="flex items-center gap-1.5">
                            <span class="inline-block w-1.5 h-1.5 rounded-full" style=format!("background:{}", spec.accent_var())></span>
                            <span>{spec.label.clone()}</span>
                        </span>
                    }
                }).collect_view()}
            </div>

            <div class="absolute bottom-3 right-4 flex items-center gap-1 text-[10px] uppercase tracking-widest text-slate-600">
                <button
                    class="px-2 py-1 rounded-md bg-slate-900/60 border border-slate-800 text-slate-400 hover:bg-slate-700 hover:text-slate-200 transition-colors"
                    title="Zoom out"
                    aria-label="Zoom out"
                    on:click=move |_| zoom_by(1.0 / 1.25, true)
                >
                    "-"
                </button>
                <button
                    class="px-2 py-1 rounded-md bg-slate-900/60 border border-slate-800 text-slate-400 hover:bg-slate-700 hover:text-slate-200 transition-colors"
                    title="Reset graph view"
                    aria-label="Reset graph view"
                    on:click=move |_| reset_view()
                >
                    {move || format!("{:.2}x", rendered_viewport.get().scale)}
                </button>
                <button
                    class="px-2 py-1 rounded-md bg-slate-900/60 border border-slate-800 text-slate-400 hover:bg-slate-700 hover:text-slate-200 transition-colors"
                    title="Zoom in"
                    aria-label="Zoom in"
                    on:click=move |_| zoom_by(1.25, true)
                >
                    "+"
                </button>
                <span class="px-2 py-1">
                    {move || format!("graph · {} nodes", visible_ids.get().len())}
                </span>
            </div>
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(id: u32, title: &str, node_type: &str, x: f32, y: f32) -> Node {
        Node {
            id,
            title: title.to_string(),
            summary: String::new(),
            node_type: node_type.to_string(),
            tags: Vec::new(),
            x,
            y,
            path: format!("{title}.md"),
            sha: String::new(),
        }
    }

    #[test]
    fn compact_labels_keep_focus_titles_longer() {
        let title = "A very long operational decision title that would crowd the graph";

        assert!(compact_label(title, false, false).len() < compact_label(title, false, true).len());
        assert!(compact_label(title, true, false).len() < compact_label(title, false, false).len());
    }

    #[test]
    fn node_hit_radius_covers_all_visual_states() {
        let base_r = 1.5;
        let idle = node_visual_radius(base_r, false, false);
        let hovered = node_visual_radius(base_r, false, true);
        let selected = node_visual_radius(base_r, true, true);
        let hit = node_hit_radius(base_r);

        assert!(hit > idle);
        assert!(hit > hovered);
        assert!(hit > selected);
    }

    #[test]
    fn focused_labels_stay_in_the_focused_neighbourhood() {
        let nodes = vec![
            node(1, "Selected", "concept", 10.0, 10.0),
            node(2, "Neighbour", "concept", 30.0, 30.0),
            node(3, "Distant Hub", "concept", 80.0, 80.0),
        ];
        let visible_ids = HashSet::from([1, 2, 3]);
        let degrees = HashMap::from([(1, 1), (2, 1), (3, 12)]);
        let adjacency = HashMap::from([(1, HashSet::from([2]))]);

        let labels = visible_label_ids(&nodes, &visible_ids, &degrees, &adjacency, Some(1), None);

        assert!(labels.contains(&1));
        assert!(labels.contains(&2));
        assert!(!labels.contains(&3));
    }

    #[test]
    fn overview_hides_low_degree_tag_labels() {
        let nodes = vec![
            node(1, "Concept", "concept", 10.0, 10.0),
            node(2, "#small", "tag", 40.0, 40.0),
        ];
        let visible_ids = HashSet::from([1, 2]);
        let degrees = HashMap::from([(1, 1), (2, 1)]);
        let adjacency = HashMap::new();

        let labels = visible_label_ids(
            &nodes,
            &visible_ids,
            &degrees,
            &adjacency,
            None,
            Some("tag"),
        );

        assert!(labels.contains(&1));
        assert!(!labels.contains(&2));
    }

    #[test]
    fn viewport_clamps_focus_near_graph_edges() {
        let bounds = GraphBounds {
            min_x: 0.0,
            min_y: 0.0,
            max_x: 100.0,
            max_y: 100.0,
        };
        let view = clamp_viewport(
            Viewport {
                cx: 4.0,
                cy: 96.0,
                scale: 2.0,
            },
            bounds,
        );
        let size = view.view_size();

        assert_eq!(size, 50.0);
        assert_eq!(view.cx, 25.0);
        assert_eq!(view.cy, 75.0);
    }

    #[test]
    fn viewport_scale_is_limited_to_success_criteria_range() {
        let bounds = GraphBounds {
            min_x: 0.0,
            min_y: 0.0,
            max_x: 100.0,
            max_y: 100.0,
        };

        assert_eq!(
            clamp_viewport(
                Viewport {
                    cx: 50.0,
                    cy: 50.0,
                    scale: 0.01
                },
                bounds
            )
            .scale,
            MIN_SCALE
        );
        assert_eq!(
            clamp_viewport(
                Viewport {
                    cx: 50.0,
                    cy: 50.0,
                    scale: 99.0
                },
                bounds
            )
            .scale,
            MAX_SCALE
        );
    }
}
