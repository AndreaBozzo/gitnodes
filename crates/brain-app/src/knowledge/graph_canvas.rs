use std::collections::{HashMap, HashSet};
#[cfg(feature = "hydrate")]
use std::{cell::Cell, cell::RefCell, rc::Rc};

use leptos::prelude::*;
#[cfg(feature = "hydrate")]
use wasm_bindgen::{JsCast, closure::Closure};

use super::types::{Edge, Node};
use brain_domain::EdgeKind;

#[cfg(feature = "hydrate")]
type RafClosure = Closure<dyn FnMut(f64)>;

const MIN_SCALE: f32 = 0.25;
const MAX_SCALE: f32 = 4.0;
const BASE_VIEW_SIZE: f32 = 100.0;
const NODE_HOVER_BUMP: f32 = 0.5;
const NODE_SELECTED_BUMP: f32 = 0.8;
#[cfg(any(feature = "hydrate", test))]
const NODE_HIT_TARGET_BUFFER: f32 = 0.35;
#[cfg(any(feature = "hydrate", test))]
const NODE_HOVER_HYSTERESIS: f32 = 0.9;
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

fn node_base_radius(is_tag: bool, degree: usize) -> f32 {
    // Document nodes: linear scaling up to degree 4 (was 6). Past degree 4
    // the visual hub footprint was large enough to cover 2-3 of its
    // neighbours on dense brains like the Pokémon mock, making leaf nodes
    // hidden under their own hub. Capping earlier keeps super-hubs
    // distinguishable without swallowing their neighbourhood.
    if is_tag {
        0.9_f32 + (degree as f32).min(4.0) * 0.12
    } else {
        1.5_f32 + (degree as f32).min(4.0) * 0.18
    }
}

#[cfg(any(feature = "hydrate", test))]
fn node_hit_radius(base_r: f32) -> f32 {
    base_r + NODE_SELECTED_BUMP + NODE_HIT_TARGET_BUFFER
}

fn node_click_radius(base_r: f32, visible_count: usize) -> f32 {
    let buffer = if visible_count > 70 {
        0.18
    } else if visible_count > 45 {
        0.28
    } else {
        0.45
    };
    base_r + buffer
}

#[cfg(any(feature = "hydrate", test))]
fn node_hover_leave_radius(base_r: f32) -> f32 {
    node_hit_radius(base_r) + NODE_HOVER_HYSTERESIS
}

fn viewport_focus_id(selected: Option<u32>, _hovered: Option<u32>) -> Option<u32> {
    selected
}

#[cfg(any(feature = "hydrate", test))]
fn hover_node_at(
    nodes: &[Node],
    visible_ids: &HashSet<u32>,
    degrees: &HashMap<u32, usize>,
    tag_type: Option<&str>,
    current: Option<u32>,
    x: f32,
    y: f32,
) -> Option<u32> {
    let node_distance_sq = |node: &Node| {
        let dx = node.x - x;
        let dy = node.y - y;
        dx * dx + dy * dy
    };
    let base_radius = |node: &Node| {
        let degree = *degrees.get(&node.id).unwrap_or(&0);
        node_base_radius(tag_type == Some(node.node_type.as_str()), degree)
    };

    if let Some(current_id) = current
        && let Some(node) = nodes
            .iter()
            .find(|node| node.id == current_id && visible_ids.contains(&node.id))
    {
        let radius = node_hover_leave_radius(base_radius(node));
        if node_distance_sq(node) <= radius * radius {
            return Some(current_id);
        }
    }

    nodes
        .iter()
        .filter(|node| visible_ids.contains(&node.id))
        .filter_map(|node| {
            let distance_sq = node_distance_sq(node);
            let radius = node_hit_radius(base_radius(node));
            (distance_sq <= radius * radius).then_some((node.id, distance_sq))
        })
        .min_by(|(_, a), (_, b)| a.total_cmp(b))
        .map(|(id, _)| id)
}

