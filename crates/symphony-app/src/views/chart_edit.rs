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

/// Charts without a colour of their own take one of these, by node, so
/// several charts on one scene stay told apart (and keep their colour).
/// No amber: that's the current frame's colour.
const COLOURS: [[u8; 3]; 6] = [
    [240, 122, 160],
    [110, 196, 240],
    [150, 214, 110],
    [226, 118, 226],
    [186, 150, 240],
    [96, 214, 196],
];

/// The colour chart `id` is drawn in.
pub fn colour(chart: &Chart, id: NodeId) -> Color32 {
    let [r, g, b] = chart.colour.unwrap_or(COLOURS[id.0 % COLOURS.len()]);
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
}

#[derive(Default)]
pub struct ChartEdit {
    /// What's being dragged, on which chart.
    dragging: Option<(NodeId, Grab)>,
    /// A change from the point card not yet closed as an undo step (a
    /// slider still held).
    card_pending: bool,
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

/// Draw every visible chart and edit the selected one. Returns true while
/// a chart takes the Transform tool's clicks (so they don't select or move
/// artwork).
pub fn ui(
    ui: &egui::Ui,
    response: &egui::Response,
    painter: &egui::Painter,
    s: &mut Session,
    state: &mut ChartEdit,
    view: Affine,
) -> bool {
    state.refresh(s);
    if state.built.is_empty() {
        state.dragging = None;
        return false;
    }
    let Some(plane) = s.doc.project_plane(s.frame, 0.0) else {
        return false;
    };
    let m = view.compose(plane);
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
    let handles = if editing && !chart.locked {
        b.handles_on(s.frame)
    } else {
        Vec::new()
    };
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
    if !editing {
        state.dragging = None;
        return false;
    }
    card(ui, response, s, id, &chart, &mut state.card_pending);
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
                let alt = ui.input(|i| i.modifiers.alt);
                drag(s, node, grab, [x, y], alt, &state.built[&node].curve);
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
            (Some(Grab::Handle(..)), _) => {}
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
    let faint = colour.gamma_multiply(0.55);
    painter.add(egui::Shape::line(
        path_shape(b, screen),
        EStroke::new(1.25, faint),
    ));
    for p in &b.chart.points {
        painter.circle_filled(screen(p.pos), 2.5, faint);
    }
}

/// The selected chart in full: path, a tick per frame, points with their
/// frame numbers, and where it is now.
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
    let path = path_shape(b, screen);
    painter.add(egui::Shape::line(
        path.clone(),
        EStroke::new(4.0, theme::BG_BASE.gamma_multiply(0.6)),
    ));
    painter.add(egui::Shape::line(path, EStroke::new(2.0, colour)));
    // Spacing: one tick across the path on every frame.
    // A dark edge under each tick keeps it readable on light artwork.
    let (edge, tick) = (
        EStroke::new(3.5, theme::BG_BASE.gamma_multiply(0.6)),
        EStroke::new(1.5, colour),
    );
    for mark in &b.marks {
        if b.chart.point_on(mark.frame).is_some() {
            continue;
        }
        let p = screen(mark.pos);
        let n = normal(screen, mark);
        let long = if active == Some(Grab::Tick(mark.frame)) {
            8.0
        } else {
            5.0
        };
        painter.line_segment([p - n * (long + 1.0), p + n * (long + 1.0)], edge);
        painter.line_segment([p - n * long, p + n * long], tick);
    }
    // The current point's handles: a line out to a small square.
    for &(pid, after, from, to) in handles {
        let (a, h) = (screen(from), screen(to));
        painter.line_segment(
            [a, h],
            EStroke::new(1.0, theme::TEXT_PRIMARY.gamma_multiply(0.8)),
        );
        let size = if active == Some(Grab::Handle(pid, after)) {
            9.0
        } else {
            7.0
        };
        let r = egui::Rect::from_center_size(h, Vec2::splat(size));
        painter.rect_filled(r.expand(1.0), 1.0, theme::BG_BASE);
        painter.rect_filled(r, 1.0, theme::TEXT_PRIMARY);
    }
    for point in &b.chart.points {
        let p = screen(point.pos);
        let big = active == Some(Grab::Point(point.id));
        let r = if big { 6.5 } else { 5.0 };
        painter.circle_filled(p, r + 1.5, theme::BG_BASE);
        match point.role {
            Role::Key => {
                painter.circle_filled(p, r, colour);
            }
            Role::Breakdown => {
                painter.circle_stroke(p, r - 1.0, EStroke::new(2.0, colour));
            }
        }
        // The frame number on a small dark tag, readable on any artwork.
        let galley =
            painter.layout_no_wrap(point.frame.to_string(), theme::dense(), theme::TEXT_PRIMARY);
        let tag = egui::Rect::from_min_size(
            p + Vec2::new(8.0, -8.0 - galley.size().y - 2.0),
            galley.size() + Vec2::new(8.0, 2.0),
        );
        painter.rect_filled(tag, 4.0, theme::BG_BASE.gamma_multiply(0.85));
        painter.galley(tag.min + Vec2::new(4.0, 1.0), galley, theme::TEXT_PRIMARY);
    }
    // Now: amber, as the current frame is on the Timeline, with an arrow
    // the way it's going.
    if let Some(mark) = b.mark(frame) {
        let p = screen(mark.pos);
        let now = EStroke::new(2.0, theme::ACCENT_TIME);
        painter.circle_stroke(p, 9.0, now);
        let n = normal(screen, mark);
        let d = Vec2::new(n.y, -n.x);
        if b.marks.len() > 1 {
            let tip = p + d * 20.0;
            painter.line_segment([p + d * 9.0, tip], now);
            painter.add(egui::Shape::convex_polygon(
                vec![
                    tip + d * 2.0,
                    tip - d * 6.0 + n * 4.5,
                    tip - d * 6.0 - n * 4.5,
                ],
                theme::ACCENT_TIME,
                EStroke::NONE,
            ));
        }
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

/// What a click does now, the point on this frame's settings, and a way
/// out: a small card at the canvas corner, so the chart is edited on the
/// canvas with no panel to open.
fn card(
    ui: &egui::Ui,
    response: &egui::Response,
    s: &mut Session,
    id: NodeId,
    chart: &Chart,
    pending: &mut bool,
) {
    let at = response.rect.left_top() + Vec2::new(12.0, 12.0);
    let here = chart.point_on(s.frame).map(|p| p.id);
    let text = if chart.locked {
        "This chart is locked. Unlock it in its properties to edit it.".to_owned()
    } else if chart.points.is_empty() {
        format!(
            "Click where the artwork is on frame {} to start the chart.",
            s.frame
        )
    } else if here.is_some() {
        "Alt-drag a handle for a sharp turn; Alt-click removes the point.".to_owned()
    } else {
        format!(
            "Click to place the chart on frame {}. Slide a tick along the path \
             to change the spacing.",
            s.frame
        )
    };
    egui::Area::new(egui::Id::unique("smart-chart-card"))
        .order(egui::Order::Foreground)
        .fixed_pos(at)
        .show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_max_width(236.0);
                ui.spacing_mut().slider_width = 96.0;
                ui.label(
                    egui::RichText::new(text)
                        .font(theme::dense())
                        .color(theme::TEXT_SECONDARY),
                );
                if let (Some(pid), false) = (here, chart.locked) {
                    ui.separator();
                    let mut draft = chart.clone();
                    point_controls(ui, &mut draft, pid);
                    if draft != *chart {
                        edit(
                            s,
                            id,
                            "Change chart point",
                            Some(format!("chart-card-{}-{}", id.0, pid.0)),
                            |c| *c = draft,
                        );
                        *pending = true;
                    }
                }
                // A slider drag is one step; each click is its own.
                if *pending && !ui.input(|i| i.pointer.any_down()) {
                    s.end_gesture();
                    *pending = false;
                }
                ui.horizontal(|ui| {
                    if widgets::primary_button(ui, "Done", true).clicked() {
                        // Back to the layer: the chart stays, quietly.
                        s.selected_node = s.doc.layers.get(s.selected).map(|l| l.node);
                    }
                    if let (Some(pid), false) = (here, chart.locked) {
                        let spaced =
                            chart
                                .points
                                .iter()
                                .position(|p| p.id == pid)
                                .is_some_and(|i| {
                                    [i.checked_sub(1), Some(i)]
                                        .into_iter()
                                        .flatten()
                                        .filter_map(|j| chart.points.get(j))
                                        .any(|p| {
                                            !p.timing.spacing.is_empty()
                                                || p.timing.ease_in > 0.0
                                                || p.timing.ease_out > 0.0
                                        })
                                });
                        if ui
                            .add_enabled(spaced, egui::Button::new("Even spacing"))
                            .on_hover_text(
                                "Clear the eases and slid ticks on both sides of this point.",
                            )
                            .clicked()
                        {
                            edit(s, id, "Even spacing", None, |c| c.even_spacing(pid));
                        }
                    }
                });
            });
        });
}

/// The point `pid`'s settings: key or breakdown, its shape, the eases
/// into and out of it, and what the segment after it does.
fn point_controls(ui: &mut egui::Ui, c: &mut Chart, pid: PointId) {
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
}

#[cfg(test)]
mod tests {
    use super::*;

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
