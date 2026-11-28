//! Smart Charts on the canvas (docs/plans/smart-charts.md, milestone 2).
//!
//! Every visible chart draws its path over Camera, quietly. Select one
//! (click its path with the Transform tool, or pick it in the Timeline or
//! Node view) and, with Transform:
//!
//! - click the canvas to place the chart on this frame: a key past either
//!   end, a breakdown between two points (a point already on this frame
//!   moves there instead);
//! - drag a point to move it; Alt-click one to remove it; click one to go
//!   to its frame.
//!
//! The selected chart shows a tick on every frame, so the spacing reads as
//! speed (close ticks are slow, far apart fast), the frame number on each
//! point, and where the chart is on the current frame with its direction.
//!
//! One drag is one undo step. The overlay is painted on screen only, so it
//! never reaches a render or an export.

use std::collections::HashMap;

use eframe::egui::{self, Color32, Pos2, Stroke as EStroke, Vec2};
use symphony_core::chart::{Chart, ChartCurve, PathKind, PointId, Role, Tangent};
use symphony_core::graph::{NodeId, NodeKind};
use symphony_core::history::Edit;
use symphony_core::math::Affine;
use symphony_ui::{theme, widgets};

use crate::session::{Session, Tool};

/// How near (screen points) a press must be to grab a point.
const GRAB: f32 = 10.0;
/// How near a press must be to a frame's tick to slide it.
const TICK_GRAB: f32 = 6.0;
/// How near a click must be to a chart's path to select that chart.
const PATH_HIT: f32 = 6.0;
/// Polyline points per segment when drawing the path.
const STEPS: usize = 24;

/// The chart colours, in the order new charts take them: the first chart
/// in a scene is always lilac, the second mint, and so on, so an animator
/// who charts the head first gets a lilac head in every scene. Soft
/// tones from the same family as the interface's own (the Function
/// view's curves), kept away from its meanings: no selection blue, time
/// amber or danger red. A shadow under the line keeps them readable on
/// the light stage.
pub const COLOURS: [(&str, [u8; 3]); 8] = [
    ("Lilac", [201, 154, 242]),
    ("Mint", [108, 203, 142]),
    ("Aqua", [92, 207, 208]),
    ("Rose", [240, 140, 192]),
    ("Lime", [184, 216, 106]),
    ("Periwinkle", [150, 156, 246]),
    ("Orchid", [226, 132, 226]),
    ("Sage", [160, 190, 160]),
];

/// The colour a new chart in `graph` takes: the first in the order that
/// no chart uses yet, or round again once all are taken.
pub fn next_colour(graph: &symphony_core::graph::Graph) -> [u8; 3] {
    let used: Vec<[u8; 3]> = graph
        .nodes()
        .filter_map(|(id, n)| match &n.kind {
            NodeKind::Chart(c) => Some(c.colour.unwrap_or(COLOURS[id.0 % COLOURS.len()].1)),
            _ => None,
        })
        .collect();
    COLOURS
        .iter()
        .map(|(_, rgb)| *rgb)
        .find(|rgb| !used.contains(rgb))
        .unwrap_or(COLOURS[used.len() % COLOURS.len()].1)
}

/// The chart colours as a row of swatches to click.
pub fn colour_swatches(ui: &mut egui::Ui, chart: &mut Chart, id: NodeId) {
    let current = colour(chart, id);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        for (name, rgb) in COLOURS {
            let c = Color32::from_rgb(rgb[0], rgb[1], rgb[2]);
            let (rect, response) = ui.allocate_exact_size(Vec2::splat(18.0), egui::Sense::click());
            ui.painter().circle_filled(rect.center(), 7.0, c);
            if c == current {
                ui.painter().circle_stroke(
                    rect.center(),
                    8.5,
                    EStroke::new(1.5, theme::TEXT_PRIMARY),
                );
            } else if response.hovered() {
                ui.painter().circle_stroke(
                    rect.center(),
                    8.5,
                    EStroke::new(1.0, theme::TEXT_SECONDARY),
                );
            }
            if response.on_hover_text(name).clicked() {
                chart.colour = Some(rgb);
            }
        }
    });
}

/// Canvas HUD glass (glass-lite: overlays over the canvas are nearly
/// opaque, about 86%, with a faint light rim), premultiplied.
const HUD: Color32 = Color32::from_rgba_premultiplied(26, 28, 31, 219);
const HUD_RIM: Color32 = Color32::from_rgba_premultiplied(31, 31, 31, 31);
/// Handles show when the pointer is this near their point.
const HANDLE_REVEAL: f32 = 56.0;

/// The colour chart `id` is drawn in.
pub fn colour(chart: &Chart, id: NodeId) -> Color32 {
    let [r, g, b] = chart.colour.unwrap_or(COLOURS[id.0 % COLOURS.len()].1);
    Color32::from_rgb(r, g, b)
}

/// What a press on the selected chart takes hold of.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Grab {
    Point(PointId),
    /// A point's handle: the one after it (towards the next point) or
    /// before it.
    Handle(PointId, bool),
    /// A frame's tick: sliding it along the path sets that frame's
    /// spacing.
    Tick(u32),
    /// The point's turn knob: keys the chart's rotation on its frame.
    Turn(PointId),
    /// The point's size square: keys the chart's scale on its frame.
    Size(PointId),
}

/// Screen length of the turn needle, and of the needle to the size
/// square at 100%.
const NEEDLE: f32 = 56.0;
const SIZE_BASE: f32 = 30.0;

/// Rotation and scale on the canvas for the point on this frame: a needle
/// from the point at the chart's angle, a round knob at its end to turn,
/// and a square along it whose distance is the size.
struct Pose {
    pid: PointId,
    frame: u32,
    at: Pos2,
    dir: Vec2,
    /// The stage angle (degrees) the needle has at 0°: straight up on
    /// screen, clear of a path that mostly runs sideways.
    base: f32,
    scale: f32,
}

impl Pose {
    fn new(
        chart: &Chart,
        curve: &ChartCurve,
        frame: u32,
        screen: &impl Fn([f32; 2]) -> Pos2,
    ) -> Option<Self> {
        let p = chart.point_on(frame)?;
        let m = curve.sample(frame as f32)?;
        let at = screen(p.pos);
        let up = |sign: f32| (screen([p.pos[0], p.pos[1] + sign]) - at).y;
        let base: f32 = if up(1.0) < up(-1.0) { 90.0 } else { -90.0 };
        let (sin, cos) = (base + m.rotation).to_radians().sin_cos();
        let dir = (screen([p.pos[0] + cos, p.pos[1] + sin]) - at).normalized();
        Some(Pose {
            pid: p.id,
            frame,
            at,
            dir: if dir.is_finite() { dir } else { -Vec2::Y },
            base,
            scale: m.scale[0],
        })
    }

    fn knob(&self) -> Pos2 {
        self.at + self.dir * NEEDLE
    }

    fn size(&self) -> Pos2 {
        self.at + self.dir * (SIZE_BASE * self.scale).clamp(10.0, NEEDLE - 12.0)
    }
}

