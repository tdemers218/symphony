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
use symphony_core::chart::{Chart, ChartCurve, PointId, Role};
use symphony_core::graph::{NodeId, NodeKind};
use symphony_core::history::Edit;
use symphony_core::math::Affine;
use symphony_ui::{theme, widgets};

use crate::session::{Session, Tool};

/// How near (screen points) a press must be to grab a point.
const GRAB: f32 = 10.0;
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

#[derive(Default)]
pub struct ChartEdit {
    /// The point being dragged, on which chart.
    dragging: Option<(NodeId, PointId)>,
    /// Scene and edit count the charts were last checked at.
    stamp: Option<(String, u64)>,
    built: HashMap<NodeId, Built>,
}

/// A chart made ready to draw: rebuilt only when the chart itself changes.
struct Built {
    chart: Chart,
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
            marks,
        }
    }

    fn mark(&self, frame: u32) -> Option<&Mark> {
        let first = self.marks.first()?.frame;
        self.marks.get(frame.checked_sub(first)? as usize)
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
    let on_screen: Vec<(PointId, u32, Pos2)> = chart
        .points
        .iter()
        .map(|p| (p.id, p.frame, screen(p.pos)))
        .collect();
    let nearest = pointer.and_then(|p| {
        on_screen
            .iter()
            .map(|(pid, _, q)| (*pid, q.distance(p)))
            .filter(|(_, d)| *d <= GRAB)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(pid, _)| pid)
    });
    let active = state
        .dragging
        .filter(|(n, _)| *n == id)
        .map(|(_, p)| p)
        .or(nearest);
    draw_selected(painter, b, colour(&chart, id), &screen, s.frame, active);
    if !editing {
        state.dragging = None;
        return false;
    }
    hint(ui, response, s, &chart);
    if chart.locked {
        return true;
    }
    let back = m.inverse();
    if response.drag_started() {
        state.dragging = nearest.map(|p| (id, p));
    }
    if let Some((node, pid)) = state.dragging {
        if response.dragged() {
            if let (Some(p), Some(back)) = (response.interact_pointer_pos(), back) {
                let (x, y) = back.apply(p.x, p.y);
                edit(
                    s,
                    node,
                    "Move chart point",
                    Some(format!("chart-point-{}-{}", node.0, pid.0)),
                    |c| {
                        if let Some(p) = c.point_mut(pid) {
                            p.pos = [x, y];
                        }
                    },
                );
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
            (Some(pid), true) => edit(s, id, "Remove chart point", None, |c| {
                c.remove_point(pid);
            }),
            (Some(pid), false) => {
                if let Some(p) = chart.point(pid) {
                    s.frame = p.frame;
                }
            }
            (None, false) => {
                if let (Some(p), Some(back)) = (pointer, back) {
                    let (x, y) = back.apply(p.x, p.y);
                    place(s, id, [x, y]);
                }
            }
            (None, true) => {}
        }
    }
    if nearest.is_some() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
    } else if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
    }
    true
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
    active: Option<PointId>,
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
        painter.line_segment([p - n * 6.0, p + n * 6.0], edge);
        painter.line_segment([p - n * 5.0, p + n * 5.0], tick);
    }
    for point in &b.chart.points {
        let p = screen(point.pos);
        let big = active == Some(point.id);
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

/// What a click does now, and a way out, at the canvas corner.
fn hint(ui: &egui::Ui, response: &egui::Response, s: &mut Session, chart: &Chart) {
    let at = response.rect.left_top() + Vec2::new(12.0, 12.0);
    let text = if chart.locked {
        "This chart is locked. Unlock it in its properties to edit it.".to_owned()
    } else if chart.points.is_empty() {
        format!(
            "Click where the artwork is on frame {} to start the chart.",
            s.frame
        )
    } else {
        format!(
            "Click to place the chart on frame {}. Drag a point to move it; \
             Alt-click removes one.",
            s.frame
        )
    };
    egui::Area::new(egui::Id::unique("smart-chart-hint"))
        .order(egui::Order::Foreground)
        .fixed_pos(at)
        .show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_max_width(280.0);
                ui.label(
                    egui::RichText::new(text)
                        .font(theme::dense())
                        .color(theme::TEXT_SECONDARY),
                );
                if widgets::primary_button(ui, "Done", true).clicked() {
                    // Back to the layer: the chart stays, quietly.
                    s.selected_node = s.doc.layers.get(s.selected).map(|l| l.node);
                }
            });
        });
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