#[cfg(feature = "hydrate")]
fn graph_coords_from_client(
    target: Option<web_sys::EventTarget>,
    client_x: i32,
    client_y: i32,
    viewport: Viewport,
) -> Option<(f32, f32)> {
    let target = target?;
    let element = target.dyn_into::<web_sys::Element>().ok()?;
    let rect = element.get_bounding_client_rect();
    let width = rect.width();
    let height = rect.height();
    if width <= 0.0 || height <= 0.0 {
        return None;
    }

    let rendered_side = width.min(height);
    let offset_x = (width - rendered_side) * 0.5;
    let offset_y = (height - rendered_side) * 0.5;
    let local_x = f64::from(client_x) - rect.left() - offset_x;
    let local_y = f64::from(client_y) - rect.top() - offset_y;
    let (view_x, view_y, view_w, view_h) = viewport.rect();

    Some((
        view_x + (local_x / rendered_side) as f32 * view_w,
        view_y + (local_y / rendered_side) as f32 * view_h,
    ))
}

#[cfg(feature = "hydrate")]
fn pointer_graph_coords(ev: &web_sys::PointerEvent, viewport: Viewport) -> Option<(f32, f32)> {
    graph_coords_from_client(ev.current_target(), ev.client_x(), ev.client_y(), viewport)
}

#[cfg(feature = "hydrate")]
fn mouse_graph_coords(ev: &web_sys::MouseEvent, viewport: Viewport) -> Option<(f32, f32)> {
    graph_coords_from_client(ev.current_target(), ev.client_x(), ev.client_y(), viewport)
}

#[cfg(any(feature = "hydrate", test))]
fn click_node_at(
    nodes: &[Node],
    visible_ids: &HashSet<u32>,
    degrees: &HashMap<u32, usize>,
    tag_type: Option<&str>,
    x: f32,
    y: f32,
) -> Option<u32> {
    nodes
        .iter()
        .filter(|node| visible_ids.contains(&node.id))
        .filter_map(|node| {
            let dx = node.x - x;
            let dy = node.y - y;
            let distance_sq = dx * dx + dy * dy;
            let degree = *degrees.get(&node.id).unwrap_or(&0);
            let base_r = node_base_radius(tag_type == Some(node.node_type.as_str()), degree);
            let radius = node_click_radius(base_r, visible_ids.len());
            (distance_sq <= radius * radius).then_some((node.id, distance_sq))
        })
        .min_by(|(_, a), (_, b)| a.total_cmp(b))
        .map(|(id, _)| id)
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
    if has_focus || visible_count > 70 {
        14
    } else if visible_count > 45 {
        18
    } else {
        26
    }
}

fn presentation_label(title: &str, is_tag: bool, is_focus: bool) -> String {
    if is_tag || is_focus {
        return title.trim().to_string();
    }

    let Some((prefix, rest)) = title.split_once(':') else {
        return title.trim().to_string();
    };
    let rest = rest.trim();
    let prefix = prefix.trim();

    if rest.is_empty() || prefix.chars().count() > 24 {
        title.trim().to_string()
    } else {
        rest.to_string()
    }
}

fn compact_label(title: &str, is_tag: bool, is_focus: bool) -> String {
    let title = presentation_label(title, is_tag, is_focus);
    let title_len = title.chars().count();
    let limit = if is_focus {
        34
    } else if is_tag || title_len <= 18 {
        18
    } else {
        21
    };

    if title_len <= limit {
        return title;
    }

    let mut out: String = title.chars().take(limit.saturating_sub(3)).collect();
    out.push_str("...");
    out
}

fn label_font_size(is_tag: bool, visible_count: usize, has_focus: bool) -> f32 {
    if is_tag {
        1.0
    } else if has_focus {
        1.45
    } else if visible_count > 70 {
        1.2
    } else if visible_count > 45 {
        1.3
    } else {
        1.45
    }
}

fn estimated_label_width(label: &str, font_size: f32) -> f32 {
    label.chars().count() as f32 * font_size * 0.58 + 1.0
}

