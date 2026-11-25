//! Node graph. Data flows top to bottom like Harmony's Node view:
//! Peg -> Drawing -> Composite -> Display / Write.
//!
//! Node IDs are slots that are never reused, so removing a node leaves a
//! gap instead of renumbering the others (E2.2).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::anim::Value;
use crate::id::Id;
use crate::math::Affine;
use crate::rig::{Bone, BoneField, BonePose};
use crate::xsheet::Xsheet;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct NodeId(pub usize);

/// Transform parameters on a peg. Order: scale and rotate around the
/// pivot, then translate by position.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PegParams {
    pub x: Value,
    pub y: Value,
    /// Positive depth places artwork farther from the camera.
    #[serde(default)]
    pub z: Value,
    pub rotation: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tilt: Option<Box<[Value; 2]>>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub tilt_enabled: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub face_camera: bool,
    pub scale_x: Option<Value>,
    pub scale_y: Option<Value>,
    pub pivot: (f32, f32),
}

impl PegParams {
    pub fn transform_at(&self, frame: u32) -> Affine {
        self.stage_transform(frame).transform
    }

    fn flat_transform_at(&self, frame: u32) -> Affine {
        let sx = self.scale_x.as_ref().map_or(1.0, |v| v.at(frame));
        let sy = self.scale_y.as_ref().map_or(1.0, |v| v.at(frame));
        let (px, py) = self.pivot;
        Affine::translate(self.x.at(frame), self.y.at(frame))
            .compose(Affine::translate(px, py))
            .compose(Affine::rotate_deg(self.rotation.at(frame)))
            .compose(Affine::scale(sx, sy))
            .compose(Affine::translate(-px, -py))
    }

    fn stage_transform(&self, frame: u32) -> StagePose {
        let base = self.flat_transform_at(frame);
        let z = self.z.at(frame);
        let Some(angles) = self.tilt.as_ref().filter(|_| self.tilt_enabled) else {
            return StagePose::from_rows([
                [base.a, base.c, 0.0, base.tx],
                [base.b, base.d, 0.0, base.ty],
                [0.0, 0.0, 1.0, z],
            ]);
        };
        let angle = |v: &Value| {
            let value = v.at(frame);
            if value.is_finite() {
                value.to_radians()
            } else {
                0.0
            }
        };
        let (sx, cx) = angle(&angles[0]).sin_cos();
        let (sy, cy) = angle(&angles[1]).sin_cos();
        let (px, py) = base.apply(self.pivot.0, self.pivot.1);
        let rotate = |x: f32, y: f32| [cy * x + sy * sx * y, cx * y, -sy * x + cy * sx * y];
        let a = rotate(base.a, base.b);
        let b = rotate(base.c, base.d);
        let t = rotate(base.tx - px, base.ty - py);
        StagePose::from_rows([
            [a[0], b[0], sy * cx, t[0] + px],
            [a[1], b[1], -sx, t[1] + py],
            [a[2], b[2], cy * cx, t[2] + z],
        ])
    }

