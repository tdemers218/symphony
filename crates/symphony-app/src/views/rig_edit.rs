use eframe::egui::{self, Pos2, Stroke};
use symphony_core::{
    anim::Value,
    graph::{NodeId, NodeKind},
    history::Edit,
    math::Affine,
};
use symphony_ui::theme;

use crate::session::{Session, Tool};

#[derive(Default)]
pub struct RigEdit {
    drag: Option<Drag>,
}

#[derive(Clone, Copy)]
enum Drag {
    Tip(NodeId),
    MoveSkeleton {
        id: NodeId,
        grab: Pos2,
        start: [f32; 2],
    },
}

pub fn ui(
    ui: &egui::Ui,
    response: &egui::Response,
    painter: &egui::Painter,
    s: &mut Session,
    state: &mut RigEdit,
    view: Affine,
) -> bool {
    let selected = s.selected_node.filter(|&id| {
        s.doc
            .graph
            .node(id)
            .is_some_and(|node| matches!(node.kind, NodeKind::Bone(_) | NodeKind::Skeleton(_)))
    });
    let Some(selected) = selected.filter(|_| s.tool == Tool::Transform) else {
        finish(s, state);
        return false;
    };
    let root = s.doc.graph.rig_root(selected);
    let graph = &s.doc.graph;
    let mut handles = Vec::new();
    for (id, _) in graph.nodes().filter(|(id, _)| graph.bone(*id).is_some()) {
        if graph.rig_root(id) != root {
            continue;
        }
        let Some((pose, screen)) = graph.bone_pose(id, s.frame).zip(screen_basis(s, id, view))
        else {
            continue;
        };
        let from = pos(screen.apply(pose.posed.tx, pose.posed.ty));
        let (x, y) = pose.tip();
        let tip = pos(screen.apply(x, y));
        let controller = graph.smart_controller(id, s.frame);
        let target = graph
            .bone(id)
            .filter(|_| controller == Some(id))
            .and_then(|bone| bone.smart.as_ref())
            .and_then(|joint| {
                let axes = screen.compose(graph.smart_target_basis(id, s.frame)?);
                Some(pos(axes.apply(
                    joint.target[0].at(s.frame),
                    joint.target[1].at(s.frame),
                )))
            });
        let handle = target.unwrap_or(tip);
        let ink = if id == selected {
            theme::ACCENT_SELECT
        } else {
            theme::TEXT_SECONDARY
        };
        painter.line_segment([from, tip], Stroke::new(5.0, theme::BG_BASE));
        painter.line_segment([from, tip], Stroke::new(2.0, ink));
        painter.circle_filled(handle, 5.5, theme::BG_BASE);
        if target.is_some() {
            if handle.distance(tip) > 6.0 {
                painter.line_segment([tip, handle], Stroke::new(1.0, ink.gamma_multiply(0.6)));
            }
            painter.rect_stroke(
                egui::Rect::from_center_size(handle, egui::vec2(8.0, 8.0)),
                1.0,
                Stroke::new(1.5, ink),
                egui::StrokeKind::Inside,
            );
        } else {
            painter.circle_filled(tip, 3.0, ink);
        }
        handles.push((
            id,
            from,
            handle,
            graph.parent_bone(id).is_some(),
            controller.is_none_or(|owner| owner == id),
        ));
    }
    // Draw joints over the tips they share with their parent.
    for &(id, joint, _, connected, _) in &handles {
        if connected {
            let ink = if id == selected {
                theme::ACCENT_SELECT
            } else {
                theme::TEXT_SECONDARY
            };
            painter.circle_filled(joint, 6.0, theme::BG_BASE);
            painter.circle_stroke(joint, 4.0, Stroke::new(1.5, ink));
        }
    }
    let nearest = |pointer: Pos2, joint: bool| {
        handles
            .iter()
            .filter(|(_, _, _, connected, movable)| if joint { *connected } else { *movable })
            .map(|&(id, start, tip, _, _)| (id, pointer.distance(if joint { start } else { tip })))
            .filter(|(_, distance)| *distance <= 10.0)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(id, _)| id)
    };
    let skeleton = graph.node(selected).and_then(|node| {
        let NodeKind::Skeleton(peg) = &node.kind else {
            return None;
        };
        let (_, origin, _, _, _) = handles.iter().find(|(_, _, _, connected, _)| !connected)?;
        Some((*origin, [peg.x.at(s.frame), peg.y.at(s.frame)]))
    });
    if let Some((origin, _)) = skeleton {
        painter.circle_filled(origin, 7.0, theme::BG_BASE);
        painter.circle_stroke(origin, 5.0, Stroke::new(2.0, theme::ACCENT_SELECT));
    }
    let menu_key = response.id.with("rig-joint-menu");
    if response.secondary_clicked() {
        let id = response
            .interact_pointer_pos()
            .and_then(|p| nearest(p, true));
        ui.data_mut(|data| data.insert_temp(menu_key, id));
    }
    let menu = ui
        .data(|data| data.get_temp::<Option<NodeId>>(menu_key))
        .flatten();
    if let Some((id, label)) =
        menu.and_then(|id| super::node::smart_joint_label(s, id).map(|label| (id, label)))
    {
        response.context_menu(|ui| {
            if ui.button(label).clicked() {
                super::node::toggle_smart_joint(s, id);
                ui.close();
            }
        });
    }
    if response.drag_started_by(egui::PointerButton::Primary) {
        state.drag = ui.input(|i| i.pointer.press_origin()).and_then(|grab| {
            if let Some((_, start)) = skeleton.filter(|(origin, _)| origin.distance(grab) <= 10.0) {
                return Some(Drag::MoveSkeleton {
                    id: selected,
                    grab,
                    start,
                });
            }
            nearest(grab, false).map(Drag::Tip)
        });
        if let Some(Drag::Tip(id)) = state.drag {
            s.select_graph_node(id, true);
        }
    }
    if response.clicked_by(egui::PointerButton::Primary) {
        if let Some(id) = response
            .interact_pointer_pos()
            .and_then(|p| nearest(p, true).or_else(|| nearest(p, false)))
        {
            s.select_graph_node(id, true);
        }
    }
    if let Some(drag) = state.drag {
        if let Some(p) = response.interact_pointer_pos() {
            match drag {
                Drag::Tip(id) => pose_tip(s, id, view, p),
                Drag::MoveSkeleton { id, grab, start } => {
                    move_skeleton(s, id, view, grab, p, start)
                }
            }
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        }
    } else if response
        .hover_pos()
        .is_some_and(|p| nearest(p, false).is_some())
    {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
    }
    if !ui.input(|i| i.pointer.primary_down()) {
        finish(s, state);
    }
    true
}

