//! Editable scene graph, with separate transform and image ports.
use std::collections::{BTreeMap, BTreeSet};

use eframe::egui::{self, Align2, Pos2, Rect, Sense, Stroke, StrokeKind, Vec2};
use symphony_core::{
    graph::{BlendMode, BlurMode, Effect, Link, Node, NodeId, NodeKey, NodeKind},
    history::Edit,
};

use super::rope::Cable;
use crate::session::Session;

const CABLE_WIDTH: f32 = 2.0;
const ACTIVE_CABLE_WIDTH: f32 = 2.4;
use symphony_ui::{theme, widgets};

/// Node corner radius at 100% zoom: well rounded, scaled with zoom.
const NODE_RADIUS: f32 = 9.0;

type Input = (NodeId, usize);

enum Drag {
    Pan,
    Marquee {
        start: Pos2,
        extend: bool,
    },
    Node(NodeId, Vec2),
    Cable {
        source: NodeId,
        old_input: Option<Input>,
    },
}

#[derive(Default)]
pub struct NodeViewState {
    pan: Vec2,
    selection: BTreeSet<NodeId>,
    selection_scene: String,
    zoom: Option<f32>,
    keyboard_zoom: Option<f32>,
    anchor_x: Option<f32>,
    fit_next: Option<Rect>,
    drag: Option<Drag>,
    library_search: String,
    properties: Option<PropertyDraft>,
    properties_open: bool,
    preview: Option<egui::TextureHandle>,
    preview_open: bool,
    /// Links and their ends as last drawn, to animate connections that
    /// appear (drawn out from the output) and disappear (fade).
    drawn_links: Vec<(Link, Pos2, Pos2)>,
    drawn_scene: String,
    link_anims: Vec<LinkAnim>,
    /// Where (and when) a dragged cable was last let go, so the cable's
    /// animation starts from there.
    released: Option<(Pos2, f64)>,
    /// The live drag curve and its shape at release.
    drag_cable: Option<Cable>,
    released_cable: Option<Cable>,
    /// When the drag curve last updated.
    cable_time: Option<f64>,
    /// The hover thumbnail: what it shows (drawing, scene version, frame)
    /// and its texture, plus the last card it was shown beside (for its
    /// fade-out).
    thumb: Option<(
        (symphony_core::id::Id, u64, u32),
        Option<egui::TextureHandle>,
    )>,
    thumb_at: Option<(Rect, String)>,
    /// The Skeleton setup view, when a skeleton is open in the panel.
    pub setup: super::skeleton_setup::SetupState,
}

#[derive(Clone)]
struct LinkAnim {
    link: Option<Link>,
    start: f64,
    motion: CableMotion,
    curve: Cable,
}

/// What a cable is doing, like a real one.
#[derive(Clone, Copy)]
enum CableMotion {
    /// A new connection: its end goes onto the port from where it was let
    /// go, or shoots out of the output for connections made elsewhere.
    Connect { from_end: Option<Pos2> },
    /// Moved or swapped to another input: the end travels from the old
    /// input to the new one.
    Move { from_end: Pos2 },
    /// Disconnected or dropped loose: the cable winds into its output.
    Retract { source: NodeId, from: Pos2 },
}

impl CableMotion {
    fn seconds(self) -> f64 {
        match self {
            CableMotion::Connect { .. } | CableMotion::Move { .. } => 0.42,
            CableMotion::Retract { .. } => 0.24,
        }
    }
}

impl LinkAnim {
    fn retract(
        link: Option<Link>,
        source: NodeId,
        ends: (Pos2, Pos2),
        curve: Option<Cable>,
        now: f64,
    ) -> Self {
        let (from, end) = ends;
        Self {
            link,
            start: now,
            motion: CableMotion::Retract { source, from },
            curve: curve.unwrap_or_else(|| Cable::new(from, end)),
        }
    }
}

/// Can a node pass its input straight through (peg to peg, image to
/// image)? Pulling one out of a chain then rejoins its neighbours.
fn passes_through(kind: &NodeKind) -> bool {
    matches!(
        kind,
        NodeKind::Peg(_) | NodeKind::MeshWarp(_) | NodeKind::Effect(_) | NodeKind::Composite
    )
}

/// The links after pulling `id` out of its chain: its own links go, and
/// what fed its first input now feeds what it fed (if it passes through).
fn detached_links(graph: &symphony_core::graph::Graph, id: NodeId) -> Vec<Link> {
    let before = graph.links();
    let mut after: Vec<Link> = before
        .iter()
        .copied()
        .filter(|l| l.from != id && l.to != id)
        .collect();
    let heals = graph.node(id).is_some_and(|n| passes_through(&n.kind));
    if let (true, Some(source)) = (heals, graph.input(id, 0)) {
        for out in before.iter().filter(|l| l.from == id) {
            if !after.iter().any(|l| l.to == out.to && l.port == out.port) {
                after.push(Link {
                    from: source,
                    to: out.to,
                    port: out.port,
                });
            }
        }
    }
    after
}

/// Why a node can't be deleted, if it can't.
pub fn undeletable(s: &Session, id: NodeId) -> Option<&'static str> {
    if id == s.doc.display {
        return Some("The Camera node shows the scene; it can't be deleted");
    }
    if s.doc.layers.iter().any(|l| l.peg == id) {
        return Some(
            "This is a layer's own peg: delete the layer to remove it, or add another peg",
        );
    }
    if matches!(
        s.doc.graph.node(id).map(|n| &n.kind),
        Some(NodeKind::Drawing { .. })
    ) && s.layer_count() <= 1
    {
        return Some("A scene needs at least one layer");
    }
    None
}

/// Delete nodes, as one undo step. Pegs, Mesh Warps and effects are
/// pulled out of their chains, which close up behind them; a Drawing
/// node takes its layer with it. Returns how many were deleted (a note
/// says why any couldn't be).
pub fn delete_nodes(s: &mut Session, ids: &[NodeId]) -> usize {
    let mut why = None;
    let mut ids: Vec<NodeId> = ids
        .iter()
        .copied()
        .filter(|&id| match undeletable(s, id) {
            Some(reason) => {
                why = Some(reason);
                false
            }
            None => s.doc.graph.node(id).is_some(),
        })
        .collect();
    ids.sort();
    ids.dedup();
    // Keep at least one layer.
    let mut layers: Vec<usize> = ids
        .iter()
        .filter_map(|&id| s.doc.layers.iter().position(|l| l.node == id))
        .collect();
    while !layers.is_empty() && layers.len() >= s.layer_count() {
        let keep = layers.pop().unwrap_or(0);
        ids.retain(|&id| s.doc.layers.get(keep).is_none_or(|l| l.node != id));
        why = Some("A scene needs at least one layer");
    }
    let mut edits = Vec::new();
    // The other nodes: pulled out of a scratch copy one by one, so their
    // chains heal around each other.
    let mut scratch = s.doc.graph.clone();
    let others: Vec<NodeId> = ids
        .iter()
        .copied()
        .filter(|id| !s.doc.layers.iter().any(|l| l.node == *id))
        .collect();
    for &id in &others {
        let after = detached_links(&scratch, id);
        scratch.replace_links(after);
        scratch.remove(id);
    }
    if !others.is_empty() {
        edits.push(Edit::RewireNodes {
            before: s.doc.graph.links().to_vec(),
            after: scratch.links().to_vec(),
            was_custom: s.doc.custom_wiring,
        });
        for &id in &others {
            if let Some(node) = s.doc.graph.node(id).cloned() {
                edits.push(Edit::RemoveNode {
                    id,
                    node,
                    position: s.doc.node_positions.get(&id).copied().unwrap_or([0.0, 0.0]),
                });
            }
        }
    }
    let mut layers: Vec<usize> = ids
        .iter()
        .filter_map(|&id| s.doc.layers.iter().position(|l| l.node == id))
        .collect();
    layers.sort_unstable();
    for &i in layers.iter().rev() {
        if let Some(snap) = s.doc.snapshot_layer(i) {
            edits.push(Edit::RemoveLayer(Box::new(snap)));
        }
    }
    let n = others.len() + layers.len();
    if n > 0 {
        let name = if n == 1 {
            "Delete node"
        } else {
            "Delete nodes"
        };
        s.history.apply(&mut s.doc, name, edits);
        if s.selected_node.is_some_and(|id| ids.contains(&id)) {
            s.selected_node = None;
        }
        s.clamp_selection();
        s.fit_tool_to_context();
    }
    if let Some(why) = why {
        s.note(why);
    }
    n
}

struct PropertyDraft {
    id: NodeId,
    scene: String,
    version: u64,
    frame: u32,
    initial: Node,
    initial_fov: Option<f32>,
    fov: Option<f32>,
    initial_motion: Option<symphony_core::doc::CameraMotion>,
    motion: Option<symphony_core::doc::CameraMotion>,
    node: Node,
    bone_thumb: Option<Option<egui::TextureHandle>>,
}

impl PropertyDraft {
    fn new(s: &Session, id: NodeId) -> Option<Self> {
        let mut node = s.doc.graph.node_at_frame(id, s.frame)?;
        node.name = node_name(&node).to_owned();
        let fov = (id == s.doc.display).then(|| s.doc.camera_fov.at(s.frame));
        let motion = (id == s.doc.display).then(|| s.doc.camera_motion.clone());
        Some(Self {
            id,
            scene: s.key.clone(),
            version: s.doc.version,
            frame: s.frame,
            initial: node.clone(),
            initial_fov: fov,
            fov,
            initial_motion: motion.clone(),
            motion,
            node,
            bone_thumb: None,
        })
    }
}

const W: f32 = 136.0;
const H: f32 = 36.0;
const GRID: f32 = 24.0;

pub fn kind_label(kind: &NodeKind) -> &'static str {
    match kind {
        NodeKind::Drawing { .. } => "Drawing",
        NodeKind::Peg(_) => "Peg",
        NodeKind::Skeleton(_) => "Skeleton",
        NodeKind::Bone(_) => "Bone",
        NodeKind::MeshWarp(_) => "Mesh Warp",
        NodeKind::Composite => "Composite",
        NodeKind::Display => "Camera",
        NodeKind::Write => "Write",
        NodeKind::Chart(_) => "Smart Chart",
        NodeKind::Effect(Effect::Mask { .. }) => "Mask",
        NodeKind::Effect(Effect::Blur { .. }) => "Blur",
        NodeKind::Effect(Effect::Grain { .. }) => "Grain",
        NodeKind::Effect(Effect::Opacity { .. }) => "Opacity",
        NodeKind::Effect(Effect::Blend { .. }) => "Blend Modes",
        NodeKind::Gradient(_) => "Gradient",
    }
}

fn effect_nodes() -> [NodeKind; 6] {
    [
        NodeKind::Effect(Effect::Mask { inverted: false }),
        NodeKind::Effect(Effect::Blur {
            radius: 12.0,
            mode: BlurMode::Gaussian,
        }),
        NodeKind::Effect(Effect::Grain {
            amount: 0.16,
            seed: 0,
        }),
        NodeKind::Gradient(Default::default()),
        NodeKind::Effect(Effect::Opacity { amount: 0.5 }),
        NodeKind::Effect(Effect::Blend {
            mode: BlendMode::Multiply,
            amount: 1.0,
        }),
    ]
}

pub fn addable_nodes() -> impl Iterator<Item = NodeKind> {
    [
        NodeKind::Peg(Default::default()),
        NodeKind::Skeleton(Default::default()),
        mesh_warp(1920, 1080),
        NodeKind::Composite,
    ]
    .into_iter()
    .chain(effect_nodes())
}

/// A new Mesh Warp: a 4 x 4 grid over the whole stage (`w` x `h`).
pub fn mesh_warp(w: u32, h: u32) -> NodeKind {
    NodeKind::MeshWarp(Box::new(symphony_core::warp::MeshWarp::new(
        4, 4, 0.0, 0.0, w as f32, h as f32,
    )))
}

/// A Mesh Warp from search or a menu, sized to this scene's stage.
fn fit_to_stage(s: &Session, kind: NodeKind) -> NodeKind {
    match kind {
        NodeKind::MeshWarp(_) => mesh_warp(s.doc.settings.width, s.doc.settings.height),
        other => other,
    }
}

pub(super) fn node_name(node: &Node) -> &str {
    if matches!(node.kind, NodeKind::Display) && node.name == "Display" {
        "Camera"
    } else {
        &node.name
    }
}

pub fn add_from_search(s: &mut Session, state: &mut NodeViewState, kind: NodeKind) {
    let kind = fit_to_stage(s, kind);
    // A Smart Chart stands on its own (nothing is wired to it): it's
    // selected with the Transform tool, so the next click on the canvas
    // places its first point.
    let chart = matches!(kind, NodeKind::Chart(_));
    // A Mesh Warp found by search bends the selected layer straight away.
    if matches!(kind, NodeKind::MeshWarp(_)) {
        if let Some(node) = s.doc.layers.get(s.selected).map(|l| l.node) {
            insert_near(s, node, kind);
            return;
        }
    }
    // An effect found by search applies to the selected layer straight
    // away: it goes between that layer's drawing node and where its image
    // went, so typing "subtract" is enough.
    if let NodeKind::Effect(_) = kind {
        if insert_after_selected_layer(s, state, kind.clone()) {
            return;
        }
    }
    let x = (s.doc.layers.len().saturating_sub(1) as f32 / 2.0 + 1.0) * (W + 26.0);
    let mut y = 105.0;
    while s
        .doc
        .node_positions
        .values()
        .any(|p| (p[0] - x).abs() < W && (p[1] - y).abs() < H + 10.0)
    {
        y += H + 20.0;
    }
    add_node(s, kind, [x, y]);
    state.pan = Vec2::new(-x, (105.0 - y).min(0.0));
    if chart {
        s.fit_tool_to_context();
    }
}

/// A new, empty Smart Chart.
pub fn smart_chart() -> NodeKind {
    NodeKind::Chart(Box::default())
}

/// Insert an effect node after the selected layer's drawing node, in one
/// undo step. False when that layer has no wired drawing node.
fn insert_after_selected_layer(s: &mut Session, state: &mut NodeViewState, kind: NodeKind) -> bool {
    let Some((column, drawing)) = s.doc.layers.get(s.selected).map(|l| (l.column, l.node)) else {
        return false;
    };
    let Some(old) = s
        .doc
        .graph
        .links()
        .iter()
        .find(|l| l.from == drawing)
        .copied()
    else {
        return false;
    };
    let id = s.doc.graph.next_id();
    let at = s
        .doc
        .node_positions
        .get(&drawing)
        .copied()
        .unwrap_or([0.0, 0.0]);
    let position = [at[0], at[1] + H + 30.0];
    let before = s.doc.graph.links().to_vec();
    let mut after: Vec<Link> = before.iter().filter(|l| **l != old).copied().collect();
    after.push(Link {
        from: drawing,
        to: id,
        port: 0,
    });
    after.push(Link { from: id, ..old });
    let label = kind_label(&kind);
    let layer = s
        .doc
        .xsheet
        .column(column)
        .map_or_else(|| "the layer".to_owned(), |c| c.name.clone());
    let was_custom = s.doc.custom_wiring;
    s.history.apply(
        &mut s.doc,
        "Add node",
        vec![
            Edit::AddNode {
                id,
                node: Node {
                    name: label.into(),
                    kind,
                    enabled: true,
                },
                position,
            },
            Edit::RewireNodes {
                before,
                after,
                was_custom,
            },
        ],
    );
    s.select_graph_node(id, true);
    state.pan = Vec2::new(-position[0], (105.0 - position[1]).min(0.0));
    s.notice = Some(format!("Added {label} to {layer}"));
    true
}

/// Add a node next to `target` in the nearest place in the hierarchy that
/// fits, in one undo step: a peg becomes the target's new parent (between
/// it and its old one); an effect or composite goes after the drawing the
/// target leads to (between it and where its image went). Anything else,
/// or a target with no such place, is added unconnected beside it.
pub fn insert_near(s: &mut Session, target: NodeId, kind: NodeKind) {
    let graph = &s.doc.graph;
    let kind_of = |id: NodeId| graph.node(id).map(|n| n.kind.clone());
    let at = s
        .doc
        .node_positions
        .get(&target)
        .copied()
        .unwrap_or([0.0, 0.0]);
    // (link to replace, its source, its destination) for the new node.
    let splice: Option<(Option<Link>, Option<NodeId>, Input)> = match &kind {
        NodeKind::Peg(_) | NodeKind::MeshWarp(_)
            if matches!(
                kind_of(target),
                Some(NodeKind::Drawing { .. } | NodeKind::Peg(_) | NodeKind::MeshWarp(_))
            ) =>
        {
            let parent = graph.input(target, 0);
            let old = parent.map(|from| Link {
                from,
                to: target,
                port: 0,
            });
            Some((old, parent, (target, 0)))
        }
        NodeKind::Effect(_) | NodeKind::Composite => {
            // From a peg, go down to the drawing it carries.
            let mut src = target;
            let mut guard = 0;
            while matches!(kind_of(src), Some(NodeKind::Peg(_))) && guard < 32 {
                guard += 1;
                match graph.links().iter().find(|l| l.from == src && l.port == 0) {
                    Some(l) => src = l.to,
                    None => break,
                }
            }
            graph
                .links()
                .iter()
                .find(|l| l.from == src)
                .copied()
                .map(|old| (Some(old), Some(src), (old.to, old.port)))
        }
        _ => None,
    };
    let Some((old, from, to)) = splice else {
        add_node(s, kind, [at[0] + W + 30.0, at[1]]);
        return;
    };
    let id = s.doc.graph.next_id();
    let position = if matches!(kind, NodeKind::Peg(_) | NodeKind::MeshWarp(_)) {
        [at[0], at[1] - (H + 30.0)]
    } else {
        [at[0], at[1] + H + 30.0]
    };
    let position = free_spot(s, position, W);
    let before = s.doc.graph.links().to_vec();
    let mut after: Vec<Link> = before
        .iter()
        .filter(|l| Some(**l) != old && (l.to, l.port) != to)
        .copied()
        .collect();
    if let Some(from) = from {
        after.push(Link {
            from,
            to: id,
            port: 0,
        });
    }
    after.push(Link {
        from: id,
        to: to.0,
        port: to.1,
    });
    let label = kind_label(&kind);
    let was_custom = s.doc.custom_wiring;
    s.history.apply(
        &mut s.doc,
        "Add node",
        vec![
            Edit::AddNode {
                id,
                node: Node {
                    name: label.into(),
                    kind,
                    enabled: true,
                },
                position,
            },
            Edit::RewireNodes {
                before,
                after,
                was_custom,
            },
        ],
    );
    s.select_graph_node(id, true);
}

fn add_node(s: &mut Session, kind: NodeKind, position: [f32; 2]) {
    if matches!(kind, NodeKind::Skeleton(_) | NodeKind::Bone(_)) {
        add_rig_node(s, kind, position);
        return;
    }
    // Placed clear of the others from the start (one undo step).
    let position = free_spot(s, position, W);
    let id = s.doc.graph.next_id();
    let name = if matches!(kind, NodeKind::Chart(_)) {
        unique_name(s, kind_label(&kind))
    } else {
        kind_label(&kind).into()
    };
    s.history.apply(
        &mut s.doc,
        "Add node",
        vec![Edit::AddNode {
            id,
            node: Node {
                name,
                kind,
                enabled: true,
            },
            position,
        }],
    );
    s.select_graph_node(id, true);
}

/// `base`, or `base 2`, `base 3`... when a node already has that name, so
/// several charts are told apart in the Timeline and search.
fn unique_name(s: &Session, base: &str) -> String {
    let taken = |name: &str| s.doc.graph.nodes().any(|(_, n)| n.name == name);
    if !taken(base) {
        return base.to_owned();
    }
    (2..)
        .map(|i| format!("{base} {i}"))
        .find(|name| !taken(name))
        .unwrap_or_else(|| base.to_owned())
}

/// Add `bone` under `parent` (a Skeleton or a bone), connected, as one
/// undo step; it's selected. For growing bones in the setup view.
pub fn add_bone_under(
    s: &mut Session,
    parent: NodeId,
    bone: symphony_core::rig::Bone,
) -> Option<NodeId> {
    if !matches!(
        s.doc.graph.node(parent).map(|n| &n.kind),
        Some(NodeKind::Skeleton(_) | NodeKind::Bone(_))
    ) {
        return None;
    }
    let id = s.doc.graph.next_id();
    let near = s
        .doc
        .node_positions
        .get(&parent)
        .copied()
        .unwrap_or([0.0, 0.0]);
    let position = free_spot(s, [near[0], near[1] + H + 28.0], W);
    let before = s.doc.graph.links().to_vec();
    let mut after = before.clone();
    after.push(Link {
        from: parent,
        to: id,
        port: 0,
    });
    let was_custom = s.doc.custom_wiring;
    s.history.apply(
        &mut s.doc,
        "Grow bone",
        vec![
            Edit::AddNode {
                id,
                node: Node {
                    name: "Bone".into(),
                    kind: NodeKind::Bone(bone),
                    enabled: true,
                },
                position,
            },
            Edit::RewireNodes {
                before,
                after,
                was_custom,
            },
        ],
    );
    s.select_graph_node(id, true);
    Some(id)
}

fn add_rig_node(s: &mut Session, kind: NodeKind, position: [f32; 2]) {
    let parent = selected_node(s).filter(|&id| {
        matches!(kind, NodeKind::Bone(_))
            && s.doc
                .graph
                .node(id)
                .is_some_and(|node| matches!(node.kind, NodeKind::Skeleton(_) | NodeKind::Bone(_)))
    });
    let first = s.doc.graph.next_id();
    let position = free_spot(s, position, W);
    let mut edits = Vec::new();
    let make_node = |id, kind: NodeKind, position| Edit::AddNode {
        id,
        node: Node {
            name: kind_label(&kind).into(),
            kind,
            enabled: true,
        },
        position,
    };
    let (parent, bone) = if let Some(parent) = parent {
        (parent, first)
    } else {
        edits.push(make_node(
            first,
            NodeKind::Skeleton(Default::default()),
            position,
        ));
        (first, NodeId(first.0 + 1))
    };
    let bone_at = if first == bone {
        position
    } else {
        free_spot_with(
            s,
            [position[0], position[1] + H + 28.0],
            W,
            Some((first, position)),
        )
    };
    edits.push(make_node(bone, NodeKind::Bone(Default::default()), bone_at));
    let before = s.doc.graph.links().to_vec();
    let mut after = before.clone();
    after.push(Link {
        from: parent,
        to: bone,
        port: 0,
    });
    edits.push(Edit::RewireNodes {
        before,
        after,
        was_custom: s.doc.custom_wiring,
    });
    s.history.apply(
        &mut s.doc,
        if first == bone {
            "Add bone"
        } else {
            "Add skeleton"
        },
        edits,
    );
    s.select_graph_node(bone, true);
}

pub fn smart_joint_label(s: &Session, id: NodeId) -> Option<&'static str> {
    let bone = s.doc.graph.bone(id)?;
    if s.doc.graph.parent_bone(id).is_some() && bone.smart.is_some() {
        Some("Remove smart joint")
    } else if s.doc.graph.can_make_smart(id) {
        Some("Make smart joint")
    } else {
        None
    }
}

