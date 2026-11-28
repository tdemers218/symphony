//! Editing a Mesh Warp in the Camera view (Harmony 27: select the node,
//! show its controls, and move its points). With a Mesh Warp node
//! selected and the Transform tool:
//!
//! - Grid: the grid shows over the artwork, bent as it bends the art;
//!   dragging a point bends everything under it.
//! - Points (like Harmony 24's Shape-Aware Deformation): first place the
//!   points: click the artwork to add one, drag one to move it, Alt-click
//!   one to remove it. Then Build mesh: a mesh is made over the artwork's
//!   shape, and dragging a point bends the art as rigidly as it can,
//!   moving only what's connected to it. Edit points goes back to
//!   placing them.
//!
//! In Animate mode a move keys the point on the current frame; otherwise
//! it sets it for every frame. One drag is one undo step.

use eframe::egui::{self, Pos2, Stroke as EStroke, Vec2};
use symphony_core::anim::Value;
use symphony_core::graph::{NodeId, NodeKind};
use symphony_core::history::Edit;
use symphony_core::math::Affine;
use symphony_core::warp::{MeshWarp, WarpField, WarpKind};
use symphony_ui::{theme, widgets};

use crate::session::{Session, Tool};

/// How near (screen points) a press must be to grab a point.
const GRAB: f32 = 10.0;

#[derive(Default)]
pub struct WarpEdit {
    /// The point being dragged.
    dragging: Option<usize>,
    hover: Option<usize>,
}

/// The selected Mesh Warp, and its space -> screen.
fn target(s: &Session, view: Affine) -> Option<(NodeId, MeshWarp, Affine)> {
    let id = s.selected_node?;
    let NodeKind::MeshWarp(w) = &s.doc.graph.node(id)?.kind else {
        return None;
    };
    // The warp works in its node's parent space (what the pegs above it
    // make of it), on the stage plane at their depth.
    let pose = s.doc.graph.world_pose(id, s.frame).ok()?;
    let plane = s.doc.project_plane(s.frame, pose.depth)?;
    Some((
        id,
        (**w).clone(),
        view.compose(plane).compose(pose.transform),
    ))
}