#[derive(Default)]
pub struct ChartEdit {
    /// What's being dragged, on which chart.
    dragging: Option<(NodeId, Grab)>,
    /// A change from the point bar not yet closed as an undo step (a
    /// value still being dragged).
    card_pending: bool,
    /// The rotate and scale needle is shown (toggled from the bar).
    pub(crate) show_pose: bool,
    /// The bar's size when last shown, to choose where it covers least.
    bar_size: Option<Vec2>,
    /// Scene and edit count the charts were last checked at.
    stamp: Option<(String, u64)>,
    built: HashMap<NodeId, Built>,
}

/// A chart made ready to draw: rebuilt only when the chart itself changes.
struct Built {
    chart: Chart,
    curve: ChartCurve,
    /// The path in stage space.
    path: Vec<[f32; 2]>,
    /// Where the chart is on each whole frame of its range.
    marks: Vec<Mark>,
}

#[derive(Clone, Copy)]
struct Mark {
    frame: u32,
    pos: [f32; 2],
    dir: [f32; 2],
}

impl Built {
    fn new(chart: &Chart) -> Self {
        let curve = ChartCurve::new(chart);
        let marks = curve.range().map_or_else(Vec::new, |(a, b)| {
            (a..=b)
                .filter_map(|frame| {
                    curve.sample(frame as f32).map(|m| Mark {
                        frame,
                        pos: m.position,
                        dir: m.direction,
                    })
                })
                .collect()
        });
        Built {
            chart: chart.clone(),
            path: curve.path(STEPS),
            curve,
            marks,
        }
    }

    fn mark(&self, frame: u32) -> Option<&Mark> {
        let first = self.marks.first()?.frame;
        self.marks.get(frame.checked_sub(first)? as usize)
    }

    /// The handles of the point on `frame` that can be dragged, as stage
    /// positions: (after, position) pairs. A corner has none, and a
    /// straight side has none on that side.
    fn handles_on(&self, frame: u32) -> Vec<(PointId, bool, [f32; 2], [f32; 2])> {
        let points = &self.chart.points;
        let Some(i) = points.iter().position(|p| p.frame == frame) else {
            return Vec::new();
        };
        let p = &points[i];
        if p.tangent == Tangent::Corner {
            return Vec::new();
        }
        let Some(&(before, after)) = self.curve.handles().get(i) else {
            return Vec::new();
        };
        let at = |o: [f32; 2]| [p.pos[0] + o[0], p.pos[1] + o[1]];
        let mut out = Vec::new();
        if i > 0 && points[i - 1].path == PathKind::Curve {
            out.push((p.id, false, p.pos, at(before)));
        }
        if i + 1 < points.len() && p.path == PathKind::Curve {
            out.push((p.id, true, p.pos, at(after)));
        }
        out
    }
}

impl ChartEdit {
    /// Bring the built charts up to date with the scene. Nothing happens
    /// while the scene is unchanged; after an edit only the charts whose
    /// content changed are rebuilt.
    fn refresh(&mut self, s: &Session) {
        if self
            .stamp
            .as_ref()
            .is_some_and(|(key, v)| *key == s.key && *v == s.doc.version)
        {
            return;
        }
        self.stamp = Some((s.key.clone(), s.doc.version));
        let mut seen = Vec::new();
        for (id, node) in s.doc.graph.nodes() {
            let NodeKind::Chart(chart) = &node.kind else {
                continue;
            };
            seen.push(id);
            if self.built.get(&id).is_none_or(|b| b.chart != **chart) {
                self.built.insert(id, Built::new(chart));
            }
        }
        self.built.retain(|id, _| seen.contains(id));
    }
}