pub fn toggle_smart_joint(s: &mut Session, id: NodeId) {
    use symphony_core::{anim::Value, rig::SmartJoint};
    let Some(label) = smart_joint_label(s, id) else {
        return;
    };
    let Some(before) = s.doc.graph.node(id).cloned() else {
        return;
    };
    let mut after = before.clone();
    let NodeKind::Bone(bone) = &mut after.kind else {
        return;
    };
    if bone.smart.is_some() {
        bone.smart = None;
    } else {
        let Some(pose) = s.doc.graph.bone_pose(id, s.frame) else {
            return;
        };
        let Some(basis) = s
            .doc
            .graph
            .smart_target_basis(id, s.frame)
            .and_then(|m| m.inverse())
        else {
            return;
        };
        let (x, y) = pose.tip();
        let (x, y) = basis.apply(x, y);
        let angle = bone.rest_angle + bone.rotation.at(s.frame);
        bone.smart = Some(SmartJoint {
            target: [Value::Constant(x), Value::Constant(y)],
            flip: angle.to_radians().sin() < 0.0,
        });
    }
    s.history.apply(
        &mut s.doc,
        label,
        vec![Edit::SetNodeProperties { id, before, after }],
    );
    s.select_graph_node(id, true);
}

fn open_properties(s: &Session, state: &mut NodeViewState, id: NodeId) {
    if state.properties.as_ref().is_none_or(|draft| draft.id != id) {
        state.properties = PropertyDraft::new(s, id);
    }
    state.properties_open = true;
}

pub fn selected_node(s: &Session) -> Option<NodeId> {
    s.selected_node
        .filter(|&id| s.doc.graph.node(id).is_some())
        .or_else(|| s.doc.layers.get(s.selected).map(|layer| layer.node))
}

pub fn set_selected_key(s: &mut Session, delete: bool) {
    if let Some(mut draft) = selected_node(s).and_then(|id| PropertyDraft::new(s, id)) {
        apply_properties(s, &mut draft, !delete, delete);
    }
}

pub fn selected_has_key(s: &Session) -> bool {
    selected_node(s).is_some_and(|id| s.doc.graph.has_key(id, s.frame))
}

pub fn show_selected_properties(s: &Session, state: &mut NodeViewState) {
    if let Some(id) = selected_node(s) {
        open_properties(s, state, id);
    }
}

pub fn reset_view(state: &mut NodeViewState) {
    state.pan = Vec2::ZERO;
    state.zoom = None;
}

pub fn show_render_preview(state: &mut NodeViewState) {
    state.preview_open = true;
    state.preview = None;
}

fn sync_properties(s: &Session, state: &mut NodeViewState) {
    if let Some(id) = selected_node(s) {
        if state.properties.as_ref().is_none_or(|draft| {
            draft.id != id
                || draft.scene != s.key
                || draft.frame != s.frame
                || draft.version != s.doc.version
        }) {
            state.properties = PropertyDraft::new(s, id);
        }
    } else {
        state.properties = None;
    }
}

pub fn inspector(ui: &mut egui::Ui, s: &mut Session, state: &mut NodeViewState) {
    sync_properties(s, state);
    let bones = state.setup.selected_bones(s);
    if let Some(draft) = &mut state.properties {
        inspector_content(ui, s, draft, &bones);
    } else {
        egui::Frame::new()
            .inner_margin(theme::CARD_PAD)
            .show(ui, |ui| {
                ui.label(
                    egui::RichText::new("Select a node to see its properties.")
                        .color(theme::TEXT_SECONDARY),
                );
            });
    }
}

fn inspector_content(
    ui: &mut egui::Ui,
    s: &mut Session,
    draft: &mut PropertyDraft,
    bones: &[NodeId],
) {
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            egui::Frame::new()
                .inner_margin(theme::CARD_PAD)
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 4.0;
                    if bones.len() > 1 {
                        ui.label(
                            egui::RichText::new(format!("{} bones selected", bones.len()))
                                .size(theme::SIZE_HEADING)
                                .strong(),
                        );
                        ui.add_space(8.0);
                        bone_batch_form(ui, s, bones);
                    } else {
                        ui.push_id(draft.id, |ui| property_form(ui, s, draft));
                    }
                });
        });
}

fn property_heading(ui: &mut egui::Ui, label: &str) {
    ui.label(
        egui::RichText::new(label)
            .strong()
            .color(theme::TEXT_PRIMARY),
    );
}

/// Share the value column at normal widths and stack the field in narrow docks.
fn property_row<R>(ui: &mut egui::Ui, label: &str, control: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let text = egui::RichText::new(label).color(theme::TEXT_SECONDARY);
    if ui.available_width() >= 240.0 {
        ui.horizontal(|ui| {
            ui.add_sized([116.0, 24.0], egui::Label::new(text).truncate())
                .on_hover_text(label);
            control(ui)
        })
        .inner
    } else {
        ui.label(text);
        control(ui)
    }
}

/// Keep box selection away from the cards and their connection targets.
const MARQUEE_CLEARANCE: f32 = 18.0;

/// Connect `source` to `input`, keeping connections where it can rather
/// than silently dropping them:
/// - an input that's already used, on a node with a free input that fits
///   (a Composite's spare port), takes the free one instead;
/// - moving a cable onto a used input swaps: the cable that was there
///   moves to the input this one came from.
///
/// Otherwise it replaces the connection, as before.
fn connect_smart(
    s: &mut Session,
    source: NodeId,
    input: Input,
    old_input: Option<Input>,
    all_inputs: &[Input],
) {
    let graph = &s.doc.graph;
    let taken_by = graph.input(input.0, input.1).filter(|&from| from != source);
    let Some(previous) = taken_by else {
        return edit_link(s, Some(source), input, old_input);
    };
    // A free input on the same node that fits: use it, keep the old one.
    if old_input.is_none() {
        if let Some(&free) = all_inputs.iter().find(|&&(node, port)| {
            node == input.0
                && graph.input(node, port).is_none()
                && graph.can_connect_to(source, node, port)
        }) {
            return edit_link(s, Some(source), free, None);
        }
    }
    // Moving a cable onto a used input: swap them.
    if let Some(from_input) = old_input {
        if graph.can_connect_to(source, input.0, input.1)
            && graph.can_connect_to(previous, from_input.0, from_input.1)
        {
            let before = graph.links().to_vec();
            let mut after = before.clone();
            after.retain(|l| (l.to, l.port) != input && (l.to, l.port) != from_input);
            after.push(Link {
                from: source,
                to: input.0,
                port: input.1,
            });
            after.push(Link {
                from: previous,
                to: from_input.0,
                port: from_input.1,
            });
            let was_custom = s.doc.custom_wiring;
            s.history.apply(
                &mut s.doc,
                "Swap connections",
                vec![Edit::RewireNodes {
                    before,
                    after,
                    was_custom,
                }],
            );
            return;
        }
    }
    edit_link(s, Some(source), input, old_input);
}

fn edit_link(s: &mut Session, source: Option<NodeId>, input: Input, old_input: Option<Input>) {
    if let Some(from) = source {
        if !s.doc.graph.can_connect_to(from, input.0, input.1) {
            s.notice =
                Some("These ports can’t connect. Check the data type or look for a loop.".into());
            return;
        }
    }
    let before = s.doc.graph.links().to_vec();
    let mut after = before.clone();
    after.retain(|link| {
        let endpoint = (link.to, link.port);
        endpoint != input && Some(endpoint) != old_input
    });
    if let Some(from) = source {
        after.push(Link {
            from,
            to: input.0,
            port: input.1,
        });
    }
    if before != after {
        let was_custom = s.doc.custom_wiring;
        let name = if source.is_some() {
            "Connect nodes"
        } else {
            "Disconnect nodes"
        };
        s.history.apply(
            &mut s.doc,
            name,
            vec![Edit::RewireNodes {
                before,
                after,
                was_custom,
            }],
        );
    }
}

fn curve(painter: &egui::Painter, from: Pos2, to: Pos2, stroke: Stroke) {
    Cable::new(from, to)
        .shape_for_ports(1.0)
        .paint(painter, stroke);
}

#[derive(Clone, Copy)]
enum AutoWire {
    Join(Link),
    Insert {
        old: Link,
        moving: NodeId,
        port: usize,
    },
}

impl AutoWire {
    fn links(self) -> [Option<Link>; 2] {
        match self {
            Self::Join(link) => [Some(link), None],
            Self::Insert { old, moving, port } => [
                Some(Link {
                    from: old.from,
                    to: moving,
                    port,
                }),
                Some(Link {
                    from: moving,
                    ..old
                }),
            ],
        }
    }

    fn replaced(self) -> Option<Link> {
        match self {
            Self::Join(_) => None,
            Self::Insert { old, .. } => Some(old),
        }
    }

    fn edit(self, s: &Session) -> Edit {
        let before = s.doc.graph.links().to_vec();
        let mut after = before.clone();
        if let Some(old) = self.replaced() {
            after.retain(|link| *link != old);
        }
        after.extend(self.links().into_iter().flatten());
        Edit::RewireNodes {
            before,
            after,
            was_custom: s.doc.custom_wiring,
        }
    }
}

fn near_cable(point: Pos2, from: Pos2, to: Pos2) -> f32 {
    let cable = Cable::new(from, to).shape_for_ports(1.0);
    let mut previous = from.to_vec2();
    let mut best = f32::INFINITY;
    for step in 1..=20 {
        let next = cable.prefix(step as f32 / 20.0).end.to_vec2();
        let edge = next - previous;
        let along =
            ((point.to_vec2() - previous).dot(edge) / edge.length_sq().max(0.0001)).clamp(0.0, 1.0);
        best = best.min((point.to_vec2() - (previous + edge * along)).length());
        previous = next;
    }
    best
}

fn nearby_wiring(
    graph: &symphony_core::graph::Graph,
    moving: NodeId,
    card: Rect,
    inputs: &BTreeMap<Input, Pos2>,
    outputs: &BTreeMap<NodeId, Pos2>,
) -> Option<AutoWire> {
    const REACH: f32 = 26.0;
    let free_inputs = || {
        inputs
            .iter()
            .filter(|((id, port), _)| *id == moving && graph.input(*id, *port).is_none())
    };
    let free_output = outputs
        .get(&moving)
        .filter(|_| !graph.links().iter().any(|link| link.from == moving));
    if free_output.is_some() {
        if let Some(((_, port), _)) = free_inputs().next() {
            let crossing = graph
                .links()
                .iter()
                .filter_map(|&old| {
                    let from = *outputs.get(&old.from)?;
                    let to = *inputs.get(&(old.to, old.port))?;
                    let point = card.center();
                    if point.y <= from.y || point.y >= to.y {
                        return None;
                    }
                    let distance = near_cable(point, from, to);
                    (distance <= REACH
                        && graph.can_connect_to(old.from, moving, *port)
                        && graph.can_connect_to(moving, old.to, old.port))
                    .then_some((
                        distance,
                        AutoWire::Insert {
                            old,
                            moving,
                            port: *port,
                        },
                    ))
                })
                .min_by(|a, b| a.0.total_cmp(&b.0));
            if let Some((_, candidate)) = crossing {
                return Some(candidate);
            }
        }
    }

    let incoming = free_inputs().flat_map(|(&(_, port), &to)| {
        outputs.iter().filter_map(move |(&from, &at)| {
            let distance = to.distance(at);
            (distance <= REACH
                && at.y <= card.center().y
                && graph.can_connect_to(from, moving, port))
            .then_some((
                distance,
                AutoWire::Join(Link {
                    from,
                    to: moving,
                    port,
                }),
            ))
        })
    });
    let outgoing = free_output.into_iter().flat_map(|&at| {
        inputs.iter().filter_map(move |(&(to, port), &point)| {
            let distance = at.distance(point);
            (distance <= REACH
                && point.y >= card.center().y
                && graph.input(to, port).is_none()
                && graph.can_connect_to(moving, to, port))
            .then_some((
                distance,
                AutoWire::Join(Link {
                    from: moving,
                    to,
                    port,
                }),
            ))
        })
    });
    incoming
        .chain(outgoing)
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, candidate)| candidate)
}

pub fn ui(ui: &mut egui::Ui, s: &mut Session, state: &mut NodeViewState) {
    // A skeleton open for setup takes the panel until Back.
    if state.setup.open.is_some() && super::skeleton_setup::ui(ui, s, &mut state.setup) {
        return;
    }
    let mut add = None;
    ui.horizontal_wrapped(|ui| {
        ui.menu_button("Add node", |ui| {
            if ui.button("Drawing").clicked() {
                s.add_layer();
                if let Some(layer) = s.doc.layers.get(s.selected) {
                    s.select_graph_node(layer.node, true);
                }
                ui.close();
            }
            for kind in [
                NodeKind::Peg(Default::default()),
                NodeKind::Skeleton(Default::default()),
                mesh_warp(s.doc.settings.width, s.doc.settings.height),
                NodeKind::Composite,
            ] {
                if ui.button(kind_label(&kind)).clicked() {
                    add = Some(kind);
                    ui.close();
                }
            }
            ui.separator();
            for kind in effect_nodes() {
                if ui.button(kind_label(&kind)).clicked() {
                    add = Some(kind);
                    ui.close();
                }
            }
        });
        if ui
            .button("Arrange")
            .on_hover_text("Arrange rigs, drawings, and effects by signal flow")
            .clicked()
        {
            arrange(s, state);
        }
        if ui.button("Reset view").clicked() {
            reset_view(state);
        }
        if ui
            .add_enabled(selected_node(s).is_some(), egui::Button::new("Properties"))
            .clicked()
        {
            show_selected_properties(s, state);
        }
        if ui.button("Render preview").clicked() {
            show_render_preview(state);
        }
    });
    canvas(ui, s, state, add);
}

pub fn windows(ctx: &egui::Context, s: &mut Session, state: &mut NodeViewState) {
    properties(ctx, s, state);
    preview(ctx, s, state);
}

fn properties(ctx: &egui::Context, s: &mut Session, state: &mut NodeViewState) {
    if !state.properties_open {
        return;
    }
    sync_properties(s, state);
    let Some(draft) = &mut state.properties else {
        return;
    };
    let bones = state.setup.selected_bones(s);
    egui::Window::new("Node Properties")
        .default_size([328.0, 420.0])
        .id(egui::Id::unique("node-properties"))
        .collapsible(false)
        .open(&mut state.properties_open)
        .show(ctx, |ui| inspector_content(ui, s, draft, &bones));
}

fn add_bone_drawing(s: &mut Session, bone: NodeId) {
    if s.doc.graph.bone(bone).is_none() {
        return;
    }
    let at = node_position(s, bone, &default_positions(s));
    let peg = free_spot(s, [at.x, at.y + H + 28.0], W);
    let drawing = free_spot_with(
        s,
        [peg[0], peg[1] + H + 28.0],
        W,
        Some((s.doc.graph.next_id(), peg)),
    );
    s.add_layer_for_bone(bone, peg, drawing);
}

pub(super) fn bone_thumbnail(
    s: &Session,
    bone: NodeId,
    ctx: &egui::Context,
) -> Option<egui::TextureHandle> {
    let drawings: Vec<_> = s
        .doc
        .layers
        .iter()
        .enumerate()
        .filter_map(|(index, layer)| {
            if s.doc.graph.drawing_bone(layer.node, s.frame) != Some(bone) {
                return None;
            }
            s.doc
                .exposed(index, s.frame)
                .map(|drawing| (layer.node, layer.column, drawing))
        })
        .collect();
    let pix = symphony_render::render_drawing_thumbnails(&s.doc, &drawings, s.frame, 96)?;
    let image = egui::ColorImage::from_rgba_premultiplied([96, 96], pix.data());
    Some(ctx.load_texture("bone-drawings", image, egui::TextureOptions::LINEAR))
}

/// Return the input that belongs only to this drawing. An existing
/// dedicated peg stays in the path, so its keyed transforms still apply.
fn drawing_attachment_input(
    graph: &symphony_core::graph::Graph,
    layer: symphony_core::doc::Layer,
) -> NodeId {
    let mut input = layer.node;
    let mut seen = BTreeSet::new();
    while let Some(parent) = graph.input(input, 0).filter(|p| seen.insert(*p)) {
        let private = graph
            .links()
            .iter()
            .filter(|link| link.from == parent)
            .count()
            == 1;
        if !private
            || !graph
                .node(parent)
                .is_some_and(|node| matches!(node.kind, NodeKind::Peg(_) | NodeKind::MeshWarp(_)))
        {
            break;
        }
        input = parent;
    }
    input
}

fn change_bone_drawing(
    s: &mut Session,
    bone: NodeId,
    layer: symphony_core::doc::Layer,
    attach: bool,
) {
    let graph = &s.doc.graph;
    let mut input = drawing_attachment_input(graph, layer);
    if !attach {
        // A shared control belongs to other drawings too. Detach this
        // branch only rather than changing the other attachments.
        if graph.input(input, 0) != Some(bone) {
            input = layer.node;
        }
    } else if !graph.can_connect_to(bone, input, 0) {
        return;
    }
    let before = graph.links().to_vec();
    let mut after = before.clone();
    after.retain(|link| link.to != input || link.port != 0);
    if attach {
        after.push(Link {
            from: bone,
            to: input,
            port: 0,
        });
    } else if input == layer.node && graph.input(layer.peg, 0).is_none() {
        after.push(Link {
            from: layer.peg,
            to: input,
            port: 0,
        });
    }
    if before != after {
        let was_custom = s.doc.custom_wiring;
        s.history.apply(
            &mut s.doc,
            if attach {
                "Attach drawing to bone"
            } else {
                "Detach drawing from bone"
            },
            vec![Edit::RewireNodes {
                before,
                after,
                was_custom,
            }],
        );
    }
}

fn bone_drawings(
    ui: &mut egui::Ui,
    s: &mut Session,
    id: NodeId,
    thumb: &mut Option<Option<egui::TextureHandle>>,
) -> bool {
    let (attached, available): (Vec<_>, Vec<_>) = s
        .doc
        .layers
        .iter()
        .copied()
        .enumerate()
        .partition(|(_, layer)| s.doc.graph.drawing_bone(layer.node, s.frame) == Some(id));
    property_heading(ui, "Drawings");
    if !attached.is_empty() {
        if let Some(texture) = thumb.get_or_insert_with(|| bone_thumbnail(s, id, ui.ctx())) {
            egui::Frame::new()
                .fill(theme::BG_BASE)
                .corner_radius(theme::RADIUS_CARD)
                .inner_margin(8)
                .show(ui, |ui| {
                    let side = ui.available_width().clamp(24.0, 96.0);
                    ui.add(
                        egui::Image::new((texture.id(), Vec2::splat(side)))
                            .alt_text("Attached drawings at the current frame"),
                    );
                });
        } else {
            ui.label("No artwork at this frame");
        }
    }
    if attached.is_empty() {
        ui.label(egui::RichText::new("No drawings attached").color(theme::TEXT_SECONDARY));
    }
    for (index, layer) in attached {
        let remove = ui
            .horizontal(|ui| {
                let name = s.layer_name(index);
                ui.add_sized(
                    [(ui.available_width() - 64.0).max(16.0), 24.0],
                    egui::Label::new(name).truncate(),
                )
                .on_hover_text(name);
                ui.add_sized([56.0, 24.0], egui::Button::new("Detach"))
                    .clicked()
            })
            .inner;
        if remove {
            change_bone_drawing(s, id, layer, false);
            return true;
        }
    }
    let mut add = None;
    let new = ui
        .horizontal_wrapped(|ui| {
            if !available.is_empty() {
                let label = if ui.available_width() < 220.0 {
                    "Attach…"
                } else {
                    "Attach drawing"
                };
                ui.menu_button(label, |ui| {
                    egui::ScrollArea::vertical()
                        .max_height(230.0)
                        .show(ui, |ui| {
                            for (index, layer) in available {
                                let other = s.doc.graph.drawing_bone(layer.node, s.frame);
                                let result = ui.button(s.layer_name(index));
                                let result = if other.is_some() {
                                    result.on_hover_text("Move this drawing from its current bone")
                                } else {
                                    result
                                };
                                if result.clicked() {
                                    add = Some(layer);
                                    ui.close();
                                }
                            }
                        });
                })
                .response
                .on_hover_text("Attach an existing drawing");
            }
            ui.button("New drawing").clicked()
        })
        .inner;
    if let Some(layer) = add {
        change_bone_drawing(s, id, layer, true);
        return true;
    }
    if new {
        add_bone_drawing(s, id);
        return true;
    }
    false
}

fn bone_number(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut f32,
    range: std::ops::RangeInclusive<f32>,
    suffix: &str,
    mixed: bool,
) -> egui::Response {
    property_row(ui, label, |ui| {
        let field = egui::DragValue::new(value).range(range).speed(0.1);
        let field = if mixed {
            field.custom_formatter(|_, _| "Mixed".into())
        } else {
            field.suffix(suffix)
        };
        ui.add_sized([84.0, 24.0], field)
    })
}

fn stretch_name(mode: symphony_core::rig::Stretch) -> &'static str {
    use symphony_core::rig::Stretch;
    match mode {
        Stretch::Rigid => "Rigid",
        Stretch::Simple => "Simple stretch",
        Stretch::SquashStretch => "Squash & stretch",
    }
}

fn stretch_picker(
    ui: &mut egui::Ui,
    mode: Option<symphony_core::rig::Stretch>,
) -> Option<symphony_core::rig::Stretch> {
    use symphony_core::rig::Stretch;
    let mut value = mode;
    ui.label(
        egui::RichText::new(if mode.is_some() {
            "Stretch mode"
        } else {
            "Stretch mode · Mixed"
        })
        .color(theme::TEXT_SECONDARY),
    );
    let vertical = ui.available_width() < 220.0;
    widgets::segmented(
        ui,
        "stretch-mode",
        &mut value,
        &[
            (Some(Stretch::Rigid), stretch_name(Stretch::Rigid)),
            (Some(Stretch::Simple), stretch_name(Stretch::Simple)),
            (
                Some(Stretch::SquashStretch),
                stretch_name(Stretch::SquashStretch),
            ),
        ],
        vertical,
    );
    value.filter(|value| Some(*value) != mode)
}

fn edit_bone_batch(
    s: &mut Session,
    ids: &[NodeId],
    field: &str,
    merged: bool,
    mut change: impl FnMut(NodeId, &mut symphony_core::rig::Bone),
) {
    let mut roots = BTreeSet::new();
    let edits: Vec<_> = ids
        .iter()
        .filter_map(|&id| {
            let before = s.doc.graph.node(id)?.clone();
            let mut after = before.clone();
            let NodeKind::Bone(bone) = &mut after.kind else {
                return None;
            };
            change(id, bone);
            if before == after {
                return None;
            }
            roots.insert(s.doc.graph.rig_root(id));
            Some(Edit::SetNodeProperties { id, before, after })
        })
        .collect();
    if edits.is_empty() {
        return;
    }

    let mut solutions = Vec::new();
    if roots
        .iter()
        .any(|&root| s.doc.graph.rig_has_anchors(root, s.frame))
    {
        let mut preview = s.doc.graph.clone();
        for edit in &edits {
            if let Edit::SetNodeProperties { id, after, .. } = edit {
                if let Some(node) = preview.node_mut(*id) {
                    *node = after.clone();
                }
            }
        }
        for root in roots {
            if let Some(solution) = preview.solve_rig_translation(root, [0.0; 2], s.frame) {
                if !solution.feasible {
                    s.note("This change would move a locked anchor.");
                    return;
                }
                solutions.push((root, solution));
            }
        }
    }
    let key = format!("bone-properties-{field}-{ids:?}");
    if !merged {
        s.end_gesture();
    }
    for edit in edits {
        s.history
            .apply_merged(&mut s.doc, "Change bone properties", &key, edit);
    }
    for (root, solution) in solutions {
        super::rig_edit::apply_rig_solution(s, root, &key, solution, None);
    }
    if !merged {
        s.end_gesture();
    }
}