/// Draw and edit the selected Mesh Warp. Returns true while a Mesh Warp
/// is being edited: the Transform tool then works on it only (not the
/// layer's box).
pub fn ui(
    ui: &egui::Ui,
    response: &egui::Response,
    painter: &egui::Painter,
    s: &mut Session,
    state: &mut WarpEdit,
    view: Affine,
) -> bool {
    if s.tool != Tool::Transform {
        state.dragging = None;
        return false;
    }
    let Some((id, warp, m)) = target(s, view) else {
        state.dragging = None;
        return false;
    };
    let screen = |p: [f32; 2]| {
        let (x, y) = m.apply(p[0], p[1]);
        Pos2::new(x, y)
    };
    let line = EStroke::new(1.0, theme::ACCENT_SELECT.gamma_multiply(0.7));
    // Where the points are on screen, and what each point's "rest" is.
    let points: Vec<[f32; 2]> = match warp.kind {
        WarpKind::Grid => {
            let grid = warp.grid_at(s.frame);
            let steps = 12;
            for r in 0..=grid.rows {
                let pts: Vec<Pos2> = (0..=grid.columns * steps)
                    .map(|i| {
                        let t = i as f32 / steps as f32 / grid.columns as f32;
                        let (a, b) = (grid.rest(r, 0), grid.rest(r, grid.columns));
                        screen(grid.apply([a[0] + (b[0] - a[0]) * t, a[1]]))
                    })
                    .collect();
                painter.add(egui::Shape::line(pts, line));
            }
            for c in 0..=grid.columns {
                let pts: Vec<Pos2> = (0..=grid.rows * steps)
                    .map(|i| {
                        let t = i as f32 / steps as f32 / grid.rows as f32;
                        let (a, b) = (grid.rest(0, c), grid.rest(grid.rows, c));
                        screen(grid.apply([a[0], a[1] + (b[1] - a[1]) * t]))
                    })
                    .collect();
                painter.add(egui::Shape::line(pts, line));
            }
            (0..=grid.rows)
                .flat_map(|r| (0..=grid.columns).map(move |c| (r, c)))
                .map(|(r, c)| grid.point(r, c))
                .collect()
        }
        WarpKind::Points => {
            // The mesh, bent, faintly.
            if let Some(WarpField::Points(f)) = warp.field_at(s.frame) {
                let faint = EStroke::new(0.75, theme::ACCENT_SELECT.gamma_multiply(0.35));
                for t in &f.mesh().triangles {
                    let [a, b, c] = t.map(|i| screen(f.now()[i as usize]));
                    painter.add(egui::Shape::closed_line(vec![a, b, c], faint));
                }
            }
            if warp.mesh.is_some() {
                warp.pins_at(s.frame)
            } else {
                warp.pins.clone()
            }
        }
    };
    let on_screen: Vec<Pos2> = points.iter().map(|&p| screen(p)).collect();
    let pointer = response.hover_pos().or(response.interact_pointer_pos());
    let point_at = |p: Pos2| {
        on_screen
            .iter()
            .enumerate()
            .map(|(i, q)| (i, q.distance(p)))
            .filter(|(_, d)| *d <= GRAB)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, _)| i)
    };
    let nearest = pointer.and_then(point_at);
    state.hover = nearest;
    if response.drag_started() {
        // The point under the press, not under the pointer now: a drag
        // only starts once the pointer has moved, and a quick one can by
        // then be nearer a neighbouring grid point.
        let origin = ui.input(|i| i.pointer.press_origin()).or(pointer);
        state.dragging = origin.and_then(point_at);
    }
    let placing = warp.kind == WarpKind::Points && warp.mesh.is_none();
    // Placing points: a click adds one; Alt-click removes one.
    if placing && response.clicked() {
        let alt = ui.input(|i| i.modifiers.alt);
        match (nearest, alt, pointer.zip(m.inverse())) {
            (Some(i), true, _) => edit(s, id, "Remove point", None, |w| {
                if i < w.pins.len() {
                    w.pins.remove(i);
                    if i < w.pin_moves.len() {
                        w.pin_moves.remove(i);
                    }
                }
            }),
            (None, false, Some((p, back))) => {
                let (x, y) = back.apply(p.x, p.y);
                edit(s, id, "Place point", None, |w| {
                    w.pins.push([x, y]);
                    w.pin_moves
                        .push([Value::Constant(0.0), Value::Constant(0.0)]);
                });
            }
            _ => {}
        }
    }
    let keyed = |i: usize| {
        match warp.kind {
            WarpKind::Grid => warp.points.get(i),
            WarpKind::Points => warp.pin_moves.get(i),
        }
        .is_some_and(|p| p.iter().any(|v| v.has_key(s.frame)))
    };
    for (i, p) in on_screen.iter().enumerate() {
        let active = state.dragging == Some(i) || state.hover == Some(i);
        let r = if active { 6.0 } else { 4.5 };
        painter.circle_filled(*p, r + 1.5, theme::BG_BASE);
        // Amber marks a key on this frame (time), as on the Timeline.
        let fill = if keyed(i) {
            theme::ACCENT_TIME
        } else {
            theme::ACCENT_SELECT
        };
        if placing {
            painter.circle_stroke(*p, r, EStroke::new(2.0, fill));
        } else {
            painter.circle_filled(*p, r, fill);
        }
    }
    if warp.kind == WarpKind::Points {
        mesh_button(ui, response, s, id, &warp);
    }
    if let Some(i) = state.dragging {
        if response.dragged() {
            if let (Some(p), Some(back)) = (response.interact_pointer_pos(), m.inverse()) {
                let (x, y) = back.apply(p.x, p.y);
                match warp.kind {
                    WarpKind::Grid => {
                        let cols = warp.columns as usize + 1;
                        let rest = warp
                            .grid_at(s.frame)
                            .rest((i / cols) as u32, (i % cols) as u32);
                        move_point(s, id, i, [x - rest[0], y - rest[1]]);
                    }
                    WarpKind::Points if placing => {
                        edit(
                            s,
                            id,
                            "Move point",
                            Some(format!("place-{}-{i}", id.0)),
                            |w| {
                                if let Some(p) = w.pins.get_mut(i) {
                                    *p = [x, y];
                                }
                            },
                        );
                    }
                    WarpKind::Points => {
                        let rest = warp.pins.get(i).copied().unwrap_or([x, y]);
                        move_point(s, id, i, [x - rest[0], y - rest[1]]);
                    }
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
    if nearest.is_some() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
    } else if placing && response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
    }
    true
}

/// Points mode's next step, on the canvas: Build mesh once points are
/// placed, Edit points once it's built.
fn mesh_button(
    ui: &egui::Ui,
    response: &egui::Response,
    s: &mut Session,
    id: NodeId,
    w: &MeshWarp,
) {
    let at = response.rect.left_top() + Vec2::new(12.0, 12.0);
    egui::Area::new(egui::Id::unique("mesh-warp-step"))
        .order(egui::Order::Foreground)
        .fixed_pos(at)
        .show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                if w.mesh.is_none() {
                    ui.label(
                        egui::RichText::new(
                            "Click the artwork to place points (Alt-click removes one).",
                        )
                        .font(theme::dense())
                        .color(theme::TEXT_SECONDARY),
                    );
                    if widgets::primary_button(ui, "Build mesh", !w.pins.is_empty()).clicked() {
                        if let Err(e) = build_mesh(s, id) {
                            s.note(e);
                        }
                    }
                } else if ui
                    .button("Edit points")
                    .on_hover_text("Go back to placing points (the mesh is rebuilt after).")
                    .clicked()
                {
                    edit(s, id, "Edit points", None, |w| w.mesh = None);
                }
            });
        });
}