/// Draw every visible chart and edit the selected one; `m` takes stage
/// space to the screen (Camera's stage plane, or a drawing's own space in
/// the Drawing view). Returns true while a chart takes the Transform
/// tool's clicks (so they don't select or move artwork).
pub fn ui(
    ui: &egui::Ui,
    response: &egui::Response,
    painter: &egui::Painter,
    s: &mut Session,
    state: &mut ChartEdit,
    m: Affine,
) -> bool {
    state.refresh(s);
    if state.built.is_empty() {
        state.dragging = None;
        return false;
    }
    let screen = |p: [f32; 2]| {
        let (x, y) = m.apply(p[0], p[1]);
        Pos2::new(x, y)
    };
    let selected = s.selected_node.filter(|id| state.built.contains_key(id));
    for (&id, b) in &state.built {
        if Some(id) != selected && b.chart.visible {
            draw_quiet(painter, b, colour(&b.chart, id), &screen);
        }
    }
    let editing = s.tool == Tool::Transform;
    let pointer = response.hover_pos().or(response.interact_pointer_pos());
    // Clicking another chart's path selects it.
    if editing && state.dragging.is_none() {
        let hit = pointer.and_then(|p| {
            state
                .built
                .iter()
                .filter(|(id, b)| Some(**id) != selected && b.chart.visible && !b.chart.locked)
                .find(|(_, b)| near_path(&b.path, &screen, p))
                .map(|(id, _)| *id)
        });
        if let Some(id) = hit {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            if response.clicked() {
                s.select_graph_node(id, true);
                return true;
            }
        }
    }
    let Some(id) = selected else {
        state.dragging = None;
        return false;
    };
    let b = &state.built[&id];
    let chart = b.chart.clone();
    // Only what's being worked on shows: a point's curve handles appear
    // when the pointer comes near it (or it has handles of its own), the
    // rotate and scale needle when it's switched on.
    let here = chart.point_on(s.frame);
    let dragging_kind = state.dragging.filter(|(n, _)| *n == id).map(|(_, g)| g);
    let handles = if editing && !chart.locked {
        let near = here.is_some_and(|p| {
            matches!(p.tangent, Tangent::Manual { .. })
                || pointer.is_some_and(|q| q.distance(screen(p.pos)) <= HANDLE_REVEAL)
                || matches!(dragging_kind, Some(Grab::Handle(..)))
        });
        if near {
            b.handles_on(s.frame)
        } else {
            Vec::new()
        }
    } else {
        Vec::new()
    };
    let pose = (editing
        && !chart.locked
        && (state.show_pose || matches!(dragging_kind, Some(Grab::Turn(_) | Grab::Size(_)))))
    .then(|| Pose::new(&chart, &b.curve, s.frame, &screen))
    .flatten();
    // Points first, then handles, then ticks: the nearest of the first
    // kind within reach.
    let grab_at = |p: Pos2| {
        let near = |targets: &mut dyn Iterator<Item = (Grab, Pos2)>, reach: f32| {
            targets
                .map(|(g, q)| (g, q.distance(p)))
                .filter(|(_, d)| *d <= reach)
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(g, _)| g)
        };
        near(
            &mut chart
                .points
                .iter()
                .map(|q| (Grab::Point(q.id), screen(q.pos))),
            GRAB,
        )
        .or_else(|| {
            near(
                &mut handles
                    .iter()
                    .map(|&(pid, after, _, h)| (Grab::Handle(pid, after), screen(h))),
                GRAB,
            )
        })
        .or_else(|| {
            let pose = pose.as_ref()?;
            near(
                &mut [
                    (Grab::Turn(pose.pid), pose.knob()),
                    (Grab::Size(pose.pid), pose.size()),
                ]
                .into_iter(),
                GRAB,
            )
        })
        .or_else(|| {
            near(
                &mut b
                    .marks
                    .iter()
                    .filter(|k| chart.point_on(k.frame).is_none())
                    .map(|k| (Grab::Tick(k.frame), screen(k.pos))),
                TICK_GRAB,
            )
        })
    };
    let nearest = pointer.and_then(grab_at);
    let active = state
        .dragging
        .filter(|(n, _)| *n == id)
        .map(|(_, g)| g)
        .or(nearest);
    draw_selected(
        painter,
        b,
        colour(&chart, id),
        &screen,
        s.frame,
        active,
        &handles,
    );
    if let Some(pose) = &pose {
        draw_pose(painter, pose, colour(&chart, id), active);
    }
    if !editing {
        state.dragging = None;
        return false;
    }
    // Esc, like Done: back to the layer, the chart stays.
    if ui.input(|i| i.key_pressed(egui::Key::Escape)) && state.dragging.is_none() {
        s.selected_node = s.doc.layers.get(s.selected).map(|l| l.node);
        return true;
    }
    let anchor = here
        .map(|p| screen(p.pos))
        .or_else(|| b.mark(s.frame).map(|m| screen(m.pos)));
    // The bar goes above or below the point, wherever it hides less of
    // the chart (or under the needle when that's showing).
    let size = state.bar_size.unwrap_or(Vec2::new(420.0, 30.0));
    let bar_at = anchor.map(|at| {
        use egui::Align2;
        // Where the bar would really be: kept inside the canvas, as the
        // Area keeps it.
        let bounds = response.rect.shrink(8.0);
        let clamp = |r: egui::Rect| {
            let dx = (bounds.left() - r.left()).max(0.0) + (bounds.right() - r.right()).min(0.0);
            let dy = (bounds.top() - r.top()).max(0.0) + (bounds.bottom() - r.bottom()).min(0.0);
            r.translate(Vec2::new(dx, dy))
        };
        let covered = |r: egui::Rect| {
            b.path.iter().filter(|&&p| r.contains(screen(p))).count()
                + 8 * b
                    .chart
                    .points
                    .iter()
                    .filter(|p| r.expand(10.0).contains(screen(p.pos)))
                    .count()
                + pose.as_ref().map_or(0, |p| {
                    50 * [p.knob(), p.size()]
                        .iter()
                        .filter(|q| r.expand(10.0).contains(**q))
                        .count()
                })
                + usize::from(r.contains(at)) * 1000
        };
        // In order of preference: above, below, then beside the point.
        let gap = 26.0;
        [
            (at - Vec2::new(0.0, gap), Align2::CENTER_BOTTOM),
            (at + Vec2::new(0.0, gap), Align2::CENTER_TOP),
            (at + Vec2::new(gap, 0.0), Align2::LEFT_CENTER),
            (at - Vec2::new(gap, 0.0), Align2::RIGHT_CENTER),
        ]
        .into_iter()
        .min_by_key(|&(pos, pivot)| covered(clamp(pivot.anchor_size(pos, size))))
        .map(|(pos, pivot)| {
            // Place it where it was measured, clamped.
            let r = clamp(pivot.anchor_size(pos, size));
            (r.min, Align2::LEFT_TOP)
        })
        .unwrap_or((at, Align2::CENTER_TOP))
    });
    state.bar_size = Some(bar(
        ui,
        response,
        s,
        id,
        &chart,
        bar_at,
        &mut state.card_pending,
        &mut state.show_pose,
    ));
    if chart.locked {
        return true;
    }
    let back = m.inverse();
    if response.drag_started() {
        // What was under the press, not under the pointer now: a drag
        // only starts once the pointer has moved, by which time it may be
        // nearer the next tick.
        let origin = ui.input(|i| i.pointer.press_origin()).or(pointer);
        state.dragging = origin.and_then(grab_at).map(|g| (id, g));
    }
    if let Some((node, grab)) = state.dragging {
        if response.dragged() {
            if let (Some(p), Some(back)) = (response.interact_pointer_pos(), back) {
                let (x, y) = back.apply(p.x, p.y);
                let (alt, shift) = ui.input(|i| (i.modifiers.alt, i.modifiers.shift));
                match (grab, &pose) {
                    (Grab::Turn(_), Some(pose)) => {
                        let pos = chart.point(pose.pid).map_or([x, y], |q| q.pos);
                        let mut degrees = (y - pos[1]).atan2(x - pos[0]).to_degrees() - pose.base;
                        if shift {
                            degrees = (degrees / 15.0).round() * 15.0;
                        }
                        let frame = pose.frame;
                        edit(
                            s,
                            node,
                            "Turn chart",
                            Some(format!("chart-turn-{}-{frame}", node.0)),
                            |c| c.set_rotation(frame, degrees),
                        );
                    }
                    (Grab::Size(_), Some(pose)) => {
                        let along = (p - pose.at).dot(pose.dir) / SIZE_BASE;
                        let size = along.clamp(0.05, 20.0);
                        let frame = pose.frame;
                        edit(
                            s,
                            node,
                            "Scale chart",
                            Some(format!("chart-scale-{}-{frame}", node.0)),
                            |c| c.set_scale(frame, [size, size]),
                        );
                    }
                    (Grab::Turn(_) | Grab::Size(_), None) => {}
                    _ => drag(s, node, grab, [x, y], alt, &state.built[&node].curve),
                }
            }
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        }
        if response.drag_stopped() {
            state.dragging = None;
            s.end_gesture();
        }
        return true;
    }
    if response.clicked() {
        let alt = ui.input(|i| i.modifiers.alt);
        match (nearest, alt) {
            (Some(Grab::Point(pid)), true) => edit(s, id, "Remove chart point", None, |c| {
                c.remove_point(pid);
            }),
            (Some(Grab::Point(pid)), false) => {
                if let Some(p) = chart.point(pid) {
                    s.frame = p.frame;
                }
            }
            (Some(Grab::Tick(frame)), _) => s.frame = frame,
            (Some(Grab::Handle(..) | Grab::Turn(_) | Grab::Size(_)), _) => {}
            (None, false) => {
                if let (Some(p), Some(back)) = (pointer, back) {
                    let (x, y) = back.apply(p.x, p.y);
                    place(s, id, [x, y]);
                }
            }
            (None, true) => {}
        }
    }
    match nearest {
        Some(Grab::Tick(_)) => ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal),
        Some(_) => ui.ctx().set_cursor_icon(egui::CursorIcon::Grab),
        None if response.hovered() => ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair),
        None => {}
    }
    true
}

/// One step of a drag on chart `id`, the pointer at `to` (stage space).
/// Each drag merges into one undo step.
fn drag(s: &mut Session, id: NodeId, grab: Grab, to: [f32; 2], alt: bool, curve: &ChartCurve) {
    match grab {
        Grab::Point(pid) => edit(
            s,
            id,
            "Move chart point",
            Some(format!("chart-point-{}-{}", id.0, pid.0)),
            |c| {
                if let Some(p) = c.point_mut(pid) {
                    p.pos = to;
                }
            },
        ),
        Grab::Handle(pid, after) => edit(
            s,
            id,
            "Shape chart",
            Some(format!("chart-handle-{}-{}", id.0, pid.0)),
            |c| move_handle(c, pid, after, to, alt),
        ),
        // Measured on screen, in `ui`.
        Grab::Turn(_) | Grab::Size(_) => {}
        Grab::Tick(frame) => {
            let Some(d) = curve.segment_distance(frame, to) else {
                return;
            };
            edit(
                s,
                id,
                "Change spacing",
                Some(format!("chart-spacing-{}-{frame}", id.0)),
                |c| {
                    c.set_spacing(frame, d);
                },
            );
        }
    }
}