fn bone_batch_form(ui: &mut egui::Ui, s: &mut Session, ids: &[NodeId]) {
    fn shared<T: Copy + PartialEq>(
        bones: &[symphony_core::rig::Bone],
        value: impl Fn(&symphony_core::rig::Bone) -> T,
    ) -> Option<T> {
        let first = value(bones.first()?);
        bones
            .iter()
            .all(|bone| value(bone) == first)
            .then_some(first)
    }

    use symphony_core::rig::{AnchorKind, Bone, Stretch};
    let bones: Vec<_> = ids
        .iter()
        .filter_map(|&id| s.doc.graph.bone(id).cloned())
        .collect();
    let Some(first) = bones.first() else { return };
    property_heading(ui, "Posing");
    let mut stiffness = first.spring_stiffness * 100.0;
    let mixed = shared(&bones, |bone| bone.spring_stiffness).is_none();
    let response = bone_number(
        ui,
        "Spring stiffness",
        &mut stiffness,
        0.0..=100.0,
        "%",
        mixed,
    );
    if response.changed() {
        edit_bone_batch(s, ids, "stiffness", response.dragged(), |_, bone| {
            bone.spring_stiffness = stiffness / 100.0
        });
    }
    if response.drag_stopped() {
        s.end_gesture();
    }

    let anchor = shared(&bones, |bone| {
        bone.anchor.as_ref().map(|anchor| anchor.kind)
    });
    let mut selected = anchor;
    ui.label(
        egui::RichText::new(if anchor.is_some() {
            "Anchor"
        } else {
            "Anchor · Mixed"
        })
        .color(theme::TEXT_SECONDARY),
    );
    widgets::segmented(
        ui,
        "batch-anchor",
        &mut selected,
        &[
            (Some(None), "Off"),
            (Some(Some(AnchorKind::Locked)), "Locked"),
            (Some(Some(AnchorKind::Aim)), "Aim"),
        ],
        false,
    );
    if let Some(mode) = selected.filter(|mode| Some(*mode) != anchor) {
        let tips: BTreeMap<_, _> = ids
            .iter()
            .filter_map(|&id| {
                s.doc
                    .graph
                    .bone_pose(id, s.frame)
                    .map(|pose| (id, pose.tip()))
            })
            .collect();
        edit_bone_batch(s, ids, "anchor", false, |id, bone| {
            if let Some(&tip) = tips.get(&id) {
                super::rig_edit::update_anchor_mode(bone, mode, tip);
            }
        });
    }

    ui.add_space(8.0);
    property_heading(ui, "Deformation");
    let mut radius = first.bend_radius;
    let mixed = shared(&bones, |bone| bone.bend_radius).is_none();
    let response = bone_number(
        ui,
        "Bend radius",
        &mut radius,
        0.0..=100_000.0,
        " px",
        mixed,
    );
    if response.changed() {
        edit_bone_batch(s, ids, "bend-radius", response.dragged(), |_, bone| {
            bone.bend_radius = radius
        });
    }
    if response.drag_stopped() {
        s.end_gesture();
    }

    if let Some(mode) = stretch_picker(ui, shared(&bones, |bone| bone.stretch_mode)) {
        edit_bone_batch(s, ids, "stretch-mode", false, |_, bone| {
            bone.stretch_mode = mode
        });
    }
    if bones.iter().all(|bone| bone.stretch_mode != Stretch::Rigid) {
        let limits = shared(&bones, |bone| bone.stretch_limits.is_some());
        let mut limit = limits.unwrap_or(false);
        if widgets::checkbox(
            ui,
            &mut limit,
            if limits.is_some() {
                "Limit stretch"
            } else {
                "Limit stretch · Mixed"
            },
        )
        .changed()
        {
            edit_bone_batch(s, ids, "stretch-limit", false, |_, bone| {
                bone.stretch_limits = limit.then(|| bone.stretch_limits.unwrap_or([0.5, 2.0]));
            });
        }
        if limits == Some(true) {
            for (axis, name, range) in [(0, "Shortest", 0.05..=1.0), (1, "Longest", 1.0..=20.0)] {
                let get = |bone: &Bone| {
                    bone.stretch_limits.expect("all selected bones have limits")[axis]
                };
                let mut value = get(first);
                let response = bone_number(
                    ui,
                    name,
                    &mut value,
                    range,
                    "×",
                    shared(&bones, get).is_none(),
                );
                if response.changed() {
                    edit_bone_batch(s, ids, name, response.dragged(), |_, bone| {
                        if let Some(limits) = &mut bone.stretch_limits {
                            limits[axis] = value;
                        }
                    });
                }
                if response.drag_stopped() {
                    s.end_gesture();
                }
            }
        }
    }
}

fn property_form(ui: &mut egui::Ui, s: &mut Session, draft: &mut PropertyDraft) {
    let mut build_mesh_now = false;
    ui.add_sized(
        [ui.available_width(), 28.0],
        egui::TextEdit::singleline(&mut draft.node.name)
            .font(egui::FontId::proportional(theme::SIZE_HEADING))
            .hint_text("Node name"),
    )
    .on_hover_text("Node name");
    ui.horizontal_wrapped(|ui| {
        ui.label(
            egui::RichText::new(kind_label(&draft.node.kind))
                .font(theme::dense())
                .color(theme::TEXT_SECONDARY),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            widgets::checkbox(ui, &mut draft.node.enabled, "Enabled");
        });
    });
    ui.add_space(8.0);
    let skeleton_anchored = matches!(draft.node.kind, NodeKind::Skeleton(_))
        && s.doc.graph.rig_has_anchors(draft.id, s.frame);
    match &mut draft.node.kind {
        NodeKind::Peg(peg) | NodeKind::Skeleton(peg) => {
            property_heading(ui, "Transform");
            let x = peg_value(ui, "Position X", &mut peg.x, s.frame, s.animate);
            let y = peg_value(ui, "Position Y", &mut peg.y, s.frame, s.animate);
            if skeleton_anchored {
                if x || y {
                    super::rig_edit::translate_skeleton(
                        s,
                        draft.id,
                        [peg.x.at(s.frame), peg.y.at(s.frame)],
                    );
                    if let Some(current) = PropertyDraft::new(s, draft.id) {
                        *draft = current;
                    }
                    return;
                }
            } else {
                peg_value(ui, "Rotation", &mut peg.rotation, s.frame, s.animate);
                ui.collapsing("Scale, pivot and depth", |ui| {
                    peg_value(ui, "Depth Z", &mut peg.z, s.frame, s.animate);
                    for (label, value) in
                        [("Scale X", &mut peg.scale_x), ("Scale Y", &mut peg.scale_y)]
                    {
                        let mut draft = value
                            .clone()
                            .unwrap_or(symphony_core::anim::Value::Constant(1.0));
                        if peg_value(ui, label, &mut draft, s.frame, s.animate) {
                            *value = Some(draft);
                        }
                    }
                    ui.horizontal(|ui| {
                        ui.label("Pivot");
                        ui.add(egui::DragValue::new(&mut peg.pivot.0).prefix("X "));
                        ui.add(egui::DragValue::new(&mut peg.pivot.1).prefix("Y "));
                    });
                });
            }
            if let Some((chart, _)) = s.doc.graph.followed_chart(draft.id) {
                ui.add_space(8.0);
                property_heading(ui, "Motion");
                let name = s.doc.graph.node(chart).map_or("", |n| n.name.as_str());
                ui.label(format!("Follows {name}"));
                ui.horizontal(|ui| {
                    widgets::checkbox(ui, &mut peg.follow.rotation, "Rotation")
                        .on_hover_text("Turn with the chart's rotation.");
                    widgets::checkbox(ui, &mut peg.follow.scale, "Scale")
                        .on_hover_text("Grow and shrink with the chart's scale.");
                });
            }
        }
        NodeKind::Bone(bone) => {
            if bone_drawings(ui, s, draft.id, &mut draft.bone_thumb) {
                if let Some(current) = PropertyDraft::new(s, draft.id) {
                    *draft = current;
                }
                return;
            }
            ui.add_space(8.0);
            property_heading(ui, "Posing");
            let mut percent = bone.spring_stiffness * 100.0;
            if bone_number(
                ui,
                "Spring stiffness",
                &mut percent,
                0.0..=100.0,
                "%",
                false,
            )
            .on_hover_text("Resists rotating away from the rest angle.")
            .changed()
            {
                bone.spring_stiffness = percent / 100.0;
            }
            use symphony_core::rig::AnchorKind;
            let parent = s.doc.graph.parent_bone(draft.id);
            let root = s.doc.graph.rig_root(draft.id);
            let anchored = s.doc.graph.rig_has_anchors(root, s.frame);
            ui.label(egui::RichText::new("Anchor").color(theme::TEXT_SECONDARY));
            let mut kind = bone.anchor.as_ref().map(|anchor| anchor.kind);
            widgets::segmented(
                ui,
                "anchor-mode",
                &mut kind,
                &[
                    (None, "Off"),
                    (Some(AnchorKind::Locked), "Locked"),
                    (Some(AnchorKind::Aim), "Aim"),
                ],
                false,
            );
            if let Some(kind) = kind {
                ui.label(match kind {
                    AnchorKind::Locked => "This tip stays planted while you pose the rig.",
                    AnchorKind::Aim => "The rig reaches toward this target.",
                });
            }
            if kind != bone.anchor.as_ref().map(|anchor| anchor.kind) {
                super::rig_edit::set_anchor_mode(s, draft.id, kind);
                if let Some(current) = PropertyDraft::new(s, draft.id) {
                    *draft = current;
                }
                return;
            }
            if let Some(anchor) = &bone.anchor {
                let mut target = anchor.target.each_ref().map(|value| value.at(s.frame));
                let changed = ui
                    .horizontal(|ui| {
                        let x = ui
                            .add(egui::DragValue::new(&mut target[0]).prefix("Target X "))
                            .changed();
                        let y = ui
                            .add(egui::DragValue::new(&mut target[1]).prefix("Y "))
                            .changed();
                        x || y
                    })
                    .inner;
                if changed {
                    super::rig_edit::pose_to(s, draft.id, target, false);
                    if let Some(current) = PropertyDraft::new(s, draft.id) {
                        *draft = current;
                    }
                    return;
                }
            } else if !anchored {
                let controller = s.doc.graph.smart_controller(draft.id, s.frame);
                if let Some(joint) = bone.smart.as_mut().filter(|_| parent.is_some()) {
                    peg_value(ui, "Target X", &mut joint.target[0], s.frame, s.animate);
                    peg_value(ui, "Target Y", &mut joint.target[1], s.frame, s.animate);
                    ui.checkbox(&mut joint.flip, "Flip bend");
                } else if let Some(owner) = controller.filter(|&id| id != draft.id) {
                    if ui.button("Select smart joint target").clicked() {
                        s.select_graph_node(owner, true);
                    }
                } else {
                    peg_value(ui, "Rotation", &mut bone.rotation, s.frame, s.animate);
                }
                if let Some(label) = smart_joint_label(s, draft.id) {
                    if ui.button(label).clicked() {
                        toggle_smart_joint(s, draft.id);
                        if let Some(current) = PropertyDraft::new(s, draft.id) {
                            *draft = current;
                        }
                        return;
                    }
                }
            }
            ui.collapsing("Rest pose and deformation", |ui| {
                if parent.is_none() {
                    ui.horizontal(|ui| {
                        ui.label("Root origin");
                        ui.add(egui::DragValue::new(&mut bone.origin[0]).prefix("X "));
                        ui.add(egui::DragValue::new(&mut bone.origin[1]).prefix("Y "));
                    });
                }
                ui.add(
                    egui::DragValue::new(&mut bone.length)
                        .range(1.0..=100_000.0)
                        .prefix("Length "),
                );
                ui.add(
                    egui::DragValue::new(&mut bone.rest_angle)
                        .prefix("Rest angle ")
                        .suffix("°"),
                );
                if parent.is_some() {
                    ui.add(
                        egui::DragValue::new(&mut bone.bend_radius)
                            .range(0.0..=100_000.0)
                            .prefix("Bend radius "),
                    )
                    .on_hover_text("How softly the artwork bends at this joint.");
                }
            });
            ui.collapsing("Stretch", |ui| {
                use symphony_core::rig::Stretch;
                if let Some(mode) = stretch_picker(ui, Some(bone.stretch_mode)) {
                    bone.stretch_mode = mode;
                }
                if bone.stretch_mode != Stretch::Rigid {
                    if !anchored {
                        peg_value(ui, "Stretch", &mut bone.stretch, s.frame, s.animate);
                    }
                    let mut limited = bone.stretch_limits.is_some();
                    widgets::checkbox(ui, &mut limited, "Limit")
                        .on_hover_text("Resistance grows as it nears its shortest or longest.");
                    if limited != bone.stretch_limits.is_some() {
                        bone.stretch_limits = limited.then_some([0.5, 2.0]);
                    }
                    if let Some([lo, hi]) = &mut bone.stretch_limits {
                        ui.horizontal(|ui| {
                            ui.add(
                                egui::DragValue::new(lo)
                                    .range(0.05..=1.0)
                                    .speed(0.01)
                                    .prefix("Shortest ×"),
                            );
                            ui.add(
                                egui::DragValue::new(hi)
                                    .range(1.0..=20.0)
                                    .speed(0.01)
                                    .prefix("Longest ×"),
                            );
                        });
                    }
                }
            });
        }
        NodeKind::MeshWarp(warp) => {
            use symphony_core::warp::{WarpKind, WarpLines};
            ui.label("Kind");
            widgets::segmented(
                ui,
                "warp-kind",
                &mut warp.kind,
                &[(WarpKind::Grid, "Grid"), (WarpKind::Points, "Points")],
                false,
            );
            ui.label("Lines");
            widgets::segmented(
                ui,
                "warp-lines",
                &mut warp.lines,
                &[
                    (WarpLines::Paths, "Paths"),
                    (WarpLines::Shape, "Whole shape"),
                ],
                false,
            );
            ui.label(
                egui::RichText::new(match warp.lines {
                    WarpLines::Paths => "Lines bend but keep their thickness.",
                    WarpLines::Shape => {
                        "Lines thicken where the warp stretches and thin where it squeezes, \
                         as if seen in perspective."
                    }
                })
                .font(theme::dense())
                .color(theme::TEXT_SECONDARY),
            );
            ui.add_space(6.0);
            match warp.kind {
                WarpKind::Grid => {
                    let (mut rows, mut columns) = (warp.rows, warp.columns);
                    ui.horizontal(|ui| {
                        ui.label("Rows");
                        ui.add(
                            egui::DragValue::new(&mut rows)
                                .range(1..=symphony_core::warp::MAX_CELLS),
                        );
                        ui.label("Columns");
                        ui.add(
                            egui::DragValue::new(&mut columns)
                                .range(1..=symphony_core::warp::MAX_CELLS),
                        );
                    });
                    if (rows, columns) != (warp.rows, warp.columns) {
                        warp.resize(rows, columns);
                    }
                    peg_value(ui, "(x) Axis", &mut warp.x, s.frame, s.animate);
                    peg_value(ui, "(y) Axis", &mut warp.y, s.frame, s.animate);
                    peg_value(ui, "Width", &mut warp.width, s.frame, s.animate);
                    peg_value(ui, "Height", &mut warp.height, s.frame, s.animate);
                    egui::ComboBox::from_label("Deformation Quality")
                        .selected_text(warp.quality.title())
                        .show_ui(ui, |ui| {
                            for q in symphony_core::warp::WarpQuality::ALL {
                                ui.selectable_value(&mut warp.quality, q, q.title());
                            }
                        });
                    ui.label(
                        "Drag the grid's points in the Camera view (Transform tool) to bend the \
                         artwork under it.",
                    );
                    if ui
                        .button("Reset points")
                        .on_hover_text("Flatten the grid (removes the points' keys too).")
                        .clicked()
                    {
                        warp.points.fill([
                            symphony_core::anim::Value::Constant(0.0),
                            symphony_core::anim::Value::Constant(0.0),
                        ]);
                    }
                }
                WarpKind::Points => {
                    ui.label(format!(
                        "{} point{}{}",
                        warp.pins.len(),
                        if warp.pins.len() == 1 { "" } else { "s" },
                        if warp.mesh.is_some() {
                            ", mesh built"
                        } else {
                            ""
                        }
                    ));
                    ui.add_enabled_ui(warp.mesh.is_none(), |ui| {
                        widgets::precise_slider(
                            ui,
                            egui::Slider::new(&mut warp.density, 6.0..=120.0)
                                .text("Mesh detail")
                                .suffix(" px"),
                        )
                        .on_hover_text("Smaller triangles bend more finely.");
                    });
                    ui.label(if warp.mesh.is_none() {
                        "In the Camera view (Transform tool), click the artwork to place points \
                         at the joints and ends you'll move; Alt-click removes one. Then build \
                         the mesh."
                    } else {
                        "Drag the points in the Camera view: the artwork bends as rigidly as it \
                         can, and only what's connected to a point follows it."
                    });
                    ui.horizontal(|ui| {
                        if warp.mesh.is_none() {
                            if ui
                                .add_enabled(!warp.pins.is_empty(), egui::Button::new("Build mesh"))
                                .clicked()
                            {
                                build_mesh_now = true;
                            }
                        } else if ui.button("Edit points").clicked() {
                            warp.mesh = None;
                        }
                        if ui
                            .add_enabled(!warp.pins.is_empty(), egui::Button::new("Clear points"))
                            .clicked()
                        {
                            warp.pins.clear();
                            warp.pin_moves.clear();
                            warp.mesh = None;
                        }
                    });
                }
            }
        }
        NodeKind::Drawing { column } => {
            if let Some(source) = s.doc.xsheet.column(*column) {
                property_heading(ui, "Exposure");
                property_row(ui, "Column", |ui| ui.label(&source.name));
                property_row(ui, "Current drawing", |ui| {
                    ui.label(
                        s.doc
                            .layer_of(*column)
                            .and_then(|layer| s.drawing_name(layer, s.frame))
                            .unwrap_or("Empty"),
                    )
                });
            }
        }
        NodeKind::Composite => {
            let count = s
                .doc
                .graph
                .links()
                .iter()
                .filter(|link| link.to == draft.id)
                .count();
            ui.label(format!("{count} connected inputs"));
            ui.label("Lower port numbers are in front.");
        }
        NodeKind::Display => {
            let input = s
                .doc
                .graph
                .input(draft.id, 0)
                .and_then(|from| s.doc.graph.node(from));
            property_heading(ui, "Camera");
            property_row(ui, "Input", |ui| {
                ui.label(input.map_or("Disconnected", node_name))
            });
            property_row(ui, "Stage", |ui| {
                ui.label(format!(
                    "{} × {}",
                    s.doc.settings.width, s.doc.settings.height
                ))
            });
            ui.add_space(8.0);
            if let Some(fov) = &mut draft.fov {
                let vertical = ui.available_width() < 168.0;
                widgets::segmented(
                    ui,
                    "lens-preset",
                    fov,
                    &[(35.0, "Tight"), (60.0, "Standard"), (90.0, "Wide")],
                    vertical,
                );
                egui::CollapsingHeader::new(format!("Lens · {:.1}°", *fov))
                    .id_salt("camera-lens")
                    .show(ui, |ui| {
                        widgets::precise_slider(
                            ui,
                            egui::Slider::new(fov, 20.0..=120.0)
                                .step_by(0.1)
                                .text("Horizontal FOV")
                                .suffix("°"),
                        );
                    });
            }
            if let Some(motion) = &mut draft.motion {
                ui.collapsing("Camera movement", |ui| {
                    peg_value(ui, "Camera X", &mut motion.x, s.frame, s.animate);
                    peg_value(ui, "Camera Y", &mut motion.y, s.frame, s.animate);
                    peg_value(ui, "Dolly Z", &mut motion.dolly, s.frame, s.animate);
                });
            }
        }
        NodeKind::Effect(Effect::Blur { radius, mode }) => {
            egui::ComboBox::from_label("Blur type")
                .selected_text(mode.title())
                .show_ui(ui, |ui| {
                    for choice in BlurMode::ALL {
                        if ui
                            .selectable_label(mode.title() == choice.title(), choice.title())
                            .clicked()
                            && mode.title() != choice.title()
                        {
                            *mode = choice;
                        }
                    }
                });
            let (label, suffix) = match mode {
                BlurMode::Motion { .. } => ("Distance", " px"),
                BlurMode::RadialSpin => ("Rotation", "°"),
                BlurMode::RadialZoom => ("Zoom amount", "%"),
                _ => ("Radius", " px"),
            };
            widgets::precise_slider(
                ui,
                egui::Slider::new(radius, 0.0..=128.0)
                    .step_by(0.1)
                    .text(label)
                    .suffix(suffix),
            );
            if let BlurMode::Motion { angle } = mode {
                widgets::precise_slider(
                    ui,
                    egui::Slider::new(angle, 0.0..=360.0)
                        .step_by(1.0)
                        .text("Angle")
                        .suffix("°"),
                );
            }
        }
        NodeKind::Effect(Effect::Grain { amount, seed }) => {
            widgets::precise_slider(
                ui,
                egui::Slider::new(amount, 0.0..=1.0)
                    .step_by(0.01)
                    .text("Amount"),
            );
            property_row(ui, "Seed", |ui| {
                ui.add_sized([84.0, 24.0], egui::DragValue::new(seed))
            });
        }
        NodeKind::Effect(Effect::Mask { inverted }) => {
            ui.checkbox(inverted, "Invert matte")
                .on_hover_text("Cut away pixels covered by the matte.");
        }
        NodeKind::Effect(Effect::Blend { mode, amount }) => {
            egui::ComboBox::from_label("Mode")
                .selected_text(mode.title())
                .show_ui(ui, |ui| {
                    for m in BlendMode::ALL {
                        ui.selectable_value(mode, m, m.title());
                    }
                });
            let mut percent = *amount * 100.0;
            if widgets::precise_slider(
                ui,
                egui::Slider::new(&mut percent, 0.0..=100.0)
                    .step_by(1.0)
                    .text("Opacity")
                    .suffix("%"),
            )
            .changed()
            {
                *amount = percent / 100.0;
            }
            ui.label(
                egui::RichText::new("Blends what's connected above onto everything behind it.")
                    .font(theme::dense())
                    .color(theme::TEXT_SECONDARY),
            );
        }
        NodeKind::Effect(Effect::Opacity { amount }) => {
            let mut percent = *amount * 100.0;
            if widgets::precise_slider(
                ui,
                egui::Slider::new(&mut percent, 0.0..=100.0)
                    .step_by(1.0)
                    .text("Opacity")
                    .suffix("%"),
            )
            .changed()
            {
                *amount = percent / 100.0;
            }
        }
        NodeKind::Gradient(gradient) => {
            ui.horizontal(|ui| {
                super::colour_picker::swatch(ui, &mut gradient.start);
                ui.label("Start");
                super::colour_picker::swatch(ui, &mut gradient.end);
                ui.label("End");
            });
            widgets::precise_slider(
                ui,
                egui::Slider::new(&mut gradient.angle, 0.0..=360.0)
                    .step_by(0.1)
                    .text("Angle")
                    .suffix("°"),
            );
        }
        NodeKind::Chart(chart) => chart_form(ui, s, chart, draft.id),
        _ => {}
    }
    // Pegs apply live, so the drawing moves as you drag a value: their
    // values carry their own keys (Auto-key writes them as you edit), so
    // the node itself is what changes. One drag is one undo step.
    if matches!(
        draft.node.kind,
        NodeKind::Peg(_)
            | NodeKind::Skeleton(_)
            | NodeKind::Bone(_)
            | NodeKind::MeshWarp(_)
            | NodeKind::Chart(_)
    ) && draft.node.kind != draft.initial.kind
        && draft.node.enabled == draft.initial.enabled
    {
        if let Some(before) = s.doc.graph.node(draft.id).cloned() {
            let mut after = before.clone();
            after.kind = draft.node.kind.clone();
            let key = format!("node-props-{}", draft.id.0);
            let root = s.doc.graph.rig_root(draft.id);
            let mut anchored_solution = None;
            if matches!(after.kind, NodeKind::Bone(_)) && s.doc.graph.rig_has_anchors(root, s.frame)
            {
                let mut preview = s.doc.graph.clone();
                if let Some(node) = preview.node_mut(draft.id) {
                    *node = after.clone();
                }
                anchored_solution = preview.solve_rig_translation(root, [0.0; 2], s.frame);
                if anchored_solution
                    .as_ref()
                    .is_some_and(|solution| !solution.feasible)
                {
                    s.note("This change would move a locked anchor.");
                    if let Some(current) = PropertyDraft::new(s, draft.id) {
                        *draft = current;
                    }
                    return;
                }
            }
            let name = if matches!(after.kind, NodeKind::Chart(_)) {
                "Change chart"
            } else {
                "Change transform"
            };
            s.history.apply_merged(
                &mut s.doc,
                name,
                &key,
                Edit::SetNodeProperties {
                    id: draft.id,
                    before,
                    after,
                },
            );
            if let Some(solution) = anchored_solution {
                super::rig_edit::apply_rig_solution(s, root, &key, solution, None);
                if let Some(current) = PropertyDraft::new(s, draft.id) {
                    *draft = current;
                }
                return;
            }
            draft.initial.kind = draft.node.kind.clone();
        }
    }
    // Building a mesh needs the artwork under the node: done on the
    // scene, then the form catches up.
    if build_mesh_now {
        if let Err(e) = super::warp_edit::build_mesh(s, draft.id) {
            s.note(e);
        }
        if let Some(current) = PropertyDraft::new(s, draft.id) {
            *draft = current;
        }
        return;
    }
    let camera_changed = draft.fov != draft.initial_fov || draft.motion != draft.initial_motion;
    let changed =
        (draft.node != draft.initial || camera_changed) && !draft.node.name.trim().is_empty();
    let keyed = s.doc.graph.has_key(draft.id, s.frame);
    let has_staged_controls = !matches!(
        draft.node.kind,
        NodeKind::Peg(_)
            | NodeKind::Skeleton(_)
            | NodeKind::Bone(_)
            | NodeKind::MeshWarp(_)
            | NodeKind::Chart(_)
    );
    // A chart's timing lives in its points, not in node keys.
    let keyable = !matches!(draft.node.kind, NodeKind::Chart(_));
    ui.add_space(8.0);
    ui.horizontal_wrapped(|ui| {
        if (has_staged_controls || changed)
            && widgets::primary_button(ui, "Apply changes", changed).clicked()
        {
            let animatable_changed = draft.node.enabled != draft.initial.enabled
                || draft.node.kind != draft.initial.kind
                || camera_changed;
            let use_key = animatable_changed && (s.animate || s.doc.graph.keys(draft.id).is_some());
            if !animatable_changed {
                apply_name(s, draft);
            } else {
                apply_properties(s, draft, use_key, false);
            }
        }
        if !keyable {
            return;
        }
        if ui
            .add_enabled(
                !draft.node.name.trim().is_empty(),
                egui::Button::new(if keyed { "Update key" } else { "Set key" }),
            )
            .on_hover_text(format!("Frame {}", s.frame))
            .clicked()
        {
            apply_properties(s, draft, true, false);
        }
        if ui
            .add_enabled(keyed, egui::Button::new("Delete key"))
            .clicked()
        {
            apply_properties(s, draft, false, true);
        }
    });
}