fn labels_overlap(a: LabelCandidate, b: LabelCandidate) -> bool {
    let horizontal_gap = (a.width + b.width) * 0.5 + 1.6;
    let vertical_gap = 3.7;
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

            let base_r = node_base_radius(is_tag, deg);
            let is_focus = focus == Some(n.id);
            let label = compact_label(&n.title, is_tag, is_focus);
            let label_size = label_font_size(is_tag, visible_ids.len(), focus.is_some());

            let mut priority = (deg as i32) * 8;
            if !is_tag {
                priority += 24;
            }
            if !is_tag && n.title.contains(':') {
                priority -= 8;
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum EdgeLegendGroup {
    Body,
    Ownership,
    Evolution,
    Geography,
    Tag,
    Other,
}

impl EdgeLegendGroup {
    const ALL: [Self; 6] = [
        Self::Body,
        Self::Ownership,
        Self::Evolution,
        Self::Geography,
        Self::Tag,
        Self::Other,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::Body => "Body",
            Self::Ownership => "Ownership",
            Self::Evolution => "Evolution",
            Self::Geography => "Geography",
            Self::Tag => "Tag",
            Self::Other => "Other",
        }
    }
}

#[derive(Clone, Copy)]
struct EdgeStyle {
    stroke: &'static str,
    width: &'static str,
    dasharray: &'static str,
}

fn edge_legend_group(kind: &EdgeKind) -> EdgeLegendGroup {
    match kind {
        EdgeKind::Body => EdgeLegendGroup::Body,
        // Tag edges get their own legend group so the "Tags" node toggle
        // can coordinate with the edge group: showing tag nodes while the
        // edge group is off would leave disconnected hubs floating on the
        // canvas. They used to be folded into Other, but conflating
        // membership edges with frontmatter catch-alls hid this bug.
        EdgeKind::Tag => EdgeLegendGroup::Tag,
        EdgeKind::Frontmatter(field) => match field.as_str() {
            "trainer" => EdgeLegendGroup::Ownership,
            "evolves_to" | "evolves_from" => EdgeLegendGroup::Evolution,
            "locations" | "encounters" => EdgeLegendGroup::Geography,
            _ => EdgeLegendGroup::Other,
        },
    }
}

fn edge_style(kind: &EdgeKind, touches: bool) -> EdgeStyle {
    // Visual hierarchy (thicker = more visible by default):
    //   Body (narrative) ── 0.26  ── the story spine
    //   Frontmatter VIPs ── 0.22  ── trainer / evolves / locations
    //   Frontmatter other ─ 0.13  ── catch-all typed edges (background metadata)
    //   Tag ─────────────── 0.16  ── virtual tag membership
    //
    // The previous calibration had Body and "Other" Frontmatter at the same
    // weight, which let the catch-all bucket (often the largest population —
    // 26x `applies_to`, 14x `moves`, 12x `pokemon_pool`, … on the Pokémon
    // mock) drown out body citations under a uniform magenta wash. Now the
    // narrative spine reads first; the catch-all Frontmatter sits behind it
    // as structural metadata, and the three semantically-loaded kinds
    // (ownership / evolution / geography) keep their dedicated weight and
    // hue so the legend remains useful.
    let body_width = if touches { "0.44" } else { "0.22" };
    let vip_width = if touches { "0.40" } else { "0.18" };
    let other_width = if touches { "0.28" } else { "0.10" };
    let tag_width = if touches { "0.32" } else { "0.12" };
    match kind {
        EdgeKind::Body => EdgeStyle {
            stroke: "#475569",
            width: body_width,
            dasharray: "none",
        },
        EdgeKind::Tag => EdgeStyle {
            stroke: "#94a3b8",
            width: tag_width,
            dasharray: "0.01 0.8",
        },
        EdgeKind::Frontmatter(field) => match field.as_str() {
            "trainer" => EdgeStyle {
                stroke: "#0ea5e9",
                width: vip_width,
                dasharray: "none",
            },
            "evolves_to" | "evolves_from" => EdgeStyle {
                stroke: "#f59e0b",
                width: vip_width,
                dasharray: "1.4 1.0",
            },
            "locations" | "encounters" => EdgeStyle {
                stroke: "#22c55e",
                width: vip_width,
                dasharray: "0.01 0.95",
            },
            _ => EdgeStyle {
                stroke: "#a78bfa",
                width: other_width,
                dasharray: "none",
            },
        },
    }
}

fn default_edge_groups(node_count: usize, edge_count: usize) -> HashSet<EdgeLegendGroup> {
    let mut groups = HashSet::from(EdgeLegendGroup::ALL);
    if node_count > 80 || edge_count > 120 {
        groups.remove(&EdgeLegendGroup::Other);
    }
    groups
}

fn overview_edge_opacity(visible_count: usize) -> f32 {
    if visible_count > 70 {
        0.26
    } else if visible_count > 45 {
        0.34
    } else {
        0.48
    }
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
    #[cfg(not(feature = "hydrate"))]
    let _ = selected_path;

    // Full adjacency built from every edge in the graph, including those
    // connecting to virtual tag nodes. When the user hides tag nodes (the
    // default), we derive a doc-only adjacency from this one — see
    // `effective_adjacency` below.
    let full_adjacency: StoredValue<HashMap<u32, HashSet<u32>>> = StoredValue::new({
        let mut m: HashMap<u32, HashSet<u32>> = HashMap::new();
        edges.with_value(|es| {
            for e in es {
                m.entry(e.from).or_default().insert(e.to);
                m.entry(e.to).or_default().insert(e.from);
            }
        });
        m
    });

    let visual_focus = Memo::new(move |_| selected.get().or_else(|| hovered.get()));

    let positions: StoredValue<HashMap<u32, (f32, f32)>> =
        StoredValue::new(nodes.with_value(|ns| ns.iter().map(|n| (n.id, (n.x, n.y))).collect()));

    let bounds = StoredValue::new(nodes.with_value(|ns| GraphBounds::from_nodes(ns)));
    let initial_viewport = bounds.with_value(|b| Viewport::overview(*b));
    let rendered_viewport = RwSignal::new(initial_viewport);
    let target_viewport = RwSignal::new(initial_viewport);
    let animation_epoch = RwSignal::new(0_u32);
    let enabled_edge_groups = RwSignal::new(default_edge_groups(
        nodes.with_value(Vec::len),
        edges.with_value(Vec::len),
    ));
    // Tag node visibility: virtual `#tag` nodes contribute ~25% of the node
    // count on tag-rich brains and produce star-shaped edge bundles that
    // cross the whole canvas. Hidden by default; the legend exposes a
    // toggle that pulls them back in when the user explicitly asks.
    // Brains without a synthetic tag spec (no `tag` type configured) have an
    // empty `tag_node_ids` set, so this code path is a no-op for them.
    let show_tag_nodes = RwSignal::new(false);
    let tag_node_ids: StoredValue<HashSet<u32>> = StoredValue::new({
        let tag_type_name = config
            .synthetic_tag_spec()
            .map(|s| s.name.clone())
            .unwrap_or_default();
        if tag_type_name.is_empty() {
            HashSet::new()
        } else {
            nodes.with_value(|ns| {
                ns.iter()
                    .filter(|n| n.node_type == tag_type_name)
                    .map(|n| n.id)
                    .collect()
            })
        }
    });
    let effective_visible: Memo<HashSet<u32>> = Memo::new(move |_| {
        let base = visible_ids.get();
        if show_tag_nodes.get() {
            base
        } else {
            tag_node_ids.with_value(|tags| {
                if tags.is_empty() {
                    base
                } else {
                    base.into_iter().filter(|id| !tags.contains(id)).collect()
                }
            })
        }
    });

    // If the user has a tag node selected or hovered and then hides tag
    // nodes (default or via toggle), the stale id would re-center the
    // viewport on an unrendered node and leave a phantom hover. Clear them
    // proactively so the canvas state matches what's visible.
    Effect::new(move |_| {
        if show_tag_nodes.get() {
            return;
        }
        tag_node_ids.with_value(|tags| {
            if tags.is_empty() {
                return;
            }
            if let Some(id) = selected.get_untracked()
                && tags.contains(&id)
            {
                selected.set(None);
            }
            if let Some(id) = hovered.get_untracked()
                && tags.contains(&id)
            {
                hovered.set(None);
            }
        });
    });

    let view_box = Memo::new(move |_| rendered_viewport.get().view_box());
    let bg_rect = Memo::new(move |_| rendered_viewport.get().rect());

    Effect::new(move |_| {
        let current_scale = target_viewport.get_untracked().scale;
        // Hover is intentionally excluded here. Re-centering the viewBox while
        // the pointer is over a node moves the hit target underneath the cursor,
        // which can create a mouseenter/mouseleave loop. Selection is stable
        // enough to drive camera movement; hover remains visual-only.
        let next = match viewport_focus_id(selected.get(), None) {
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

    let full_degrees: StoredValue<HashMap<u32, usize>> = StoredValue::new(
        full_adjacency.with_value(|a| a.iter().map(|(k, v)| (*k, v.len())).collect()),
    );

    // Effective adjacency/degrees follow `show_tag_nodes`. When tags are
    // hidden, derive a doc-only view: drop tag node entries entirely and
    // strip tag-node ids out of each doc's neighbour set. Hit radii,
    // label priorities, and hover heuristics then reflect what's actually
    // rendered instead of inflating doc nodes by their now-invisible tag
    // connections.
    let effective_adjacency: Memo<HashMap<u32, HashSet<u32>>> = Memo::new(move |_| {
        let show = show_tag_nodes.get();
        full_adjacency.with_value(|full| {
            if show {
                return full.clone();
            }
            tag_node_ids.with_value(|tags| {
                if tags.is_empty() {
                    return full.clone();
                }
                full.iter()
                    .filter(|(id, _)| !tags.contains(id))
                    .map(|(id, neighbours)| {
                        let trimmed: HashSet<u32> = neighbours
                            .iter()
                            .filter(|n| !tags.contains(n))
                            .copied()
                            .collect();
                        (*id, trimmed)
                    })
                    .collect()
            })
        })
    });
    let effective_degrees: Memo<HashMap<u32, usize>> = Memo::new(move |_| {
        let show = show_tag_nodes.get();
        if show {
            return full_degrees.with_value(Clone::clone);
        }
        // Derive directly from effective_adjacency so the doc-only counts
        // stay consistent with the trimmed neighbour sets.
        effective_adjacency.with(|a| a.iter().map(|(k, v)| (*k, v.len())).collect())
    });

    #[cfg(feature = "hydrate")]
    let update_hover_from_pointer = {
        let config_for_hover = config.clone();
        move |ev: web_sys::PointerEvent| {
            let Some((x, y)) = pointer_graph_coords(&ev, rendered_viewport.get_untracked()) else {
                return;
            };
            let visible = effective_visible.get_untracked();
            let current = hovered.get_untracked();
            let tag_type = config_for_hover
                .synthetic_tag_spec()
                .map(|spec| spec.name.clone());
            let next = nodes.with_value(|ns| {
                effective_degrees
                    .with(|d| hover_node_at(ns, &visible, d, tag_type.as_deref(), current, x, y))
            });
            if next != current {
                hovered.set(next);
            }
        }
    };

    #[cfg(feature = "hydrate")]
    let select_from_pointer = {
        let config_for_click = config.clone();
        move |ev: web_sys::MouseEvent| {
            let Some((x, y)) = mouse_graph_coords(&ev, rendered_viewport.get_untracked()) else {
                return;
            };
            let visible = effective_visible.get_untracked();
            let tag_type = config_for_click
                .synthetic_tag_spec()
                .map(|spec| spec.name.clone());
            let clicked = nodes.with_value(|ns| {
                effective_degrees
                    .with(|d| click_node_at(ns, &visible, d, tag_type.as_deref(), x, y))
            });
            let Some(path) = clicked.and_then(|id| {
                nodes.with_value(|ns| {
                    ns.iter()
                        .find(|node| node.id == id)
                        .map(|node| node.path.clone())
                })
            }) else {
                return;
            };
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
    };

    let edges_view = move || {
        let vis = effective_visible.get();
        let f = visual_focus.get();
        enabled_edge_groups.with(|enabled| {
            edges.with_value(|es| {
                positions.with_value(|pos| {
                    es.iter()
                        .filter(|e| vis.contains(&e.from) && vis.contains(&e.to))
                        .filter(|e| enabled.contains(&edge_legend_group(&e.kind)))
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
                                overview_edge_opacity(vis.len())
                            } else if touches {
                                0.95
                            } else {
                                0.05
                            };
                            let style = edge_style(&e.kind, touches);
                            view! {
                                <path
                                    d=format!("M{:.3},{:.3} Q{:.3},{:.3} {:.3},{:.3}", x1, y1, cx, cy, x2, y2)
                                    fill="none"
                                    stroke=style.stroke
                                    stroke-width=style.width
                                    stroke-dasharray=style.dasharray
                                    stroke-linecap="round"
                                    stroke-opacity=opacity
                                    style="transition: stroke 200ms ease, stroke-width 200ms ease, stroke-opacity 200ms ease;"
                                />
                            }
                        })
                        .collect_view()
                })
            })
        })
    };

    let config_for_nodes = config.clone();
    let nodes_view = move || {
        let vis = effective_visible.get();
        let label_focus = selected.get();
        let tag_type = config_for_nodes
            .synthetic_tag_spec()
            .map(|s| s.name.clone());
        let label_ids = nodes.with_value(|ns| {
            effective_degrees.with(|d| {
                effective_adjacency
                    .with(|a| visible_label_ids(ns, &vis, d, a, label_focus, tag_type.as_deref()))
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
                    let deg = effective_degrees.with(|d| *d.get(&id).unwrap_or(&0));
                    let base_r = node_base_radius(is_tag, deg);

                    let bright = Memo::new(move |_| match visual_focus.get() {
                        None => true,
                        Some(f) if f == id => true,
                        Some(f) => effective_adjacency
                            .with(|a| a.get(&f).map(|s| s.contains(&id)).unwrap_or(false)),
                    });
                    let is_selected = Memo::new(move |_| selected.get() == Some(id));
                    let is_hovered = Memo::new(move |_| hovered.get() == Some(id));

                    let label_size = label_font_size(is_tag, vis.len(), label_focus.is_some());
                    let label_offset = base_r + 2.4;
                    let label_fill = if is_tag { "#cbd5e1" } else { "#e2e8f0" };
                    let is_label_visible = label_ids.contains(&id);
                    let label = compact_label(&title, is_tag, label_focus == Some(id));
                    let node_fill_opacity = if is_tag {
                        "0.50"
                    } else if label_focus.is_none() && vis.len() > 70 {
                        "0.78"
                    } else if label_focus.is_none() && vis.len() > 45 {
                        "0.84"
                    } else {
                        "0.92"
                    };

                    view! {
                        <g
                            class="cursor-pointer"
                            style=move || format!("opacity:{}; transition: opacity 200ms ease;", if bright.get() { 1.0 } else { 0.15 })
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
                                fill-opacity=node_fill_opacity
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
                                r=format!("{:.3}", node_click_radius(base_r, vis.len()))
                                fill="transparent"
                                pointer-events="none"
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
                on:pointermove=move |ev: web_sys::PointerEvent| {
                    #[cfg(feature = "hydrate")]
                    update_hover_from_pointer(ev);
                    #[cfg(not(feature = "hydrate"))]
                    let _ = ev;
                }
                on:pointerleave=move |_| hovered.set(None)
                on:click=move |ev: web_sys::MouseEvent| {
                    #[cfg(feature = "hydrate")]
                    select_from_pointer(ev);
                    #[cfg(not(feature = "hydrate"))]
                    let _ = ev;
                }
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

            <Show when=move || effective_visible.with(HashSet::is_empty)>
                <div class="pointer-events-none absolute inset-0 flex items-center justify-center px-6">
                    <div class="max-w-sm rounded-md border border-slate-800 bg-slate-950/80 px-5 py-4 text-center shadow-2xl shadow-black/20">
                        <div class="text-xs font-semibold uppercase tracking-widest text-slate-400">
                            "No nodes in this view"
                        </div>
                        <p class="mt-2 text-sm text-slate-300">
                            {move || if !show_tag_nodes.get() && !visible_ids.with(HashSet::is_empty) {
                                "Only virtual tag nodes match this scope. Toggle Tags in the edge legend to see them."
                            } else {
                                "Clear the active scope or choose another saved view."
                            }}
                        </p>
                    </div>
                </div>
            </Show>

            <div class="pointer-events-none absolute left-4 right-4 top-3 flex flex-wrap items-center justify-end gap-2 text-[10px] uppercase tracking-widest text-slate-500">
                {config.node_types.iter().map(|spec| {
                    view! {
                        <span class="flex items-center gap-1.5 rounded-md border border-slate-800 bg-slate-900/60 px-2 py-1 backdrop-blur">
                            <span class="inline-block w-1.5 h-1.5 rounded-full" style=format!("background:{}", spec.accent_var())></span>
                            <span>{spec.label.clone()}</span>
                        </span>
                    }
                }).collect_view()}
            </div>

            <div class="absolute bottom-3 left-4 flex max-w-[calc(100%-14rem)] flex-wrap items-center gap-1.5 text-[10px] uppercase tracking-widest text-slate-500">
                {EdgeLegendGroup::ALL.into_iter().map(|group| {
                    let active = Memo::new(move |_| enabled_edge_groups.with(|enabled| enabled.contains(&group)));
                    view! {
                        <button
                            type="button"
                            class=move || {
                                if active.get() {
                                    "rounded-md border border-slate-700 bg-slate-900/80 px-2 py-1 text-slate-200 backdrop-blur transition-colors"
                                } else {
                                    "rounded-md border border-slate-800 bg-slate-950/60 px-2 py-1 text-slate-600 backdrop-blur transition-colors"
                                }
                            }
                            title=format!("Toggle {} edges", group.label())
                            aria-label=format!("Toggle {} edges", group.label())
                            on:click=move |_| {
                                enabled_edge_groups.update(|enabled| {
                                    if enabled.contains(&group) && enabled.len() > 1 {
                                        enabled.remove(&group);
                                    } else {
                                        enabled.insert(group);
                                    }
                                });
                            }
                        >
                            <span>{group.label()}</span>
                        </button>
                    }
                }).collect_view()}
                {(!tag_node_ids.with_value(HashSet::is_empty)).then(|| view! {
                    <button
                        type="button"
                        class=move || {
                            if show_tag_nodes.get() {
                                "rounded-md border border-slate-700 bg-slate-900/80 px-2 py-1 text-slate-200 backdrop-blur transition-colors"
                            } else {
                                "rounded-md border border-slate-800 bg-slate-950/60 px-2 py-1 text-slate-600 backdrop-blur transition-colors"
                            }
                        }
                        title="Toggle virtual tag nodes"
                        aria-label="Toggle virtual tag nodes"
                        on:click=move |_| {
                            show_tag_nodes.update(|v| *v = !*v);
                            // When pulling tag nodes back in, also make sure
                            // their connecting edges are visible — otherwise
                            // the user gets disconnected hubs floating on
                            // the canvas if they had previously toggled the
                            // Tag edge group off.
                            if show_tag_nodes.get() {
                                enabled_edge_groups.update(|groups| {
                                    groups.insert(EdgeLegendGroup::Tag);
                                });
                            }
                        }
                    >
                        <span>"Tags"</span>
                    </button>
                })}
            </div>

            <div class="absolute bottom-3 right-4 flex items-center gap-1 text-[10px] uppercase tracking-widest text-slate-600">
                <button
                    class="h-7 min-w-7 rounded-md bg-slate-900/70 border border-slate-800 px-2 text-slate-400 hover:bg-slate-700 hover:text-slate-200 transition-colors"
                    title="Zoom out"
                    aria-label="Zoom out"
                    on:click=move |_| zoom_by(1.0 / 1.25, true)
                >
                    "-"
                </button>
                <button
                    class="h-7 rounded-md bg-slate-900/70 border border-slate-800 px-2 text-slate-400 hover:bg-slate-700 hover:text-slate-200 transition-colors"
                    title="Reset graph view"
                    aria-label="Reset graph view"
                    on:click=move |_| reset_view()
                >
                    {move || format!("{:.2}x", rendered_viewport.get().scale)}
                </button>
                <button
                    class="h-7 min-w-7 rounded-md bg-slate-900/70 border border-slate-800 px-2 text-slate-400 hover:bg-slate-700 hover:text-slate-200 transition-colors"
                    title="Zoom in"
                    aria-label="Zoom in"
                    on:click=move |_| zoom_by(1.25, true)
                >
                    "+"
                </button>
                <span class="px-2 py-1">
                    {move || format!("graph · {} nodes", effective_visible.get().len())}
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
    fn compact_labels_drop_demo_taxonomy_prefixes_in_overview() {
        assert_eq!(compact_label("Mossa: Tuononda", false, false), "Tuononda");
        assert_eq!(
            compact_label("Mossa: Tuononda", false, true),
            "Mossa: Tuononda"
        );
    }

    #[test]
    fn dense_graphs_start_without_catch_all_edges() {
        let dense = default_edge_groups(102, 213);
        assert!(!dense.contains(&EdgeLegendGroup::Other));
        assert!(dense.contains(&EdgeLegendGroup::Body));

        let sparse = default_edge_groups(29, 40);
        assert!(sparse.contains(&EdgeLegendGroup::Other));
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
    fn dense_click_radius_is_tighter_than_hover_hit_radius() {
        let base_r = node_base_radius(false, 4);

        assert!(node_click_radius(base_r, 76) < node_hit_radius(base_r));
        assert!(node_click_radius(base_r, 19) > node_click_radius(base_r, 76));
    }

    #[test]
    fn click_node_at_prefers_nearest_node_in_overlapping_cluster() {
        let nodes = vec![
            node(1, "Dottrina Speedrun di Kanto", "strategy", 10.0, 10.0),
            node(2, "Ash Ketchum", "trainer", 12.1, 10.0),
        ];
        let visible_ids = HashSet::from([1, 2]);
        let degrees = HashMap::from([(1, 24), (2, 2)]);

        assert_eq!(
            click_node_at(&nodes, &visible_ids, &degrees, None, 12.0, 10.0),
            Some(2)
        );
    }

    #[test]
    fn hover_node_at_keeps_current_inside_hysteresis_ring() {
        let nodes = vec![
            node(1, "Current", "concept", 10.0, 10.0),
            node(2, "Other", "concept", 30.0, 10.0),
        ];
        let visible_ids = HashSet::from([1, 2]);
        let degrees = HashMap::from([(1, 0), (2, 0)]);
        let base_r = node_base_radius(false, 0);
        let just_outside_enter = node_hit_radius(base_r) + 0.25;

        assert_eq!(
            hover_node_at(
                &nodes,
                &visible_ids,
                &degrees,
                None,
                Some(1),
                10.0 + just_outside_enter,
                10.0
            ),
            Some(1)
        );
        assert_eq!(
            hover_node_at(
                &nodes,
                &visible_ids,
                &degrees,
                None,
                None,
                10.0 + just_outside_enter,
                10.0
            ),
            None
        );
    }

    #[test]
    fn viewport_focus_ignores_hover() {
        assert_eq!(viewport_focus_id(None, Some(7)), None);
        assert_eq!(viewport_focus_id(Some(3), Some(7)), Some(3));
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
    fn edge_style_follows_kind_contract() {
        let trainer = EdgeKind::Frontmatter("trainer".to_string());
        let evolution = EdgeKind::Frontmatter("evolves_to".to_string());
        let geography = EdgeKind::Frontmatter("locations".to_string());
        let other = EdgeKind::Frontmatter("custom".to_string());

        assert_eq!(edge_style(&EdgeKind::Body, false).stroke, "#475569");
        assert_eq!(edge_style(&trainer, false).stroke, "#0ea5e9");
        assert_eq!(edge_style(&evolution, false).dasharray, "1.4 1.0");
        assert_eq!(edge_style(&geography, false).stroke, "#22c55e");
        assert_eq!(edge_style(&other, false).stroke, "#a78bfa");
        assert_eq!(edge_style(&EdgeKind::Tag, false).dasharray, "0.01 0.8");
    }

    #[test]
    fn body_edges_are_thicker_than_catch_all_frontmatter() {
        // Visual hierarchy invariant: narrative body citations must read
        // first; the catch-all Frontmatter bucket sits behind them as
        // background metadata. Regressing this lets a dense `link_fields`
        // taxonomy drown out the body spine (the issue that surfaced on the
        // Pokémon mock with 70+ `applies_to`/`moves`/`pokemon_pool` edges).
        let body = edge_style(&EdgeKind::Body, false);
        let other = edge_style(&EdgeKind::Frontmatter("custom".to_string()), false);
        let body_w: f32 = body.width.parse().expect("body width is numeric");
        let other_w: f32 = other.width.parse().expect("other width is numeric");
        assert!(
            body_w > other_w,
            "expected body width {body_w} > catch-all frontmatter width {other_w}"
        );
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