pub fn finish(s: &mut Session, state: &mut RigEdit) {
    if state.drag.take().is_some() {
        s.end_gesture();
    }
}

fn pos((x, y): (f32, f32)) -> Pos2 {
    Pos2::new(x, y)
}

fn screen_basis(s: &Session, id: NodeId, view: Affine) -> Option<Affine> {
    let pose = s.doc.graph.rig_stage_pose(id, s.frame)?;
    let plane = s.doc.project_plane(s.frame, pose.depth)?;
    Some(view.compose(plane).compose(pose.transform))
}

fn pose_tip(s: &mut Session, id: NodeId, view: Affine, pointer: Pos2) {
    let graph = &s.doc.graph;
    let Some(before) = graph.node(id).cloned() else {
        return;
    };
    let mut after = before.clone();
    let NodeKind::Bone(bone) = &mut after.kind else {
        return;
    };
    let Some(screen) = screen_basis(s, id, view) else {
        return;
    };
    let (x, y) = (pointer.x, pointer.y);
    if let Some(joint) = bone
        .smart
        .as_mut()
        .filter(|_| graph.smart_controller(id, s.frame) == Some(id))
    {
        let Some(back) = graph
            .smart_target_basis(id, s.frame)
            .and_then(|basis| screen.compose(basis).inverse())
        else {
            return;
        };
        let (x, y) = back.apply(x, y);
        set_value(&mut joint.target[0], x, s.frame, s.animate);
        set_value(&mut joint.target[1], y, s.frame, s.animate);
    } else {
        let Some(pose) = graph.bone_pose(id, s.frame) else {
            return;
        };
        let Some(back) = screen.compose(pose.posed).inverse() else {
            return;
        };
        let (x, y) = back.apply(x, y);
        if x.hypot(y) < 1e-3 {
            return;
        }
        let rotation = bone.rotation.at(s.frame) + y.atan2(x).to_degrees();
        set_value(&mut bone.rotation, rotation, s.frame, s.animate);
    }
    if before != after {
        s.history.apply_merged(
            &mut s.doc,
            "Pose bone",
            &format!("bone-tip-{}", id.0),
            Edit::SetNodeProperties { id, before, after },
        );
    }
}

fn move_skeleton(s: &mut Session, id: NodeId, view: Affine, grab: Pos2, p: Pos2, start: [f32; 2]) {
    let graph = &s.doc.graph;
    let Some(before) = graph.node(id).cloned() else {
        return;
    };
    let mut after = before.clone();
    let NodeKind::Skeleton(peg) = &mut after.kind else {
        return;
    };
    let Ok(parent) = graph.world_pose(id, s.frame) else {
        return;
    };
    let Some(plane) = s
        .doc
        .project_plane(s.frame, parent.depth + peg.z.at(s.frame))
    else {
        return;
    };
    let Some(back) = view.compose(plane).compose(parent.transform).inverse() else {
        return;
    };
    let (x0, y0) = back.apply(grab.x, grab.y);
    let (x, y) = back.apply(p.x, p.y);
    set_value(&mut peg.x, start[0] + x - x0, s.frame, s.animate);
    set_value(&mut peg.y, start[1] + y - y0, s.frame, s.animate);
    if before != after {
        s.history.apply_merged(
            &mut s.doc,
            "Move skeleton",
            &format!("skeleton-{}", id.0),
            Edit::SetNodeProperties { id, before, after },
        );
    }
}

fn set_value(value: &mut Value, number: f32, frame: u32, animate: bool) {
    match value {
        Value::Constant(current) if !animate => *current = number,
        _ => value.set_key(frame, number),
    }
}