/// A Smart Chart's own properties: how it shows, and what's on it. The
/// points themselves are edited on the canvas.
fn chart_form(
    ui: &mut egui::Ui,
    s: &mut Session,
    chart: &mut symphony_core::chart::Chart,
    id: NodeId,
) {
    use symphony_core::chart::Role;
    property_heading(ui, "Smart Chart");
    property_row(ui, "Colour", |ui| {
        let shown = super::chart_edit::colour(chart, id);
        let mut rgb = [shown.r(), shown.g(), shown.b()];
        let before = rgb;
        super::colour_picker::rgb(ui, &mut rgb);
        if rgb != before {
            chart.colour = Some(rgb);
        }
    });
    widgets::checkbox(ui, &mut chart.visible, "Show on the canvas")
        .on_hover_text("Hiding only hides the guide; what it drives is unchanged.");
    widgets::checkbox(ui, &mut chart.locked, "Lock")
        .on_hover_text("Shown, but its points can't be moved.");
    ui.add_space(6.0);
    let keys = chart.points.iter().filter(|p| p.role == Role::Key).count();
    let breakdowns = chart.points.len() - keys;
    let summary = match chart.range() {
        Some((a, b)) if a != b => format!(
            "Frames {a} to {b} · {keys} key{} · {breakdowns} breakdown{}",
            if keys == 1 { "" } else { "s" },
            if breakdowns == 1 { "" } else { "s" },
        ),
        Some((a, _)) => format!("One point, on frame {a}"),
        None => "No points yet".to_owned(),
    };
    ui.label(summary);
    super::chart_edit::follow_toggle(ui, s, id);
    // The point on this frame, with every setting the canvas bar has.
    if let Some(pid) = chart.point_on(s.frame).map(|p| p.id) {
        ui.add_space(8.0);
        property_heading(ui, &format!("Point on frame {}", s.frame));
        super::chart_edit::point_controls(ui, chart, pid);
    } else {
        ui.label(
            egui::RichText::new(
                "With the Transform tool, click the canvas to place the chart on the \
                 current frame; drag a point to move it.",
            )
            .font(theme::dense())
            .color(theme::TEXT_SECONDARY),
        );
    }
}

fn apply_name(s: &mut Session, draft: &mut PropertyDraft) {
    if let Some(before) = s.doc.graph.node(draft.id).cloned() {
        let mut after = before.clone();
        after.name.clone_from(&draft.node.name);
        if before != after {
            s.history.apply(
                &mut s.doc,
                "Rename node",
                vec![Edit::SetNodeProperties {
                    id: draft.id,
                    before,
                    after,
                }],
            );
        }
        if let Some(current) = PropertyDraft::new(s, draft.id) {
            *draft = current;
        }
    }
}

fn apply_properties(s: &mut Session, draft: &mut PropertyDraft, key: bool, delete: bool) {
    let id = draft.id;
    let Some(before) = s.doc.graph.node(id).cloned() else {
        return;
    };
    let mut after = if key || delete {
        before.clone()
    } else {
        draft.node.clone()
    };
    let mut edits = Vec::new();
    if key {
        after.name.clone_from(&draft.node.name);
    }
    if key || delete {
        let before_keys = s.doc.graph.keys(id).cloned().unwrap_or_default();
        let mut after_keys = before_keys.clone();
        if delete {
            after_keys.remove(&s.frame);
        } else {
            after_keys.insert(s.frame, NodeKey::from_node(&draft.node));
        }
        if before_keys != after_keys {
            edits.push(Edit::SetNodeKeys {
                id,
                before: before_keys,
                after: after_keys,
            });
        }
        if let NodeKind::Peg(peg) | NodeKind::Skeleton(peg) = if delete {
            &before.kind
        } else {
            &draft.node.kind
        } {
            use symphony_core::anim::Value;
            let mut peg = peg.clone();
            if !delete {
                peg.scale_x.get_or_insert(Value::Constant(1.0));
                peg.scale_y.get_or_insert(Value::Constant(1.0));
            }
            for value in [&mut peg.x, &mut peg.y, &mut peg.z, &mut peg.rotation]
                .into_iter()
                .chain(peg.scale_x.iter_mut())
                .chain(peg.scale_y.iter_mut())
                .chain(peg.tilt.iter_mut().flat_map(|angles| angles.iter_mut()))
            {
                if delete {
                    value.remove_key(s.frame);
                } else {
                    value.set_key(s.frame, value.at(s.frame));
                }
            }
            after.kind = if matches!(before.kind, NodeKind::Skeleton(_)) {
                NodeKind::Skeleton(peg)
            } else {
                NodeKind::Peg(peg)
            };
        }
        if let NodeKind::Bone(bone) = if delete {
            &before.kind
        } else {
            &draft.node.kind
        } {
            let mut bone = bone.clone();
            for value in bone.values_mut() {
                if delete {
                    value.remove_key(s.frame);
                } else {
                    value.set_key(s.frame, value.at(s.frame));
                }
            }
            after.kind = NodeKind::Bone(bone);
        }
        if let NodeKind::MeshWarp(warp) = if delete {
            &before.kind
        } else {
            &draft.node.kind
        } {
            let mut warp = warp.clone();
            for value in [&mut warp.x, &mut warp.y, &mut warp.width, &mut warp.height]
                .into_iter()
                .chain(warp.points.iter_mut().flatten())
            {
                if delete {
                    value.remove_key(s.frame);
                } else {
                    value.set_key(s.frame, value.at(s.frame));
                }
            }
            after.kind = NodeKind::MeshWarp(warp);
        }
    }
    if let Some(fov) = draft.fov {
        use symphony_core::anim::Value;
        let before = s.doc.camera_fov.clone();
        let mut after = before.clone();
        if delete {
            after.remove_key(s.frame);
        } else if key
            || (fov != draft.initial_fov.unwrap_or(fov) && matches!(before, Value::Keys(_)))
        {
            after.set_key(s.frame, fov);
        } else if fov != draft.initial_fov.unwrap_or(fov) {
            after = Value::Constant(fov);
        }
        if let Some(motion) = &draft.motion {
            let before = s.doc.camera_motion.clone();
            let mut after = if delete {
                before.clone()
            } else {
                motion.clone()
            };
            for value in [&mut after.x, &mut after.y, &mut after.dolly] {
                if delete {
                    value.remove_key(s.frame);
                } else if key {
                    value.set_key(s.frame, value.at(s.frame));
                }
            }
            if before != after {
                edits.push(Edit::SetCameraMotion { before, after });
            }
        }
        if before != after {
            edits.push(Edit::SetCameraFov { before, after });
        }
    }
    if before != after {
        edits.push(Edit::SetNodeProperties { id, before, after });
    }
    let action = if delete {
        "Delete node key"
    } else if key {
        "Set node key"
    } else {
        "Change node properties"
    };
    s.history.apply(&mut s.doc, action, edits);
    if let Some(current) = PropertyDraft::new(s, id) {
        *draft = current;
    }
}

fn peg_value(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut symphony_core::anim::Value,
    frame: u32,
    animate: bool,
) -> bool {
    use symphony_core::anim::Value;
    let mut number = value.at(frame);
    let changed = property_row(ui, label, |ui| {
        ui.add_sized([84.0, 24.0], egui::DragValue::new(&mut number).speed(0.1))
            .changed()
    });
    if changed {
        match value {
            Value::Constant(current) if !animate => *current = number,
            _ => value.set_key(frame, number),
        }
    }
    changed
}

fn preview(ctx: &egui::Context, s: &Session, state: &mut NodeViewState) {
    if !state.preview_open {
        return;
    }
    egui::Window::new("Display render")
        .open(&mut state.preview_open)
        .collapsible(false)
        .show(ctx, |ui| {
            let refresh = ui.button("Render current frame").clicked();
            if refresh || state.preview.is_none() {
                let w = s.doc.settings.width.max(1);
                let h = s.doc.settings.height.max(1);
                let scale = (640.0 / w.max(h) as f32).min(1.0);
                let (w, h) = (
                    (w as f32 * scale).round().max(1.0) as u32,
                    (h as f32 * scale).round().max(1.0) as u32,
                );
                if let Some(image) = symphony_render::render_frame(&s.doc, s.frame, w, h) {
                    state.preview = Some(ctx.load_texture(
                        "node-display-preview",
                        egui::ColorImage::from_rgba_unmultiplied(
                            [w as usize, h as usize],
                            image.as_raw(),
                        ),
                        egui::TextureOptions::LINEAR,
                    ));
                }
            }
            if let Some(texture) = &state.preview {
                ui.image(texture);
            } else {
                ui.label("Couldn’t render the current frame.");
            }
        });
}

/// The space kept between node cards.
const NODE_GAP: f32 = 18.0;

/// Where nodes sit before they're moved: pegs over their drawings in a
/// row, then the Composite and the Camera.
fn default_positions(s: &Session) -> BTreeMap<NodeId, Vec2> {
    let mut positions = BTreeMap::new();
    let middle = s.doc.layers.len().saturating_sub(1) as f32 / 2.0;
    for (index, layer) in s.doc.layers.iter().enumerate() {
        let x = (index as f32 - middle) * (W + 26.0);
        positions.insert(layer.peg, Vec2::new(x, 43.0));
        positions.insert(layer.node, Vec2::new(x, 120.0));
    }
    positions.insert(s.doc.composite, Vec2::new(0.0, 209.0));
    positions.insert(s.doc.display, Vec2::new(0.0, 291.0));
    positions
}

/// A node's centre in graph space: stored, else its default place.
fn node_position(s: &Session, id: NodeId, defaults: &BTreeMap<NodeId, Vec2>) -> Vec2 {
    s.doc
        .node_positions
        .get(&id)
        .map(|p| Vec2::new(p[0], p[1]))
        .or_else(|| defaults.get(&id).copied())
        .unwrap_or_else(|| Vec2::new(W + 36.0, 43.0 + id.0 as f32 * 58.0))
}

/// Push overlapping cards apart (along whichever way they overlap less)
/// until none overlap, with a gap. `fixed` cards stay; a pair with one
/// fixed card moves only the other.
fn relax(
    positions: &mut BTreeMap<NodeId, Vec2>,
    width: impl Fn(NodeId) -> f32,
    fixed: &BTreeSet<NodeId>,
) {
    let ids: Vec<NodeId> = positions.keys().copied().collect();
    let widths: Vec<f32> = ids.iter().map(|&id| width(id)).collect();
    for _ in 0..80 {
        let mut moved = false;
        for i in 0..ids.len() {
            for j in i + 1..ids.len() {
                let (a, b) = (positions[&ids[i]], positions[&ids[j]]);
                let d = b - a;
                let ox = (widths[i] + widths[j]) / 2.0 + NODE_GAP - d.x.abs();
                let oy = H + NODE_GAP - d.y.abs();
                if ox <= 0.0 || oy <= 0.0 {
                    continue;
                }
                // Out along the smaller overlap (ties go sideways).
                let push = if ox <= oy * 1.2 {
                    let side = if d.x != 0.0 {
                        d.x.signum()
                    } else if i < j {
                        1.0
                    } else {
                        -1.0
                    };
                    Vec2::new(ox * side, 0.0)
                } else {
                    let side = if d.y != 0.0 { d.y.signum() } else { 1.0 };
                    Vec2::new(0.0, oy * side)
                };
                let (fa, fb) = (fixed.contains(&ids[i]), fixed.contains(&ids[j]));
                let (ka, kb) = match (fa, fb) {
                    (true, true) => continue,
                    (true, false) => (0.0, 1.0),
                    (false, true) => (1.0, 0.0),
                    (false, false) => (0.5, 0.5),
                };
                *positions.get_mut(&ids[i]).unwrap() -= push * ka;
                *positions.get_mut(&ids[j]).unwrap() += push * kb;
                moved = true;
            }
        }
        if !moved {
            break;
        }
    }
    // A card squeezed between others can bounce between them: give any
    // free card still overlapping the nearest clear spot, trying sideways
    // first (the graph flows top to bottom).
    let clear = |positions: &BTreeMap<NodeId, Vec2>, k: usize, at: Vec2| {
        ids.iter().enumerate().all(|(j, id)| {
            if j == k {
                return true;
            }
            let d = positions[id] - at;
            d.x.abs() >= (widths[k] + widths[j]) / 2.0 + NODE_GAP - 0.01
                || d.y.abs() >= H + NODE_GAP - 0.01
        })
    };
    for k in 0..ids.len() {
        if fixed.contains(&ids[k]) {
            continue;
        }
        let here = positions[&ids[k]];
        if clear(positions, k, here) {
            continue;
        }
        let step = 12.0;
        let spot = (1..200)
            .flat_map(|n| {
                let d = n as f32 * step;
                [
                    Vec2::new(d, 0.0),
                    Vec2::new(-d, 0.0),
                    Vec2::new(0.0, d),
                    Vec2::new(0.0, -d),
                ]
            })
            .map(|offset| here + offset)
            .find(|&at| clear(positions, k, at));
        if let Some(at) = spot {
            positions.insert(ids[k], at);
        }
    }
}

/// The moves (as edits) that clear every overlap, with `overrides`
/// standing in for those nodes' positions and `fixed` nodes staying put.
fn make_room(
    s: &Session,
    fixed: &BTreeSet<NodeId>,
    overrides: &BTreeMap<NodeId, Vec2>,
) -> Vec<Edit> {
    let defaults = default_positions(s);
    let before: BTreeMap<NodeId, Vec2> = s
        .doc
        .graph
        .nodes()
        .map(|(id, _)| (id, node_position(s, id, &defaults)))
        .collect();
    let mut after = before.clone();
    after.extend(overrides.iter().map(|(&id, &p)| (id, p)));
    relax(&mut after, |id| card_width(&s.doc.graph, id), fixed);
    after
        .into_iter()
        .filter(|(id, p)| !overrides.contains_key(id) && (before[id] - *p).length() > 0.5)
        .map(|(id, p)| Edit::MoveNode {
            id,
            before: s.doc.node_positions.get(&id).copied(),
            after: [p.x, p.y],
        })
        .collect()
}

/// The nearest place to `at` where a new card of `width` overlaps none
/// of the existing ones (it moves; they don't).
fn free_spot(s: &Session, at: [f32; 2], width: f32) -> [f32; 2] {
    free_spot_with(s, at, width, None)
}

fn free_spot_with(
    s: &Session,
    at: [f32; 2],
    width: f32,
    pending: Option<(NodeId, [f32; 2])>,
) -> [f32; 2] {
    let defaults = default_positions(s);
    let mut positions: BTreeMap<NodeId, Vec2> = s
        .doc
        .graph
        .nodes()
        .map(|(id, _)| (id, node_position(s, id, &defaults)))
        .collect();
    if let Some((id, [x, y])) = pending {
        positions.insert(id, Vec2::new(x, y));
    }
    let fixed: BTreeSet<NodeId> = positions.keys().copied().collect();
    let new = NodeId(usize::MAX);
    positions.insert(new, Vec2::new(at[0], at[1]));
    relax(
        &mut positions,
        |id| {
            if id == new {
                width
            } else {
                card_width(&s.doc.graph, id)
            }
        },
        &fixed,
    );
    let p = positions[&new];
    [p.x, p.y]
}

/// Across-positions for a layered graph (rows top to bottom) that keep
/// cables short and uncrossed: each node goes under the average of the
/// nodes feeding it, then a pass back up centres parents over their
/// children, a few times over. Each row keeps its order by those targets
/// and never overlaps. Rows are reordered in place.
fn arrange_columns(
    rows: &mut [Vec<NodeId>],
    links: &[Link],
    width: impl Fn(NodeId) -> f32,
    rig: impl Fn(NodeId) -> Option<NodeId>,
) -> BTreeMap<NodeId, f32> {
    let space = |id: NodeId| width(id) + NODE_GAP * 2.0;
    let mut x = BTreeMap::<NodeId, f32>::new();
    // Start: each row side by side, centred.
    for row in rows.iter() {
        let total: f32 = row.iter().map(|&id| space(id)).sum();
        let mut at = -total / 2.0;
        for &id in row {
            x.insert(id, at + space(id) / 2.0);
            at += space(id);
        }
    }
    let place = |row: &mut Vec<NodeId>,
                 x: &mut BTreeMap<NodeId, f32>,
                 target: &dyn Fn(NodeId, f32) -> f32| {
        let targets: BTreeMap<NodeId, f32> =
            row.iter().map(|&id| (id, target(id, x[&id]))).collect();
        // Keep each skeleton's bones adjacent even when its drawing
        // connections would otherwise pull them through another rig.
        row.sort_by(|a, b| {
            rig(*a)
                .cmp(&rig(*b))
                .then_with(|| targets[a].total_cmp(&targets[b]))
        });
        // Left to right at the targets, but never overlapping.
        let mut placed: Vec<f32> = Vec::with_capacity(row.len());
        for (k, &id) in row.iter().enumerate() {
            let mut at = targets[&id];
            if k > 0 {
                let prev = row[k - 1];
                let gap = if rig(prev) != rig(id) { 36.0 } else { 0.0 };
                at = at.max(placed[k - 1] + (space(prev) + space(id)) / 2.0 + gap);
            }
            placed.push(at);
        }
        // Then centre the row on its targets.
        let shift = row
            .iter()
            .zip(&placed)
            .map(|(id, at)| targets[id] - at)
            .sum::<f32>()
            / row.len().max(1) as f32;
        for (&id, at) in row.iter().zip(placed) {
            x.insert(id, at + shift);
        }
    };
    let mean = |ids: Vec<NodeId>, x: &BTreeMap<NodeId, f32>, or: f32| {
        let xs: Vec<f32> = ids.iter().filter_map(|id| x.get(id).copied()).collect();
        if xs.is_empty() {
            or
        } else {
            xs.iter().sum::<f32>() / xs.len() as f32
        }
    };
    let parents = |id: NodeId| {
        links
            .iter()
            .filter(|l| l.to == id)
            .map(|l| l.from)
            .collect::<Vec<_>>()
    };
    let children = |id: NodeId| {
        links
            .iter()
            .filter(|l| l.from == id)
            .map(|l| l.to)
            .collect::<Vec<_>>()
    };
    for _ in 0..4 {
        for row in rows.iter_mut().skip(1) {
            let snapshot = x.clone();
            place(row, &mut x, &|id, here| mean(parents(id), &snapshot, here));
        }
        let last = rows.len().saturating_sub(1);
        for row in rows.iter_mut().take(last).rev() {
            let snapshot = x.clone();
            place(row, &mut x, &|id, here| mean(children(id), &snapshot, here));
        }
    }
    for row in rows.iter_mut().skip(1) {
        let snapshot = x.clone();
        place(row, &mut x, &|id, here| mean(parents(id), &snapshot, here));
    }
    x
}

/// The first row a node can occupy. Connections can push it later, so
/// effects on a composite and nested composites retain top-down flow.
fn arrange_band(kind: &NodeKind) -> usize {
    match kind {
        NodeKind::Peg(_)
        | NodeKind::Skeleton(_)
        | NodeKind::Bone(_)
        | NodeKind::MeshWarp(_)
        | NodeKind::Chart(_) => 0,
        NodeKind::Drawing { .. } | NodeKind::Gradient(_) => 1,
        NodeKind::Effect(_) => 2,
        NodeKind::Composite => 3,
        NodeKind::Display | NodeKind::Write => 4,
    }
}