/// Point a handle of `pid` at `to`. The other handle turns to stay in
/// line (a smooth curve) unless `alt`, which breaks them apart.
pub fn move_handle(c: &mut Chart, pid: PointId, after: bool, to: [f32; 2], alt: bool) {
    if !matches!(
        c.point(pid).map(|p| p.tangent),
        Some(Tangent::Manual { .. })
    ) {
        c.freeze_handles(pid);
    }
    let Some(p) = c.point_mut(pid) else {
        return;
    };
    let offset = [to[0] - p.pos[0], to[1] - p.pos[1]];
    if let Tangent::Manual { before, after: out } = &mut p.tangent {
        let (this, other) = if after { (out, before) } else { (before, out) };
        *this = offset;
        let len = offset[0].hypot(offset[1]);
        if !alt && len > 1e-6 {
            let keep = other[0].hypot(other[1]).max(1e-3);
            *other = [-offset[0] / len * keep, -offset[1] / len * keep];
        }
    }
}

/// Place chart `id` at `pos` (stage space) on the current frame: a key
/// past either end, a breakdown between its points. It remembers the
/// drawing exposed there, for retiming later.
pub fn place(s: &mut Session, id: NodeId, pos: [f32; 2]) {
    let frame = s.frame;
    let drawing = s.doc.exposed(s.selected, frame);
    edit(s, id, "Place chart point", None, |c| {
        let role = match c.range() {
            Some((a, b)) if a < frame && frame < b && c.point_on(frame).is_none() => {
                Role::Breakdown
            }
            _ => Role::Key,
        };
        let pid = c.set_point(frame, pos, role);
        if let Some(p) = c.point_mut(pid) {
            p.drawing = drawing;
        }
    });
}

/// Change chart `id` as one undo step (merged by `merge` while a drag
/// goes on).
pub fn edit(
    s: &mut Session,
    id: NodeId,
    name: &str,
    merge: Option<String>,
    f: impl FnOnce(&mut Chart),
) {
    let Some(before) = s.doc.graph.node(id).cloned() else {
        return;
    };
    let mut after = before.clone();
    let NodeKind::Chart(c) = &mut after.kind else {
        return;
    };
    f(c);
    if after == before {
        return;
    }
    let e = Edit::SetNodeProperties { id, before, after };
    match merge {
        Some(key) => s.history.apply_merged(&mut s.doc, name, &key, e),
        None => s.history.apply(&mut s.doc, name, vec![e]),
    }
}

fn near_path(path: &[[f32; 2]], screen: &impl Fn([f32; 2]) -> Pos2, p: Pos2) -> bool {
    let pts: Vec<Pos2> = path.iter().map(|&q| screen(q)).collect();
    match pts.as_slice() {
        [only] => only.distance(p) <= GRAB,
        _ => pts
            .windows(2)
            .any(|w| distance_to_segment(p, w[0], w[1]) <= PATH_HIT),
    }
}