/// The drawings a Mesh Warp bends on this frame, each with its drawing ->
/// warp space transform (the pegs between them).
fn drawings_under(s: &Session, id: NodeId) -> Vec<(symphony_core::id::Id, Affine)> {
    let graph = &s.doc.graph;
    let mut out = Vec::new();
    for (layer, l) in s.doc.layers.iter().enumerate() {
        let mut chain = Vec::new();
        let mut at = graph.input(l.node, 0);
        let mut found = false;
        while let Some(n) = at {
            if n == id {
                found = true;
                break;
            }
            if chain.len() > 256 {
                break;
            }
            chain.push(n);
            at = graph.input(n, 0);
        }
        if !found {
            continue;
        }
        // Drawing -> warp: the pegs from the warp down to the drawing.
        let mut m = Affine::IDENTITY;
        for &n in chain.iter().rev() {
            if let Some(NodeKind::Peg(p)) = graph.node(n).map(|n| &n.kind) {
                m = m.compose(graph.peg_transform(n, p, s.frame));
            }
        }
        if let Some(d) = s.doc.exposed(layer, s.frame) {
            out.push((d, m));
        }
    }
    out
}

/// Build the Points mesh over the artwork under the warp (on this frame).
pub fn build_mesh(s: &mut Session, id: NodeId) -> Result<(), String> {
    let Some(NodeKind::MeshWarp(w)) = s.doc.graph.node(id).map(|n| &n.kind) else {
        return Err("Select a Mesh Warp first".into());
    };
    if w.pins.is_empty() {
        return Err("Place some points on the artwork first".into());
    }
    let (pins, density) = (w.pins.clone(), w.density);
    let under = drawings_under(s, id);
    let drawings: Vec<(&symphony_core::stroke::Drawing, Affine)> = under
        .iter()
        .filter_map(|(d, m)| s.doc.drawings.get(d).map(|dr| (dr, *m)))
        .collect();
    let shapes = symphony_vector::puppet_mesh::silhouette(&drawings, density * 0.3);
    let (mesh, placed) = symphony_vector::puppet_mesh::build(&shapes, &pins, density)
        .ok_or("There's no artwork under this Mesh Warp on this frame to build a mesh on")?;
    edit(s, id, "Build mesh", None, |w| {
        w.pins = placed;
        w.pin_moves
            .resize(w.pins.len(), [Value::Constant(0.0), Value::Constant(0.0)]);
        w.mesh = Some(mesh);
    });
    Ok(())
}

/// Change the Mesh Warp `id` as one undo step (merged by `merge` while a
/// drag goes on).
fn edit(
    s: &mut Session,
    id: NodeId,
    name: &str,
    merge: Option<String>,
    f: impl FnOnce(&mut MeshWarp),
) {
    let Some(before) = s.doc.graph.node(id).cloned() else {
        return;
    };
    let mut after = before.clone();
    let NodeKind::MeshWarp(w) = &mut after.kind else {
        return;
    };
    f(w);
    if after == before {
        return;
    }
    let e = Edit::SetNodeProperties { id, before, after };
    match merge {
        Some(key) => s.history.apply_merged(&mut s.doc, name, &key, e),
        None => s.history.apply(&mut s.doc, name, vec![e]),
    }
}

/// Set point `i`'s offset (a grid point, or a placed point once the mesh
/// is built), keyed on this frame in Animate mode (or when it's already
/// animated), as one merged undo step per drag.
fn move_point(s: &mut Session, id: NodeId, i: usize, offset: [f32; 2]) {
    let (animate, frame) = (s.animate, s.frame);
    edit(
        s,
        id,
        "Move mesh point",
        Some(format!("mesh-point-{}-{i}", id.0)),
        |w| {
            let point = match w.kind {
                WarpKind::Grid => w.points.get_mut(i),
                WarpKind::Points => w.pin_moves.get_mut(i),
            };
            let Some(point) = point else { return };
            for (value, v) in point.iter_mut().zip(offset) {
                match value {
                    Value::Constant(c) if !animate => *c = v,
                    _ => value.set_key(frame, v),
                }
            }
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Moving a point in Animate mode keys it on this frame only; off
    /// Animate it changes for every frame. Undo takes the drag back.
    #[test]
    fn moving_a_point_keys_it_in_animate_mode() {
        let mut s = Session::new_scene();
        let node = s.doc.layers[0].node;
        crate::views::node::insert_near(&mut s, node, crate::views::node::mesh_warp(100, 100));
        let id = s.doc.graph.input(node, 0).unwrap();
        assert!(matches!(
            s.doc.graph.node(id).unwrap().kind,
            NodeKind::MeshWarp(_)
        ));
        let points = |s: &Session| match &s.doc.graph.node(id).unwrap().kind {
            NodeKind::MeshWarp(w) => w.clone(),
            _ => unreachable!(),
        };
        s.animate = false;
        move_point(&mut s, id, 12, [5.0, 0.0]);
        s.end_gesture();
        assert_eq!(points(&s).grid_at(30).offsets[12], [5.0, 0.0]);
        s.animate = true;
        s.frame = 10;
        move_point(&mut s, id, 12, [20.0, 0.0]);
        s.end_gesture();
        assert!(points(&s).has_key(10));
        assert_eq!(points(&s).grid_at(10).offsets[12], [20.0, 0.0]);
        s.undo();
        assert!(!points(&s).has_key(10));
    }
}