pub fn arrange(s: &mut Session, state: &mut NodeViewState) {
    let bands: BTreeMap<_, _> = s
        .doc
        .graph
        .nodes()
        .map(|(id, node)| (id, arrange_band(&node.kind)))
        .collect();
    let mut depth: BTreeMap<_, _> = bands.iter().map(|(&id, &band)| (id, (band, 0))).collect();
    let mut pending: BTreeMap<_, usize> = s.doc.graph.nodes().map(|(id, _)| (id, 0)).collect();
    let mut downstream = BTreeMap::<NodeId, Vec<NodeId>>::new();
    for link in s.doc.graph.links() {
        if pending.contains_key(&link.from) {
            if let Some(degree) = pending.get_mut(&link.to) {
                *degree += 1;
                downstream.entry(link.from).or_default().push(link.to);
            }
        }
    }
    let mut ready: Vec<_> = pending
        .iter()
        .filter(|(_, count)| **count == 0)
        .map(|(&id, _)| id)
        .collect();
    while let Some(id) = ready.pop() {
        let (band, level) = depth[&id];
        for &next in downstream.get(&id).into_iter().flatten() {
            let target = depth.get_mut(&next).expect("known downstream node");
            if band >= bands[&next] {
                *target = (*target).max((band, level + 1));
            }
            if let Some(count) = pending.get_mut(&next) {
                *count -= 1;
                if *count == 0 {
                    ready.push(next);
                }
            }
        }
    }
    let mut by_depth = BTreeMap::<(usize, usize), Vec<NodeId>>::new();
    for (id, _) in s.doc.graph.nodes() {
        by_depth
            .entry(depth.get(&id).copied().unwrap_or_default())
            .or_default()
            .push(id);
    }
    let (row_bands, mut rows): (Vec<_>, Vec<_>) = by_depth.into_iter().unzip();
    let graph = &s.doc.graph;
    let links = graph.links();
    let rigs: BTreeMap<_, _> = graph
        .nodes()
        .filter(|(_, node)| matches!(node.kind, NodeKind::Skeleton(_) | NodeKind::Bone(_)))
        .map(|(id, _)| (id, graph.rig_root(id)))
        .collect();
    let x = arrange_columns(
        &mut rows,
        links,
        |id| card_width(graph, id),
        |id| rigs.get(&id).copied(),
    );
    let mut edits = Vec::new();
    let mut bounds = Rect::NOTHING;
    let mut y = 43.0;
    let mut previous_band = None;
    for ((band, _), ids) in row_bands.into_iter().zip(&rows) {
        if let Some(previous) = previous_band {
            y += if previous == band { 76.0 } else { 108.0 };
        }
        previous_band = Some(band);
        for &id in ids {
            let after = [x[&id], y];
            bounds = bounds.union(Rect::from_center_size(
                Pos2::new(after[0], after[1]),
                Vec2::new(card_width(&s.doc.graph, id), H),
            ));
            let before = s.doc.node_positions.get(&id).copied();
            if before != Some(after) {
                edits.push(Edit::MoveNode { id, before, after });
            }
        }
    }
    s.history.apply(&mut s.doc, "Arrange nodes", edits);
    state.fit_next = bounds.is_finite().then_some(bounds);
    state.drag = None;
}

fn card_width(graph: &symphony_core::graph::Graph, id: NodeId) -> f32 {
    if graph
        .node(id)
        .is_some_and(|node| matches!(node.kind, NodeKind::Composite))
    {
        let count = graph.links().iter().filter(|link| link.to == id).count() + 1;
        W.max(count as f32 * 16.0 + 16.0)
    } else {
        W
    }
}

/// Every node type has its own colour, the same everywhere: card tint,
/// edge and the cables leaving it. Blue (selection), amber (time) and red
/// (danger) are kept for their meanings. Types beyond the table reuse a
/// table colour shifted lighter or darker, so they read as related.
pub fn kind_colour(kind: &NodeKind) -> egui::Color32 {
    const TABLE: [(&str, (u8, u8, u8)); 14] = [
        ("Drawing", (111, 194, 168)),
        ("Peg", (186, 150, 222)),
        ("Skeleton", (181, 139, 196)),
        ("Bone", (215, 180, 215)),
        ("Mesh Warp", (214, 168, 232)),
        ("Composite", (128, 182, 214)),
        ("Camera", (214, 214, 220)),
        ("Write", (168, 172, 180)),
        ("Blur", (222, 143, 160)),
        ("Grain", (196, 176, 128)),
        ("Opacity", (150, 206, 222)),
        ("Blend Modes", (232, 156, 122)),
        ("Gradient", (172, 204, 118)),
        ("Smart Chart", (240, 122, 160)),
    ];
    let label = kind_label(kind);
    if let Some((_, (r, g, b))) = TABLE.iter().find(|(name, _)| *name == label) {
        return egui::Color32::from_rgb(*r, *g, *b);
    }
    // Out of colours: a stable cousin of one in the table.
    let hash = label.bytes().fold(2166136261u32, |h, b| {
        (h ^ u32::from(b)).wrapping_mul(16777619)
    });
    let (_, (r, g, b)) = TABLE[hash as usize % TABLE.len()];
    let shift = if hash & 0x100 == 0 { 0.82 } else { 1.12 };
    let f = |v: u8| ((f32::from(v) * shift).round().clamp(0.0, 255.0)) as u8;
    egui::Color32::from_rgb(f(r), f(g), f(b))
}

fn peg_colour(graph: &symphony_core::graph::Graph, id: NodeId) -> Option<egui::Color32> {
    graph.node(id).map(|n| kind_colour(&n.kind))
}

fn minimap(painter: &egui::Painter, viewport: Rect, bounds: Rect) {
    if !bounds.is_finite()
        || viewport.contains_rect(bounds)
        || viewport.width() <= 16.0
        || viewport.height() <= 16.0
    {
        return;
    }
    let full = bounds.union(viewport);
    let size = (viewport.size() * 0.35).min(Vec2::new(116.0, 68.0));
    let scale = (size.x / full.width()).min(size.y / full.height());
    let map = Rect::from_min_size(
        viewport.right_bottom() - full.size() * scale - Vec2::splat(12.0),
        full.size() * scale,
    );
    let visible = Rect::from_min_size(
        map.min + (viewport.min - full.min) * scale,
        viewport.size() * scale,
    );
    let softness = egui::Shadow {
        offset: [0, 1],
        blur: 8,
        spread: 0,
        color: theme::SHADOW.gamma_multiply(0.62),
    };
    painter.add(softness.as_shape(map, 4));
    painter.rect(
        map,
        4.0,
        theme::BG_RAISED.gamma_multiply(0.86),
        Stroke::new(1.0, theme::LINE_STRONG.gamma_multiply(0.8)),
        StrokeKind::Inside,
    );
    painter.rect_stroke(
        visible,
        2.0,
        Stroke::new(1.0, theme::ACCENT_SELECT),
        StrokeKind::Inside,
    );
}

fn select_in_graph(s: &mut Session, state: &mut NodeViewState, id: NodeId, extend: bool) {
    if !extend {
        state.selection.clear();
    }
    state.selection.insert(id);
    s.select_graph_node(id, true);
}

pub fn zoom_by_keyboard(state: &mut NodeViewState, zoom_in: bool) {
    state.keyboard_zoom = Some(if zoom_in { 1.2 } else { 1.0 / 1.2 });
}