    /// Does any parameter have a key on `frame`?
    pub fn has_key(&self, frame: u32) -> bool {
        [
            Some(&self.x),
            Some(&self.y),
            Some(&self.z),
            Some(&self.rotation),
            self.scale_x.as_ref(),
            self.scale_y.as_ref(),
        ]
        .into_iter()
        .flatten()
        .chain(self.tilt.iter().flat_map(|angles| angles.iter()))
        .any(|v| v.has_key(frame))
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum NodeKind {
    /// Shows whatever drawing its xsheet column exposes on the current frame.
    Drawing {
        column: Id,
    },
    Peg(PegParams),
    Skeleton(PegParams),
    Bone(Bone),
    /// Bends the artwork under it with a grid of points (Harmony's Mesh
    /// Warp). A position node, like a peg.
    /// Boxed: it's much bigger than the other kinds.
    MeshWarp(Box<crate::warp::MeshWarp>),
    /// Stacks its inputs. Port 0 (leftmost) is in front.
    Composite,
    Effect(Effect),
    Gradient(Gradient),
    /// What the Camera view shows.
    Display,
    /// Render output.
    Write,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Effect {
    Mask {
        inverted: bool,
    },
    Blur {
        radius: f32,
        #[serde(default)]
        mode: BlurMode,
    },
    Grain {
        amount: f32,
        seed: u32,
    },
    Opacity {
        amount: f32,
    },
    /// Blends its input onto everything behind it with `mode`, at
    /// `amount` opacity (the Blend Modes node).
    Blend {
        mode: BlendMode,
        amount: f32,
    },
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum BlurMode {
    #[default]
    Box,
    Gaussian,
    Motion {
        angle: f32,
    },
    RadialSpin,
    RadialZoom,
}

impl BlurMode {
    pub const ALL: [Self; 5] = [
        Self::Box,
        Self::Gaussian,
        Self::Motion { angle: 0.0 },
        Self::RadialSpin,
        Self::RadialZoom,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Self::Box => "Box",
            Self::Gaussian => "Gaussian",
            Self::Motion { .. } => "Motion",
            Self::RadialSpin => "Radial Spin",
            Self::RadialZoom => "Radial Zoom",
        }
    }
}

/// How an image combines with what's behind it. Formulas follow the W3C
/// Compositing and Blending spec, plus Add, Subtract, Divide and Linear
/// Burn as painting apps define them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BlendMode {
    #[default]
    Normal,
    Multiply,
    Screen,
    Overlay,
    Darken,
    Lighten,
    ColorDodge,
    ColorBurn,
    LinearBurn,
    Add,
    Subtract,
    Divide,
    Difference,
    Exclusion,
    HardLight,
    SoftLight,
    Hue,
    Saturation,
    Color,
    Luminosity,
}

impl BlendMode {
    pub const ALL: [BlendMode; 20] = [
        BlendMode::Normal,
        BlendMode::Multiply,
        BlendMode::Screen,
        BlendMode::Overlay,
        BlendMode::Darken,
        BlendMode::Lighten,
        BlendMode::ColorDodge,
        BlendMode::ColorBurn,
        BlendMode::LinearBurn,
        BlendMode::Add,
        BlendMode::Subtract,
        BlendMode::Divide,
        BlendMode::Difference,
        BlendMode::Exclusion,
        BlendMode::HardLight,
        BlendMode::SoftLight,
        BlendMode::Hue,
        BlendMode::Saturation,
        BlendMode::Color,
        BlendMode::Luminosity,
    ];

    pub fn title(self) -> &'static str {
        match self {
            BlendMode::Normal => "Normal",
            BlendMode::Multiply => "Multiply",
            BlendMode::Screen => "Screen",
            BlendMode::Overlay => "Overlay",
            BlendMode::Darken => "Darken",
            BlendMode::Lighten => "Lighten",
            BlendMode::ColorDodge => "Color Dodge",
            BlendMode::ColorBurn => "Color Burn",
            BlendMode::LinearBurn => "Linear Burn",
            BlendMode::Add => "Add",
            BlendMode::Subtract => "Subtract",
            BlendMode::Divide => "Divide",
            BlendMode::Difference => "Difference",
            BlendMode::Exclusion => "Exclusion",
            BlendMode::HardLight => "Hard Light",
            BlendMode::SoftLight => "Soft Light",
            BlendMode::Hue => "Hue",
            BlendMode::Saturation => "Saturation",
            BlendMode::Color => "Color",
            BlendMode::Luminosity => "Luminosity",
        }
    }

    /// Plain words people use for what this mode does, for search.
    pub fn words(self) -> &'static str {
        match self {
            BlendMode::Normal => "over regular default",
            BlendMode::Multiply => "shadow shade darker tint ink",
            BlendMode::Screen => "light glow brighter lighter",
            BlendMode::Overlay => "contrast texture light shadow",
            BlendMode::Darken => "darker min minimum",
            BlendMode::Lighten => "lighter max maximum",
            BlendMode::ColorDodge => "glow highlight bright light",
            BlendMode::ColorBurn => "dark deep shadow contrast",
            BlendMode::LinearBurn => "dark shadow burn",
            BlendMode::Add => "plus linear dodge glow light additive",
            BlendMode::Subtract => "minus remove take away",
            BlendMode::Divide => "divide brighten remove colour cast",
            BlendMode::Difference => "invert compare difference",
            BlendMode::Exclusion => "invert soft difference",
            BlendMode::HardLight => "contrast strong light",
            BlendMode::SoftLight => "gentle soft contrast light",
            BlendMode::Hue => "colour color hue tint recolour",
            BlendMode::Saturation => "colour color saturation vivid grey",
            BlendMode::Color => "colour color tint recolour colorize",
            BlendMode::Luminosity => "brightness light value luminance",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Gradient {
    pub start: [u8; 4],
    pub end: [u8; 4],
    pub angle: f32,
}

impl Default for Gradient {
    fn default() -> Self {
        Self {
            start: [35, 48, 77, 255],
            end: [173, 196, 211, 255],
            angle: 90.0,
        }
    }
}

pub enum RenderCommand {
    Drawing(RenderItem),
    Gradient(Gradient),
    BeginEffect,
    EndEffect(Effect),
    EndMask { inverted: bool },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Node {
    pub name: String,
    pub kind: NodeKind,
    pub enabled: bool,
}

/// Animated properties shared by every node, with image parameters where applicable.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct NodeKey {
    pub enabled: bool,
    pub effect: Option<Effect>,
    pub gradient: Option<Gradient>,
}

impl NodeKey {
    pub fn from_node(node: &Node) -> Self {
        Self {
            enabled: node.enabled,
            effect: match node.kind {
                NodeKind::Effect(value) => Some(value),
                _ => None,
            },
            gradient: match node.kind {
                NodeKind::Gradient(value) => Some(value),
                _ => None,
            },
        }
    }

    fn between(self, next: Self, t: f32) -> Self {
        let lerp = |from: f32, to: f32| from + (to - from) * t;
        let effect = match (self.effect, next.effect) {
            (
                Some(Effect::Blur { radius: a, mode }),
                Some(Effect::Blur {
                    radius: b,
                    mode: next,
                }),
            ) => {
                let mode = match (mode, next) {
                    (BlurMode::Motion { angle: a }, BlurMode::Motion { angle: b }) => {
                        let delta = (b - a + 180.0).rem_euclid(360.0) - 180.0;
                        BlurMode::Motion {
                            angle: (a + delta * t).rem_euclid(360.0),
                        }
                    }
                    _ if mode == next => mode,
                    _ => return self,
                };
                Some(Effect::Blur {
                    radius: lerp(a, b),
                    mode,
                })
            }
            (Some(Effect::Grain { amount: a, seed }), Some(Effect::Grain { amount: b, .. })) => {
                Some(Effect::Grain {
                    amount: lerp(a, b),
                    seed,
                })
            }
            (Some(Effect::Opacity { amount: a }), Some(Effect::Opacity { amount: b })) => {
                Some(Effect::Opacity { amount: lerp(a, b) })
            }
            // The mode holds until the next key; its opacity eases.
            (Some(Effect::Blend { mode, amount: a }), Some(Effect::Blend { amount: b, .. })) => {
                Some(Effect::Blend {
                    mode,
                    amount: lerp(a, b),
                })
            }
            _ => self.effect,
        };
        let gradient = self
            .gradient
            .zip(next.gradient)
            .map(|(a, b)| {
                let colour = |from: [u8; 4], to: [u8; 4]| {
                    std::array::from_fn(|i| lerp(from[i] as f32, to[i] as f32).round() as u8)
                };
                Gradient {
                    start: colour(a.start, b.start),
                    end: colour(a.end, b.end),
                    angle: lerp(a.angle, b.angle),
                }
            })
            .or(self.gradient);
        Self {
            enabled: self.enabled,
            effect,
            gradient,
        }
    }
}

/// Output of `from` feeds input port `port` of `to`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Link {
    pub from: NodeId,
    pub to: NodeId,
    pub port: usize,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Graph {
    nodes: Vec<Option<Node>>,
    links: Vec<Link>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    keys: BTreeMap<NodeId, BTreeMap<u32, NodeKey>>,
    #[serde(default, skip_serializing_if = "std::collections::BTreeSet::is_empty")]
    matte_drawings: std::collections::BTreeSet<NodeId>,
}

/// Spatial pose of a drawing plane and its affine projection onto XY.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StagePose {
    pub transform: Affine,
    pub depth: f32,
    rows: [[f32; 4]; 3],
    front: Option<Affine>,
    camera_facing: bool,
}

impl StagePose {
    fn from_rows(rows: [[f32; 4]; 3]) -> Self {
        let [[a, c, _, tx], [b, d, _, ty], [_, _, _, depth]] = rows;
        Self {
            transform: Affine { a, b, c, d, tx, ty },
            depth,
            rows,
            front: None,
            camera_facing: false,
        }
    }

    fn compose(self, rhs: Self) -> Self {
        Self::from_rows(self.rows.map(|row| {
            std::array::from_fn(|column| {
                let projected = row[0] * rhs.rows[0][column]
                    + row[1] * rhs.rows[1][column]
                    + row[2] * rhs.rows[2][column];
                if column == 3 {
                    projected + row[3]
                } else {
                    projected
                }
            })
        }))
    }

    fn artwork_plane(self) -> Option<Affine> {
        if !self.camera_facing {
            return None;
        }
        let [[a, c, _, tx], [b, d, _, ty], _] = self.rows;
        Some(Affine { a, b, c, d, tx, ty })
    }

    pub fn point_at(self, x: f32, y: f32) -> [f32; 3] {
        self.rows.map(|row| row[0] * x + row[1] * y + row[3])
    }
}

impl Default for StagePose {
    fn default() -> Self {
        Self::from_rows([
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
        ])
    }
}

/// One drawing to draw, with its accumulated transform.
#[derive(Clone, Debug, PartialEq)]
pub struct RenderItem {
    pub node: NodeId,
    pub column: Id,
    pub drawing: Id,
    /// The pegs' transform (Mesh Warps count as no change here: tools
    /// work on the drawing as drawn).
    pub transform: Affine,
    /// Physical plane for imported images when vector artwork faces Camera.
    pub image_transform: Option<Affine>,
    pub depth: f32,
    pub matte_pass: bool,
    /// Mesh Warps above the drawing that bend it, if any.
    pub warp: Option<std::sync::Arc<crate::warp::Warp>>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum EvalError {
    /// The graph loops back on itself (or is absurdly deep).
    Cycle(NodeId),
    UnknownNode(NodeId),
}

const MAX_DEPTH: usize = 512;

impl Graph {
    pub fn add(&mut self, name: impl Into<String>, kind: NodeKind) -> NodeId {
        self.nodes.push(Some(Node {
            name: name.into(),
            kind,
            enabled: true,
        }));
        NodeId(self.nodes.len() - 1)
    }

    pub fn node(&self, id: NodeId) -> Option<&Node> {
        self.nodes.get(id.0).and_then(Option::as_ref)
    }

    pub fn keys(&self, id: NodeId) -> Option<&BTreeMap<u32, NodeKey>> {
        self.keys.get(&id)
    }

    pub fn has_key(&self, id: NodeId, frame: u32) -> bool {
        self.keys(id).is_some_and(|keys| keys.contains_key(&frame))
            || self.node(id).is_some_and(|node| match &node.kind {
                NodeKind::Peg(peg) | NodeKind::Skeleton(peg) => peg.has_key(frame),
                NodeKind::Bone(bone) => bone.has_key(frame),
                NodeKind::MeshWarp(warp) => warp.has_key(frame),
                _ => false,
            })
    }

    pub(crate) fn set_keys(&mut self, id: NodeId, keys: BTreeMap<u32, NodeKey>) {
        if keys.is_empty() {
            self.keys.remove(&id);
        } else {
            self.keys.insert(id, keys);
        }
    }

    fn sample_key(&self, id: NodeId, frame: u32) -> Option<NodeKey> {
        let keys = self.keys(id)?;
        let Some((&start, &value)) = keys.range(..=frame).next_back() else {
            return keys.first_key_value().map(|(_, key)| *key);
        };
        if start == frame {
            return Some(value);
        }
        Some(match keys.range(frame..).next() {
            Some((&end, &next)) => {
                value.between(next, (frame - start) as f32 / (end - start) as f32)
            }
            None => value,
        })
    }

    pub fn node_at_frame(&self, id: NodeId, frame: u32) -> Option<Node> {
        let mut node = self.node(id)?.clone();
        if let Some(key) = self.sample_key(id, frame) {
            node.enabled = key.enabled;
            match &mut node.kind {
                NodeKind::Effect(effect) => *effect = key.effect.unwrap_or(*effect),
                NodeKind::Gradient(gradient) => *gradient = key.gradient.unwrap_or(*gradient),
                _ => {}
            }
        }
        Some(node)
    }

    pub fn node_mut(&mut self, id: NodeId) -> Option<&mut Node> {
        self.nodes.get_mut(id.0).and_then(Option::as_mut)
    }

    /// Remove a node and its links. Returns them so the removal can be undone.
    pub fn remove(&mut self, id: NodeId) -> Option<(Node, Vec<Link>)> {
        let node = self.nodes.get_mut(id.0)?.take()?;
        let (gone, keep): (Vec<Link>, Vec<Link>) =
            self.links.iter().partition(|l| l.from == id || l.to == id);
        self.links = keep;
        Some((node, gone))
    }

    /// Put a removed node back in its original slot.
    pub fn restore(&mut self, id: NodeId, node: Node, links: &[Link]) {
        if self.nodes.len() <= id.0 {
            self.nodes.resize(id.0 + 1, None);
        }
        self.nodes[id.0] = Some(node);
        for l in links {
            self.connect(l.from, l.to, l.port);
        }
    }

    pub fn next_id(&self) -> NodeId {
        NodeId(self.nodes.len())
    }

    pub fn replace_links(&mut self, links: Vec<Link>) {
        self.links = links;
    }

    pub fn is_drawing_matte(&self, id: NodeId) -> bool {
        self.matte_drawings.contains(&id)
    }

    pub(crate) fn set_drawing_matte(&mut self, id: NodeId, matte: bool) {
        if matte {
            self.matte_drawings.insert(id);
        } else {
            self.matte_drawings.remove(&id);
        }
    }

    pub fn can_connect_to(&self, from: NodeId, to: NodeId, port: usize) -> bool {
        let Some(target) = self.node(to) else {
            return false;
        };
        let mask = matches!(target.kind, NodeKind::Effect(Effect::Mask { .. }));
        if self.is_drawing_matte(from) && !(mask && port == 1) {
            return false;
        }
        if !matches!(target.kind, NodeKind::Composite) && port > usize::from(mask) {
            return false;
        }
        self.can_connect(from, to)
    }

    /// Image and transform ports carry different data. Reject incompatible
    /// endpoints and cycles before committing an interactive connection.
    pub fn can_connect(&self, from: NodeId, to: NodeId) -> bool {
        let (Some(source), Some(target)) = (self.node(from), self.node(to)) else {
            return false;
        };
        let valid = match source.kind {
            NodeKind::Peg(_) | NodeKind::MeshWarp(_) => matches!(
                target.kind,
                NodeKind::Peg(_)
                    | NodeKind::MeshWarp(_)
                    | NodeKind::Skeleton(_)
                    | NodeKind::Drawing { .. }
            ),
            NodeKind::Skeleton(_) => matches!(target.kind, NodeKind::Bone(_)),
            NodeKind::Bone(_) => matches!(
                target.kind,
                NodeKind::Bone(_)
                    | NodeKind::Peg(_)
                    | NodeKind::MeshWarp(_)
                    | NodeKind::Drawing { .. }
            ),
            NodeKind::Composite
            | NodeKind::Drawing { .. }
            | NodeKind::Effect(_)
            | NodeKind::Gradient(_) => matches!(
                target.kind,
                NodeKind::Composite | NodeKind::Effect(_) | NodeKind::Display | NodeKind::Write
            ),
            NodeKind::Display | NodeKind::Write => false,
        };
        if !valid {
            return false;
        }
        if self.bone(to).is_some_and(|bone| bone.smart.is_some())
            && self.bone(from).is_some_and(|bone| {
                bone.smart.is_some() || self.smart_child(from).is_some_and(|other| other != to)
            })
        {
            return false;
        }
        let mut pending = vec![to];
        let mut visited = std::collections::BTreeSet::new();
        while let Some(node) = pending.pop() {
            if node == from {
                return false;
            }
            if visited.insert(node) {
                pending.extend(self.links.iter().filter(|l| l.from == node).map(|l| l.to));
            }
        }
        true
    }

    pub fn nodes(&self) -> impl Iterator<Item = (NodeId, &Node)> {
        self.nodes
            .iter()
            .enumerate()
            .filter_map(|(i, n)| n.as_ref().map(|n| (NodeId(i), n)))
    }

    pub fn links(&self) -> &[Link] {
        &self.links
    }

    /// Connect `from` into `to`'s input `port`, replacing whatever was there.
    pub fn connect(&mut self, from: NodeId, to: NodeId, port: usize) {
        self.links.retain(|l| !(l.to == to && l.port == port));
        self.links.push(Link { from, to, port });
    }

    pub fn disconnect_inputs(&mut self, to: NodeId) {
        self.links.retain(|l| l.to != to);
    }

    pub fn input(&self, to: NodeId, port: usize) -> Option<NodeId> {
        self.links
            .iter()
            .find(|l| l.to == to && l.port == port)
            .map(|l| l.from)
    }

    /// Inputs sorted by port.
    fn inputs(&self, to: NodeId) -> Vec<(usize, NodeId)> {
        let mut v: Vec<_> = self
            .links
            .iter()
            .filter(|l| l.to == to)
            .map(|l| (l.port, l.from))
            .collect();
        v.sort();
        v
    }

    /// Everything visible from `out` (a Display or Write) on `frame`, back to front.
    pub fn evaluate(
        &self,
        out: NodeId,
        xsheet: &Xsheet,
        frame: u32,
    ) -> Result<Vec<RenderItem>, EvalError> {
        Ok(self
            .render_commands(out, xsheet, frame)?
            .into_iter()
            .filter_map(|command| match command {
                RenderCommand::Drawing(item) if !item.matte_pass => Some(item),
                _ => None,
            })
            .collect())
    }

    pub fn render_commands(
        &self,
        out: NodeId,
        xsheet: &Xsheet,
        frame: u32,
    ) -> Result<Vec<RenderCommand>, EvalError> {
        let mut commands = Vec::new();
        self.eval_image(out, xsheet, frame, 0, false, &mut commands)?;
        Ok(commands)
    }

    /// The full transform a drawing node gets from its pegs on `frame`.
    pub fn world_transform(&self, drawing: NodeId, frame: u32) -> Result<Affine, EvalError> {
        self.world_pose(drawing, frame).map(|pose| pose.transform)
    }

    pub fn world_pose(&self, node: NodeId, frame: u32) -> Result<StagePose, EvalError> {
        match self.input(node, 0) {
            Some(p) => self.eval_transform(p, frame, 0),
            None => Ok(StagePose::default()),
        }
    }

    fn get(&self, id: NodeId, depth: usize) -> Result<&Node, EvalError> {
        if depth > MAX_DEPTH {
            return Err(EvalError::Cycle(id));
        }
        self.node(id).ok_or(EvalError::UnknownNode(id))
    }

    fn eval_image(
        &self,
        id: NodeId,
        xs: &Xsheet,
        frame: u32,
        depth: usize,
        matte_pass: bool,
        out: &mut Vec<RenderCommand>,
    ) -> Result<(), EvalError> {
        let node = self.get(id, depth)?;
        let key = self.sample_key(id, frame);
        let enabled = key.map_or(node.enabled, |key| key.enabled);
        if !enabled && !matches!(node.kind, NodeKind::Effect(_)) {
            return Ok(());
        }
        match &node.kind {
            NodeKind::Display | NodeKind::Write => {
                if let Some(src) = self.input(id, 0) {
                    self.eval_image(src, xs, frame, depth + 1, matte_pass, out)?;
                }
            }
            NodeKind::Composite => {
                // Highest port is furthest back, so it's emitted first.
                for (_, src) in self.inputs(id).into_iter().rev() {
                    self.eval_image(src, xs, frame, depth + 1, matte_pass, out)?;
                }
            }
            NodeKind::Gradient(gradient) => out.push(RenderCommand::Gradient(
                key.and_then(|key| key.gradient).unwrap_or(*gradient),
            )),
            NodeKind::Effect(Effect::Mask { inverted }) => {
                if let Some(image) = self.input(id, 0) {
                    if let Some(matte) = self.input(id, 1).filter(|_| enabled) {
                        let inverted = match key.and_then(|key| key.effect) {
                            Some(Effect::Mask { inverted }) => inverted,
                            _ => *inverted,
                        };
                        out.push(RenderCommand::BeginEffect);
                        self.eval_image(image, xs, frame, depth + 1, matte_pass, out)?;
                        out.push(RenderCommand::BeginEffect);
                        self.eval_image(matte, xs, frame, depth + 1, true, out)?;
                        out.push(RenderCommand::EndMask { inverted });
                    } else {
                        self.eval_image(image, xs, frame, depth + 1, matte_pass, out)?;
                    }
                }
            }
            NodeKind::Effect(effect) => {
                if let Some(src) = self.input(id, 0) {
                    if enabled {
                        out.push(RenderCommand::BeginEffect);
                    }
                    self.eval_image(src, xs, frame, depth + 1, matte_pass, out)?;
                    if enabled {
                        out.push(RenderCommand::EndEffect(
                            key.and_then(|key| key.effect).unwrap_or(*effect),
                        ));
                    }
                }
            }
            NodeKind::Drawing { column } => {
                if !matte_pass && self.is_drawing_matte(id) {
                    return Ok(());
                }
                let col = xs.column(*column);
                if let Some(drawing) = col.filter(|c| c.visible).and_then(|c| c.get(frame)) {
                    let pose = match self.input(id, 0) {
                        Some(p) => self.eval_transform(p, frame, depth + 1)?,
                        None => StagePose::default(),
                    };
                    let warp = match self.input(id, 0) {
                        Some(p) => self.warp_chain(p, frame, depth + 1)?,
                        None => None,
                    };
                    out.push(RenderCommand::Drawing(RenderItem {
                        node: id,
                        column: *column,
                        drawing,
                        transform: pose.transform,
                        image_transform: pose.artwork_plane(),
                        depth: pose.depth,
                        matte_pass,
                        warp,
                    }));
                }
            }
            NodeKind::Peg(_)
            | NodeKind::MeshWarp(_)
            | NodeKind::Skeleton(_)
            | NodeKind::Bone(_) => {}
        }
        Ok(())
    }

    /// The Mesh Warps between a drawing and the stage, from the position
    /// node `id` above it upward; None when none of them bends anything.
    fn warp_chain(
        &self,
        id: NodeId,
        frame: u32,
        depth: usize,
    ) -> Result<Option<std::sync::Arc<crate::warp::Warp>>, EvalError> {
        // The chain from the nearest node up to the root.
        let mut chain = Vec::new();
        let mut at = Some(id);
        while let Some(n) = at {
            if chain.len() + depth > MAX_DEPTH {
                return Err(EvalError::Cycle(n));
            }
            chain.push(n);
            at = self.input(n, 0);
        }
        let bound_bone = chain
            .iter()
            .copied()
            .find(|&id| self.bone(id).is_some() && self.enabled_at(id, frame));
        let mut steps = Vec::new();
        let mut outer = Affine::IDENTITY;
        let mut lines = crate::warp::WarpLines::Paths;
        for &n in chain.iter().rev() {
            let node = self.get(n, depth)?;
            let enabled = self
                .sample_key(n, frame)
                .map_or(node.enabled, |key| key.enabled);
            if !enabled {
                continue;
            }
            match &node.kind {
                NodeKind::Peg(p) | NodeKind::Skeleton(p) => {
                    outer = outer.compose(p.transform_at(frame))
                }
                NodeKind::Bone(bone) if bound_bone == Some(n) => {
                    let pose = self.bone_pose_inner(n, frame, depth)?;
                    let radius = self
                        .parent_bone(n)
                        .map_or(0.0, |_| bone.bend_radius.clamp(0.0, pose.length));
                    let field = BoneField {
                        rest_inverse: pose.rest.inverse().unwrap_or(Affine::IDENTITY),
                        parent: pose.parent_skin,
                        child: pose.skin(),
                        radius,
                    };
                    steps.push(crate::warp::WarpStep {
                        outer,
                        field: crate::warp::WarpField::Bone(Box::new(field)),
                    });
                    outer = Affine::IDENTITY;
                }
                NodeKind::MeshWarp(w) => {
                    if let Some(field) = w.field_at(frame) {
                        steps.push(crate::warp::WarpStep { outer, field });
                        if w.lines == crate::warp::WarpLines::Shape {
                            lines = w.lines;
                        }
                        outer = Affine::IDENTITY;
                    }
                }
                _ => {}
            }
        }
        let warp = crate::warp::Warp {
            steps,
            inner: outer,
            lines,
        };
        Ok((!warp.steps.is_empty() && !warp.is_flat()).then(|| std::sync::Arc::new(warp)))
    }

    /// The nearest bone above a drawing and the intervening peg transform.
    fn drawing_bone_binding(&self, drawing: NodeId, frame: u32) -> Option<(NodeId, Affine)> {
        let mut id = drawing;
        let mut inner = Affine::IDENTITY;
        for _ in 0..MAX_DEPTH {
            id = self.input(id, 0)?;
            match &self.node(id)?.kind {
                NodeKind::Bone(_) => return Some((id, inner)),
                NodeKind::Peg(peg) => {
                    if self.enabled_at(id, frame) {
                        inner = peg.transform_at(frame).compose(inner);
                    }
                }
                NodeKind::MeshWarp(_) => {}
                _ => return None,
            }
        }
        None
    }

    pub fn drawing_bone(&self, drawing: NodeId, frame: u32) -> Option<NodeId> {
        self.drawing_bone_binding(drawing, frame).map(|(id, _)| id)
    }

    /// The attached bone's rest endpoints in the drawing's local coordinates.
    pub fn drawing_bone_guide(&self, drawing: NodeId, frame: u32) -> Option<[(f32, f32); 2]> {
        let (id, inner) = self.drawing_bone_binding(drawing, frame)?;
        let pose = self.bone_pose(id, frame)?;
        let guide = inner.inverse()?.compose(pose.rest);
        Some([guide.apply(0.0, 0.0), guide.apply(pose.length, 0.0)])
    }

    pub fn bone(&self, id: NodeId) -> Option<&Bone> {
        match &self.node(id)?.kind {
            NodeKind::Bone(bone) => Some(bone),
            _ => None,
        }
    }

    pub fn parent_bone(&self, id: NodeId) -> Option<NodeId> {
        let parent = self.input(id, 0)?;
        self.bone(parent).map(|_| parent)
    }

    fn enabled_at(&self, id: NodeId, frame: u32) -> bool {
        self.node(id).is_some_and(|node| {
            self.sample_key(id, frame)
                .map_or(node.enabled, |key| key.enabled)
        })
    }

    fn smart_child(&self, upper: NodeId) -> Option<NodeId> {
        self.links
            .iter()
            .filter(|link| link.from == upper && link.port == 0)
            .map(|link| link.to)
            .filter(|&id| self.bone(id).is_some_and(|b| b.smart.is_some()))
            .min()
    }

    /// The lower bone storing the active IK target for either member of the pair.
    pub fn smart_controller(&self, id: NodeId, frame: u32) -> Option<NodeId> {
        if !self.enabled_at(id, frame) {
            return None;
        }
        let controller = if self.parent_bone(id).is_some_and(|parent| {
            self.enabled_at(parent, frame) && self.smart_child(parent) == Some(id)
        }) {
            Some(id)
        } else {
            self.smart_child(id)
                .filter(|&child| self.enabled_at(child, frame))
        };
        controller.filter(|_| !self.rig_has_anchors(self.rig_root(id), frame))
    }

    /// A smart joint owns two bones, so another solver cannot drive either one.
    pub fn can_make_smart(&self, lower: NodeId) -> bool {
        let Some(upper) = self.parent_bone(lower) else {
            return false;
        };
        self.bone(lower).is_some_and(|b| b.smart.is_none())
            && self.bone(upper).is_some_and(|b| b.smart.is_none())
            && self.smart_child(upper).is_none()
            && self.smart_child(lower).is_none()
    }

    /// Position of the bone relative to its skeleton, before Camera projection.
    pub fn bone_pose(&self, id: NodeId, frame: u32) -> Option<BonePose> {
        self.bone(id)?;
        self.bone_pose_inner(id, frame, 0).ok()
    }

    fn bone_pose_inner(&self, id: NodeId, frame: u32, depth: usize) -> Result<BonePose, EvalError> {
        self.get(id, depth)?;
        let bone = self.bone(id).ok_or(EvalError::UnknownNode(id))?;
        let parent = self.parent_bone(id);
        let (rest_base, posed_base, parent_skin) = if let Some(parent) = parent {
            let parent = self.bone_pose_inner(parent, frame, depth + 1)?;
            // Children start at the stretched tip, without being stretched.
            (
                parent.rest.compose(Affine::translate(parent.length, 0.0)),
                parent.posed.compose(Affine::translate(parent.reach(), 0.0)),
                parent.skin(),
            )
        } else {
            let origin = Affine::translate(bone.origin[0], bone.origin[1]);
            (origin, origin, Affine::IDENTITY)
        };
        let rest_turn = Affine::rotate_deg(bone.rest_angle);
        let mut turn = bone.rest_angle;
        if self.enabled_at(id, frame) {
            turn += bone.rotation.at(frame);
            if let Some(lower) = self.smart_controller(id, frame) {
                if let Some((upper, lower_bone)) = self
                    .parent_bone(lower)
                    .and_then(|upper| self.bone(upper))
                    .zip(self.bone(lower))
                {
                    if let Some(joint) = &lower_bone.smart {
                        let [aim, bend] = joint.solve(frame, upper.length(), lower_bone.length());
                        turn = if lower == id {
                            bend
                        } else {
                            bone.rest_angle + aim
                        };
                    }
                }
            }
        }
        let stretch = if self.enabled_at(id, frame) {
            bone.stretch_at(frame)
        } else {
            1.0
        };
        Ok(BonePose {
            rest: rest_base.compose(rest_turn),
            posed: posed_base.compose(Affine::rotate_deg(turn)),
            parent_skin,
            length: bone.length(),
            scale: bone.stretch_scale(stretch),
        })
    }

    /// Every bone in the rig `root` (see [`Graph::rig_root`]), parents
    /// before their children.
    pub fn rig_bones(&self, root: NodeId) -> Vec<NodeId> {
        let mut bones: Vec<(usize, NodeId)> = self
            .nodes()
            .filter(|(id, _)| self.bone(*id).is_some() && self.rig_root(*id) == root)
            .map(|(id, _)| {
                let mut depth = 0;
                let mut at = id;
                while let Some(p) = self.parent_bone(at) {
                    depth += 1;
                    at = p;
                    if depth > MAX_DEPTH {
                        break;
                    }
                }
                (depth, id)
            })
            .collect();
        bones.sort();
        bones.into_iter().map(|(_, id)| id).collect()
    }

    /// Pose a tip, or move its anchor target, while preserving every locked
    /// tip in the rig. Coordinates are relative to the skeleton peg.
    pub fn solve_rig(
        &self,
        dragged: NodeId,
        target: [f32; 2],
        frame: u32,
    ) -> Option<crate::rig::RigSolution> {
        let root = self.rig_root(dragged);
        self.solve_anchored_rig(root, Some((dragged, target)), [0.0; 2], frame, None)
    }

    /// Translate the skeleton by a local delta while its anchors remain
    /// in their original stage positions.
    pub fn solve_rig_translation(
        &self,
        root: NodeId,
        delta: [f32; 2],
        frame: u32,
    ) -> Option<crate::rig::RigSolution> {
        self.solve_anchored_rig(root, None, delta, frame, None)
    }

    pub fn flip_anchored_bend(&self, bone: NodeId, frame: u32) -> Option<crate::rig::RigSolution> {
        let lower = if self.parent_bone(bone).is_some() {
            bone
        } else {
            self.links
                .iter()
                .find(|link| link.from == bone && self.bone(link.to).is_some())?
                .to
        };
        self.solve_anchored_rig(self.rig_root(lower), None, [0.0; 2], frame, Some(lower))
    }

    pub fn rig_has_anchors(&self, root: NodeId, frame: u32) -> bool {
        self.rig_bones(root).into_iter().any(|id| {
            self.enabled_at(id, frame) && self.bone(id).is_some_and(|bone| bone.anchor.is_some())
        })
    }

    fn solve_anchored_rig(
        &self,
        root: NodeId,
        drag: Option<(NodeId, [f32; 2])>,
        translation: [f32; 2],
        frame: u32,
        flip: Option<NodeId>,
    ) -> Option<crate::rig::RigSolution> {
        use crate::rig::{AnchorKind, RigEdge, RigSolution};
        let bones = self.rig_bones(root);
        let mut joint_index = BTreeMap::new();
        let mut points = Vec::new();
        let mut pins = Vec::new();
        let mut aims = Vec::new();
        let mut edges = Vec::new();
        let mut bases = Vec::new();
        for &id in &bones {
            let bone = self.bone(id)?;
            let pose = self.bone_pose(id, frame)?;
            let start = if let Some(parent) = self.parent_bone(id) {
                *joint_index.get(&parent)?
            } else {
                let index = points.len();
                points.push([pose.posed.tx, pose.posed.ty]);
                bases.push(index);
                index
            };
            let end = points.len();
            let (x, y) = pose.tip();
            points.push([x, y]);
            joint_index.insert(id, end);
            let (min, max) = bone.stretch_range();
            let length = bone.length();
            edges.push(RigEdge {
                start,
                end,
                length: pose.reach(),
                range: [length * min, length * max],
                spring_stiffness: bone.spring_stiffness,
                rest_angle: bone.rest_angle,
            });
            if let Some(anchor) = bone.anchor.as_ref().filter(|_| self.enabled_at(id, frame)) {
                let goal = [anchor.target[0].at(frame), anchor.target[1].at(frame)];
                match anchor.kind {
                    AnchorKind::Locked => pins.push((end, goal)),
                    AnchorKind::Aim => aims.push((end, goal)),
                }
            }
        }
        if pins.is_empty() && aims.is_empty() {
            return None;
        }
        let reflected = flip.and_then(|id| {
            let upper = self.parent_bone(id)?;
            let joint = joint_index[&upper];
            if pins.iter().any(|&(i, _)| i == joint) {
                return None;
            }
            let tip = joint_index[&id];
            let edge = edges.iter().find(|edge| edge.end == joint)?;
            let [x, y] = points[edge.start];
            let axis = [points[tip][0] - x, points[tip][1] - y];
            let len_squared = axis[0] * axis[0] + axis[1] * axis[1];
            if len_squared < 1e-6 {
                return None;
            }
            let d = [points[joint][0] - x, points[joint][1] - y];
            let along = (d[0] * axis[0] + d[1] * axis[1]) / len_squared;
            let next = [
                x + 2.0 * along * axis[0] - d[0],
                y + 2.0 * along * axis[1] - d[1],
            ];
            Some((joint, tip, next))
        });
        if flip.is_some() && reflected.is_none() {
            return Some(RigSolution::default());
        }
        let drive = drag.and_then(|(id, mut goal)| {
            let index = *joint_index.get(&id)?;
            let bone = self.bone(id)?;
            if bone.spring_stiffness > 0.0 && bone.anchor.is_none() {
                let pose = self.bone_pose(id, frame)?;
                let (x, y) = pose.posed.inverse()?.apply(goal[0], goal[1]);
                let wanted = bone.rotation.at(frame) + y.atan2(x).to_degrees();
                let turn = (bone.spring_rotation(wanted) - bone.rotation.at(frame)).to_radians();
                let reach = x.hypot(y);
                let (x, y) = pose.posed.apply(reach * turn.cos(), reach * turn.sin());
                goal = [x, y];
            }
            Some((index, goal))
        });
        let attempt = |fraction: f32| {
            let shift = translation.map(|v| v * fraction);
            let local = |p: [f32; 2]| [p[0] - shift[0], p[1] - shift[1]];
            let mut hard: Vec<_> = bases.iter().map(|&i| (i, points[i])).collect();
            let mut soft: Vec<_> = aims.iter().map(|&(i, goal)| (i, local(goal))).collect();
            let mut seed = points.clone();
            if let Some((joint, tip, next)) = reflected {
                seed[joint] = next;
                if !pins.iter().any(|&(i, _)| i == tip) {
                    hard.push((tip, local(points[tip])));
                }
            }
            let mut accepted_target = None;
            for &(i, goal) in &pins {
                let target = if let Some((_, desired)) = drive.filter(|(index, _)| *index == i) {
                    let target = [
                        goal[0] + (desired[0] - goal[0]) * fraction,
                        goal[1] + (desired[1] - goal[1]) * fraction,
                    ];
                    accepted_target = Some(target);
                    target
                } else {
                    local(goal)
                };
                hard.push((i, target));
            }
            if let Some((i, goal)) = drive {
                if aims.iter().any(|&(j, _)| j == i) {
                    soft.retain(|&(j, _)| j != i);
                    soft.push((i, goal));
                    accepted_target = Some(goal);
                } else if !pins.iter().any(|&(j, _)| j == i) {
                    let from = points[i];
                    hard.push((
                        i,
                        [
                            from[0] + (goal[0] - from[0]) * fraction,
                            from[1] + (goal[1] - from[1]) * fraction,
                        ],
                    ));
                }
            }
            let solved = crate::rig::solve_tree(&seed, &edges, &hard, &soft)?;
            Some((solved, shift, accepted_target))
        };
        let result = attempt(1.0).or_else(|| {
            let mut result = attempt(0.0);
            let (mut lo, mut hi) = (0.0, 1.0);
            for _ in 0..10 {
                let middle = (lo + hi) * 0.5;
                if let Some(solution) = attempt(middle) {
                    result = Some(solution);
                    lo = middle;
                } else {
                    hi = middle;
                }
            }
            result
        });
        let Some((solved, translation, target)) = result else {
            return Some(RigSolution::default());
        };
        let mut angles = BTreeMap::new();
        let mut changes = Vec::new();
        for (&id, edge) in bones.iter().zip(&edges) {
            let bone = self.bone(id)?;
            let [x, y] = solved[edge.start];
            let [tx, ty] = solved[edge.end];
            let angle = (ty - y).atan2(tx - x).to_degrees();
            let parent_angle = self
                .parent_bone(id)
                .and_then(|p| angles.get(&p))
                .copied()
                .unwrap_or(0.0);
            angles.insert(id, angle);
            let wanted = angle - parent_angle - bone.rest_angle;
            let now = bone.rotation.at(frame);
            let rotation = now + (wanted - now + 180.0).rem_euclid(360.0) - 180.0;
            let stretch = (tx - x).hypot(ty - y) / bone.length();
            if (rotation - now).abs() > 1e-3 || (stretch - bone.stretch_at(frame)).abs() > 1e-4 {
                changes.push((id, rotation, stretch));
            }
        }
        Some(RigSolution {
            changes,
            feasible: true,
            translation,
            target,
        })
    }

    /// The skeleton that owns this bone, or the disconnected chain's root bone.
    pub fn rig_root(&self, mut id: NodeId) -> NodeId {
        for _ in 0..MAX_DEPTH {
            if self.bone(id).is_none() {
                break;
            }
            let Some(parent) = self.input(id, 0) else {
                break;
            };
            id = parent;
        }
        id
    }

    /// Scene-space peg above the bone chain.
    pub fn rig_stage_pose(&self, mut id: NodeId, frame: u32) -> Option<StagePose> {
        for _ in 0..MAX_DEPTH {
            match self.parent_bone(id) {
                Some(parent) => id = parent,
                None => return self.world_pose(id, frame).ok(),
            }
        }
        None
    }

    /// The IK target axes relative to the skeleton peg.
    pub fn smart_target_basis(&self, lower: NodeId, frame: u32) -> Option<Affine> {
        let upper = self.parent_bone(lower)?;
        let bone = self.bone(upper)?;
        let base = if let Some(parent) = self.parent_bone(upper) {
            let pose = self.bone_pose(parent, frame)?;
            pose.posed.compose(Affine::translate(pose.reach(), 0.0))
        } else {
            Affine::translate(bone.origin[0], bone.origin[1])
        };
        Some(base.compose(Affine::rotate_deg(bone.rest_angle)))
    }

    fn eval_transform(&self, id: NodeId, frame: u32, depth: usize) -> Result<StagePose, EvalError> {
        let node = self.get(id, depth)?;
        let parent = match self.input(id, 0) {
            Some(p) => self.eval_transform(p, frame, depth + 1)?,
            None => StagePose::default(),
        };
        Ok(match &node.kind {
            NodeKind::Peg(p) | NodeKind::Skeleton(p)
                if self
                    .sample_key(id, frame)
                    .map_or(node.enabled, |key| key.enabled) =>
            {
                let local = p.stage_transform(frame);
                let mut pose = parent.compose(local);
                if parent.front.is_some() || p.tilt_enabled {
                    let front = parent
                        .front
                        .unwrap_or(parent.transform)
                        .compose(p.flat_transform_at(frame));
                    pose.front = Some(front);
                    if p.tilt_enabled && p.face_camera {
                        let (x, y) = p.pivot;
                        let [px, py, _] = pose.point_at(x, y);
                        let (fx, fy) = front.apply(x, y);
                        pose.transform = Affine::translate(px - fx, py - fy).compose(front);
                        pose.camera_facing = true;
                    } else if parent.camera_facing {
                        pose.transform = parent.transform.compose(local.transform);
                        pose.camera_facing = true;
                    }
                }
                pose
            }
            _ => parent,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::xsheet::Column;

    #[test]
    fn face_camera_preserves_ink_orientation_without_flattening_the_stage_plane() {
        let mut graph = Graph::default();
        let id = graph.add(
            "Tilt",
            NodeKind::Peg(PegParams {
                tilt_enabled: true,
                face_camera: true,
                tilt: Some(Box::new([Value::Constant(0.0), Value::Constant(90.0)])),
                pivot: (10.0, 0.0),
                ..Default::default()
            }),
        );
        let child = graph.add("Child peg", NodeKind::Peg(PegParams::default()));
        let drawing = graph.add("Drawing", NodeKind::Drawing { column: Id(1) });
        graph.connect(id, child, 0);
        graph.connect(child, drawing, 0);
        let pose = graph.world_pose(drawing, 1).unwrap();
        assert_eq!(pose.transform.apply(20.0, 0.0), (20.0, 0.0));
        let [x, _, z] = pose.point_at(20.0, 0.0);
        assert!((x - 10.0).abs() < 1e-4 && (z + 10.0).abs() < 1e-4);
        assert!(pose.artwork_plane().unwrap().a.abs() < 1e-4);
    }

    #[test]
    fn tilted_peg_has_spatial_depth_and_retains_its_pivot() {
        let mut graph = Graph::default();
        let mut peg = PegParams {
            x: Value::Constant(15.0),
            z: Value::Constant(30.0),
            pivot: (2.0, 0.0),
            tilt_enabled: true,
            tilt: Some(Box::new([Value::Constant(0.0), Value::Constant(60.0)])),
            ..Default::default()
        };
        let mut angle = peg.tilt.as_ref().unwrap()[1].clone();
        angle.set_key(1, 60.0);
        angle.set_key(12, 0.0);
        peg.tilt.as_mut().unwrap()[1] = angle;
        let id = graph.add("Tilt", NodeKind::Peg(peg));
        let drawing = graph.add("Drawing", NodeKind::Drawing { column: Id(1) });
        graph.connect(id, drawing, 0);
        assert!(graph.has_key(id, 1));
        let pose = graph.world_pose(drawing, 1).unwrap();
        let pivot = pose.point_at(2.0, 0.0);
        let end = pose.point_at(12.0, 0.0);
        assert!((pivot[0] - 17.0).abs() < 1e-4 && (pivot[2] - 30.0).abs() < 1e-4);
        assert!((end[0] - 22.0).abs() < 1e-4 && end[2] < 22.0);
        assert_eq!(
            graph.world_pose(drawing, 12).unwrap().point_at(12.0, 0.0),
            [27.0, 0.0, 30.0]
        );
    }

    #[test]
    fn blur_reads_legacy_box_settings_and_holds_modes_at_keys() {
        let effect = serde_json::from_str::<Effect>(r#"{"Blur":{"radius":12.0}}"#).unwrap();
        assert_eq!(
            effect,
            Effect::Blur {
                radius: 12.0,
                mode: BlurMode::Box
            }
        );
        let key = |radius, mode| NodeKey {
            enabled: true,
            effect: Some(Effect::Blur { radius, mode }),
            gradient: None,
        };
        let start = key(12.0, BlurMode::Box);
        assert_eq!(start.between(key(24.0, BlurMode::Gaussian), 0.5), start);
        assert_eq!(
            start.between(key(24.0, BlurMode::Box), 0.5),
            key(18.0, BlurMode::Box),
        );
    }

    /// Body (back) under a peg, Head (front); a Composite into a Display.
    fn scene() -> (Graph, Xsheet, NodeId, NodeId) {
        let (head_c, body_c) = (Id(1), Id(2));
        let mut xs = Xsheet::default();
        let mut head = Column::new(head_c, "Head");
        head.expose(Id(10), 1, 4);
        let mut body = Column::new(body_c, "Body");
        body.expose(Id(20), 1, 2);
        xs.columns = vec![head, body];

        let mut g = Graph::default();
        let mut peg = PegParams::default();
        peg.x.set_key(1, 0.0);
        peg.x.set_key(3, 20.0);
        let body_peg = g.add("Body-P", NodeKind::Peg(peg));
        let head_n = g.add("Head", NodeKind::Drawing { column: head_c });
        let body_n = g.add("Body", NodeKind::Drawing { column: body_c });
        let comp = g.add("Composite", NodeKind::Composite);
        let display = g.add("Display", NodeKind::Display);
        g.connect(body_peg, body_n, 0);
        g.connect(head_n, comp, 0);
        g.connect(body_n, comp, 1);
        g.connect(comp, display, 0);
        (g, xs, display, body_peg)
    }

    #[test]
    fn back_to_front_with_peg() {
        let (g, xs, display, _) = scene();
        let items = g.evaluate(display, &xs, 2).unwrap();
        let cols: Vec<_> = items.iter().map(|i| i.column).collect();
        assert_eq!(cols, [Id(2), Id(1)]);
        assert_eq!(items[0].transform.tx, 10.0);
        assert_eq!(items[1].transform, Affine::IDENTITY);
    }

    #[test]
    fn empty_cell_draws_nothing() {
        let (g, xs, display, _) = scene();
        let items = g.evaluate(display, &xs, 3).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].column, Id(1));
    }

    #[test]
    fn hidden_column_draws_nothing() {
        let (g, mut xs, display, _) = scene();
        xs.columns[0].visible = false;
        assert_eq!(g.evaluate(display, &xs, 1).unwrap().len(), 1);
    }

    #[test]
    fn cycle_is_an_error() {
        let (mut g, xs, display, body_peg) = scene();
        g.connect(body_peg, body_peg, 0);
        assert!(matches!(
            g.evaluate(display, &xs, 1),
            Err(EvalError::Cycle(_))
        ));
    }

    #[test]
    fn drawing_bone_guide_uses_rest_pose_and_accounts_for_drawing_peg() {
        let (mut graph, _, _, peg) = scene();
        let drawing = graph
            .links()
            .iter()
            .find(|link| link.from == peg)
            .unwrap()
            .to;
        assert_eq!(graph.drawing_bone_guide(drawing, 2), None);
        let bone = graph.add(
            "Bone",
            NodeKind::Bone(Bone {
                origin: [30.0, 40.0],
                length: 80.0,
                rotation: Value::Constant(90.0),
                ..Bone::default()
            }),
        );
        graph.connect(bone, peg, 0);
        assert_eq!(
            graph.drawing_bone_guide(drawing, 2),
            Some([(20.0, 40.0), (100.0, 40.0)])
        );
    }

    #[test]
    fn a_bone_deforms_multiple_drawings_and_emits_no_artwork_of_its_own() {
        let (mut graph, xs, display, _) = scene();
        let drawings: Vec<_> = graph
            .nodes()
            .filter_map(|(id, node)| matches!(node.kind, NodeKind::Drawing { .. }).then_some(id))
            .collect();
        let skeleton = graph.add(
            "Skeleton",
            NodeKind::Skeleton(PegParams {
                x: Value::Constant(30.0),
                ..PegParams::default()
            }),
        );
        let upper = graph.add(
            "Upper",
            NodeKind::Bone(Bone {
                length: 100.0,
                ..Bone::default()
            }),
        );
        let lower = graph.add(
            "Lower",
            NodeKind::Bone(Bone {
                rotation: Value::Constant(90.0),
                ..Bone::default()
            }),
        );
        assert!(graph.can_connect(skeleton, upper));
        assert!(graph.can_connect(upper, lower));
        assert!(!graph.can_connect(lower, display));
        graph.connect(skeleton, upper, 0);
        graph.connect(upper, lower, 0);
        for id in drawings {
            assert!(graph.can_connect(lower, id));
            graph.connect(lower, id, 0);
        }

        let commands = graph.render_commands(display, &xs, 1).unwrap();
        assert_eq!(commands.len(), 2);
        for command in commands {
            let RenderCommand::Drawing(item) = command else {
                panic!("expected drawing")
            };
            let warp = item.warp.unwrap();
            assert_eq!(warp.bend([50.0, 0.0]), [50.0, 0.0]);
            let near_joint = warp.bend([114.0, 0.0]);
            assert!(near_joint[0] > 100.0 && near_joint[0] < 114.0);
            assert!(near_joint[1] > 0.0 && near_joint[1] < 14.0);
            let tip = warp.bend([160.0, 0.0]);
            assert!((tip[0] - 100.0).abs() < 0.001 && (tip[1] - 60.0).abs() < 0.001);
            assert_eq!(warp.outer().tx, 30.0);
            let raster = warp.raster_transform().unwrap().apply(160.0, 0.0);
            assert!((raster.0 - 130.0).abs() < 0.001 && (raster.1 - 60.0).abs() < 0.001);
        }
        assert!(graph.render_commands(lower, &xs, 1).unwrap().is_empty());
    }

    #[test]
    fn smart_joint_reaches_an_animated_target_and_holds_bone_lengths() {
        use crate::rig::SmartJoint;
        let mut graph = Graph::default();
        let upper = graph.add(
            "Upper",
            NodeKind::Bone(Bone {
                length: 100.0,
                ..Bone::default()
            }),
        );
        let lower = graph.add(
            "Lower",
            NodeKind::Bone(Bone {
                length: 80.0,
                ..Bone::default()
            }),
        );
        graph.connect(upper, lower, 0);
        assert!(graph.can_make_smart(lower));
        let mut x = Value::Constant(100.0);
        x.set_key(1, 100.0);
        x.set_key(3, 140.0);
        let NodeKind::Bone(bone) = &mut graph.node_mut(lower).unwrap().kind else {
            unreachable!()
        };
        bone.smart = Some(SmartJoint {
            target: [x, Value::Constant(60.0)],
            flip: false,
        });
        let branch = graph.add("Branch", NodeKind::Bone(Bone::default()));
        graph.connect(upper, branch, 0);
        assert!(!graph.can_make_smart(branch));
        let below = graph.add("Below", NodeKind::Bone(Bone::default()));
        graph.connect(lower, below, 0);
        assert!(!graph.can_make_smart(below));
        assert!(graph.has_key(lower, 3));

        let pose = graph.bone_pose(lower, 2).unwrap();
        let (x, y) = pose.tip();
        assert!((x - 120.0).abs() < 0.001 && (y - 60.0).abs() < 0.001);
        assert!((pose.posed.tx.hypot(pose.posed.ty) - 100.0).abs() < 0.001);
        assert!(((x - pose.posed.tx).hypot(y - pose.posed.ty) - 80.0).abs() < 0.001);
        let saved = serde_json::to_vec(&graph).unwrap();
        let loaded: Graph = serde_json::from_slice(&saved).unwrap();
        assert_eq!(loaded.bone_pose(lower, 2), Some(pose));
    }

    #[test]
    fn remove_and_restore_keeps_ids() {
        let (mut g, xs, display, body_peg) = scene();
        let (node, links) = g.remove(body_peg).unwrap();
        assert!(g.node(body_peg).is_none());
        g.restore(body_peg, node, &links);
        assert_eq!(g.evaluate(display, &xs, 2).unwrap()[0].transform.tx, 10.0);
    }
    /// A leg: hip -> knee -> ankle -> toe, three bones of 100.
    fn leg() -> (Graph, [NodeId; 3]) {
        let mut graph = Graph::default();
        let skeleton = graph.add("Skeleton", NodeKind::Skeleton(PegParams::default()));
        let mut bones = [NodeId(0); 3];
        let mut up = skeleton;
        for (i, b) in bones.iter_mut().enumerate() {
            *b = graph.add(
                format!("Bone {i}"),
                NodeKind::Bone(Bone {
                    length: 100.0,
                    ..Bone::default()
                }),
            );
            graph.connect(up, *b, 0);
            up = *b;
        }
        (graph, bones)
    }

    fn apply(graph: &mut Graph, changes: &[(NodeId, f32, f32)]) {
        for &(id, rotation, stretch) in changes {
            if let Some(NodeKind::Bone(b)) = graph.node_mut(id).map(|n| &mut n.kind) {
                b.rotation = Value::Constant(rotation);
                b.stretch = Value::Constant(stretch);
            }
        }
    }

    fn near(a: (f32, f32), b: [f32; 2], within: f32) -> bool {
        (a.0 - b[0]).hypot(a.1 - b[1]) <= within
    }

    #[test]
    fn spring_resists_rotation_with_an_anchored_rig() {
        let (mut graph, [hip, _, foot]) = leg();
        let NodeKind::Bone(bone) = &mut graph.node_mut(foot).unwrap().kind else {
            unreachable!()
        };
        bone.anchor = Some(Box::new(crate::rig::Anchor {
            kind: crate::rig::AnchorKind::Locked,
            target: [Value::Constant(230.0), Value::Constant(0.0)],
        }));
        let drag = [80.0, 60.0];
        let loose = graph.solve_rig(hip, drag, 1).unwrap();
        let NodeKind::Bone(bone) = &mut graph.node_mut(hip).unwrap().kind else {
            unreachable!()
        };
        bone.spring_stiffness = 1.0;
        let stiff = graph.solve_rig(hip, drag, 1).unwrap();
        apply(&mut graph, &stiff.changes);
        let stiff_tip = graph.bone_pose(hip, 1).unwrap().tip();
        assert!(stiff_tip.1.abs() < 15.0, "{stiff_tip:?}");
        assert!(near(
            graph.bone_pose(foot, 1).unwrap().tip(),
            [230.0, 0.0],
            0.04
        ));
        apply(&mut graph, &loose.changes);
        assert!(near(graph.bone_pose(hip, 1).unwrap().tip(), drag, 0.04));
    }

    #[test]
    fn smart_rigs_solve_around_locked_anchors() {
        let (mut graph, [hip, knee, foot]) = leg();
        // No anchors: posing stays bone by bone.
        assert!(graph.solve_rig(knee, [150.0, 80.0], 1).is_none());
        // The foot's tip locked a little in from where it is.
        if let Some(NodeKind::Bone(b)) = graph.node_mut(foot).map(|n| &mut n.kind) {
            b.anchor = Some(Box::new(crate::rig::Anchor {
                kind: crate::rig::AnchorKind::Locked,
                target: [Value::Constant(280.0), Value::Constant(0.0)],
            }));
        }
        // Drag the knee in (the leg bends): it gets there, the foot stays
        // planted (the foot bone is 100 long, as far as the anchor is from
        // the knee), the hip stays.
        let changes = graph.solve_rig(knee, [180.0, 0.0], 1).unwrap();
        apply(&mut graph, &changes.changes);
        let pose = |id| graph.bone_pose(id, 1).unwrap();
        assert!(
            near(pose(knee).tip(), [180.0, 0.0], 0.5),
            "{:?}",
            pose(knee).tip()
        );
        assert!(
            near(pose(foot).tip(), [280.0, 0.0], 0.5),
            "{:?}",
            pose(foot).tip()
        );
        assert!(near(pose(hip).posed.apply(0.0, 0.0), [0.0, 0.0], 1e-3));
        // Bones keep their length (rigid).
        let len = |id| {
            let p = pose(id);
            let (a, b) = (p.posed.apply(0.0, 0.0), p.tip());
            (b.0 - a.0).hypot(b.1 - a.1)
        };
        assert!((len(hip) - 100.0).abs() < 0.5 && (len(knee) - 100.0).abs() < 0.5);
    }

    #[test]
    fn translating_body_stops_at_reach_without_moving_the_pin() {
        let (mut graph, [hip, _, foot]) = leg();
        let node = graph.node_mut(foot).unwrap();
        let NodeKind::Bone(bone) = &mut node.kind else {
            unreachable!()
        };
        bone.anchor = Some(Box::new(crate::rig::Anchor {
            kind: crate::rig::AnchorKind::Locked,
            target: [Value::Constant(300.0), Value::Constant(0.0)],
        }));
        let root = graph.rig_root(hip);
        let stopped = graph.solve_rig_translation(root, [-80.0, 0.0], 1).unwrap();
        assert!(stopped.translation[0].abs() < 0.02);
        let allowed = graph.solve_rig_translation(root, [70.0, 0.0], 1).unwrap();
        assert_eq!(allowed.translation, [70.0, 0.0]);
        apply(&mut graph, &allowed.changes);
        let (x, y) = graph.bone_pose(foot, 1).unwrap().tip();
        assert!(near((x + allowed.translation[0], y), [300.0, 0.0], 0.04));
    }

    #[test]
    fn flipping_an_anchored_bend_keeps_its_tip_planted() {
        let (mut graph, [upper, lower, _]) = leg();
        let NodeKind::Bone(upper_bone) = &mut graph.node_mut(upper).unwrap().kind else {
            unreachable!()
        };
        upper_bone.rotation = Value::Constant(45.0);
        let NodeKind::Bone(lower_bone) = &mut graph.node_mut(lower).unwrap().kind else {
            unreachable!()
        };
        lower_bone.rotation = Value::Constant(-90.0);
        let tip = graph.bone_pose(lower, 1).unwrap().tip();
        let NodeKind::Bone(lower_bone) = &mut graph.node_mut(lower).unwrap().kind else {
            unreachable!()
        };
        lower_bone.anchor = Some(Box::new(crate::rig::Anchor {
            kind: crate::rig::AnchorKind::Locked,
            target: [Value::Constant(tip.0), Value::Constant(tip.1)],
        }));
        let result = graph.flip_anchored_bend(lower, 1).unwrap();
        assert!(result.feasible);
        apply(&mut graph, &result.changes);
        assert!(near(
            graph.bone_pose(lower, 1).unwrap().tip(),
            [tip.0, tip.1],
            0.04
        ));
        assert!(graph.bone_pose(upper, 1).unwrap().tip().1 < -60.0);
    }

    #[test]
    fn dragging_a_joint_respects_both_branches_of_a_rig() {
        let (mut graph, [_, knee, foot]) = leg();
        let branch = graph.add(
            "Second foot",
            NodeKind::Bone(Bone {
                length: 100.0,
                rest_angle: 90.0,
                anchor: Some(Box::new(crate::rig::Anchor {
                    kind: crate::rig::AnchorKind::Locked,
                    target: [Value::Constant(200.0), Value::Constant(100.0)],
                })),
                ..Bone::default()
            }),
        );
        graph.connect(knee, branch, 0);
        let NodeKind::Bone(bone) = &mut graph.node_mut(foot).unwrap().kind else {
            unreachable!()
        };
        bone.anchor = Some(Box::new(crate::rig::Anchor {
            kind: crate::rig::AnchorKind::Locked,
            target: [Value::Constant(300.0), Value::Constant(0.0)],
        }));
        let result = graph.solve_rig(knee, [275.0, 50.0], 1).unwrap();
        apply(&mut graph, &result.changes);
        assert!(near(
            graph.bone_pose(foot, 1).unwrap().tip(),
            [300.0, 0.0],
            0.04
        ));
        assert!(near(
            graph.bone_pose(branch, 1).unwrap().tip(),
            [200.0, 100.0],
            0.04
        ));
    }

    #[test]
    fn aim_target_moves_an_upstream_joint_when_it_can_reach() {
        let (mut graph, [hip, _, foot]) = leg();
        let NodeKind::Bone(bone) = &mut graph.node_mut(foot).unwrap().kind else {
            unreachable!()
        };
        bone.anchor = Some(Box::new(crate::rig::Anchor {
            kind: crate::rig::AnchorKind::Aim,
            target: [Value::Constant(300.0), Value::Constant(0.0)],
        }));
        let result = graph.solve_rig(foot, [150.0, 150.0], 1).unwrap();
        assert_eq!(result.target, Some([150.0, 150.0]));
        apply(&mut graph, &result.changes);
        assert!(near(
            graph.bone_pose(foot, 1).unwrap().tip(),
            [150.0, 150.0],
            0.06
        ));
        assert!(!near(
            graph.bone_pose(hip, 1).unwrap().tip(),
            [100.0, 0.0],
            0.5
        ));
    }

    #[test]
    fn stretchy_bones_reach_and_aim_anchors_aim() {
        let (mut graph, [hip, knee, foot]) = leg();
        for id in [hip, knee] {
            if let Some(NodeKind::Bone(b)) = graph.node_mut(id).map(|n| &mut n.kind) {
                b.stretch_mode = crate::rig::Stretch::Simple;
                b.stretch_limits = Some([0.5, 1.5]);
            }
        }
        // The foot aims straight down.
        if let Some(NodeKind::Bone(b)) = graph.node_mut(foot).map(|n| &mut n.kind) {
            b.anchor = Some(Box::new(crate::rig::Anchor {
                kind: crate::rig::AnchorKind::Aim,
                target: [Value::Constant(250.0), Value::Constant(1000.0)],
            }));
        }
        // Pull the knee out of rigid reach (250 > 200): the bones stretch.
        let changes = graph.solve_rig(knee, [250.0, 0.0], 1).unwrap();
        apply(&mut graph, &changes.changes);
        let pose = |id| graph.bone_pose(id, 1).unwrap();
        assert!(
            near(pose(knee).tip(), [250.0, 0.0], 0.5),
            "{:?}",
            pose(knee).tip()
        );
        let (a, b) = (pose(foot).posed.apply(0.0, 0.0), pose(foot).tip());
        assert!(
            (b.0 - a.0).abs() < 1.0 && b.1 > a.1 + 90.0,
            "the foot points down"
        );
    }
}
