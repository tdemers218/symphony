//! Select and Transform tools on the canvas, following Harmony 27:
//! - Select works on whole strokes of the current drawing. Click to pick,
//!   Shift adds and Control subtracts. Click empty space to place polygon
//!   anchors, drag an anchor to curve it, or drag on empty space to lasso.
//! - Transform works on layers through their pegs (cut-out animation):
//!   click artwork to pick its layer, then move, rotate or scale it. In
//!   Animate mode this sets keyframes.
//!
//! Both use the same bounding box: drag inside to move, a handle to scale
//! (Shift keeps proportions), just outside a corner to rotate.

use eframe::egui::{self, Pos2, Rect, Stroke as EStroke, Vec2};
use symphony_core::id::Id;
use symphony_core::math::Affine;
use symphony_ui::icons::Icon;
use symphony_ui::theme;
use symphony_vector::{bounds_of, hits, inside_polygon, scale_of};

use crate::session::{PegValues, Session, Tool};

const HANDLE: f32 = 7.0;
/// How far outside a corner still grabs rotation, in screen points.
const ROTATE_ZONE: f32 = 24.0;
/// Pick tolerance in screen points.
const PICK: f32 = 5.0;
const LASSO_THRESHOLD: f32 = 8.0;
const CLOSE_RADIUS: f32 = 16.0;

#[derive(Clone, Copy)]
struct Anchor {
    at: Pos2,
    handle: Vec2,
}