fn canvas(ui: &mut egui::Ui, s: &mut Session, state: &mut NodeViewState, add: Option<NodeKind>) {
    let primary = selected_node(s);
    if state.selection_scene != s.key
        || s.selected_node
            .is_some_and(|id| !state.selection.contains(&id))
    {
        state.selection_scene.clone_from(&s.key);
        state.selection.clear();
        state.selection.extend(primary);
    }
    state.selection.retain(|&id| s.doc.graph.node(id).is_some());
    let (resp, painter) = ui.allocate_painter(ui.available_size(), Sense::click_and_drag());
    let viewport = resp.rect;
    let anchor =
        viewport.left_top() + Vec2::X * *state.anchor_x.get_or_insert(viewport.width() / 2.0);
    let mut zoom = state.zoom.unwrap_or(1.0);
    if let Some(bounds) = state.fit_next.take() {
        let room = (viewport.size() - Vec2::splat(36.0)).max(Vec2::splat(1.0));
        zoom = (room.x / bounds.width())
            .min(room.y / bounds.height())
            .clamp(0.25, 1.0);
        state.zoom = Some(zoom);
        state.pan = viewport.center() - anchor - bounds.center().to_vec2() * zoom;
    }
    let keyboard_zoom = state.keyboard_zoom.take();
    if state.drag.is_none() {
        let pointer = resp.hover_pos();
        let scroll = pointer.map_or(0.0, |_| ui.input(|i| i.smooth_scroll_delta.y));
        let factor = keyboard_zoom.unwrap_or(1.0) * (scroll * 0.004).exp();
        let new_zoom = (zoom * factor).clamp(0.25, 2.5);
        if new_zoom != zoom {
            let point = pointer.unwrap_or(viewport.center());
            let local = (point - anchor - state.pan) / zoom;
            state.pan = point - anchor - local * new_zoom;
            state.zoom = Some(new_zoom);
            zoom = new_zoom;
            if scroll != 0.0 {
                ui.input_mut(|input| input.smooth_scroll_delta = Vec2::ZERO);
            }
        }
    }
    let origin = anchor + state.pan;
    if let Some(item) = resp.dnd_release_payload::<LibraryItem>() {
        if let Some(pointer) = ui.input(|input| input.pointer.interact_pos()) {
            let at = (pointer - origin) / zoom;
            add_library_item(s, state, &item, Some([at.x, at.y]));
        }
    } else if resp.dnd_hover_payload::<LibraryItem>().is_some() {
        painter.rect_stroke(
            viewport.shrink(2.0),
            theme::RADIUS_CONTROL,
            Stroke::new(1.5, theme::ACCENT_SELECT),
            StrokeKind::Inside,
        );
    }
    let grid = GRID * zoom * if zoom < 0.5 { 2.0 } else { 1.0 };
    let grid_origin = viewport.min
        + Vec2::new(
            (origin.x - viewport.left()).rem_euclid(grid),
            state.pan.y.rem_euclid(grid),
        );
    let radius = f32::from(theme::GLASS_RADIUS.saturating_sub(4));
    painter.rect_filled(viewport, radius, theme::BG_INPUT);
    let inset = viewport.shrink(radius);
    let rounded_contains = |point: Pos2| {
        let closest = point.clamp(inset.min, inset.max);
        point.distance_sq(closest) < radius * radius
    };
    let mut y = grid_origin.y;
    while y < viewport.bottom() {
        let mut x = grid_origin.x;
        while x < viewport.right() {
            let dot = Pos2::new(x, y);
            if inset.is_positive() && rounded_contains(dot) {
                painter.circle_filled(dot, 0.65, theme::LINE_SUBTLE);
            }
            x += grid;
        }
        y += grid;
    }

    // Where every node sits: stored, or the default layout. Dragged nodes
    // follow the pointer; the others make room (they push apart so no two
    // cards overlap) and glide to where they're going.
    let defaults = default_positions(s);
    let mut placed: BTreeMap<NodeId, Vec2> = s
        .doc
        .graph
        .nodes()
        .filter(|(_, node)| !matches!(node.kind, NodeKind::Bone(_)))
        .map(|(id, _)| (id, node_position(s, id, &defaults)))
        .collect();
    state.selection.retain(|id| placed.contains_key(id));
    let mut held = BTreeSet::new();
    if let Some(Drag::Node(moving, delta)) = state.drag {
        for (id, position) in placed.iter_mut() {
            if *id == moving || state.selection.contains(id) {
                *position += delta;
                held.insert(*id);
            }
        }
        relax(&mut placed, |id| card_width(&s.doc.graph, id), &held);
    }
    for (id, position) in placed.iter_mut() {
        let key = resp.id.with(("node-glide", *id));
        if held.contains(id) {
            // Carried by the pointer: its glide stays where it is, so on
            // release it doesn't swoop in from where the drag began.
            symphony_ui::motion::jump(ui.ctx(), key.with(0), position.x);
            symphony_ui::motion::jump(ui.ctx(), key.with(1), position.y);
        } else {
            *position = Vec2::new(
                symphony_ui::motion::value(
                    ui.ctx(),
                    key.with(0),
                    position.x,
                    symphony_ui::motion::Motion::Select,
                ),
                symphony_ui::motion::value(
                    ui.ctx(),
                    key.with(1),
                    position.y,
                    symphony_ui::motion::Motion::Select,
                ),
            );
        }
    }

    // Pegs show their Motion port only in scenes that have a chart, so
    // the usual graph looks as it always has.
    let has_chart = s
        .doc
        .graph
        .nodes()
        .any(|(_, n)| matches!(n.kind, NodeKind::Chart(_)));
    let mut cards = BTreeMap::new();
    let mut inputs = BTreeMap::<Input, Pos2>::new();
    let mut outputs = BTreeMap::new();
    for (id, node) in s.doc.graph.nodes() {
        let Some(&position) = placed.get(&id) else {
            continue;
        };
        let center = origin + position * zoom;
        let ports = if matches!(node.kind, NodeKind::Composite) {
            let mut used: Vec<_> = s
                .doc
                .graph
                .links()
                .iter()
                .filter(|l| l.to == id)
                .map(|l| l.port)
                .collect();
            used.sort_unstable();
            let free = (0..)
                .find(|port| used.binary_search(port).is_err())
                .unwrap_or(0);
            used.push(free);
            used.sort_unstable();
            used
        } else if matches!(node.kind, NodeKind::Gradient(_) | NodeKind::Chart(_)) {
            vec![]
        } else if matches!(node.kind, NodeKind::Effect(Effect::Mask { .. }))
            || (matches!(node.kind, NodeKind::Peg(_)) && has_chart)
        {
            vec![0, 1]
        } else {
            vec![0]
        };
        let width = card_width(&s.doc.graph, id) * zoom;
        let card = Rect::from_center_size(center, Vec2::new(width, H * zoom));
        cards.insert(id, (card, position));
        let count = ports.len() as f32;
        for (index, port) in ports.into_iter().enumerate() {
            let x = card.left() + width * ((index as f32 + 0.5) / count);
            inputs.insert((id, port), Pos2::new(x, card.top()));
        }
        if !matches!(node.kind, NodeKind::Display | NodeKind::Write) {
            outputs.insert(id, card.center_bottom());
        }
    }

    let alt = ui.input(|i| i.modifiers.alt);
    if let Some(Drag::Node(id, _)) = state.drag {
        if !alt && state.selection.len() <= 1 && ui.input(|i| i.modifiers.shift) {
            let connected = s
                .doc
                .graph
                .links()
                .iter()
                .filter_map(|link| {
                    let from = outputs.get(&link.from)?;
                    let to = inputs.get(&(link.to, link.port))?;
                    let y = to.y - from.y;
                    if !(0.0..=160.0).contains(&y) {
                        return None;
                    }
                    let dx = if link.from == id {
                        to.x - from.x
                    } else if link.to == id {
                        from.x - to.x
                    } else {
                        return None;
                    };
                    (dx.abs() <= 20.0).then_some(dx)
                })
                .min_by(|a, b| a.abs().total_cmp(&b.abs()));
            if let Some(dx) = connected {
                if let Some((card, position)) = cards.get_mut(&id) {
                    let offset = Vec2::X * dx;
                    *card = card.translate(offset);
                    *position += offset / zoom;
                    if let Some(out) = outputs.get_mut(&id) {
                        *out += offset;
                    }
                    for ((node, _), input) in &mut inputs {
                        if *node == id {
                            *input += offset;
                        }
                    }
                }
            }
        }
    }

    if let Some(kind) = add {
        let mut center = viewport.center() + Vec2::new(W * 0.65 * zoom, 0.0);
        while cards.values().any(|(card, _)| {
            card.expand(12.0)
                .intersects(Rect::from_center_size(center, Vec2::new(W, H) * zoom))
        }) {
            center.y += (H + 20.0) * zoom;
        }
        let position = (center - origin) / zoom;
        add_node(s, kind, [position.x, position.y]);
        ui.ctx().request_repaint();
    }

    // Generous targets: ports are small, hands aren't.
    let hit_radius = 16.0 * zoom.clamp(0.6, 1.0);
    let input_at = |p: Pos2| {
        inputs
            .iter()
            .filter_map(|(&key, &at)| {
                let distance = at.distance(p);
                (distance <= hit_radius).then_some((key, distance))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(key, _)| key)
    };
    // Every input, by node, in port order (for picking a free one).
    let cards_inputs: Vec<Input> = inputs.keys().copied().collect();
    let output_at = |p: Pos2| {
        outputs
            .iter()
            .find(|(_, at)| at.distance(p) <= hit_radius)
            .map(|(&key, _)| key)
    };
    let node_at = |p| {
        cards
            .iter()
            .rev()
            .find(|(_, (card, _))| card.contains(p))
            .map(|(&id, _)| id)
    };
    // Where a cable from `source` dropped at `p` connects: the port under
    // it, else the nearest fitting input of the node under it (empty ports
    // first), else the nearest fitting input within reach.
    let drop_target = |graph: &symphony_core::graph::Graph,
                       source: NodeId,
                       old_input: Option<Input>,
                       p: Pos2|
     -> Option<Input> {
        let node = node_at(p);
        let fits = |input: &Input| {
            input.0 != source
                && graph.can_connect_to(source, input.0, input.1)
                && (Some(*input) != old_input
                    || node == Some(input.0)
                    || inputs
                        .get(input)
                        .is_some_and(|at| at.distance(p) <= hit_radius))
        };
        if let Some(input) = input_at(p).filter(fits) {
            return Some(input);
        }
        let near = |only: Option<NodeId>, reach: f32| {
            inputs
                .iter()
                .filter(|(input, _)| fits(input) && only.is_none_or(|n| input.0 == n))
                .map(|(&input, &at)| {
                    let taken = graph.input(input.0, input.1).is_some();
                    (input, at.distance(p) + if taken { 1000.0 } else { 0.0 })
                })
                .filter(|(_, d)| *d <= reach)
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(input, _)| input)
        };
        match node {
            Some(node) if node != source => near(Some(node), f32::INFINITY),
            Some(_) => None,
            None => near(None, 40.0 * zoom.clamp(0.6, 1.0)),
        }
    };
    let wire_at = |graph: &symphony_core::graph::Graph, point: Pos2| {
        if let Some(input) = input_at(point) {
            return graph.input(input.0, input.1).map(|from| Link {
                from,
                to: input.0,
                port: input.1,
            });
        }
        if node_at(point).is_some()
            || output_at(point)
                .and_then(|id| outputs.get(&id))
                .is_some_and(|p| p.distance(point) <= hit_radius * 0.5)
        {
            return None;
        }
        graph
            .links()
            .iter()
            .filter_map(|&link| {
                let from = *outputs.get(&link.from)?;
                let to = *inputs.get(&(link.to, link.port))?;
                let distance = near_cable(point, from, to);
                (distance <= 8.0).then_some((distance, link))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, link)| link)
    };
    let space = ui.input(|i| i.key_down(egui::Key::Space));
    let middle = resp.drag_started_by(egui::PointerButton::Middle);
    if space && resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
    }
    if middle || resp.drag_started_by(egui::PointerButton::Primary) {
        if let Some(point) = ui.input(|i| i.pointer.press_origin()) {
            state.drag = if space || middle {
                Some(Drag::Pan)
            } else if let Some(link) =
                wire_at(&s.doc.graph, point).filter(|link| outputs.contains_key(&link.from))
            {
                Some(Drag::Cable {
                    source: link.from,
                    old_input: Some((link.to, link.port)),
                })
            } else if let Some(source) = output_at(point) {
                Some(Drag::Cable {
                    source,
                    old_input: None,
                })
            } else if let Some(node) = node_at(point) {
                let extend = state.selection.contains(&node);
                select_in_graph(s, state, node, extend);
                Some(Drag::Node(node, Vec2::ZERO))
            } else if cards.values().all(|(card, _)| {
                !card
                    .expand(MARQUEE_CLEARANCE * zoom.clamp(0.6, 1.0))
                    .contains(point)
            }) {
                Some(Drag::Marquee {
                    start: point,
                    extend: ui.input(|i| i.modifiers.shift),
                })
            } else {
                None
            };
        }
    }
    if resp.dragged() {
        match state.drag.as_mut() {
            Some(Drag::Pan) => {
                state.pan += resp.drag_delta();
                ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
            }
            Some(Drag::Node(_, delta)) => *delta += resp.drag_delta() / zoom,
            _ => {}
        }
        ui.ctx().request_repaint();
    }
    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        state.drag = None;
    }
    // Delete or Backspace over the Node view deletes the selected nodes.
    let delete_key = resp.hovered()
        && state.drag.is_none()
        && !ui.ctx().egui_wants_keyboard_input()
        && ui.input(|i| i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace));
    if delete_key {
        let chosen: Vec<NodeId> = state
            .selection
            .iter()
            .copied()
            .chain(s.selected_node.filter(|id| cards.contains_key(id)))
            .collect();
        if delete_nodes(s, &chosen) > 0 {
            state.selection.clear();
        }
    }
    let mut auto_wire = match state.drag {
        Some(Drag::Node(id, delta))
            if alt && state.selection.len() <= 1 && delta.length_sq() > 0.0 =>
        {
            cards
                .get(&id)
                .and_then(|(card, _)| nearby_wiring(&s.doc.graph, id, *card, &inputs, &outputs))
        }
        _ => None,
    };
    let detach = match state.drag {
        Some(Drag::Node(id, delta))
            if alt
                && state.selection.len() <= 1
                && auto_wire.is_none()
                && delta.length() * zoom >= 48.0
                && s.doc
                    .graph
                    .links()
                    .iter()
                    .any(|link| link.from == id || link.to == id) =>
        {
            Some(id)
        }
        _ => None,
    };
    let now = ui.input(|i| i.time);
    let animated = symphony_ui::motion::enabled(ui.ctx());
    let marquee = |start| {
        resp.interact_pointer_pos()
            .map(|end| Rect::from_two_pos(start, end))
    };
    if let Some(Drag::Marquee { start, .. }) = state.drag {
        if let Some(rect) = marquee(start) {
            painter.rect(
                rect,
                2.0,
                theme::ACCENT_SELECT.gamma_multiply(0.16),
                Stroke::new(1.0, theme::ACCENT_SELECT),
                StrokeKind::Inside,
            );
        }
    }
    if resp.drag_stopped() {
        match state.drag.take() {
            Some(Drag::Marquee { start, extend }) => {
                if let Some(rect) = marquee(start) {
                    let hits = cards
                        .iter()
                        .filter(|(_, (card, _))| rect.intersects(*card))
                        .map(|(&id, _)| id);
                    if !extend {
                        state.selection.clear();
                    }
                    state.selection.extend(hits);
                    if !s
                        .selected_node
                        .is_some_and(|id| state.selection.contains(&id))
                    {
                        if let Some(&id) = state.selection.iter().next() {
                            s.select_graph_node(id, true);
                        } else {
                            s.selected_node = None;
                        }
                    }
                }
            }
            Some(Drag::Node(id, delta)) if delta.length_sq() > 0.0 => {
                if cards.contains_key(&id) {
                    // Keep every selected node's relative position during a group move.
                    let mut edits: Vec<_> = cards
                        .iter()
                        .filter(|(moving, _)| **moving == id || state.selection.contains(moving))
                        .map(|(&id, (_, position))| Edit::MoveNode {
                            id,
                            before: s.doc.node_positions.get(&id).copied(),
                            after: [position.x, position.y],
                        })
                        .collect();
                    // Neighbours it pushed aside move in the same step.
                    let overrides: BTreeMap<NodeId, Vec2> = cards
                        .iter()
                        .filter(|(moving, _)| **moving == id || state.selection.contains(moving))
                        .map(|(&id, (_, position))| (id, *position))
                        .collect();
                    let fixed: BTreeSet<NodeId> = overrides.keys().copied().collect();
                    edits.extend(make_room(s, &fixed, &overrides));
                    let action = if let Some(wiring) = auto_wire.take() {
                        edits.push(wiring.edit(s));
                        "Move and connect node"
                    } else if detach == Some(id) {
                        // Pulled out of its chain: the chain closes up
                        // behind it.
                        let before = s.doc.graph.links().to_vec();
                        let after = detached_links(&s.doc.graph, id);
                        edits.push(Edit::RewireNodes {
                            before,
                            after,
                            was_custom: s.doc.custom_wiring,
                        });
                        "Move and disconnect node"
                    } else {
                        "Move node"
                    };
                    s.history.apply(&mut s.doc, action, edits);
                }
            }
            Some(Drag::Cable { source, old_input }) => {
                let release = resp
                    .interact_pointer_pos()
                    .or_else(|| ui.input(|i| i.pointer.latest_pos()));
                let target = release
                    .filter(|p| viewport.contains(*p))
                    .and_then(|p| drop_target(&s.doc.graph, source, old_input, p));
                if old_input.is_none() && target.is_none() {
                    let end = release.or_else(|| state.drag_cable.map(|cable| cable.end));
                    if animated {
                        if let (Some(&from), Some(end)) = (outputs.get(&source), end) {
                            state.link_anims.push(LinkAnim::retract(
                                None,
                                source,
                                (from, end),
                                state.drag_cable.take(),
                                now,
                            ));
                        }
                    }
                } else if let Some(point) = release {
                    state.released = Some((point, now));
                    state.released_cable = state.drag_cable.take();
                    // A cable lifted off an input and let go close to where
                    // it was (a slip, or grabbing the port while aiming for
                    // the node) snaps back instead of disconnecting.
                    let slipped = old_input
                        .and_then(|input| inputs.get(&input))
                        .is_some_and(|&at| at.distance(point) <= hit_radius);
                    if let Some(input) = target.filter(|&i| old_input != Some(i)) {
                        connect_smart(s, source, input, old_input, &cards_inputs);
                    } else if !slipped && node_at(point).is_none() && output_at(point).is_none() {
                        // Pulled clearly away into empty space: disconnect.
                        if let Some(input) = old_input {
                            edit_link(s, None, input, None);
                        }
                    } else if let Some(input) = old_input {
                        // Kept: the cable springs back onto its port.
                        let link = Link {
                            from: source,
                            to: input.0,
                            port: input.1,
                        };
                        state.link_anims.retain(|a| a.link != Some(link));
                        state.link_anims.push(LinkAnim {
                            link: Some(link),
                            start: now,
                            motion: CableMotion::Move { from_end: point },
                            curve: state.released_cable.take().unwrap_or_else(|| {
                                Cable::new(outputs.get(&source).copied().unwrap_or(point), point)
                            }),
                        });
                    }
                }
            }
            _ => {}
        }
        ui.ctx().request_repaint();
    }
    let cut = if alt && state.drag.is_none() {
        resp.interact_pointer_pos()
            .or_else(|| resp.hover_pos())
            .and_then(|point| wire_at(&s.doc.graph, point))
    } else {
        None
    };
    if !space && (resp.clicked() || resp.double_clicked()) {
        if let Some(link) = cut.filter(|_| resp.clicked()) {
            edit_link(s, None, (link.to, link.port), None);
        } else if let Some(id) = resp.interact_pointer_pos().and_then(node_at) {
            let extend = ui.input(|i| i.modifiers.shift);
            select_in_graph(s, state, id, extend);
            ui.ctx().request_repaint();
            if resp.double_clicked() {
                // A Skeleton opens in its setup view; other nodes show
                // their properties.
                if matches!(
                    s.doc.graph.node(id).map(|n| &n.kind),
                    Some(NodeKind::Skeleton(_))
                ) {
                    state.setup.open = Some(id);
                } else {
                    open_properties(s, state, id);
                }
            }
        }
    }

    let dt = state.cable_time.map_or(0.0, |last| (now - last) as f32);
    state.cable_time = Some(now);
    let graph = &s.doc.graph;
    let mut ends: Vec<(Link, Pos2, Pos2)> = Vec::new();
    for &actual in graph.links() {
        let mut link = actual;
        if graph.bone(link.from).is_some() {
            link.from = graph.rig_root(link.from);
        }
        if let (Some(&from), Some(&to)) =
            (outputs.get(&link.from), inputs.get(&(link.to, link.port)))
        {
            ends.push((link, from, to));
        }
    }
    // Where a cable was just let go, and its rope (a fraction of a second).
    let recent = state.released.is_some_and(|(_, at)| now - at < 0.25);
    let released = state.released.filter(|_| recent).map(|(p, _)| p);
    if !recent {
        state.released_cable = None;
    }
    if state.drawn_scene != s.key {
        // A different scene: nothing to animate from.
        state.drawn_scene = s.key.clone();
        state.link_anims.clear();
    } else if animated
        && state
            .drawn_links
            .iter()
            .map(|(link, ..)| link)
            .ne(ends.iter().map(|(link, ..)| link))
    {
        let appeared: Vec<(Link, Pos2, Pos2)> = ends
            .iter()
            .copied()
            .filter(|(l, ..)| !state.drawn_links.iter().any(|(d, ..)| d == l))
            .collect();
        let mut gone: Vec<(Link, Pos2, Pos2)> = state
            .drawn_links
            .iter()
            .copied()
            .filter(|(l, ..)| !ends.iter().any(|(e, ..)| e == l))
            .collect();
        for (link, from, _) in appeared {
            // The same output lost a link this frame: it moved (or swapped).
            let (motion, start_end) = match gone.iter().position(|(g, ..)| g.from == link.from) {
                Some(i) => {
                    let old_end = gone.remove(i).2;
                    let start = released.unwrap_or(old_end);
                    (CableMotion::Move { from_end: start }, Some(start))
                }
                None => (CableMotion::Connect { from_end: released }, released),
            };
            let curve = state
                .released_cable
                .take()
                .unwrap_or_else(|| Cable::new(from, start_end.unwrap_or(from)));
            state.link_anims.retain(|a| a.link != Some(link));
            state.link_anims.push(LinkAnim {
                link: Some(link),
                start: now,
                motion,
                curve,
            });
        }
        for (link, from, to) in gone {
            state.link_anims.retain(|a| a.link != Some(link));
            state.link_anims.push(LinkAnim::retract(
                Some(link),
                link.from,
                (from, released.unwrap_or(to)),
                state.released_cable.take(),
                now,
            ));
        }
    }
    if !animated {
        state.link_anims.clear();
        state.drag_cable = None;
        state.released_cable = None;
    }
    state.drawn_links = ends;
    state
        .link_anims
        .retain(|a| now - a.start < a.motion.seconds());
    if !state.link_anims.is_empty() {
        ui.ctx().request_repaint();
    }
    let link_selected =
        |link: Link| state.selection.contains(&link.from) || state.selection.contains(&link.to);
    let selected_outputs: BTreeSet<_> = s
        .doc
        .graph
        .links()
        .iter()
        .filter(|link| link_selected(**link))
        .map(|link| link.from)
        .collect();
    let cable_style = |link: Link, removing: bool| {
        let colour = peg_colour(&s.doc.graph, link.from).unwrap_or(theme::LINE_CONTROL);
        if removing {
            Stroke::new(ACTIVE_CABLE_WIDTH, theme::STATE_DANGER)
        } else if link_selected(link) {
            Stroke::new(
                ACTIVE_CABLE_WIDTH,
                colour.lerp_to_gamma(theme::ACCENT_SELECT, 0.7),
            )
        } else {
            Stroke::new(CABLE_WIDTH, colour)
        }
    };
    // Step and draw every moving cable.
    let mut moving: Vec<Link> = Vec::new();
    for a in &state.link_anims {
        let progress = ((now - a.start) / a.motion.seconds()).clamp(0.0, 1.0) as f32;
        match a.motion {
            CableMotion::Retract { source, from } => {
                let ink = peg_colour(&s.doc.graph, source).unwrap_or(theme::LINE_CONTROL);
                let from = outputs.get(&source).copied().unwrap_or(from);
                a.curve.retract(from, progress).paint_moving(
                    &painter,
                    Stroke::new(CABLE_WIDTH, ink.gamma_multiply(1.0 - 0.5 * progress)),
                    (progress * std::f32::consts::TAU).sin() * (1.0 - progress) * 13.0,
                );
            }
            CableMotion::Connect { .. } | CableMotion::Move { .. } => {
                let Some(link) = a.link else { continue };
                let (Some(&from), Some(&to)) =
                    (outputs.get(&link.from), inputs.get(&(link.to, link.port)))
                else {
                    continue;
                };
                moving.push(link);
                let start = match a.motion {
                    CableMotion::Move { from_end } => Some(from_end),
                    CableMotion::Connect { from_end } => from_end,
                    CableMotion::Retract { .. } => None,
                };
                let remaining = (1.0 - progress).powi(3);
                let routed = progress * progress * (3.0 - 2.0 * progress);
                let cable = if let Some(start) = start {
                    Cable::new(from, to.lerp(start, remaining))
                        .shape_for_ports(routed)
                        .with_offset(a.curve.offset() * remaining)
                } else {
                    Cable::new(from, to)
                        .shape_for_ports(1.0)
                        .prefix(1.0 - remaining)
                };
                let flex = (progress * std::f32::consts::TAU).sin() * (1.0 - progress) * 9.0;
                cable.paint_moving(&painter, cable_style(link, false), flex);
            }
        }
    }
    for &(link, from, to) in &state.drawn_links {
        if auto_wire.and_then(AutoWire::replaced) == Some(link) || moving.contains(&link) {
            continue;
        }
        let input = (link.to, link.port);
        if matches!(state.drag, Some(Drag::Cable { old_input: Some(old), .. }) if old == input) {
            continue;
        }
        let removing =
            cut == Some(link) || detach.is_some_and(|id| link.from == id || link.to == id);
        curve(&painter, from, to, cable_style(link, removing));
    }
    if let Some(Drag::Cable { source, old_input }) = state.drag {
        if let (Some(&from), Some(pointer)) =
            (outputs.get(&source), ui.input(|i| i.pointer.latest_pos()))
        {
            // The cable jumps to the input it would connect to.
            let to = drop_target(&s.doc.graph, source, old_input, pointer)
                .and_then(|input| inputs.get(&input).copied())
                .unwrap_or(pointer);
            let ink = peg_colour(&s.doc.graph, source).unwrap_or(theme::ACCENT_SELECT);
            if animated {
                let previous = state.drag_cable.unwrap_or_else(|| {
                    let picked = match state.drag {
                        Some(Drag::Cable {
                            old_input: Some(input),
                            ..
                        }) => inputs.get(&input).copied(),
                        _ => None,
                    };
                    Cable::new(from, picked.unwrap_or(from))
                });
                let cable = previous.follow(from, to, dt);
                state.drag_cable = Some(cable);
                cable.paint(&painter, Stroke::new(ACTIVE_CABLE_WIDTH, ink));
                if cable.offset().length_sq() > 0.01 {
                    ui.ctx().request_repaint();
                }
            } else {
                curve(&painter, from, to, Stroke::new(ACTIVE_CABLE_WIDTH, ink));
            }
        }
    } else {
        state.drag_cable = None;
    }
    // While pulling a node out, show the chain closing up behind it.
    if let Some(id) = detach {
        let before = s.doc.graph.links();
        for link in detached_links(&s.doc.graph, id) {
            if before.contains(&link) {
                continue;
            }
            if let (Some(&from), Some(&to)) =
                (outputs.get(&link.from), inputs.get(&(link.to, link.port)))
            {
                curve(&painter, from, to, Stroke::new(2.0, theme::ACCENT_SELECT));
            }
        }
    }
    if let Some(wiring) = auto_wire {
        for link in wiring.links().into_iter().flatten() {
            if let (Some(&from), Some(&to)) =
                (outputs.get(&link.from), inputs.get(&(link.to, link.port)))
            {
                curve(&painter, from, to, Stroke::new(2.0, theme::ACCENT_SELECT));
            }
        }
    }
    // Selected nodes draw last, so their lift and shadow sit above the rest.
    let mut order: Vec<(&NodeId, &(Rect, _))> = cards.iter().collect();
    order.sort_by_key(|(id, _)| state.selection.contains(id));
    for (id, (card, _)) in order {
        let Some(node) = s.doc.graph.node(*id) else {
            continue;
        };
        let selected = state.selection.contains(id);
        // Selecting eases the card up a little, with a shadow under it and
        // a thicker rim (Motion presence, so it eases out again too).
        let lift = symphony_ui::motion::presence(ui.ctx(), resp.id.with(("lift", *id)), selected);
        let card = &Rect::from_center_size(card.center(), card.size() * (1.0 + 0.12 * lift));
        // Rounded, and the same shape at any zoom: corners scale with the card.
        let radius = NODE_RADIUS * zoom;
        let peg = peg_colour(&s.doc.graph, *id);
        let matte = s.doc.graph.is_drawing_matte(*id);
        let title = if matte {
            format!("{} · Matte", node_name(node))
        } else {
            node_name(node).to_owned()
        };
        // Selection keeps the node's own colour; the rim just thickens.
        let edge = peg.unwrap_or(theme::LINE_STRONG);
        let face = peg.map_or(theme::BG_RAISED, |colour| {
            theme::BG_RAISED.lerp_to_gamma(colour, 0.12)
        });
        // Unselected nodes sit a little darker, so the selected one stands
        // out; a disabled node is grey with no border.
        let face = if !node.enabled {
            theme::BG_INPUT
        } else {
            face.lerp_to_gamma(theme::BG_BASE, 0.35 * (1.0 - lift))
        };
        let edge = if node.enabled {
            edge
        } else {
            egui::Color32::TRANSPARENT
        };
        if lift > 0.0 {
            let shadow = egui::epaint::Shadow {
                offset: [0, (6.0 * zoom * lift) as i8],
                blur: (16.0 * zoom * lift).min(255.0) as u8,
                spread: 0,
                color: egui::Color32::from_black_alpha((110.0 * lift) as u8),
            };
            painter.add(shadow.as_shape(*card, radius));
        }
        painter.rect(
            *card,
            radius,
            face,
            Stroke::new(1.0 + 2.0 * lift, edge),
            StrokeKind::Inside,
        );
        let mut font = theme::body();
        font.size *= zoom;
        widgets::fitted_text(
            ui,
            &painter,
            card.center(),
            Align2::CENTER_CENTER,
            &title,
            font,
            if node.enabled {
                theme::TEXT_PRIMARY
            } else {
                theme::TEXT_DISABLED
            },
            card.width() - 16.0 * zoom,
        );
    }

    let hover = resp.hover_pos();
    let menu_id = resp.id.with("menu-target");
    if resp.secondary_clicked() {
        let targets = resp
            .interact_pointer_pos()
            .map(|point| {
                (
                    wire_at(&s.doc.graph, point)
                        .map(|link| (link.to, link.port))
                        .or_else(|| input_at(point)),
                    node_at(point),
                )
            })
            .unwrap_or_default();
        ui.data_mut(|data| data.insert_temp(menu_id, targets));
    }
    let mut disconnect = None;
    let mut convert = None;
    let mut smart_joint = None;
    let mut delete: Option<Vec<NodeId>> = None;
    let mut unlink = None;
    let mut open_setup = None;
    if let Some((input, node)) =
        ui.data(|data| data.get_temp::<(Option<Input>, Option<NodeId>)>(menu_id))
    {
        let input = input.filter(|&(node, port)| s.doc.graph.input(node, port).is_some());
        let drawing = node.filter(|&id| {
            s.doc
                .graph
                .node(id)
                .is_some_and(|node| matches!(node.kind, NodeKind::Drawing { .. }))
        });
        let joint = node
            .or(input.map(|(id, _)| id))
            .and_then(|id| smart_joint_label(s, id).map(|label| (id, label)));
        let node = node.filter(|&id| s.doc.graph.node(id).is_some());
        if input.is_some() || node.is_some() || joint.is_some() {
            resp.context_menu(|ui| {
                if let Some(input) = input {
                    if ui.button("Disconnect input").clicked() {
                        disconnect = Some(input);
                        ui.close();
                    }
                }
                if let Some((id, label)) = joint {
                    if ui.button(label).clicked() {
                        smart_joint = Some(id);
                        ui.close();
                    }
                }
                if let Some(id) = drawing {
                    if let Some(label) = drawing_conversion_label(s, id) {
                        if ui.button(label).clicked() {
                            convert = Some(id);
                            ui.close();
                        }
                    }
                }
                if let Some(id) = node.filter(|&id| {
                    matches!(
                        s.doc.graph.node(id).map(|n| &n.kind),
                        Some(NodeKind::Skeleton(_))
                    )
                }) {
                    if ui.button("Open skeleton setup").clicked() {
                        open_setup = Some(id);
                        ui.close();
                    }
                }
                if let Some(id) = node {
                    let linked = s
                        .doc
                        .graph
                        .links()
                        .iter()
                        .any(|l| l.from == id || l.to == id);
                    if ui
                        .add_enabled(linked, egui::Button::new("Disconnect all"))
                        .clicked()
                    {
                        unlink = Some(id);
                        ui.close();
                    }
                    ui.separator();
                    // The whole selection when the node is part of it.
                    let group: Vec<NodeId> = if state.selection.contains(&id) {
                        state.selection.iter().copied().collect()
                    } else {
                        vec![id]
                    };
                    let label = if group.len() > 1 {
                        format!("Delete {} nodes", group.len())
                    } else {
                        "Delete node".to_owned()
                    };
                    let why = (group.len() == 1).then(|| undeletable(s, id)).flatten();
                    let button = ui.add_enabled(why.is_none(), egui::Button::new(label));
                    let button = match why {
                        Some(why) => button.on_disabled_hover_text(why),
                        None => button,
                    };
                    if button.clicked() {
                        delete = Some(group);
                        ui.close();
                    }
                }
            });
        }
    }
    if let Some(id) = open_setup {
        state.setup.open = Some(id);
    }
    if let Some(ids) = delete {
        if delete_nodes(s, &ids) > 0 {
            state.selection.clear();
        }
    }
    if let Some(id) = unlink {
        let before = s.doc.graph.links().to_vec();
        let after: Vec<Link> = before
            .iter()
            .copied()
            .filter(|l| l.from != id && l.to != id)
            .collect();
        let was_custom = s.doc.custom_wiring;
        s.history.apply(
            &mut s.doc,
            "Disconnect node",
            vec![Edit::RewireNodes {
                before,
                after,
                was_custom,
            }],
        );
    }
    if let Some(input) = disconnect {
        edit_link(s, None, input, None);
    }
    if let Some(id) = smart_joint {
        toggle_smart_joint(s, id);
    }
    if let Some(id) = convert {
        convert_drawing(s, id);
        ui.ctx().request_repaint();
    }

    for (&input, &point) in &inputs {
        let node = s.doc.graph.node(input.0);
        let peg = node.is_some_and(|n| matches!(n.kind, NodeKind::Peg(_)));
        let motion = peg && input.1 == symphony_core::graph::MOTION_PORT;
        if zoom >= 0.75
            && node.is_some_and(|node| matches!(node.kind, NodeKind::Effect(Effect::Mask { .. })))
        {
            painter.text(
                point - Vec2::Y * 8.0,
                Align2::CENTER_BOTTOM,
                if input.1 == 0 { "Image" } else { "Matte" },
                theme::dense(),
                theme::TEXT_SECONDARY,
            );
        }
        // Named while a chart's cable is looking for it.
        let chart_cable = matches!(state.drag, Some(Drag::Cable { source, .. })
            if matches!(s.doc.graph.node(source).map(|n| &n.kind), Some(NodeKind::Chart(_))));
        if motion && zoom >= 0.75 && chart_cable {
            painter.text(
                point - Vec2::Y * 8.0,
                Align2::CENTER_BOTTOM,
                "Motion",
                theme::dense(),
                theme::TEXT_SECONDARY,
            );
        }
        let shape = if motion {
            Socket::Motion
        } else if node.is_some_and(|n| {
            matches!(
                n.kind,
                NodeKind::Peg(_)
                    | NodeKind::Drawing { .. }
                    | NodeKind::Skeleton(_)
                    | NodeKind::Bone(_)
            )
        }) {
            Socket::Transform
        } else {
            Socket::Image
        };
        let compatible = match state.drag {
            Some(Drag::Cable { source, .. }) => {
                s.doc.graph.can_connect_to(source, input.0, input.1)
            }
            _ => true,
        };
        let filled = s.doc.graph.input(input.0, input.1).is_some();
        let target = match state.drag {
            Some(Drag::Cable { source, old_input }) => {
                hover.and_then(|p| drop_target(&s.doc.graph, source, old_input, p)) == Some(input)
            }
            _ => hover.is_some_and(|p| p.distance(point) <= hit_radius),
        };
        let highlighted = (target && compatible)
            || state.selection.contains(&input.0)
            || s.doc
                .graph
                .input(input.0, input.1)
                .is_some_and(|source| state.selection.contains(&source));
        let ink = s
            .doc
            .graph
            .input(input.0, input.1)
            .and_then(|from| peg_colour(&s.doc.graph, from));
        socket(&painter, point, shape, filled, highlighted, ink, zoom);
    }
    for (id, point) in outputs {
        let shape = match s.doc.graph.node(id).map(|n| &n.kind) {
            Some(NodeKind::Chart(_)) => Socket::Motion,
            Some(NodeKind::Peg(_) | NodeKind::Skeleton(_) | NodeKind::Bone(_)) => Socket::Transform,
            _ => Socket::Image,
        };
        let highlighted = hover.is_some_and(|p| p.distance(point) <= hit_radius)
            || state.selection.contains(&id)
            || selected_outputs.contains(&id);
        socket(
            &painter,
            point,
            shape,
            true,
            highlighted,
            peg_colour(&s.doc.graph, id),
            zoom,
        );
    }
    let bounds = cards
        .values()
        .fold(Rect::NOTHING, |bounds, (card, _)| bounds.union(*card));
    minimap(&painter, viewport, bounds);
    let hovered = if state.drag.is_none() {
        hover.and_then(node_at)
    } else {
        None
    };
    hover_thumbnail(
        ui,
        s,
        state,
        hovered.and_then(|id| Some((id, cards.get(&id)?.0))),
    );
}

/// Hovering a drawing node shows a small picture of its current drawing
/// beside it, fading in and out.
fn hover_thumbnail(
    ui: &egui::Ui,
    s: &Session,
    state: &mut NodeViewState,
    hovered: Option<(NodeId, Rect)>,
) {
    const SIDE: u32 = 112;
    let target = hovered.and_then(|(id, card)| {
        let NodeKind::Drawing { column } = s.doc.graph.node(id)?.kind else {
            return None;
        };
        let col = s.doc.xsheet.column(column)?;
        let drawing = col.get(s.frame);
        Some((id, card, column, drawing, col.name.clone()))
    });
    if let Some((id, card, column, drawing, name)) = &target {
        let label = match drawing.and_then(|d| s.doc.drawings.get(&d)) {
            Some(d) => format!("{name}, drawing {}", d.name),
            None => format!("{name}: no drawing on this frame"),
        };
        state.thumb_at = Some((*card, label));
        if let Some(d) = drawing {
            let key = (*d, s.doc.version, s.frame);
            if state.thumb.as_ref().is_none_or(|(k, _)| *k != key) {
                let texture = symphony_render::render_drawing_thumbnail(
                    &s.doc, *id, *column, *d, s.frame, SIDE,
                )
                .map(|pix| {
                    ui.ctx().load_texture(
                        "node-hover-thumbnail",
                        egui::ColorImage::from_rgba_premultiplied(
                            [pix.width() as usize, pix.height() as usize],
                            pix.data(),
                        ),
                        egui::TextureOptions::LINEAR,
                    )
                });
                state.thumb = Some((key, texture));
            }
        } else {
            state.thumb = None;
        }
    }
    let shown = symphony_ui::motion::presence(
        ui.ctx(),
        egui::Id::unique("node-hover-thumbnail"),
        target.is_some(),
    );
    let Some((card, label)) = state.thumb_at.clone().filter(|_| shown > 0.0) else {
        return;
    };
    let stage = crate::session::CanvasColours::get(s.colours.stage);
    let texture = state.thumb.as_ref().and_then(|(_, t)| t.clone());
    egui::Area::new(egui::Id::unique("node-hover-thumbnail-area"))
        .order(egui::Order::Tooltip)
        .fixed_pos(card.right_top() + Vec2::new(10.0, 0.0))
        .interactable(false)
        .fade_in(false)
        .show(ui.ctx(), |ui| {
            ui.multiply_opacity(shown);
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                let size = Vec2::splat(SIDE as f32);
                let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
                ui.painter().rect_filled(rect, theme::RADIUS_CONTROL, stage);
                if let Some(t) = texture {
                    ui.painter().image(
                        t.id(),
                        rect,
                        Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                        egui::Color32::WHITE,
                    );
                }
                ui.label(
                    egui::RichText::new(label)
                        .font(theme::dense())
                        .color(theme::TEXT_SECONDARY),
                );
            });
        });
}

