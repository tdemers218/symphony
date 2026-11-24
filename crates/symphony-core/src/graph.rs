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
    pub scale_x: Option<Value>,
    pub scale_y: Option<Value>,
    pub pivot: (f32, f32),
}

impl PegParams {
    pub fn transform_at(&self, frame: u32) -> Affine {
        let sx = self.scale_x.as_ref().map_or(1.0, |v| v.at(frame));
        let sy = self.scale_y.as_ref().map_or(1.0, |v| v.at(frame));
        let (px, py) = self.pivot;
        Affine::translate(self.x.at(frame), self.y.at(frame))
            .compose(Affine::translate(px, py))
            .compose(Affine::rotate_deg(self.rotation.at(frame)))
            .compose(Affine::scale(sx, sy))
            .compose(Affine::translate(-px, -py))
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

/// Camera-facing artwork keeps a 2D transform and a separate depth.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StagePose {
    pub transform: Affine,
    pub depth: f32,
}

impl Default for StagePose {
    fn default() -> Self {
        Self {
            transform: Affine::IDENTITY,
            depth: 0.0,
        }
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

    pub(crate) fn replace_links(&mut self, links: Vec<Link>) {
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
        if self.parent_bone(id).is_some_and(|parent| {
            self.enabled_at(parent, frame) && self.smart_child(parent) == Some(id)
        }) {
            return Some(id);
        }
        self.smart_child(id)
            .filter(|&child| self.enabled_at(child, frame))
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
            let end = Affine::translate(parent.length, 0.0);
            (
                parent.rest.compose(end),
                parent.posed.compose(end),
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
        Ok(BonePose {
            rest: rest_base.compose(rest_turn),
            posed: posed_base.compose(Affine::rotate_deg(turn)),
            parent_skin,
            length: bone.length(),
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
            pose.posed.compose(Affine::translate(pose.length, 0.0))
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
                StagePose {
                    transform: parent.transform.compose(p.transform_at(frame)),
                    depth: parent.depth + p.z.at(frame),
                }
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
}