fn outline(anchors: &[Anchor], closed: bool) -> Vec<Pos2> {
    let Some(first) = anchors.first() else {
        return Vec::new();
    };
    let mut points = vec![first.at];
    for (i, pair) in anchors
        .iter()
        .enumerate()
        .take(anchors.len() - usize::from(!closed))
    {
        let next = anchors[(i + 1) % anchors.len()];
        if pair.handle == Vec2::ZERO && next.handle == Vec2::ZERO {
            points.push(next.at);
            continue;
        }
        let a = pair.at;
        let b = a + pair.handle;
        let d = next.at;
        let c = d - next.handle;
        let length = a.distance(b) + b.distance(c) + c.distance(d);
        let steps = (length / 4.0).ceil().clamp(4.0, 96.0) as usize;
        for step in 1..=steps {
            let t = step as f32 / steps as f32;
            let u = 1.0 - t;
            points.push(
                Pos2::ZERO
                    + a.to_vec2() * (u * u * u)
                    + b.to_vec2() * (3.0 * u * u * t)
                    + c.to_vec2() * (3.0 * u * t * t)
                    + d.to_vec2() * (t * t * t),
            );
        }
    }
    points
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Kind {
    Move,
    Scale(usize),
    Rotate,
}

struct Drag {
    kind: Kind,
    /// Drawing-local box at drag start.
    lo: [f32; 2],
    hi: [f32; 2],
    /// Drawing-local -> screen at drag start.
    to_screen: Affine,
    start: Pos2,
    /// Drawing -> scene at drag start (Transform tool).
    world: Affine,
    /// Peg values at drag start (Transform tool).
    peg: Option<PegValues>,
}

#[derive(Default)]
pub struct SelectState {
    /// Camera input uses the projected drawing for hit tests.
    pub camera_space: bool,
    drag: Option<Drag>,
    lasso: Vec<Pos2>,
    polygon: Vec<Anchor>,
    polygon_drag: Option<Pos2>,
    lasso_active: bool,
    polygon_owner: Option<(String, Option<Id>, symphony_core::stroke::ArtLayer)>,
    marquee: Option<Pos2>,
    /// Drawing-local transform being previewed while dragging (Select tool).
    pub preview: Option<Affine>,
    /// Cursor hint for the handle under the pointer (D8.11).
    pub hover: Option<Icon>,
}

impl SelectState {
    pub fn busy(&self) -> bool {
        self.drag.is_some() || !self.lasso.is_empty() || self.marquee.is_some()
    }
}

/// One drawn layer: (layer index, drawing, drawing -> scene transform).
pub type Item = (usize, Id, Affine);

fn corners(lo: [f32; 2], hi: [f32; 2]) -> [(f32, f32); 8] {
    let (mx, my) = ((lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0);
    [
        (lo[0], lo[1]),
        (hi[0], lo[1]),
        (hi[0], hi[1]),
        (lo[0], hi[1]),
        (mx, lo[1]),
        (hi[0], my),
        (mx, hi[1]),
        (lo[0], my),
    ]
}

/// The handle opposite `i`, which stays put while scaling.
fn anchor(i: usize) -> usize {
    match i {
        0..=3 => (i + 2) % 4,
        4 => 6,
        5 => 7,
        6 => 4,
        _ => 5,
    }
}

fn pos(p: (f32, f32)) -> Pos2 {
    Pos2::new(p.0, p.1)
}

/// What the pointer would grab at `p`, given the box in screen space.
fn grab(p: Pos2, lo: [f32; 2], hi: [f32; 2], m: Affine) -> Option<Kind> {
    let pts = corners(lo, hi).map(|c| pos(m.apply(c.0, c.1)));
    if let Some(i) = pts.iter().position(|h| h.distance(p) <= HANDLE + 2.0) {
        return Some(Kind::Scale(i));
    }
    let quad: Vec<[f32; 2]> = pts[..4].iter().map(|q| [q.x, q.y]).collect();
    if inside_polygon(p.x, p.y, &quad) {
        return Some(Kind::Move);
    }
    if pts[..4].iter().any(|c| c.distance(p) <= ROTATE_ZONE) {
        return Some(Kind::Rotate);
    }
    None
}

/// Drawing-local transform for a drag from `start` to `now`.
fn drag_transform(d: &Drag, now: Pos2, uniform: bool) -> Option<Affine> {
    let inv = d.to_screen.inverse()?;
    let (sx, sy) = inv.apply(d.start.x, d.start.y);
    let (nx, ny) = inv.apply(now.x, now.y);
    let c = corners(d.lo, d.hi);
    Some(match d.kind {
        Kind::Move => Affine::translate(nx - sx, ny - sy),
        Kind::Rotate => {
            let (cx, cy) = ((d.lo[0] + d.hi[0]) / 2.0, (d.lo[1] + d.hi[1]) / 2.0);
            let a = (sy - cy).atan2(sx - cx);
            let b = (ny - cy).atan2(nx - cx);
            Affine::translate(cx, cy)
                .compose(Affine::rotate_deg((b - a).to_degrees()))
                .compose(Affine::translate(-cx, -cy))
        }
        Kind::Scale(i) => {
            let (ax, ay) = c[anchor(i)];
            let ratio = |n: f32, s: f32, a: f32| {
                if (s - a).abs() < 1e-3 {
                    1.0
                } else {
                    (n - a) / (s - a)
                }
            };
            let (mut fx, mut fy) = (ratio(nx, sx, ax), ratio(ny, sy, ay));
            match i {
                4 | 6 => fx = 1.0,
                5 | 7 => fy = 1.0,
                _ => {}
            }
            if uniform && i < 4 {
                let f = if fx.abs() > fy.abs() { fx } else { fy };
                (fx, fy) = (f, f);
            }
            Affine::translate(ax, ay)
                .compose(Affine::scale(fx, fy))
                .compose(Affine::translate(-ax, -ay))
        }
    })
}

fn draw_box(
    painter: &egui::Painter,
    lo: [f32; 2],
    hi: [f32; 2],
    m: Affine,
    cut: Option<&[[f32; 2]]>,
) {
    let pts = corners(lo, hi).map(|c| pos(m.apply(c.0, c.1)));
    let outline = cut.map_or_else(
        || pts[..4].to_vec(),
        |points| points.iter().map(|&[x, y]| pos(m.apply(x, y))).collect(),
    );
    painter.add(egui::Shape::closed_line(
        outline,
        EStroke::new(1.0, theme::ACCENT_SELECT),
    ));
    for p in pts {
        let r = Rect::from_center_size(p, Vec2::splat(HANDLE));
        painter.rect_filled(r, 1.0, theme::ON_ACCENT);
        painter.rect_stroke(
            r,
            1.0,
            EStroke::new(1.0, theme::ACCENT_SELECT),
            egui::StrokeKind::Inside,
        );
    }
}

/// Handle Select and Transform input and draw their overlays.
/// `view` maps scene -> screen. `items` are the visible layers.
pub fn ui(
    ui: &egui::Ui,
    response: &egui::Response,
    painter: &egui::Painter,
    s: &mut Session,
    st: &mut SelectState,
    view: Affine,
    items: &[Item],
) {
    let (shift, alt, ctrl, enter, escape, view_pointer) = ui.input(|i| {
        (
            i.modifiers.shift,
            i.modifiers.alt,
            i.modifiers.ctrl,
            i.key_pressed(egui::Key::Enter),
            i.key_pressed(egui::Key::Escape),
            i.pointer.press_origin(),
        )
    });
    let pointer = response.interact_pointer_pos().or(response.hover_pos());
    let transform_tool = s.tool == Tool::Transform;
    let cutter = !transform_tool && s.cutter_mode;

    // The box we're working with: drawing-local bounds and local -> screen.
    let target: Option<([f32; 2], [f32; 2], Affine)> = if transform_tool {
        let layer = s.selected;
        items
            .iter()
            .find(|(l, _, _)| *l == layer)
            .and_then(|(_, d, world)| {
                let (lo, hi) = drawing_bounds(s.doc.drawings.get(d)?)?;
                Some((lo, hi, view.compose(*world)))
            })
    } else {
        s.validate_selection();
        let world = world_of(s, items);
        s.selection_bounds()
            .map(|(lo, hi)| (lo, hi, view.compose(world)))
    };

    st.hover = match (&st.drag, target, pointer) {
        (Some(d), _, _) => Some(if d.kind == Kind::Rotate {
            Icon::RotateRight
        } else {
            Icon::Transform
        }),
        (None, Some((lo, hi, m)), Some(p)) => match grab(p, lo, hi, m) {
            Some(Kind::Rotate) => Some(Icon::RotateRight),
            Some(Kind::Scale(_)) => Some(Icon::Transform),
            _ => None,
        },
        _ => None,
    };

    if cutter {
        st.hover = Some(Icon::Cutter);
    }

    let drawing = s.selection.drawing;
    let has_polygon = |st: &SelectState| !st.polygon.is_empty() || st.polygon_drag.is_some();
    if has_polygon(st)
        && (transform_tool
            || escape
            || !st
                .polygon_owner
                .as_ref()
                .is_some_and(|(key, d, art)| key == &s.key && *d == drawing && *art == s.art))
    {
        st.polygon.clear();
        st.polygon_drag = None;
    }
    let mut finish_polygon = enter && !st.polygon.is_empty() && response.hovered();

    let camera_space = st.camera_space;
    // Start a gesture.
    if response.drag_started_by(egui::PointerButton::Primary) {
        if let Some(p) = view_pointer.or(response.interact_pointer_pos()) {
            let on_box = target.and_then(|(lo, hi, m)| grab(p, lo, hi, m).map(|k| (k, lo, hi, m)));
            let start_drag =
                |kind: Kind, lo: [f32; 2], hi: [f32; 2], m: Affine, s: &mut Session| {
                    let peg = if transform_tool {
                        s.transform_peg(s.selected).map(|_| current_peg(s))
                    } else {
                        None
                    };
                    if transform_tool && kind != Kind::Move {
                        let (cx, cy) = ((lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0);
                        s.ensure_pivot(s.selected, (cx, cy));
                    }
                    let world = if camera_space {
                        s.doc
                            .layers
                            .get(s.selected)
                            .and_then(|layer| s.doc.graph.world_transform(layer.node, s.frame).ok())
                            .unwrap_or(Affine::IDENTITY)
                    } else {
                        view.inverse().unwrap_or(Affine::IDENTITY).compose(m)
                    };
                    Drag {
                        kind,
                        lo,
                        hi,
                        to_screen: m,
                        start: p,
                        world,
                        peg,
                    }
                };
            if !st.polygon.is_empty() {
                st.polygon_drag = Some(p);
            } else if let Some((kind, lo, hi, m)) = on_box.filter(|(k, ..)| {
                !cutter
                    && (transform_tool || !shift && !ctrl)
                    && (*k != Kind::Move
                        || s.easy_drag
                        || hit_selection(s, p, view, items, transform_tool))
            }) {
                st.drag = Some(start_drag(kind, lo, hi, m, s));
            } else if !cutter
                && (transform_tool || !shift && !ctrl)
                && pick(s, p, view, items, transform_tool, false, false).is_some()
            {
                // Dragging unselected artwork selects it and moves it at once.
                let fresh = if transform_tool {
                    items
                        .iter()
                        .find(|(l, _, _)| *l == s.selected)
                        .and_then(|(_, d, w)| {
                            let (lo, hi) = drawing_bounds(s.doc.drawings.get(d)?)?;
                            Some((lo, hi, view.compose(*w)))
                        })
                } else {
                    let world = world_of(s, items);
                    s.selection_bounds()
                        .map(|(lo, hi)| (lo, hi, view.compose(world)))
                };
                if let Some((lo, hi, m)) = fresh {
                    st.drag = Some(start_drag(Kind::Move, lo, hi, m, s));
                }
            } else if !transform_tool {
                let marquee = s.marquee_mode != alt;
                if marquee {
                    st.marquee = Some(p);
                } else {
                    st.lasso = vec![p];
                    st.lasso_active = false;
                }
            }
        }
    }

    // Continue it.
    if response.dragged_by(egui::PointerButton::Primary) {
        if let Some(p) = response.interact_pointer_pos() {
            if let Some(d) = &st.drag {
                if let Some(l) = drag_transform(d, p, shift) {
                    if transform_tool {
                        apply_peg_drag(s, d, l);
                    } else {
                        st.preview = Some(l);
                    }
                }
            } else if let Some(start) = st.lasso.first() {
                st.lasso_active |= start.distance(p) >= LASSO_THRESHOLD;
                if st.lasso_active && st.lasso.last().is_some_and(|q| q.distance(p) > 2.0) {
                    st.lasso.push(p);
                }
            }
        }
    }

    // Finish it.
    if response.drag_stopped() {
        if let (Some(d), Some(p)) = (st.drag.take(), response.interact_pointer_pos()) {
            if transform_tool {
                s.end_gesture();
            } else if let Some(l) = drag_transform(&d, p, shift) {
                let name = match d.kind {
                    Kind::Move => "Move selection",
                    Kind::Rotate => "Rotate selection",
                    Kind::Scale(_) => "Scale selection",
                };
                s.transform_selection(l, name);
            }
        }
        st.preview = None;
        if let Some(at) = st.polygon_drag.take() {
            let end = response.interact_pointer_pos().unwrap_or(at);
            if st.polygon.len() >= 3
                && at.distance(st.polygon[0].at) <= CLOSE_RADIUS
                && at.distance(end) < LASSO_THRESHOLD
            {
                finish_polygon = true;
            } else {
                st.polygon.push(Anchor {
                    at,
                    handle: if at.distance(end) >= LASSO_THRESHOLD {
                        end - at
                    } else {
                        Vec2::ZERO
                    },
                });
            }
        }
        let area: Option<Vec<[f32; 2]>> = if let Some(a) = st.marquee.take() {
            let b = response.interact_pointer_pos().unwrap_or(a);
            let r = Rect::from_two_pos(a, b);
            Some(vec![
                [r.min.x, r.min.y],
                [r.max.x, r.min.y],
                [r.max.x, r.max.y],
                [r.min.x, r.max.y],
            ])
        } else if std::mem::take(&mut st.lasso_active) {
            let mut points = std::mem::take(&mut st.lasso);
            if let Some(end) = response.interact_pointer_pos() {
                points.push(end);
            }
            (points.len() >= 3).then(|| points.into_iter().map(|p| [p.x, p.y]).collect())
        } else {
            if let Some(at) = st.lasso.first().copied() {
                if cutter || pick(s, at, view, items, false, shift, ctrl).is_none() {
                    st.polygon.push(Anchor {
                        at,
                        handle: Vec2::ZERO,
                    });
                }
            }
            st.lasso.clear();
            None
        };
        if let Some(poly) = area {
            select_area(s, &poly, view, items, shift, ctrl);
        }
    }

    if response.clicked_by(egui::PointerButton::Primary) {
        if let Some(p) = response.interact_pointer_pos() {
            if st.polygon.len() >= 3
                && (p.distance(st.polygon[0].at) <= CLOSE_RADIUS || response.double_clicked())
            {
                finish_polygon = true;
            } else if cutter
                || !st.polygon.is_empty()
                || (pick(s, p, view, items, transform_tool, shift, ctrl).is_none()
                    && !transform_tool)
            {
                st.polygon.push(Anchor {
                    at: p,
                    handle: Vec2::ZERO,
                });
            }
        }
    }
    if !st.polygon.is_empty() {
        st.polygon_owner = Some((s.key.clone(), drawing, s.art));
    }
    if finish_polygon && st.polygon.len() >= 3 {
        let anchors = std::mem::take(&mut st.polygon);
        let points: Vec<_> = outline(&anchors, true)
            .into_iter()
            .map(|p| [p.x, p.y])
            .collect();
        select_area(s, &points, view, items, shift, ctrl);
    }
    if !st.polygon.is_empty() {
        let mut anchors = st.polygon.clone();
        if let Some(at) = st.polygon_drag {
            anchors.push(Anchor {
                at,
                handle: pointer.unwrap_or(at) - at,
            });
        } else if let Some(at) = response.hover_pos() {
            anchors.push(Anchor {
                at,
                handle: Vec2::ZERO,
            });
        }
        let line = outline(&anchors, false);
        if line.len() > 1 {
            painter.add(egui::Shape::line(
                line,
                EStroke::new(1.2, theme::ACCENT_SELECT),
            ));
        }
        if st.polygon.len() >= 3 {
            let start = st.polygon[0].at;
            let active = pointer.is_some_and(|p| p.distance(start) <= CLOSE_RADIUS);
            painter.circle_stroke(
                start,
                if active { CLOSE_RADIUS } else { 6.0 },
                EStroke::new(1.0, theme::ACCENT_SELECT),
            );
        }
        for anchor in &st.polygon {
            painter.circle_filled(anchor.at, 3.5, theme::ACCENT_SELECT);
        }
        if let (Some(at), Some(end)) = (st.polygon_drag, pointer) {
            painter.line_segment(
                [at - (end - at), end],
                EStroke::new(1.0, theme::ACCENT_SELECT),
            );
        }
    }

    // Overlays.
    if let Some(a) = st.marquee {
        if let Some(b) = response.interact_pointer_pos() {
            painter.rect_stroke(
                Rect::from_two_pos(a, b),
                0.0,
                EStroke::new(1.0, theme::ACCENT_SELECT),
                egui::StrokeKind::Middle,
            );
        }
    }
    if st.lasso.len() > 1 {
        painter.add(egui::Shape::line(
            st.lasso.clone(),
            EStroke::new(1.0, theme::ACCENT_SELECT),
        ));
    }
    if let Some((lo, hi, m)) = target {
        let m = st.preview.map_or(m, |l| m.compose(l));
        let cut = if transform_tool {
            None
        } else {
            s.selection_cut_outline()
        };
        draw_box(painter, lo, hi, m, cut);
        if s.show_selected_points && !transform_tool {
            let mark = |x, y| {
                let at = pos(m.apply(x, y));
                if painter.clip_rect().contains(at) {
                    painter.circle(
                        at,
                        3.0,
                        theme::BG_BASE,
                        EStroke::new(1.0, theme::ACCENT_SELECT),
                    );
                }
            };
            for stroke in s.selected_strokes() {
                if stroke.shape.is_empty() {
                    for point in &stroke.points {
                        mark(point.x, point.y);
                    }
                } else {
                    for &[x, y] in stroke.shape.iter().flatten() {
                        mark(x, y);
                    }
                }
            }
            for fill in s.selected_fills() {
                for &[x, y] in fill.contours.iter().flatten() {
                    mark(x, y);
                }
            }
        }

        if transform_tool {
            if let Some(p) = s.transform_peg(s.selected) {
                // The pivot is amber, because it belongs to animation (DESIGN §4.3).
                let world = items
                    .iter()
                    .find(|(l, _, _)| *l == s.selected)
                    .map(|(_, _, w)| *w)
                    .unwrap_or(Affine::IDENTITY);
                let c = view.compose(world).apply(p.pivot.0, p.pivot.1);
                painter.circle_stroke(pos(c), 5.0, EStroke::new(2.0, theme::ACCENT_TIME));
            }
        }
    }
}

/// The selected layer's drawing -> scene transform as this view draws it
/// (the Drawing view shows drawings without pegs).
fn world_of(s: &Session, items: &[Item]) -> Affine {
    items
        .iter()
        .find(|(l, _, _)| *l == s.selected)
        .map(|(_, _, w)| *w)
        .unwrap_or_else(|| {
            s.doc
                .layers
                .get(s.selected)
                .and_then(|l| s.doc.graph.world_transform(l.node, s.frame).ok())
                .unwrap_or(Affine::IDENTITY)
        })
}

fn current_peg(s: &Session) -> PegValues {
    let p = s.transform_peg(s.selected).cloned().unwrap_or_default();
    let f = s.frame;
    PegValues {
        x: p.x.at(f),
        y: p.y.at(f),
        rotation: p.rotation.at(f),
        scale_x: p.scale_x.as_ref().map_or(1.0, |v| v.at(f)),
        scale_y: p.scale_y.as_ref().map_or(1.0, |v| v.at(f)),
    }
}

/// Turn a drawing-local drag transform into peg values from the drag start.
fn apply_peg_drag(s: &mut Session, d: &Drag, l: Affine) {
    let Some(start) = d.peg else { return };
    let layer = s.selected;
    match d.kind {
        Kind::Move => {
            // A drawing-local offset, carried into scene space by the
            // drawing's own rotation and scale.
            let linear = Affine {
                tx: 0.0,
                ty: 0.0,
                ..d.world
            };
            let (dx, dy) = linear.apply(l.tx, l.ty);
            let (dx, dy) = s
                .transform_peg_id(layer)
                .and_then(|id| s.doc.graph.world_transform(id, s.frame).ok())
                .and_then(|parent| {
                    Affine {
                        tx: 0.0,
                        ty: 0.0,
                        ..parent
                    }
                    .inverse()
                })
                .map_or((dx, dy), |parent| parent.apply(dx, dy));
            s.adjust_peg(layer, |v| {
                v.x = start.x + dx;
                v.y = start.y + dy;
            });
        }
        Kind::Rotate => {
            let deg = l.b.atan2(l.a).to_degrees();
            s.adjust_peg(layer, |v| v.rotation = start.rotation + deg);
        }
        Kind::Scale(_) => {
            let (fx, fy) = (l.a, l.d);
            s.adjust_peg(layer, |v| {
                v.scale_x = start.scale_x * fx;
                v.scale_y = start.scale_y * fy;
            });
        }
    }
}

/// Is `p` on the current selection's artwork?
fn hit_selection(s: &Session, p: Pos2, view: Affine, items: &[Item], transform_tool: bool) -> bool {
    if transform_tool {
        return layer_at(s, p, view, items) == Some(s.selected);
    }
    let world = world_of(s, items);
    let m = view.compose(world);
    let Some(inv) = m.inverse() else { return false };
    let (x, y) = inv.apply(p.x, p.y);
    let r = PICK / scale_of(m).max(1e-6);
    s.selected_strokes().iter().any(|st| hits(st, x, y, r))
        || s.selected_fills().iter().any(|f| f.contains(x, y))
}

/// The front-most layer with artwork under `p`.
fn layer_at(s: &Session, p: Pos2, view: Affine, items: &[Item]) -> Option<usize> {
    items.iter().rev().find_map(|(layer, d, world)| {
        let m = view.compose(*world);
        let inv = m.inverse()?;
        let (x, y) = inv.apply(p.x, p.y);
        let r = PICK / scale_of(m).max(1e-6);
        let dr = s.doc.drawings.get(d)?;
        dr.strokes
            .iter()
            .any(|st| hits(st, x, y, r))
            .then_some(*layer)
    })
}

/// Click picking. Transform picks a layer; Select picks a stroke in the
/// current drawing (Shift adds, Control subtracts).
fn pick(
    s: &mut Session,
    p: Pos2,
    view: Affine,
    items: &[Item],
    transform_tool: bool,
    add: bool,
    remove: bool,
) -> Option<usize> {
    if transform_tool {
        let layer = layer_at(s, p, view, items)?;
        s.selected = layer;
        return Some(layer);
    }
    s.validate_selection();
    let d = s.selection.drawing?;
    let world = world_of(s, items);
    let m = view.compose(world);
    let inv = m.inverse()?;
    let (x, y) = inv.apply(p.x, p.y);
    let r = PICK / scale_of(m).max(1e-6);
    // A line under the pen first, else the colour zone it's in.
    let dr = s.doc.drawings.get(&d)?;
    let hit = dr
        .strokes
        .iter()
        .rev()
        .find(|st| st.art == s.art && hits(st, x, y, r))
        .map(|st| st.id)
        .or_else(|| {
            dr.fills
                .iter()
                .rev()
                .find(|f| f.art == s.art && f.contains(x, y))
                .map(|f| f.id)
        })?;
    if remove {
        s.selection.strokes.retain(|id| *id != hit);
    } else if add {
        if !s.selection.strokes.contains(&hit) {
            s.selection.strokes.push(hit);
        }
    } else if !s.selection.strokes.contains(&hit) {
        s.selection.strokes = vec![hit];
    }
    Some(s.selected)
}

/// The box around a drawing's strokes and colour zones.
pub(super) fn drawing_bounds(d: &symphony_core::stroke::Drawing) -> Option<([f32; 2], [f32; 2])> {
    let zones = d.fills.iter().flat_map(|f| f.contours.iter().flatten());
    let strokes = bounds_of(&d.strokes);
    let mut acc = strokes;
    for p in zones {
        acc = Some(match acc {
            None => (*p, *p),
            Some((lo, hi)) => (
                [lo[0].min(p[0]), lo[1].min(p[1])],
                [hi[0].max(p[0]), hi[1].max(p[1])],
            ),
        });
    }
    acc
}

/// Lasso or marquee: select strokes whose middle falls inside `poly` (screen).
fn select_area(
    s: &mut Session,
    poly: &[[f32; 2]],
    view: Affine,
    items: &[Item],
    add: bool,
    remove: bool,
) {
    s.validate_selection();
    let Some(d) = s.selection.drawing else { return };
    let world = world_of(s, items);
    let m = view.compose(world);
    if s.cutter_mode {
        if let Some(local) = m.inverse() {
            let polygon = poly
                .iter()
                .map(|&[x, y]| {
                    let (x, y) = local.apply(x, y);
                    [x, y]
                })
                .collect();
            s.cut_area(d, polygon, add, remove);
            s.cutter_mode = false;
        }
        return;
    }
    let Some(dr) = s.doc.drawings.get(&d) else {
        return;
    };
    let inside: Vec<Id> = dr
        .strokes
        .iter()
        .filter(|st| st.art == s.art)
        .filter(|st| {
            symphony_vector::bounds(st).is_some_and(|(lo, hi)| {
                let (x, y) = m.apply((lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0);
                inside_polygon(x, y, poly)
            })
        })
        .map(|st| st.id)
        .chain(
            // Colour zones whose middle is inside.
            dr.fills
                .iter()
                .filter(|f| f.art == s.art)
                .filter(|f| {
                    let mut it = f.contours.iter().flatten();
                    let Some(first) = it.next() else {
                        return false;
                    };
                    let (lo, hi) = it.fold((*first, *first), |(lo, hi), p| {
                        (
                            [lo[0].min(p[0]), lo[1].min(p[1])],
                            [hi[0].max(p[0]), hi[1].max(p[1])],
                        )
                    });
                    let (x, y) = m.apply((lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0);
                    inside_polygon(x, y, poly)
                })
                .map(|f| f.id),
        )
        .collect();
    if remove {
        s.selection.strokes.retain(|id| !inside.contains(id));
    } else if add {
        for id in inside {
            if !s.selection.strokes.contains(&id) {
                s.selection.strokes.push(id);
            }
        }
    } else {
        s.selection.strokes = inside;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::Harness;

    #[test]
    fn short_gesture_places_an_anchor_and_next_drag_bends_the_outline() {
        let mut session = Session::new_scene();
        session.tool = Tool::Select;
        let mut harness = Harness::builder()
            .with_size(Vec2::new(500.0, 400.0))
            .build_ui_state(
                |ui, (session, state)| {
                    let (response, painter) =
                        ui.allocate_painter(ui.available_size(), egui::Sense::click_and_drag());
                    super::ui(
                        ui,
                        &response,
                        &painter,
                        session,
                        state,
                        Affine::IDENTITY,
                        &[],
                    );
                },
                (session, SelectState::default()),
            );
        let first = Pos2::new(100.0, 100.0);
        let next = Pos2::new(180.0, 100.0);
        let handle = Vec2::new(30.0, 35.0);
        let mut pointer = |pos, pressed| {
            harness
                .input_mut()
                .events
                .push(egui::Event::PointerMoved(pos));
            if let Some(pressed) = pressed {
                harness.input_mut().events.push(egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                });
            }
            harness.run_steps(1);
        };
        pointer(first, Some(true));
        pointer(first + Vec2::new(4.0, 1.0), None);
        pointer(first + Vec2::new(4.0, 1.0), Some(false));
        pointer(next, Some(true));
        pointer(next + handle, None);
        pointer(next + handle, Some(false));
        let anchors = &harness.state().1.polygon;
        assert_eq!(anchors.len(), 2);
        assert_eq!(anchors[0].handle, Vec2::ZERO);
        assert_eq!(anchors[1].at, next);
        assert_eq!(anchors[1].handle, handle);
        assert!(outline(anchors, false).iter().any(|point| point.y < 95.0));
    }
}