/// A port's shape says what it carries: images are round, transforms
/// square, a chart's motion a diamond.
#[derive(Clone, Copy, PartialEq)]
enum Socket {
    Image,
    Transform,
    Motion,
}

fn socket(
    painter: &egui::Painter,
    point: Pos2,
    shape: Socket,
    filled: bool,
    hover: bool,
    ink: Option<egui::Color32>,
    zoom: f32,
) {
    let radius = if hover { 5.0 } else { 3.5 } * zoom.clamp(0.25, 1.4);
    let color = if hover {
        theme::ACCENT_SELECT
    } else {
        ink.unwrap_or(theme::LINE_CONTROL)
    };
    let fill = if filled { color } else { theme::BG_INPUT };
    let stroke = Stroke::new(1.0, color);
    match shape {
        Socket::Transform => {
            painter.rect(
                Rect::from_center_size(point, Vec2::splat(radius * 2.0)),
                1.0,
                fill,
                stroke,
                StrokeKind::Inside,
            );
        }
        Socket::Motion => {
            let r = radius * 1.3;
            painter.add(egui::Shape::convex_polygon(
                vec![
                    point - Vec2::Y * r,
                    point + Vec2::X * r,
                    point + Vec2::Y * r,
                    point - Vec2::X * r,
                ],
                fill,
                stroke,
            ));
        }
        Socket::Image => {
            painter.circle(point, radius, fill, stroke);
        }
    }
}

#[derive(Clone)]
enum LibraryItem {
    Drawing,
    Node(NodeKind),
}

fn add_library_item(
    s: &mut Session,
    state: &mut NodeViewState,
    item: &LibraryItem,
    at: Option<[f32; 2]>,
) {
    match item {
        LibraryItem::Drawing => {
            let at = at.map(|at| free_spot(s, at, W));
            s.add_layer_at(at);
            if let Some(id) = s.doc.layers.get(s.selected).map(|layer| layer.node) {
                s.select_graph_node(id, true);
            }
        }
        LibraryItem::Node(kind) => match at {
            Some(at) => add_node(s, fit_to_stage(s, kind.clone()), at),
            None => add_from_search(s, state, kind.clone()),
        },
    }
}

fn library_details(kind: &NodeKind) -> (&'static str, &'static str) {
    match kind {
        NodeKind::Drawing { .. } => (
            "Artwork",
            "Exposes a drawing from the Timeline and sends it into the composite.",
        ),
        NodeKind::Peg(_) => (
            "Rigging",
            "Moves, rotates, scales, or changes the depth of connected artwork.",
        ),
        NodeKind::Skeleton(_) => (
            "Rigging",
            "Moves the whole bone rig. A new skeleton starts with a connected bone.",
        ),
        NodeKind::Bone(_) => (
            "Rigging",
            "Poses attached drawings and bends vector artwork around a joint.",
        ),
        NodeKind::MeshWarp(_) => (
            "Rigging",
            "Deforms artwork with a grid or points placed on the drawing.",
        ),
        NodeKind::Composite => (
            "Compositing",
            "Stacks image inputs in port order. The leftmost input renders in front.",
        ),
        NodeKind::Effect(Effect::Mask { .. }) => (
            "Compositing",
            "Uses a matte to reveal or hide parts of an image.",
        ),
        NodeKind::Effect(Effect::Blend { .. }) => {
            ("Compositing", "Blends an image with the artwork behind it.")
        }
        NodeKind::Effect(Effect::Opacity { .. }) => (
            "Compositing",
            "Adjusts the transparency of the input image.",
        ),
        NodeKind::Gradient(_) => (
            "Artwork",
            "Generates a two-colour gradient for a background or effect.",
        ),
        NodeKind::Effect(Effect::Blur { .. }) => (
            "Effects",
            "Softens an image with box, Gaussian, motion or radial blur.",
        ),
        NodeKind::Effect(Effect::Grain { .. }) => (
            "Effects",
            "Adds an adjustable grain texture to the input image.",
        ),
        NodeKind::Display => (
            "Output",
            "Shows the final composite in Camera. Every scene starts with one.",
        ),
        NodeKind::Write => (
            "Output",
            "Marks an output in the node graph. Export is available from the File menu.",
        ),
        NodeKind::Chart(_) => (
            "Motion",
            "A timing and spacing chart: points on key poses, a smooth path and \
             frame spacing. A guide on its own; changes no artwork.",
        ),
    }
}

pub fn library(ui: &mut egui::Ui, s: &mut Session, state: &mut NodeViewState) {
    let mut add = None;
    egui::Frame::new()
        .inner_margin(egui::Margin::same(theme::CARD_PAD))
        .show(ui, |ui| {
            ui.add(egui::TextEdit::singleline(&mut state.library_search).hint_text("Find a node"));
            let query = state.library_search.trim().to_lowercase();
            let drawing = NodeKind::Drawing {
                column: symphony_core::id::Id(0),
            };
            egui::ScrollArea::vertical().show(ui, |ui| {
                for group in [
                    "Artwork",
                    "Rigging",
                    "Motion",
                    "Compositing",
                    "Effects",
                    "Output",
                ] {
                    let items: Vec<_> = std::iter::once(drawing.clone())
                        .chain(addable_nodes())
                        .chain([smart_chart()])
                        .chain([NodeKind::Display, NodeKind::Write])
                        .filter(|kind| {
                            let (category, description) = library_details(kind);
                            category == group
                                && [kind_label(kind), category, description]
                                    .iter()
                                    .any(|text| text.to_lowercase().contains(&query))
                        })
                        .collect();
                    if items.is_empty() {
                        continue;
                    }
                    ui.heading(group);
                    for kind in items {
                        let title = kind_label(&kind);
                        let (_, description) = library_details(&kind);
                        let item = match kind {
                            NodeKind::Drawing { .. } => Some(LibraryItem::Drawing),
                            NodeKind::Display | NodeKind::Write => None,
                            _ => Some(LibraryItem::Node(kind)),
                        };
                        ui.horizontal(|ui| {
                            let sense = if item.is_some() {
                                Sense::drag()
                            } else {
                                Sense::hover()
                            };
                            let name = ui.add(egui::Label::new(title).sense(sense));
                            if let Some(item) = item {
                                name.dnd_set_drag_payload(item.clone());
                                if ui
                                    .small_button("Add")
                                    .on_hover_text(format!("Add {title}"))
                                    .clicked()
                                {
                                    add = Some(item);
                                }
                            }
                        });
                        ui.label(egui::RichText::new(description).color(theme::TEXT_SECONDARY));
                        ui.add_space(5.0);
                    }
                }
                if !s.doc.harmony_nodes.is_empty() {
                    ui.collapsing("Imported scene", |ui| {
                        for (name, count) in &s.doc.harmony_nodes {
                            if name.to_lowercase().contains(&query) {
                                ui.label(format!("{name} × {count}"));
                            }
                        }
                        for link in &s.doc.harmony_connections {
                            if [link.group.as_str(), link.from.as_str(), link.to.as_str()]
                                .iter()
                                .any(|name| name.to_lowercase().contains(&query))
                            {
                                ui.label(format!(
                                    "{} [{}] → {} [{}]",
                                    link.from, link.output, link.to, link.input
                                ));
                            }
                        }
                    });
                }
                ui.collapsing("Harmony reference", |ui| {
                    for row in
                        include_str!("../../../../docs/reference/harmony-27-nodes.tsv").lines()
                    {
                        let mut fields = row.split('\t');
                        let (Some(category), Some(name), Some(path)) =
                            (fields.next(), fields.next(), fields.next())
                        else {
                            continue;
                        };
                        if category.to_lowercase().contains(&query)
                            || name.to_lowercase().contains(&query)
                        {
                            ui.hyperlink_to(
                                name,
                                format!("https://docs.toonboom.com/help/harmony-27/premium/{path}"),
                            )
                            .on_hover_text(category);
                        }
                    }
                });
            });
        });
    if let Some(item) = add {
        add_library_item(s, state, &item, None);
    }
}

pub fn drawing_conversion_label(s: &Session, id: NodeId) -> Option<&'static str> {
    let node = s.doc.graph.node(id)?;
    matches!(node.kind, NodeKind::Drawing { .. }).then(|| {
        if s.doc.graph.is_drawing_matte(id) {
            "Convert to Drawing"
        } else {
            "Convert to Mask Drawing"
        }
    })
}