fn distance_to_segment(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let len2 = ab.length_sq();
    let t = if len2 > 1e-6 {
        ((p - a).dot(ab) / len2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    p.distance(a + ab * t)
}

fn path_shape(b: &Built, screen: &impl Fn([f32; 2]) -> Pos2) -> Vec<Pos2> {
    b.path.iter().map(|&p| screen(p)).collect()
}

/// An unselected chart: its path and its points, thin and quiet.
fn draw_quiet(
    painter: &egui::Painter,
    b: &Built,
    colour: Color32,
    screen: &impl Fn([f32; 2]) -> Pos2,
) {
    let faint = colour.gamma_multiply(0.7);
    let path = path_shape(b, screen);
    painter.add(egui::Shape::line(
        path.iter().map(|p| *p + Vec2::new(0.0, 1.0)).collect(),
        EStroke::new(2.0, Color32::from_black_alpha(40)),
    ));
    painter.add(egui::Shape::line(path, EStroke::new(1.0, faint)));
    let ring = EStroke::new(1.0, colour.gamma_multiply(0.75));
    for p in &b.chart.points {
        let at = screen(p.pos);
        match p.role {
            Role::Key => {
                painter.circle_stroke(at, 3.0, ring);
            }
            Role::Breakdown => {
                painter.add(egui::Shape::closed_line(triangle(at, 4.0), ring));
            }
        }
    }
}

/// An upward triangle around `at`, `r` from its centre to each corner.
fn triangle(at: Pos2, r: f32) -> Vec<Pos2> {
    vec![
        at + Vec2::new(0.0, -r),
        at + Vec2::new(r * 0.866, r * 0.5),
        at + Vec2::new(-r * 0.866, r * 0.5),
    ]
}

/// Text that reads on `fill`: dark on a light colour, white on a dark one.
fn ink_on(fill: Color32) -> Color32 {
    let luma =
        0.299 * f32::from(fill.r()) + 0.587 * f32::from(fill.g()) + 0.114 * f32::from(fill.b());
    if luma > 150.0 {
        theme::BG_BASE
    } else {
        theme::ON_ACCENT
    }
}

/// A chart point as on a paper timing chart, on glass: a key is its frame
/// number in a circle, a breakdown its number in a triangle. The point on
/// the current frame is filled with the chart's colour.
fn point_chip(
    painter: &egui::Painter,
    at: Pos2,
    point: &symphony_core::chart::ChartPoint,
    colour: Color32,
    current: bool,
    hot: bool,
) {
    let (fill, ink) = if current {
        (colour, ink_on(colour))
    } else {
        (HUD, theme::TEXT_PRIMARY)
    };
    let font = egui::FontId::proportional(10.5);
    let galley = painter.layout_no_wrap(point.frame.to_string(), font, ink);
    let grow = if hot { 1.5 } else { 0.0 };
    let rim = EStroke::new(1.25, colour);
    let shadow = Color32::from_black_alpha(56);
    if current {
        painter.circle_filled(
            at,
            galley.size().x / 2.0 + 10.0 + grow,
            colour.gamma_multiply(0.22),
        );
    }
    match point.role {
        Role::Key => {
            let r = (galley.size().x / 2.0 + 4.5).max(8.5) + grow;
            painter.circle_filled(at + Vec2::new(0.0, 1.5), r + 1.0, shadow);
            painter.circle_filled(at, r, fill);
            painter.circle_stroke(at, r - 1.5, EStroke::new(1.0, HUD_RIM));
            painter.circle_stroke(at, r, rim);
            painter.galley(at - galley.size() / 2.0, galley, ink);
        }
        Role::Breakdown => {
            let r = (galley.size().x / 2.0 + 6.0).max(11.0) + grow;
            let shape = triangle(at, r);
            painter.add(egui::Shape::convex_polygon(
                triangle(at + Vec2::new(0.0, 1.5), r + 1.0),
                shadow,
                EStroke::NONE,
            ));
            painter.add(egui::Shape::convex_polygon(shape, fill, rim));
            let centre = at + Vec2::new(0.0, r * 0.14);
            painter.galley(centre - galley.size() / 2.0, galley, ink);
        }
    }
}

/// The needle, turn knob and size square of the point on this frame.
fn draw_pose(painter: &egui::Painter, pose: &Pose, colour: Color32, active: Option<Grab>) {
    painter.line_segment(
        [pose.at, pose.knob()],
        EStroke::new(1.0, colour.gamma_multiply(0.7)),
    );
    let hot = |g: Grab| active == Some(g);
    let r = if hot(Grab::Turn(pose.pid)) { 6.5 } else { 5.5 };
    painter.circle_filled(
        pose.knob() + Vec2::new(0.0, 1.0),
        r + 1.0,
        Color32::from_black_alpha(56),
    );
    painter.circle_filled(pose.knob(), r, HUD);
    painter.circle_stroke(pose.knob(), r, EStroke::new(1.25, colour));
    painter.circle_filled(pose.knob(), 1.75, colour);
    let half = if hot(Grab::Size(pose.pid)) { 5.0 } else { 4.0 };
    let square = egui::Rect::from_center_size(pose.size(), Vec2::splat(half * 2.0));
    painter.rect(
        square,
        1.5,
        HUD,
        EStroke::new(1.25, colour),
        egui::StrokeKind::Inside,
    );
}

fn draw_selected(
    painter: &egui::Painter,
    b: &Built,
    colour: Color32,
    screen: &impl Fn([f32; 2]) -> Pos2,
    frame: u32,
    active: Option<Grab>,
    handles: &[(PointId, bool, [f32; 2], [f32; 2])],
) {
    // A hidden chart still shows while selected, dimmed, so it can be
    // found and shown again.
    let colour = if b.chart.visible {
        colour
    } else {
        colour.gamma_multiply(0.45)
    };
    // A thin line lifted off the artwork by a soft shadow, like the
    // glass chips: readable on the light stage and on dark artwork.
    let path = path_shape(b, screen);
    let shadow: Vec<Pos2> = path.iter().map(|p| *p + Vec2::new(0.0, 1.5)).collect();
    painter.add(egui::Shape::line(
        shadow.clone(),
        EStroke::new(5.0, Color32::from_black_alpha(28)),
    ));
    painter.add(egui::Shape::line(
        shadow,
        EStroke::new(2.5, Color32::from_black_alpha(70)),
    ));
    painter.add(egui::Shape::line(path, EStroke::new(1.75, colour)));
    // Spacing: a short tick across the path on every frame, as on paper.
    for mark in &b.marks {
        if b.chart.point_on(mark.frame).is_some() {
            continue;
        }
        let p = screen(mark.pos);
        let n = normal(screen, mark);
        let (long, width) = if active == Some(Grab::Tick(mark.frame)) {
            (7.0, 2.0)
        } else {
            (4.0, 1.25)
        };
        let drop = Vec2::new(0.0, 1.5);
        painter.line_segment(
            [p - n * long + drop, p + n * long + drop],
            EStroke::new(width + 1.25, Color32::from_black_alpha(70)),
        );
        painter.line_segment([p - n * long, p + n * long], EStroke::new(width, colour));
    }
    // The current point's handles: a line out to a small glass dot.
    for &(pid, after, from, to) in handles {
        let (a, h) = (screen(from), screen(to));
        painter.line_segment([a, h], EStroke::new(1.0, colour.gamma_multiply(0.6)));
        let r = if active == Some(Grab::Handle(pid, after)) {
            4.5
        } else {
            3.5
        };
        painter.circle_filled(h, r, Color32::WHITE);
        painter.circle_stroke(h, r, EStroke::new(1.25, colour));
    }
    // Now, between points: an amber ring (amber is time, as on the
    // Timeline) with a soft glow.
    if b.chart.point_on(frame).is_none() {
        if let Some(mark) = b.mark(frame) {
            let p = screen(mark.pos);
            painter.circle_filled(p, 10.0, theme::ACCENT_TIME.gamma_multiply(0.18));
            painter.circle_stroke(p, 6.5, EStroke::new(2.0, theme::ACCENT_TIME));
        }
    }
    for point in &b.chart.points {
        point_chip(
            painter,
            screen(point.pos),
            point,
            colour,
            point.frame == frame,
            active == Some(Grab::Point(point.id)),
        );
    }
}

/// The screen direction across the path at `mark` (unit length).
fn normal(screen: &impl Fn([f32; 2]) -> Pos2, mark: &Mark) -> Vec2 {
    let a = screen(mark.pos);
    let b = screen([mark.pos[0] + mark.dir[0], mark.pos[1] + mark.dir[1]]);
    let d = (b - a).normalized();
    let d = if d.is_finite() && d != Vec2::ZERO {
        d
    } else {
        Vec2::X
    };
    Vec2::new(-d.y, d.x)
}

/// The point bar: a small glass pill under the point on this frame (or
/// where the chart is now) with what an animator reaches for most, and
/// the rest behind More. The full set is in Node Properties too.
#[allow(clippy::too_many_arguments)]
fn bar(
    ui: &egui::Ui,
    response: &egui::Response,
    s: &mut Session,
    id: NodeId,
    chart: &Chart,
    place: Option<(Pos2, egui::Align2)>,
    pending: &mut bool,
    show_pose: &mut bool,
) -> Vec2 {
    let here = chart.point_on(s.frame).map(|p| p.id);
    let (pos, pivot) = place.unwrap_or((
        response.rect.center_bottom() - Vec2::new(0.0, 14.0),
        egui::Align2::CENTER_BOTTOM,
    ));
    // Concentric: the bar is a capsule whose controls are capsules set
    // INSET in from every edge, so each inner curve shares its centre
    // with the outer one (inner radius = outer radius - inset).
    let frame = egui::Frame::new()
        .fill(HUD)
        .stroke(EStroke::new(1.0, HUD_RIM))
        .corner_radius(BAR_RADIUS)
        .inner_margin(egui::Margin::same(INSET as i8))
        .shadow(egui::epaint::Shadow {
            offset: [0, 4],
            blur: 14,
            spread: 0,
            color: Color32::from_black_alpha(70),
        });
    let shown = egui::Area::new(egui::Id::unique("smart-chart-bar"))
        .order(egui::Order::Foreground)
        .pivot(pivot)
        .fixed_pos(pos)
        .constrain_to(response.rect.shrink(8.0))
        .show(ui.ctx(), |ui| {
            frame.show(ui, |ui| {
                // Glass-lite controls: a faint light fill, so each one
                // reads as something to press, on or off; capsules of
                // CONTROL height.
                let control = Color32::from_white_alpha(20);
                let v = ui.visuals_mut();
                v.selection.bg_fill = theme::ACCENT_SELECT_FILL;
                v.selection.stroke.color = theme::ON_ACCENT;
                for w in [
                    &mut v.widgets.inactive,
                    &mut v.widgets.hovered,
                    &mut v.widgets.active,
                    &mut v.widgets.open,
                ] {
                    w.corner_radius = CONTROL_RADIUS.into();
                    w.expansion = 0.0;
                }
                v.widgets.inactive.weak_bg_fill = control;
                v.widgets.inactive.bg_fill = control;
                let sp = ui.spacing_mut();
                sp.button_padding = Vec2::new(10.0, 0.0);
                sp.interact_size.y = CONTROL;
                sp.item_spacing = Vec2::splat(INSET);
                ui.horizontal(|ui| {
                    ui.set_min_height(CONTROL);
                    let mut draft = chart.clone();
                    if chart.locked {
                        hint(ui, "Locked");
                        if ui.small_button("Unlock").clicked() {
                            draft.locked = false;
                        }
                    } else if let Some(pid) = here {
                        bar_controls(ui, &mut draft, pid, show_pose);
                    } else if chart.points.is_empty() {
                        hint(
                            ui,
                            &format!("Click the artwork to start on frame {}", s.frame),
                        );
                    } else {
                        hint(ui, &format!("Click to place frame {}", s.frame));
                    }
                    let mut done = false;
                    ui.menu_button("More", |ui| {
                        more_menu(ui, s, id, &mut draft, here, &mut done);
                    });
                    if draft != *chart {
                        let key = here.map_or_else(
                            || format!("chart-bar-{}", id.0),
                            |pid| format!("chart-card-{}-{}", id.0, pid.0),
                        );
                        edit(s, id, "Change chart point", Some(key), |c| *c = draft);
                        *pending = true;
                    }
                    if done {
                        // Back to the layer: the chart stays, quietly.
                        s.selected_node = s.doc.layers.get(s.selected).map(|l| l.node);
                    }
                });
            });
        });
    // A value drag is one step; each click is its own.
    if *pending && !ui.input(|i| i.pointer.any_down()) {
        s.end_gesture();
        *pending = false;
    }
    shown.response.rect.size()
}

/// Bar geometry (concentric capsules): controls CONTROL tall with radius
/// CONTROL_RADIUS, INSET from every edge of the bar and from each other;
/// the bar's radius is theirs plus INSET.
const CONTROL: f32 = 22.0;
const INSET: f32 = 4.0;
const CONTROL_RADIUS: u8 = 11;
const BAR_RADIUS: u8 = theme::around(CONTROL_RADIUS, INSET as u8);
/// The switch's sliding highlight sits this far inside its track.
const SWITCH_INSET: f32 = 2.0;

/// A two-way switch as a capsule track with a capsule highlight inset in
/// it (concentric), sliding between the two (two-way switches slide).
fn pill_switch<T: PartialEq + Copy>(
    ui: &mut egui::Ui,
    salt: &str,
    current: &mut T,
    options: [(T, &str); 2],
) -> bool {
    let font = theme::body();
    let galleys = options.map(|(_, label)| {
        ui.painter()
            .layout_no_wrap(label.to_owned(), font.clone(), theme::TEXT_PRIMARY)
    });
    let widths = galleys.each_ref().map(|g| g.size().x + 20.0);
    let (track, _) = ui.allocate_exact_size(
        Vec2::new(widths[0] + widths[1], CONTROL),
        egui::Sense::hover(),
    );
    let painter = ui.painter();
    painter.rect_filled(track, CONTROL_RADIUS, Color32::from_white_alpha(20));
    let halves = [
        egui::Rect::from_min_size(track.min, Vec2::new(widths[0], CONTROL)),
        egui::Rect::from_min_size(
            track.min + Vec2::new(widths[0], 0.0),
            Vec2::new(widths[1], CONTROL),
        ),
    ];
    let mut changed = false;
    let mut chosen = None;
    for (i, ((value, label), half)) in options.iter().zip(halves).enumerate() {
        let response = ui
            .interact(half, ui.scope_id().with((salt, i)), egui::Sense::click())
            .on_hover_cursor(egui::CursorIcon::PointingHand);
        response.widget_info(|| {
            egui::WidgetInfo::selected(egui::Role::RadioButton, true, *current == *value, *label)
        });
        if response.clicked() && *current != *value {
            *current = *value;
            changed = true;
        }
        if *current == *value {
            chosen = Some(half);
        }
    }
    if let Some(half) = chosen {
        let origin = track.min.to_vec2();
        let r = symphony_ui::motion::slide(
            ui.ctx(),
            ui.scope_id().with((salt, "highlight")),
            half.shrink(SWITCH_INSET).translate(-origin),
        )
        .translate(origin);
        ui.painter().rect_filled(
            r,
            theme::inside(CONTROL_RADIUS, SWITCH_INSET as u8),
            theme::ACCENT_SELECT_FILL,
        );
    }
    for (((value, _), half), galley) in options.iter().zip(halves).zip(galleys) {
        let ink = if *current == *value {
            theme::ON_ACCENT
        } else {
            theme::TEXT_PRIMARY
        };
        ui.painter()
            .galley(half.center() - galley.size() / 2.0, galley, ink);
    }
    changed
}

/// A round icon button, CONTROL across (a capsule's end, concentric with
/// the bar's).
fn pill_icon(
    ui: &mut egui::Ui,
    icon: symphony_ui::icons::Icon,
    on: bool,
    tip: &str,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(CONTROL), egui::Sense::click());
    let fill = if on {
        theme::ACCENT_SELECT_FILL
    } else if response.hovered() {
        Color32::from_white_alpha(34)
    } else {
        Color32::from_white_alpha(20)
    };
    ui.painter()
        .circle_filled(rect.center(), CONTROL / 2.0, fill);
    let ink = if on {
        theme::ON_ACCENT
    } else {
        theme::TEXT_PRIMARY
    };
    symphony_ui::icons::paint(
        ui.painter(),
        egui::Rect::from_center_size(rect.center(), Vec2::splat(14.0)),
        icon,
        ink,
    );
    response.on_hover_text(tip)
}

fn hint(ui: &mut egui::Ui, text: &str) {
    // Text sits a capsule's padding in from the bar's round end.
    ui.add_space(6.0);
    ui.label(
        egui::RichText::new(text)
            .font(theme::dense())
            .color(theme::TEXT_SECONDARY),
    );
}

/// The bar's everyday controls for the point `pid`.
fn bar_controls(ui: &mut egui::Ui, c: &mut Chart, pid: PointId, show_pose: &mut bool) {
    let Some(i) = c.points.iter().position(|p| p.id == pid) else {
        return;
    };
    let (has_prev, next) = (i > 0, c.points.get(i + 1).map(|p| p.frame));
    let mut role = c.points[i].role;
    if pill_switch(
        ui,
        "chart-bar-role",
        &mut role,
        [(Role::Key, "Key"), (Role::Breakdown, "Breakdown")],
    ) {
        c.points[i].role = role;
    }
    let ease = |ui: &mut egui::Ui, label: &str, value: &mut f32, tip: &str| {
        let mut p = (*value * 100.0).round();
        if ui
            .add(
                egui::DragValue::new(&mut p)
                    .range(0.0..=100.0)
                    .speed(1.0)
                    .prefix(label)
                    .suffix("%"),
            )
            .on_hover_text(tip)
            .changed()
        {
            *value = p / 100.0;
        }
    };
    if has_prev {
        ease(
            ui,
            "In ",
            &mut c.points[i - 1].timing.ease_in,
            "Slow in: frames bunch up arriving here.",
        );
    }
    if let Some(next) = next {
        ease(
            ui,
            "Out ",
            &mut c.points[i].timing.ease_out,
            "Slow out: frames bunch up leaving here.",
        );
        let p = &mut c.points[i];
        if ui
            .add(egui::Button::new("Hold").selected(p.timing.hold))
            .on_hover_text(format!("Stay here until frame {next}, then jump."))
            .clicked()
        {
            p.timing.hold = !p.timing.hold;
        }
        let straight = p.path == PathKind::Line;
        if ui
            .add(egui::Button::new("Line").selected(straight))
            .on_hover_text(format!("A straight line to frame {next}."))
            .clicked()
        {
            p.path = if straight {
                PathKind::Curve
            } else {
                PathKind::Line
            };
        }
    }
    if pill_icon(
        ui,
        symphony_ui::icons::Icon::RotateRight,
        *show_pose,
        "Rotate and scale on this frame",
    )
    .clicked()
    {
        *show_pose = !*show_pose;
    }
}

/// Everything else: the point's shape and exact rotation and scale, even
/// spacing, what the chart drives, and Done.
fn more_menu(
    ui: &mut egui::Ui,
    s: &mut Session,
    id: NodeId,
    c: &mut Chart,
    here: Option<PointId>,
    done: &mut bool,
) {
    ui.set_min_width(220.0);
    if let (Some(pid), false) = (here, c.locked) {
        point_controls(ui, c, pid);
        let spaced = c.points.iter().position(|p| p.id == pid).is_some_and(|i| {
            [i.checked_sub(1), Some(i)]
                .into_iter()
                .flatten()
                .filter_map(|j| c.points.get(j))
                .any(|p| {
                    !p.timing.spacing.is_empty()
                        || p.timing.ease_in > 0.0
                        || p.timing.ease_out > 0.0
                })
        });
        if ui
            .add_enabled(spaced, egui::Button::new("Even spacing"))
            .on_hover_text("Clear the eases and slid ticks on both sides of this point.")
            .clicked()
        {
            c.even_spacing(pid);
        }
        ui.separator();
    }
    colour_swatches(ui, c, id);
    follow_toggle(ui, s, id);
    hint(
        ui,
        "Alt-drag a handle for a sharp turn. Alt-click removes a point.",
    );
    if widgets::primary_button(ui, "Done", true)
        .on_hover_text("Back to the layer (Esc). The chart stays.")
        .clicked()
    {
        *done = true;
        ui.close();
    }
}

/// "<layer> follows this chart": the selected layer's peg on the chart.
pub fn follow_toggle(ui: &mut egui::Ui, s: &mut Session, id: NodeId) {
    if s.layer_peg(s.selected).is_none() {
        return;
    }
    let name = s
        .doc
        .column(s.selected)
        .map_or_else(String::new, |c| c.name.clone());
    let mut follows = s.layer_chart(s.selected) == Some(id);
    if widgets::checkbox(ui, &mut follows, &format!("{name} follows this chart"))
        .on_hover_text("The layer's peg moves along the chart.")
        .changed()
    {
        s.set_layer_chart(s.selected, follows.then_some(id));
    }
}

/// The point `pid`'s settings: key or breakdown, its shape, the eases
/// into and out of it, and what the segment after it does.
pub fn point_controls(ui: &mut egui::Ui, c: &mut Chart, pid: PointId) {
    let Some(i) = c.points.iter().position(|p| p.id == pid) else {
        return;
    };
    let (has_prev, next) = (i > 0, c.points.get(i + 1).map(|p| p.frame));
    let mut role = c.points[i].role;
    if widgets::segmented(
        ui,
        "chart-role",
        &mut role,
        &[(Role::Key, "Key"), (Role::Breakdown, "Breakdown")],
        false,
    ) {
        c.points[i].role = role;
    }
    #[derive(Clone, Copy, PartialEq)]
    enum Shape {
        Smooth,
        Corner,
        Handles,
    }
    let shape = match c.points[i].tangent {
        Tangent::Auto => Shape::Smooth,
        Tangent::Corner => Shape::Corner,
        Tangent::Manual { .. } => Shape::Handles,
    };
    let mut chosen = shape;
    ui.horizontal(|ui| {
        widgets::radio_value(ui, &mut chosen, Shape::Smooth, "Smooth")
            .on_hover_text("The curve flows through this point on its own.");
        widgets::radio_value(ui, &mut chosen, Shape::Corner, "Corner")
            .on_hover_text("A sharp turn here.");
        widgets::radio_value(ui, &mut chosen, Shape::Handles, "Handles")
            .on_hover_text("Shape the curve here with handles.");
    });
    if chosen != shape {
        match chosen {
            Shape::Smooth => c.points[i].tangent = Tangent::Auto,
            Shape::Corner => c.points[i].tangent = Tangent::Corner,
            Shape::Handles => c.freeze_handles(pid),
        }
    }
    let percent = |ui: &mut egui::Ui, label: &str, value: &mut f32, tip: &str| {
        let mut p = *value * 100.0;
        if widgets::precise_slider(
            ui,
            egui::Slider::new(&mut p, 0.0..=100.0)
                .step_by(1.0)
                .text(label)
                .suffix("%"),
        )
        .on_hover_text(tip)
        .changed()
        {
            *value = p / 100.0;
        }
    };
    if has_prev {
        percent(
            ui,
            "Slow in",
            &mut c.points[i - 1].timing.ease_in,
            "Frames bunch up arriving here.",
        );
    }
    if let Some(next) = next {
        percent(
            ui,
            "Slow out",
            &mut c.points[i].timing.ease_out,
            "Frames bunch up leaving here.",
        );
        let p = &mut c.points[i];
        ui.horizontal(|ui| {
            widgets::checkbox(ui, &mut p.timing.hold, "Hold")
                .on_hover_text(format!("Stay here until frame {next}, then jump."));
            let mut straight = p.path == PathKind::Line;
            if widgets::checkbox(ui, &mut straight, "Straight")
                .on_hover_text(format!("A straight line to frame {next}."))
                .changed()
            {
                p.path = if straight {
                    PathKind::Line
                } else {
                    PathKind::Curve
                };
            }
        });
    }
    // The chart's turn and size on this point's frame, typed exactly
    // (720° is two turns).
    let frame = c.points[i].frame;
    ui.horizontal(|ui| {
        ui.label("Rotation");
        let mut turn = c.rotation.at(frame);
        if ui
            .add(egui::DragValue::new(&mut turn).speed(1.0).suffix("°"))
            .on_hover_text("Rotation on this frame. Drag the round knob on the canvas.")
            .changed()
        {
            c.set_rotation_turns(frame, turn);
        }
        ui.label("Scale");
        let mut size = c.scale[0].at(frame) * 100.0;
        if ui
            .add(
                egui::DragValue::new(&mut size)
                    .speed(1.0)
                    .range(5.0..=2000.0)
                    .suffix("%"),
            )
            .on_hover_text("Scale on this frame. Drag the square on the canvas.")
            .changed()
        {
            c.set_scale(frame, [size / 100.0, size / 100.0]);
        }
    });
    // 3D: tilt about X and Y on this frame, as a peg's 3D rotation.
    ui.horizontal(|ui| {
        ui.label("Tilt");
        let mut tilt = c.tilt_at(frame);
        let mut changed = false;
        for (axis, value) in ["X ", "Y "].into_iter().zip(tilt.iter_mut()) {
            changed |= ui
                .add(
                    egui::DragValue::new(value)
                        .speed(1.0)
                        .prefix(axis)
                        .suffix("°"),
                )
                .on_hover_text("3D tilt on this frame. A following peg tilts with it.")
                .changed();
        }
        if changed {
            c.set_tilt(frame, tilt);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// New charts take the colours in order (the first is always lilac),
    /// skipping ones in use; a freed colour is reused first.
    #[test]
    fn new_charts_take_colours_in_order() {
        let mut s = Session::new_scene();
        let first = new_chart(&mut s);
        let second = new_chart(&mut s);
        let rgb = |s: &Session, id| chart_of(s, id).colour;
        assert_eq!(rgb(&s, first), Some(COLOURS[0].1));
        assert_eq!(rgb(&s, second), Some(COLOURS[1].1));
        crate::views::node::delete_nodes(&mut s, &[first]);
        let third = new_chart(&mut s);
        assert_eq!(rgb(&s, third), Some(COLOURS[0].1), "lilac was free again");
    }

    fn chart_of(s: &Session, id: NodeId) -> Chart {
        match &s.doc.graph.node(id).unwrap().kind {
            NodeKind::Chart(c) => (**c).clone(),
            _ => unreachable!(),
        }
    }

    fn new_chart(s: &mut Session) -> NodeId {
        let mut state = crate::views::node::NodeViewState::default();
        crate::views::node::add_from_search(s, &mut state, NodeKind::Chart(Box::default()));
        s.selected_node.unwrap()
    }

    /// Search adds a chart on its own (wired to nothing), selects it and
    /// picks the Transform tool so the next click places a point.
    #[test]
    fn a_chart_from_search_is_selected_with_transform() {
        let mut s = Session::new_scene();
        let links = s.doc.graph.links().to_vec();
        let id = new_chart(&mut s);
        assert!(matches!(
            s.doc.graph.node(id).unwrap().kind,
            NodeKind::Chart(_)
        ));
        assert_eq!(s.tool, Tool::Transform);
        assert_eq!(s.doc.graph.links(), links.as_slice());
    }

    /// Frame 1 and frame 9 are keys; a point placed between them is a
    /// breakdown; placing on a frame that has one moves it.
    #[test]
    fn placing_points_makes_keys_then_breakdowns() {
        let mut s = Session::new_scene();
        let id = new_chart(&mut s);
        s.frame = 1;
        place(&mut s, id, [0.0, 0.0]);
        s.frame = 9;
        place(&mut s, id, [80.0, 0.0]);
        s.frame = 5;
        place(&mut s, id, [40.0, 30.0]);
        let c = chart_of(&s, id);
        let roles: Vec<(u32, Role)> = c.points.iter().map(|p| (p.frame, p.role)).collect();
        assert_eq!(
            roles,
            vec![(1, Role::Key), (5, Role::Breakdown), (9, Role::Key)]
        );
        place(&mut s, id, [40.0, 50.0]);
        let c = chart_of(&s, id);
        assert_eq!(c.points.len(), 3);
        assert_eq!(c.point_on(5).unwrap().pos, [40.0, 50.0]);
        s.undo();
        assert_eq!(chart_of(&s, id).point_on(5).unwrap().pos, [40.0, 30.0]);
    }

    /// Dragging a handle freezes the point's shape and turns the other
    /// handle to stay in line; Alt breaks them apart.
    #[test]
    fn a_handle_drag_keeps_the_curve_smooth_unless_alt() {
        let mut c = Chart::default();
        c.set_point(1, [0.0, 0.0], Role::Key);
        let mid = c.set_point(7, [60.0, -40.0], Role::Key);
        c.set_point(13, [120.0, 0.0], Role::Key);
        move_handle(&mut c, mid, true, [90.0, -40.0], false);
        let Tangent::Manual { before, after } = c.point(mid).unwrap().tangent else {
            panic!("handles are frozen");
        };
        assert_eq!(after, [30.0, 0.0]);
        assert!(before[0] < 0.0 && before[1].abs() < 1e-4, "{before:?}");
        move_handle(&mut c, mid, true, [60.0, -10.0], true);
        let Tangent::Manual {
            before: kept,
            after,
        } = c.point(mid).unwrap().tangent
        else {
            unreachable!()
        };
        assert_eq!(after, [0.0, 30.0]);
        assert_eq!(kept, before, "Alt leaves the other handle alone");
    }

    /// Sliding a tick changes where the chart is on that frame, as one
    /// undo step, and leaves the path alone.
    #[test]
    fn sliding_a_tick_respaces_that_frame() {
        let mut s = Session::new_scene();
        let id = new_chart(&mut s);
        s.frame = 1;
        place(&mut s, id, [0.0, 0.0]);
        s.frame = 11;
        place(&mut s, id, [100.0, 0.0]);
        let curve = ChartCurve::new(&chart_of(&s, id));
        for x in [40.0, 30.0, 25.0] {
            drag(&mut s, id, Grab::Tick(6), [x, 3.0], false, &curve);
        }
        s.end_gesture();
        let after = ChartCurve::new(&chart_of(&s, id));
        assert!((after.position(6.0).unwrap()[0] - 25.0).abs() < 0.5);
        assert_eq!(after.path(8), curve.path(8));
        s.undo();
        let back = ChartCurve::new(&chart_of(&s, id)).position(6.0).unwrap();
        assert!((back[0] - 50.0).abs() < 0.5, "one step: {back:?}");
    }

    /// A whole drag of a point is one undo step.
    #[test]
    fn dragging_a_point_is_one_undo_step() {
        let mut s = Session::new_scene();
        let id = new_chart(&mut s);
        place(&mut s, id, [0.0, 0.0]);
        let pid = chart_of(&s, id).points[0].id;
        for i in 1..=20 {
            edit(
                &mut s,
                id,
                "Move chart point",
                Some(format!("chart-point-{}-{}", id.0, pid.0)),
                |c| c.point_mut(pid).unwrap().pos = [i as f32, 0.0],
            );
        }
        s.end_gesture();
        assert_eq!(chart_of(&s, id).points[0].pos, [20.0, 0.0]);
        s.undo();
        assert_eq!(chart_of(&s, id).points[0].pos, [0.0, 0.0]);
    }

    /// An unchanged scene rebuilds no chart; editing one chart rebuilds
    /// that one only.
    #[test]
    fn charts_rebuild_only_when_they_change() {
        let mut s = Session::new_scene();
        let a = new_chart(&mut s);
        let b = new_chart(&mut s);
        place(&mut s, a, [0.0, 0.0]);
        place(&mut s, b, [10.0, 0.0]);
        let mut state = ChartEdit::default();
        state.refresh(&s);
        let marks = |st: &ChartEdit, id| st.built[&id].path.as_ptr();
        let (pa, pb) = (marks(&state, a), marks(&state, b));
        state.refresh(&s);
        assert_eq!((marks(&state, a), marks(&state, b)), (pa, pb));
        s.frame = 9;
        place(&mut s, a, [50.0, 0.0]);
        state.refresh(&s);
        assert_ne!(marks(&state, a), pa, "the edited chart is rebuilt");
        assert_eq!(marks(&state, b), pb, "the other chart is kept");
        assert_eq!(state.built[&a].marks.len(), 9, "a tick on every frame");
    }
}