pub fn convert_drawing(s: &mut Session, id: NodeId) {
    let Some(label) = drawing_conversion_label(s, id) else {
        return;
    };
    let before = s.doc.graph.is_drawing_matte(id);
    s.history.apply(
        &mut s.doc,
        label,
        vec![Edit::SetDrawingMatte {
            id,
            before,
            after: !before,
        }],
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn overlapping(s: &Session) -> Vec<(NodeId, NodeId)> {
        let defaults = default_positions(s);
        let ids: Vec<NodeId> = s.doc.graph.nodes().map(|(id, _)| id).collect();
        let mut out = Vec::new();
        for (k, &a) in ids.iter().enumerate() {
            for &b in &ids[k + 1..] {
                let (pa, pb) = (
                    node_position(s, a, &defaults),
                    node_position(s, b, &defaults),
                );
                let (wa, wb) = (card_width(&s.doc.graph, a), card_width(&s.doc.graph, b));
                if (pa.x - pb.x).abs() < (wa + wb) / 2.0 && (pa.y - pb.y).abs() < H {
                    out.push((a, b));
                }
            }
        }
        out
    }

    /// Cards pushed apart: none overlap, and held cards stay put.
    #[test]
    fn relaxing_clears_overlaps_and_keeps_held_cards() {
        let ids = [NodeId(1), NodeId(2), NodeId(3)];
        let mut p: BTreeMap<NodeId, Vec2> =
            ids.iter().map(|&id| (id, Vec2::new(5.0, 5.0))).collect();
        let held: BTreeSet<NodeId> = [NodeId(2)].into();
        relax(&mut p, |_| W, &held);
        assert_eq!(p[&NodeId(2)], Vec2::new(5.0, 5.0));
        for a in ids {
            for b in ids {
                if a < b {
                    let d = p[&a] - p[&b];
                    assert!(
                        d.x.abs() >= W + NODE_GAP - 0.5 || d.y.abs() >= H + NODE_GAP - 0.5,
                        "{a:?} {b:?} {d:?}"
                    );
                }
            }
        }
    }

    /// Arrange: nothing overlaps, each peg sits straight above its drawing,
    /// and cables between rows don't cross.
    #[test]
    fn bone_batch_updates_only_selected_fields_and_undoes_together() {
        use symphony_core::rig::Bone;
        let mut s = Session::new_scene();
        let graph = &mut s.doc.graph;
        let root = graph.add("Rig", NodeKind::Skeleton(Default::default()));
        let a = graph.add(
            "Shoulder",
            NodeKind::Bone(Bone {
                length: 90.0,
                ..Bone::default()
            }),
        );
        let b = graph.add(
            "Hip",
            NodeKind::Bone(Bone {
                length: 140.0,
                ..Bone::default()
            }),
        );
        let other = graph.add("Hand", NodeKind::Bone(Bone::default()));
        for id in [a, b, other] {
            graph.connect(root, id, 0);
        }
        edit_bone_batch(&mut s, &[a, b], "stiffness", true, |_, bone| {
            bone.spring_stiffness = 0.2
        });
        edit_bone_batch(&mut s, &[a, b], "stiffness", true, |_, bone| {
            bone.spring_stiffness = 0.7
        });
        s.end_gesture();
        let stiffness = |s: &Session, id| s.doc.graph.bone(id).unwrap().spring_stiffness;
        assert_eq!(
            [stiffness(&s, a), stiffness(&s, b), stiffness(&s, other)],
            [0.7, 0.7, 0.0]
        );
        assert_eq!(
            [
                s.doc.graph.bone(a).unwrap().length,
                s.doc.graph.bone(b).unwrap().length
            ],
            [90.0, 140.0]
        );
        s.undo();
        assert_eq!([stiffness(&s, a), stiffness(&s, b)], [0.0, 0.0]);
    }

    #[test]
    fn bone_add_drawing_wires_a_layer_and_undoes_in_one_step() {
        let mut s = Session::new_scene();
        add_node(&mut s, NodeKind::Bone(Default::default()), [0.0, 0.0]);
        let bone = s.selected_node.unwrap();
        add_bone_drawing(&mut s, bone);
        let layer = s.doc.layers[s.selected];
        assert_eq!(s.doc.graph.input(layer.peg, 0), Some(bone));
        assert_eq!(s.doc.graph.input(layer.node, 0), Some(layer.peg));
        assert_eq!(s.doc.graph.drawing_bone(layer.node, s.frame), Some(bone));
        add_bone_drawing(&mut s, bone);
        assert_eq!(s.layer_count(), 3);
        s.undo();
        assert_eq!(s.layer_count(), 2);
        s.undo();
        assert_eq!(s.layer_count(), 1);
        s.redo();
        assert_eq!(s.doc.graph.drawing_bone(layer.node, s.frame), Some(bone));
        change_bone_drawing(&mut s, bone, layer, false);
        assert_eq!(s.doc.graph.drawing_bone(layer.node, s.frame), None);
        s.undo();
        assert_eq!(s.doc.graph.drawing_bone(layer.node, s.frame), Some(bone));
    }

    #[test]
    fn arrange_aligns_artwork_and_first_effects_across_unequal_rigs() {
        let mut s = Session::demo();
        let a = s.doc.layers[0].node;
        let b = s.doc.layers[1].node;
        let composite = s.doc.composite;
        let g = &mut s.doc.graph;
        let skeleton = g.add("Rig", NodeKind::Skeleton(Default::default()));
        let upper = g.add("Upper", NodeKind::Bone(Default::default()));
        let lower = g.add("Lower", NodeKind::Bone(Default::default()));
        g.connect(skeleton, upper, 0);
        g.connect(upper, lower, 0);
        g.connect(lower, a, 0);
        let effect = || NodeKind::Effect(Effect::Opacity { amount: 0.5 });
        let first = g.add("First effect", effect());
        let parallel = g.add("Parallel effect", effect());
        let second = g.add("Second effect", effect());
        g.connect(a, first, 0);
        g.connect(first, second, 0);
        g.connect(b, parallel, 0);
        g.connect(second, composite, 0);
        g.connect(parallel, composite, 1);
        let links = g.links().to_vec();

        let mut state = NodeViewState::default();
        arrange(&mut s, &mut state);
        let y = |id| s.doc.node_positions[&id][1];
        assert!(y(skeleton) < y(upper) && y(upper) < y(lower) && y(lower) < y(a));
        assert_eq!(y(a), y(b));
        assert_eq!(y(first), y(parallel));
        assert!(y(a) < y(first) && y(first) < y(second) && y(second) < y(composite));
        assert!(links.iter().all(|link| y(link.from) < y(link.to)));
        assert_eq!(s.doc.graph.links(), links);
        assert!(overlapping(&s).is_empty());
        let arranged = s.doc.node_positions.clone();
        arrange(&mut s, &mut state);
        assert_eq!(s.doc.node_positions, arranged);
        s.undo();
        assert!(s.doc.node_positions.is_empty());
    }

    #[test]
    fn arrange_is_tidy_and_uncrossed() {
        let mut s = Session::new_scene();
        s.add_layer();
        s.add_layer();
        let mut state = NodeViewState::default();
        arrange(&mut s, &mut state);
        assert!(overlapping(&s).is_empty(), "{:?}", overlapping(&s));
        let defaults = default_positions(&s);
        let at = |id| node_position(&s, id, &defaults);
        for layer in &s.doc.layers {
            assert!(
                (at(layer.peg).x - at(layer.node).x).abs() < 1.0,
                "peg straight above its drawing"
            );
        }
        // No two links between the same pair of rows cross.
        let links = s.doc.graph.links().to_vec();
        for a in &links {
            for b in &links {
                let (a0, a1, b0, b1) = (at(a.from), at(a.to), at(b.from), at(b.to));
                if (a0.y - b0.y).abs() < 1.0
                    && (a1.y - b1.y).abs() < 1.0
                    && a.from != b.from
                    && a.to != b.to
                {
                    assert!(
                        (a0.x - b0.x) * (a1.x - b1.x) >= 0.0,
                        "cables cross: {a:?} {b:?}"
                    );
                }
            }
        }
    }

    /// A node added where another sits moves clear of it.
    #[test]
    fn added_nodes_make_room() {
        let mut s = Session::new_scene();
        let peg = s.doc.layers[0].peg;
        let p = node_position(&s, peg, &default_positions(&s));
        add_node(&mut s, NodeKind::Composite, [p.x, p.y]);
        assert!(overlapping(&s).is_empty(), "{:?}", overlapping(&s));
    }

    #[test]
    fn adding_either_rig_node_starts_a_pair_and_adding_bone_extends_it() {
        for kind in [
            NodeKind::Skeleton(Default::default()),
            NodeKind::Bone(Default::default()),
        ] {
            let mut s = Session::new_scene();
            let root = s.doc.graph.next_id();
            add_node(&mut s, kind, [0.0, 40.0]);
            let upper = s.selected_node.unwrap();
            assert!(matches!(
                s.doc.graph.node(root).unwrap().kind,
                NodeKind::Skeleton(_)
            ));
            assert_eq!(s.doc.graph.input(upper, 0), Some(root));
            assert!(overlapping(&s).is_empty());
            add_node(&mut s, NodeKind::Bone(Default::default()), [0.0, 40.0]);
            let lower = s.selected_node.unwrap();
            assert_eq!(s.doc.graph.input(lower, 0), Some(upper));
            assert_eq!(smart_joint_label(&s, lower), Some("Make smart joint"));
            toggle_smart_joint(&mut s, lower);
            assert_eq!(smart_joint_label(&s, lower), Some("Remove smart joint"));
            s.frame = 4;
            set_selected_key(&mut s, false);
            assert!(s.doc.graph.bone(lower).unwrap().has_key(4));
            set_selected_key(&mut s, true);
            assert!(!s.doc.graph.bone(lower).unwrap().has_key(4));
            s.undo();
            s.undo();
            s.undo();
            assert_eq!(smart_joint_label(&s, lower), Some("Make smart joint"));
            s.undo();
            assert!(s.doc.graph.node(lower).is_none());
            s.undo();
            assert!(s.doc.graph.node(upper).is_none() && s.doc.graph.node(root).is_none());
        }
    }

    #[test]
    fn connecting_keeps_connections_where_it_can() {
        let mut s = Session::new_scene();
        s.add_layer();
        let comp = s.doc.composite;
        let from = |s: &Session, port| s.doc.graph.input(comp, port);
        // Two sources wired into the Composite's first two ports.
        let (a, b) = (s.doc.layers[0].node, s.doc.layers[1].node);
        let ports: Vec<Input> = (0..6).map(|p| (comp, p)).collect();
        for &(_, p) in &ports {
            edit_link(&mut s, None, (comp, p), None);
        }
        edit_link(&mut s, Some(a), (comp, 0), None);
        edit_link(&mut s, Some(b), (comp, 1), None);
        assert_eq!((from(&s, 0), from(&s, 1)), (Some(a), Some(b)));
        // Move a's cable onto b's input: they swap.
        connect_smart(&mut s, a, (comp, 1), Some((comp, 0)), &ports);
        assert_eq!((from(&s, 0), from(&s, 1)), (Some(b), Some(a)));
        s.undo();
        assert_eq!((from(&s, 0), from(&s, 1)), (Some(a), Some(b)));
        // Free b, then connect it again onto a's (used) input: it takes a
        // free port and a stays connected.
        edit_link(&mut s, None, (comp, 1), None);
        connect_smart(&mut s, b, (comp, 0), None, &ports);
        assert_eq!(from(&s, 0), Some(a), "a kept its connection");
        assert!(ports.iter().any(|&(_, p)| p != 0 && from(&s, p) == Some(b)));
    }

    #[test]
    fn drawing_conversion_is_reversible_and_ignores_other_nodes() {
        let mut s = Session::new_scene();
        let layer = s.doc.layers[0];
        assert_eq!(drawing_conversion_label(&s, layer.peg), None);
        convert_drawing(&mut s, layer.peg);
        assert_eq!(
            drawing_conversion_label(&s, layer.node),
            Some("Convert to Mask Drawing")
        );

        convert_drawing(&mut s, layer.node);
        assert_eq!(
            drawing_conversion_label(&s, layer.node),
            Some("Convert to Drawing")
        );
        convert_drawing(&mut s, layer.node);
        assert!(!s.doc.graph.is_drawing_matte(layer.node));
        s.undo();
        assert!(s.doc.graph.is_drawing_matte(layer.node));
        s.undo();
        assert!(!s.doc.graph.is_drawing_matte(layer.node));
    }

    #[test]
    fn a_peg_wired_to_a_drawing_moves_it() {
        use symphony_core::stroke::{Point, Stroke, StrokeKind};
        let mut s = Session::new_scene();
        let st = Stroke {
            id: s.doc.ids.fresh(),
            kind: StrokeKind::Pencil,
            art: Default::default(),
            swatch: s.swatch,
            min_width: 20.0,
            max_width: 20.0,
            points: [(-100.0, 0.0), (100.0, 0.0)]
                .iter()
                .map(|&(x, y)| Point {
                    x,
                    y,
                    pressure: 1.0,
                    width_offset: 0.0,
                    pen: Default::default(),
                })
                .collect(),
            tips: Default::default(),
            texture: None,
            shape: Vec::new(),
        };
        s.commit_stroke(0, s.frame, st);
        let layer = s.doc.layers[0];
        let render = |s: &Session| {
            symphony_render::render_camera(&s.doc, s.frame, 192, 108, None)
                .unwrap()
                .data()
                .to_vec()
        };
        let before = render(&s);
        // A new peg, wired into the drawing's transform input, moved right.
        add_node(&mut s, NodeKind::Peg(Default::default()), [0.0, 0.0]);
        let peg = s.doc.graph.nodes().map(|(id, _)| id).max().unwrap();
        edit_link(&mut s, Some(peg), (layer.node, 0), None);
        eprintln!("links {:?}", s.doc.graph.links());
        let mut node = s.doc.graph.node(peg).unwrap().clone();
        if let NodeKind::Peg(p) = &mut node.kind {
            p.x = symphony_core::anim::Value::Constant(300.0);
        }
        let old = s.doc.graph.node(peg).unwrap().clone();
        s.history.apply(
            &mut s.doc,
            "Move peg",
            vec![Edit::SetNodeProperties {
                id: peg,
                before: old,
                after: node,
            }],
        );
        eprintln!(
            "world {:?}",
            s.doc.graph.world_transform(layer.node, s.frame)
        );
        assert!(render(&s) != before, "the peg moves the drawing");
    }

    #[test]
    fn node_colours_are_per_type_and_stable() {
        let all: Vec<NodeKind> = addable_nodes()
            .chain([
                NodeKind::Display,
                NodeKind::Write,
                NodeKind::Drawing {
                    column: symphony_core::id::Id(1),
                },
            ])
            .collect();
        // The same type always has the same colour...
        assert_eq!(
            kind_colour(&NodeKind::Peg(Default::default())),
            kind_colour(&NodeKind::Peg(Default::default()))
        );
        // ...and every type we have is distinct.
        let mut seen: Vec<egui::Color32> = all.iter().map(kind_colour).collect();
        seen.sort_by_key(|c| c.to_array());
        seen.dedup();
        assert_eq!(seen.len(), all.len());
    }

    #[test]
    fn pulling_a_node_out_of_a_chain_closes_the_chain() {
        let s = Session::new_scene();
        let mut g = s.doc.graph.clone();
        // Drawing -> Blur -> whatever the drawing fed.
        let layer = s.doc.layers[0];
        let old = *g.links().iter().find(|l| l.from == layer.node).unwrap();
        let blur = g.add(
            "Blur",
            NodeKind::Effect(Effect::Blur {
                radius: 4.0,
                mode: BlurMode::Box,
            }),
        );
        g.disconnect_inputs(old.to);
        g.connect(layer.node, blur, 0);
        g.connect(blur, old.to, old.port);
        let after = detached_links(&g, blur);
        assert!(after.iter().all(|l| l.from != blur && l.to != blur));
        assert!(
            after.contains(&Link {
                from: layer.node,
                ..old
            }),
            "the drawing feeds where the blur fed: {after:?}"
        );
        // A drawing node doesn't pass its peg input through as an image,
        // so pulling one out doesn't wire its peg into a composite.
        let healed = detached_links(&g, layer.node);
        assert!(!healed.iter().any(|l| l.from == layer.peg && l.to == blur));
    }

    #[test]
    fn drawing_thumbnails_show_the_artwork() {
        use symphony_core::stroke::{Point, Stroke, StrokeKind};
        let mut s = Session::new_scene();
        let st = Stroke {
            id: s.doc.ids.fresh(),
            kind: StrokeKind::Pencil,
            art: Default::default(),
            swatch: s.swatch,
            min_width: 6.0,
            max_width: 6.0,
            points: [(-50.0, -20.0), (40.0, 30.0)]
                .iter()
                .map(|&(x, y)| Point {
                    x,
                    y,
                    pressure: 1.0,
                    width_offset: 0.0,
                    pen: Default::default(),
                })
                .collect(),
            tips: Default::default(),
            texture: None,
            shape: Vec::new(),
        };
        s.commit_stroke(0, s.frame, st);
        let layer = s.doc.layers[0];
        let column = s.doc.xsheet.column(layer.column).unwrap();
        let drawing = column.get(s.frame).expect("demo drawing");
        let pix = symphony_render::render_drawing_thumbnail(
            &s.doc,
            layer.node,
            layer.column,
            drawing,
            s.frame,
            96,
        )
        .expect("a thumbnail");
        assert!(pix.data().chunks(4).filter(|p| p[3] > 0).count() > 50);
    }
    use egui_kittest::Harness;

    #[test]
    fn wheel_zoom_anchors_the_pointer_and_resize_keeps_the_graph_origin() {
        let mut harness = Harness::builder()
            .with_size(Vec2::new(760.0, 560.0))
            .build_ui_state(
                |ui, (session, state)| canvas(ui, session, state, None),
                (Session::new_scene(), NodeViewState::default()),
            );
        harness.run_steps(2);
        let anchor = harness.state().1.anchor_x.unwrap();
        let pointer = Pos2::new(320.0, 195.0);
        harness
            .input_mut()
            .events
            .push(egui::Event::PointerMoved(pointer));
        harness.run_steps(1);
        harness.input_mut().events.push(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: Vec2::new(0.0, 80.0),
            phase: egui::TouchPhase::Move,
            source: egui::MouseWheelSource::Unknown,
            modifiers: egui::Modifiers::NONE,
        });
        harness.run_steps(1);
        let state = &harness.state().1;
        let zoom = state.zoom.unwrap();
        assert!(zoom > 1.0, "scrolling up zooms in");
        // The zoom transform interpolates between the original origin and pointer.
        assert!(state.pan.x > 0.0 && state.pan.y < 0.0);
        let pan = state.pan;
        harness.set_size(Vec2::new(1000.0, 630.0));
        harness.input_mut().events.push(egui::Event::PointerGone);
        harness.run_steps(2);
        let state = &harness.state().1;
        assert_eq!(state.anchor_x, Some(anchor));
        assert_eq!(state.pan, pan);
        assert_eq!(state.zoom, Some(zoom));
    }

    fn pointer_gesture<S>(
        harness: &mut Harness<'_, S>,
        positions: [Pos2; 3],
        modifiers: egui::Modifiers,
        button: egui::PointerButton,
    ) {
        for (pos, pressed) in positions.into_iter().zip([Some(true), None, Some(false)]) {
            harness.input_mut().events.extend([
                egui::Event::ModifiersChanged(modifiers),
                egui::Event::PointerMoved(pos),
            ]);
            if let Some(pressed) = pressed {
                harness.input_mut().events.push(egui::Event::PointerButton {
                    pos,
                    button,
                    pressed,
                    modifiers,
                });
            }
            harness.run_steps(1);
        }
    }

    #[test]
    fn empty_drag_selects_cards_and_space_drag_pans_over_them() {
        let mut harness = Harness::builder()
            .with_size(Vec2::new(760.0, 560.0))
            .build_ui_state(
                |ui, (s, state, origin)| {
                    *origin = ui.available_rect_before_wrap().center_top();
                    canvas(ui, s, state, None);
                },
                (Session::new_scene(), NodeViewState::default(), Pos2::ZERO),
            );
        harness.run_steps(2);
        let (s, _, origin) = harness.state();
        let layer = s.doc.layers[0];
        let at = *origin + node_position(s, layer.node, &default_positions(s));
        let start = at - Vec2::new(120.0, 30.0);
        let end = at + Vec2::new(100.0, 20.0);
        pointer_gesture(
            &mut harness,
            [start, end, end],
            egui::Modifiers::NONE,
            egui::PointerButton::Primary,
        );
        assert_eq!(harness.state().1.selection, BTreeSet::from([layer.node]));
        assert_eq!(harness.state().1.pan, Vec2::ZERO);

        harness.input_mut().events.push(egui::Event::Key {
            key: egui::Key::Space,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        });
        let end = at + Vec2::new(45.0, 30.0);
        pointer_gesture(
            &mut harness,
            [at, end, end],
            egui::Modifiers::NONE,
            egui::PointerButton::Primary,
        );
        assert_eq!(harness.state().1.pan, end - at);
        assert_eq!(harness.state().1.selection, BTreeSet::from([layer.node]));
        harness.input_mut().events.push(egui::Event::Key {
            key: egui::Key::Space,
            physical_key: None,
            pressed: false,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        });
        let middle_end = end + Vec2::new(30.0, 24.0);
        pointer_gesture(
            &mut harness,
            [end, middle_end, middle_end],
            egui::Modifiers::NONE,
            egui::PointerButton::Middle,
        );
        assert_eq!(harness.state().1.pan, middle_end - at);
        assert_eq!(harness.state().1.selection, BTreeSet::from([layer.node]));
    }

    #[test]
    fn cable_drag_disconnects_and_close_card_drag_does_not_marquee() {
        let mut harness = Harness::builder()
            .with_size(Vec2::new(760.0, 560.0))
            .build_ui_state(
                |ui, (s, state, origin)| {
                    *origin = ui.available_rect_before_wrap().center_top();
                    canvas(ui, s, state, None);
                },
                (Session::new_scene(), NodeViewState::default(), Pos2::ZERO),
            );
        harness.run_steps(2);
        let (s, state, origin) = harness.state();
        let selection = state.selection.clone();
        let layer = s.doc.layers[0];
        let centre = *origin + node_position(s, layer.node, &default_positions(s));
        let near_card = centre - Vec2::new(W * 0.5 + 6.0, 0.0);
        let over_node = centre + Vec2::new(W, H);
        pointer_gesture(
            &mut harness,
            [near_card, over_node, over_node],
            egui::Modifiers::NONE,
            egui::PointerButton::Primary,
        );
        assert_eq!(harness.state().1.selection, selection);
        let (_, state, _) = harness.state();
        let (wire, from, to) = *state
            .drawn_links
            .iter()
            .find(|(link, ..)| link.from == layer.peg)
            .unwrap();
        let start = from.lerp(to, 0.5);
        let free = to + Vec2::new(29.0, -4.0);
        pointer_gesture(
            &mut harness,
            [start, free, free],
            egui::Modifiers::NONE,
            egui::PointerButton::Primary,
        );
        let (s, _, _) = harness.state();
        assert_eq!(s.doc.graph.input(wire.to, wire.port), None);
    }

    #[test]
    fn loose_cable_retracts_without_creating_a_link() {
        for animated in [true, false] {
            let mut harness = Harness::builder()
                .with_size(Vec2::new(760.0, 560.0))
                .build_ui_state(
                    |ui, (s, state)| canvas(ui, s, state, None),
                    (Session::new_scene(), NodeViewState::default()),
                );
            theme::set_motion(&harness.ctx, animated);
            harness.run_steps(2);
            let (s, state) = harness.state();
            let before = s.doc.graph.links().to_vec();
            let source = s.doc.layers[0].peg;
            let from = state
                .drawn_links
                .iter()
                .find(|(link, ..)| link.from == source)
                .unwrap()
                .1;
            let away = from + Vec2::new(190.0, 35.0);
            pointer_gesture(
                &mut harness,
                [from, away, away],
                egui::Modifiers::NONE,
                egui::PointerButton::Primary,
            );
            let (s, state) = harness.state();
            assert_eq!(s.doc.graph.links(), before);
            assert_eq!(state.link_anims.iter().any(|animation| {
                animation.link.is_none()
                    && matches!(animation.motion, CableMotion::Retract { source: id, .. } if id == source)
            }), animated);
        }
    }

    #[test]
    fn option_click_cuts_one_wire_and_option_drag_detaches_a_node() {
        let mut harness = Harness::builder()
            .with_size(Vec2::new(760.0, 560.0))
            .build_ui_state(
                |ui, (s, state, origin)| {
                    *origin = ui.available_rect_before_wrap().center_top();
                    canvas(ui, s, state, None);
                },
                (Session::new_scene(), NodeViewState::default(), Pos2::ZERO),
            );
        harness.run_steps(2);
        let origin = harness.state().2;
        let layer = harness.state().0.doc.layers[0];
        let cable = origin + Vec2::Y * 82.0;
        pointer_gesture(
            &mut harness,
            [cable; 3],
            egui::Modifiers::ALT,
            egui::PointerButton::Primary,
        );
        assert_eq!(harness.state().0.doc.graph.input(layer.node, 0), None);
        harness.state_mut().0.undo();
        assert_eq!(
            harness.state().0.doc.graph.input(layer.node, 0),
            Some(layer.peg)
        );

        let from = origin + Vec2::Y * 120.0;
        let away = from + Vec2::X * 180.0;
        pointer_gesture(
            &mut harness,
            [from, away, away],
            egui::Modifiers::ALT,
            egui::PointerButton::Primary,
        );
        let s = &harness.state().0;
        assert_eq!(s.doc.graph.input(layer.node, 0), None);
        assert_eq!(s.doc.graph.input(s.doc.composite, 0), None);
    }

    #[test]
    fn shift_click_selects_both_nodes_and_drag_moves_the_group() {
        let mut harness = Harness::builder()
            .with_size(Vec2::new(760.0, 560.0))
            .build_ui_state(
                |ui, (s, state, origin)| {
                    *origin = ui.available_rect_before_wrap().center_top();
                    canvas(ui, s, state, None);
                },
                (Session::new_scene(), NodeViewState::default(), Pos2::ZERO),
            );
        harness.run_steps(2);
        let origin = harness.state().2;
        let layer = harness.state().0.doc.layers[0];
        let peg = origin + Vec2::Y * 43.0;
        let drawing = origin + Vec2::Y * 120.0;
        pointer_gesture(
            &mut harness,
            [peg; 3],
            egui::Modifiers::NONE,
            egui::PointerButton::Primary,
        );
        pointer_gesture(
            &mut harness,
            [drawing; 3],
            egui::Modifiers::SHIFT,
            egui::PointerButton::Primary,
        );
        assert_eq!(
            harness.state().1.selection,
            BTreeSet::from([layer.peg, layer.node])
        );
        assert_eq!(harness.state().0.selected_node, Some(layer.node));
        let end = drawing + Vec2::new(50.0, 10.0);
        pointer_gesture(
            &mut harness,
            [drawing, end, end],
            egui::Modifiers::NONE,
            egui::PointerButton::Primary,
        );
        assert_eq!(
            harness.state().0.doc.node_positions[&layer.node],
            [50.0, 130.0]
        );
        assert_eq!(
            harness.state().0.doc.node_positions[&layer.peg],
            [50.0, 53.0]
        );
    }

    #[test]
    fn alt_drag_inserts_a_peg_into_a_cable_in_one_undo_step() {
        let mut session = Session::new_scene();
        let layer = session.doc.layers[0];
        let start = [160.0, 82.0];
        let inserted = session.doc.graph.next_id();
        add_node(&mut session, NodeKind::Peg(Default::default()), start);
        let mut harness = Harness::builder()
            .with_size(Vec2::new(760.0, 560.0))
            .build_ui_state(
                |ui, (session, state, origin)| {
                    *origin = ui.available_rect_before_wrap().center_top();
                    canvas(ui, session, state, None);
                },
                (session, NodeViewState::default(), Pos2::ZERO),
            );
        harness.run_steps(2);
        let origin = harness.state().2;
        let from = origin + Vec2::new(start[0], start[1]);
        let to = origin + Vec2::new(0.0, 82.0);
        let alt = egui::Modifiers::ALT;
        for (pos, pressed) in [(from, Some(true)), (to, None), (to, Some(false))] {
            harness.input_mut().events.extend([
                egui::Event::ModifiersChanged(alt),
                egui::Event::PointerMoved(pos),
            ]);
            if let Some(pressed) = pressed {
                harness.input_mut().events.push(egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: alt,
                });
            }
            harness.run_steps(1);
        }
        let s = &mut harness.state_mut().0;
        assert_eq!(s.doc.graph.input(inserted, 0), Some(layer.peg));
        assert_eq!(s.doc.graph.input(layer.node, 0), Some(inserted));
        s.undo();
        assert_eq!(s.doc.graph.input(inserted, 0), None);
        assert_eq!(s.doc.graph.input(layer.node, 0), Some(layer.peg));
        assert_eq!(s.doc.node_positions[&inserted], start);
    }

    #[test]
    fn shift_drag_aligns_connected_ports_without_changing_wiring() {
        let mut harness = Harness::builder()
            .with_size(Vec2::new(760.0, 560.0))
            .build_ui_state(
                |ui, (session, state, origin)| {
                    *origin = ui.available_rect_before_wrap().center_top();
                    canvas(ui, session, state, None);
                },
                (Session::new_scene(), NodeViewState::default(), Pos2::ZERO),
            );
        harness.run_steps(2);
        let (s, _, origin) = harness.state();
        let layer = s.doc.layers[0];
        let start = *origin + Vec2::new(0.0, 43.0);
        let end = *origin + Vec2::new(14.0, 58.0);
        let shift = egui::Modifiers::SHIFT;
        for (pos, pressed) in [(start, Some(true)), (end, None), (end, Some(false))] {
            harness
                .input_mut()
                .events
                .push(egui::Event::ModifiersChanged(shift));
            harness
                .input_mut()
                .events
                .push(egui::Event::PointerMoved(pos));
            if let Some(pressed) = pressed {
                harness.input_mut().events.push(egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: shift,
                });
            }
            harness.run_steps(1);
        }
        let s = &harness.state().0;
        assert_eq!(s.doc.node_positions[&layer.peg], [0.0, 58.0]);
        assert_eq!(s.doc.graph.input(layer.node, 0), Some(layer.peg));
    }

    #[test]
    fn arranging_follows_wires_and_undo_restores_positions() {
        let mut s = Session::new_scene();
        let mut state = NodeViewState::default();
        let layer = s.doc.layers[0];
        s.doc.node_positions.insert(layer.peg, [200.0, 120.0]);
        arrange(&mut s, &mut state);
        assert!(s
            .doc
            .graph
            .links()
            .iter()
            .all(|link| s.doc.node_positions[&link.from][1] < s.doc.node_positions[&link.to][1]));
        assert!(state.fit_next.is_some());
        s.undo();
        assert_eq!(s.doc.node_positions[&layer.peg], [200.0, 120.0]);
        assert!(!s.doc.node_positions.contains_key(&layer.node));
    }

    #[test]
    fn camera_pan_and_peg_depth_can_be_keyed_and_retimed() {
        let mut s = Session::new_scene();
        let camera = s.doc.display;
        let layer = s.doc.layers[0];
        let mut draft = PropertyDraft::new(&s, camera).unwrap();
        apply_properties(&mut s, &mut draft, true, false);
        s.frame = 11;
        draft = PropertyDraft::new(&s, camera).unwrap();
        let motion = draft.motion.as_mut().unwrap();
        motion.x.set_key(11, 100.0);
        motion.dolly.set_key(11, 200.0);
        apply_properties(&mut s, &mut draft, true, false);

        s.frame = 1;
        let mut draft = PropertyDraft::new(&s, layer.peg).unwrap();
        apply_properties(&mut s, &mut draft, true, false);
        s.frame = 11;
        draft = PropertyDraft::new(&s, layer.peg).unwrap();
        let NodeKind::Peg(peg) = &mut draft.node.kind else {
            unreachable!()
        };
        peg.z.set_key(11, 300.0);
        apply_properties(&mut s, &mut draft, true, false);

        assert_eq!(s.doc.camera_motion.at(6), [50.0, 0.0, 100.0]);
        assert_eq!(s.doc.graph.world_pose(layer.node, 6).unwrap().depth, 150.0);
        assert_eq!(s.doc.graph.world_pose(layer.node, 11).unwrap().depth, 300.0);
        s.frame = 5;
        s.insert_inbetweens(2);
        assert!(s.doc.camera_motion.dolly.has_key(13));
        assert!(s.doc.graph.has_key(layer.peg, 13));
        assert_eq!(s.doc.camera_motion.at(13), [100.0, 0.0, 200.0]);
    }

    #[test]
    fn effect_keys_interpolate_and_are_undoable() {
        let mut s = Session::new_scene();
        add_node(
            &mut s,
            NodeKind::Effect(Effect::Opacity { amount: 0.0 }),
            [0.0, 80.0],
        );
        let effect = s.selected_node.unwrap();
        s.doc.graph.connect(s.doc.composite, effect, 0);
        s.doc.graph.connect(effect, s.doc.display, 0);
        let mut draft = PropertyDraft::new(&s, effect).unwrap();
        apply_properties(&mut s, &mut draft, true, false);
        s.frame = 11;
        draft = PropertyDraft::new(&s, effect).unwrap();
        draft.node.kind = NodeKind::Effect(Effect::Opacity { amount: 1.0 });
        apply_properties(&mut s, &mut draft, true, false);

        let amount_at = |s: &Session| {
            s.doc
                .graph
                .render_commands(s.doc.display, &s.doc.xsheet, 6)
                .unwrap()
                .into_iter()
                .find_map(|command| match command {
                    symphony_core::graph::RenderCommand::EndEffect(Effect::Opacity { amount }) => {
                        Some(amount)
                    }
                    _ => None,
                })
        };
        assert_eq!(amount_at(&s), Some(0.5));
        s.undo();
        assert_eq!(amount_at(&s), Some(0.0));
    }

    #[test]
    fn camera_opens_properties_on_double_click_and_search_adds_a_node() {
        let mut harness = Harness::builder()
            .with_size(Vec2::new(760.0, 560.0))
            .build_ui_state(
                |ui, (session, state, origin)| {
                    *origin = ui.available_rect_before_wrap().center_top();
                    canvas(ui, session, state, None);
                },
                (Session::new_scene(), NodeViewState::default(), Pos2::ZERO),
            );
        harness.run_steps(2);
        let (session, _, origin) = harness.state();
        let camera = session.doc.display;
        let pos = *origin + Vec2::Y * 291.0;
        harness
            .input_mut()
            .events
            .push(egui::Event::PointerMoved(pos));
        harness.run_steps(1);
        for (i, pressed) in [true, false, true, false].into_iter().enumerate() {
            harness.input_mut().time = Some(1.0 + i as f64 * 0.03);
            harness.input_mut().events.extend([
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ]);
            harness.run_steps(1);
            if i == 1 {
                assert_eq!(harness.state().0.selected_node, Some(camera));
            }
        }
        let (session, state, _) = harness.state_mut();
        let draft = state.properties.as_ref().unwrap();
        assert_eq!(draft.id, camera);
        assert_eq!(draft.node.name, "Camera");

        add_from_search(session, state, NodeKind::Gradient(Default::default()));
        let added = session.selected_node.unwrap();
        assert!(matches!(
            session.doc.graph.node(added).unwrap().kind,
            NodeKind::Gradient(_)
        ));
        session.undo();
        assert!(session.doc.graph.node(added).is_none());
    }

    #[test]
    fn drag_nodes_and_rewire_inputs_with_undo() {
        let mut h = Harness::builder()
            .with_size(Vec2::new(760.0, 560.0))
            .build_ui_state(
                |ui, (session, state, origin, add)| {
                    *origin = ui.available_rect_before_wrap().center_top();
                    canvas(ui, session, state, add.take());
                },
                (
                    Session::new_scene(),
                    NodeViewState::default(),
                    Pos2::ZERO,
                    None::<NodeKind>,
                ),
            );
        h.run_steps(2);
        let (session, _, origin, _) = h.state();
        let (peg, drawing) = (session.doc.layers[0].peg, session.doc.layers[0].node);
        let (composite, display) = (session.doc.composite, session.doc.display);
        let point = |x, y| *origin + Vec2::new(x, y);
        let peg_center = point(0.0, 43.0);
        let moved_peg = point(-110.0, 58.0);
        let peg_output = moved_peg + Vec2::Y * (H / 2.0);
        let drawing_input = point(0.0, 120.0 - H / 2.0);
        let composite_output = point(0.0, 209.0 + H / 2.0);
        let display_input = point(0.0, 291.0 - H / 2.0);
        let blank = point(190.0, 110.0);
        let drag = |h: &mut Harness<'_, _>, from, to| {
            for (pos, pressed) in [(from, Some(true)), (to, None), (to, Some(false))] {
                h.input_mut().events.push(egui::Event::PointerMoved(pos));
                if let Some(pressed) = pressed {
                    h.input_mut().events.push(egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    });
                }
                h.run_steps(1);
            }
        };

        drag(&mut h, peg_center, moved_peg);
        assert_eq!(h.state().0.doc.node_positions[&peg], [-110.0, 58.0]);
        drag(&mut h, drawing_input, blank);
        assert_eq!(h.state().0.doc.graph.input(drawing, 0), None);
        drag(&mut h, peg_output, drawing_input);
        assert_eq!(h.state().0.doc.graph.input(drawing, 0), Some(peg));

        h.state_mut().3 = Some(NodeKind::Composite);
        let extra = h.state().0.doc.graph.next_id();
        h.run_steps(2);
        let [x, y] = h.state().0.doc.node_positions[&extra];
        let extra_output = blank + Vec2::new(x - 190.0, y - 110.0 + H / 2.0);
        drag(&mut h, extra_output, display_input);
        assert_eq!(h.state().0.doc.graph.input(display, 0), Some(extra));
        drag(&mut h, composite_output, display_input);
        assert_eq!(h.state().0.doc.graph.input(display, 0), Some(composite));

        let session = &mut h.state_mut().0;
        session.history.undo(&mut session.doc);
        assert_eq!(session.doc.graph.input(display, 0), Some(extra));
        session.history.redo(&mut session.doc);
        assert_eq!(session.doc.graph.input(display, 0), Some(composite));

        let extra_input = extra_output - Vec2::Y * H;
        drag(&mut h, display_input, extra_input);
        let session = &mut h.state_mut().0;
        assert_eq!(session.doc.graph.input(display, 0), None);
        assert_eq!(session.doc.graph.input(extra, 0), Some(composite));
        session.history.undo(&mut session.doc);
        assert_eq!(session.doc.graph.input(display, 0), Some(composite));
        assert_eq!(session.doc.graph.input(extra, 0), None);
    }
}
