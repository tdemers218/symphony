//! The scene document (ENGINEERING E2.1): settings, layers, the xsheet, the
//! node graph, drawings and the palette. Everything a scene file holds.
//! UI state (current frame, selection, tool) is not here (E3.5).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::graph::{Graph, Link, Node, NodeId, NodeKind, PegParams};
use crate::id::{Id, IdGen};
use crate::palette::{Palette, Swatch};
use crate::stroke::Drawing;
use crate::xsheet::{Column, Xsheet};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    pub width: u32,
    pub height: u32,
    pub fps: f32,
    pub length: u32,
    /// Black bars over the camera frame: rounded edges or a cinematic
    /// shape. Drawn in the Camera view and in exports.
    #[serde(default, skip_serializing_if = "Bars::is_off")]
    pub bars: Bars,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            width: 1920,
            height: 1080,
            fps: 24.0,
            length: 48,
            bars: Bars::default(),
        }
    }
}

/// Black bars framing the picture.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Bars {
    pub on: bool,
    /// Width / height of the picture left between the bars; 0 = the
    /// frame's own shape (bars only round the corners).
    pub aspect: f32,
    /// Corner radius of the picture, in frame pixels.
    pub radius: f32,
}

impl Bars {
    pub fn is_off(&self) -> bool {
        !self.on
    }

    /// The picture left between the bars in a `w` x `h` frame centred on
    /// the origin: (x, y, width, height, corner radius). None when there
    /// are no bars to draw.
    pub fn opening(&self, w: f32, h: f32) -> Option<([f32; 4], f32)> {
        if !self.on || w <= 0.0 || h <= 0.0 {
            return None;
        }
        let (mut ow, mut oh) = (w, h);
        if self.aspect.is_finite() && self.aspect > 0.0 {
            if self.aspect > w / h {
                oh = w / self.aspect;
            } else {
                ow = h * self.aspect;
            }
        }
        let r = self.radius.clamp(0.0, ow.min(oh) / 2.0);
        if (ow - w).abs() < 0.01 && (oh - h).abs() < 0.01 && r <= 0.0 {
            return None;
        }
        Some(([-ow / 2.0, -oh / 2.0, ow, oh], r))
    }

    /// The opening's outline as points (corners rounded), centred on the
    /// origin, clockwise from the top-left.
    pub fn outline(&self, w: f32, h: f32) -> Option<Vec<[f32; 2]>> {
        let ([x, y, ow, oh], r) = self.opening(w, h)?;
        let steps = if r > 0.0 { 10 } else { 0 };
        let mut pts = Vec::new();
        let corners = [
            ([x + ow - r, y + r], -90.0f32),
            ([x + ow - r, y + oh - r], 0.0),
            ([x + r, y + oh - r], 90.0),
            ([x + r, y + r], 180.0),
        ];
        for (c, start) in corners {
            for i in 0..=steps {
                let a = (start + 90.0 * i as f32 / steps.max(1) as f32).to_radians();
                pts.push([c[0] + r * a.cos(), c[1] + r * a.sin()]);
            }
        }
        Some(pts)
    }
}

/// A timeline layer: its column plus its drawing node and peg.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Layer {
    pub column: Id,
    pub node: NodeId,
    pub peg: NodeId,
}

/// Everything needed to put a removed layer back exactly as it was.
#[derive(Clone, Debug, PartialEq)]
pub struct LayerSnapshot {
    pub index: usize,
    pub layer: Layer,
    pub column: Column,
    pub node: Node,
    pub peg: Node,
    pub links: Vec<Link>,
}

/// A connection in the imported source graph, including its enclosing group.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SourceConnection {
    pub group: String,
    pub from: String,
    pub output: usize,
    pub to: String,
    pub input: usize,
}

pub const DEFAULT_CAMERA_FOV: f32 = 60.0;

fn default_camera_fov() -> crate::anim::Value {
    crate::anim::Value::Constant(DEFAULT_CAMERA_FOV)
}

/// A camera looking straight down Z at flat artwork. Dolly is positive forward.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CameraMotion {
    pub x: crate::anim::Value,
    pub y: crate::anim::Value,
    pub dolly: crate::anim::Value,
}

impl CameraMotion {
    pub fn at(&self, frame: u32) -> [f32; 3] {
        [self.x.at(frame), self.y.at(frame), self.dolly.at(frame)].map(|v| {
            if v.is_finite() {
                v
            } else {
                0.0
            }
        })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Scene {
    /// Source node counts for the Harmony import report.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub harmony_nodes: BTreeMap<String, usize>,
    /// Original scene XML retains source wiring, attributes and unknown modules.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub harmony_source: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub harmony_connections: Vec<SourceConnection>,
    pub settings: Settings,
    pub xsheet: Xsheet,
    pub graph: Graph,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub node_positions: BTreeMap<NodeId, [f32; 2]>,
    #[serde(default)]
    pub custom_wiring: bool,
    /// Front to back: index 0 is the top of the Timeline and composite port 0.
    pub layers: Vec<Layer>,
    pub drawings: BTreeMap<Id, Drawing>,
    pub palette: Palette,
    /// Brush tips and paper grains used by textured strokes, by content
    /// key, so a scene carries its brushes with it.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub textures: BTreeMap<u64, crate::stroke::Texture>,
    /// Imported files (images, sound), by content key. The files live in
    /// the project's `media/` folder (or the app's media cache before the
    /// first save); see `media_dirs`.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub media: BTreeMap<u64, Media>,
    /// Where to look for media files, in order (the project's `media/`
    /// folder, then the app's cache). Set when a scene is opened or
    /// created; not saved.
    #[serde(skip)]
    pub media_dirs: Vec<std::path::PathBuf>,
    /// Sound clips on the timeline, in the order they were added.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sounds: Vec<SoundClip>,
    pub composite: NodeId,
    pub display: NodeId,
    #[serde(default = "default_camera_fov")]
    pub camera_fov: crate::anim::Value,
    #[serde(default)]
    pub camera_motion: CameraMotion,
    pub ids: IdGen,
    /// Bumped by every applied edit (E3.6). Not saved.
    #[serde(skip)]
    pub version: u64,
}

/// An imported file.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Media {
    pub kind: MediaKind,
    /// File name inside the media folders ("<content key>.<ext>").
    pub file: String,
    /// The name it was imported under, for display.
    pub name: String,
    /// Pixel size (images).
    #[serde(default)]
    pub size: [u32; 2],
    /// Length in seconds (sound).
    #[serde(default)]
    pub seconds: f32,
}

/// A sound placed on the timeline.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SoundClip {
    /// Key into `Scene::media`.
    pub media: u64,
    pub name: String,
    /// The frame its first sample plays on.
    pub start: u32,
    #[serde(default)]
    pub muted: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MediaKind {
    Image,
    Sound,
}

impl Scene {
    /// Where an imported file is on disk, if it's anywhere we know.
    pub fn media_path(&self, key: u64) -> Option<std::path::PathBuf> {
        let m = self.media.get(&key)?;
        self.media_dirs
            .iter()
            .map(|d| d.join(&m.file))
            .find(|p| p.is_file())
    }

    /// Horizontal framing for the 2D camera. The original framing is 60°.
    pub fn camera_scale(&self, frame: u32) -> f32 {
        let fov = self.camera_fov.at(frame);
        let fov = if fov.is_finite() {
            fov.clamp(20.0, 120.0)
        } else {
            DEFAULT_CAMERA_FOV
        };
        (DEFAULT_CAMERA_FOV.to_radians() * 0.5).tan() / (fov.to_radians() * 0.5).tan()
    }

    /// The camera's original distance from artwork at depth zero, in scene units.
    pub fn stage_distance(&self) -> f32 {
        self.settings.width.max(1) as f32
    }

    pub fn distance_to_plane(&self, frame: u32, depth: f32) -> f32 {
        self.stage_distance() + depth - self.camera_motion.at(frame)[2]
    }

    /// Camera-space affine projection for one camera-facing plane. Ignore planes
    /// within the near clip distance or behind the camera.
    pub fn project_plane(&self, frame: u32, depth: f32) -> Option<crate::math::Affine> {
        use crate::math::Affine;
        let [x, y, _] = self.camera_motion.at(frame);
        let distance = self.distance_to_plane(frame, depth);
        if distance < self.stage_distance() * 0.02 || !distance.is_finite() {
            return None;
        }
        let scale = self.stage_distance() / distance;
        Some(Affine::scale(scale, scale).compose(Affine::translate(-x, -y)))
    }

    pub fn camera_transform(&self, node: NodeId, frame: u32) -> Option<crate::math::Affine> {
        let pose = self.graph.world_pose(node, frame).ok()?;
        Some(
            self.project_plane(frame, pose.depth)?
                .compose(pose.transform),
        )
    }

    pub fn new(settings: Settings) -> Self {
        let mut graph = Graph::default();
        let composite = graph.add("Composite", NodeKind::Composite);
        let display = graph.add("Camera", NodeKind::Display);
        graph.connect(composite, display, 0);
        let mut ids = IdGen::default();
        let swatch = |ids: &mut IdGen, name: &str, rgb| Swatch {
            id: ids.fresh(),
            name: name.into(),
            rgb,
            opacity: 255,
            protected: false,
        };
        let palette = Palette {
            name: "Scene palette".into(),
            swatches: vec![
                swatch(&mut ids, "Line", [24, 24, 32]),
                swatch(&mut ids, "Skin", [240, 196, 160]),
                swatch(&mut ids, "Shirt", [64, 112, 220]),
                swatch(&mut ids, "Hair", [92, 56, 40]),
                swatch(&mut ids, "Shadow", [120, 120, 150]),
                swatch(&mut ids, "Accent", [220, 70, 60]),
            ],
        };
        Scene {
            harmony_nodes: BTreeMap::new(),
            harmony_source: None,
            harmony_connections: Vec::new(),
            settings,
            xsheet: Xsheet::default(),
            graph,
            node_positions: BTreeMap::new(),
            custom_wiring: false,
            layers: Vec::new(),
            drawings: BTreeMap::new(),
            palette,
            textures: BTreeMap::new(),
            media: BTreeMap::new(),
            media_dirs: Vec::new(),
            sounds: Vec::new(),
            composite,
            display,
            camera_fov: default_camera_fov(),
            camera_motion: CameraMotion::default(),
            ids,
            version: 0,
        }
    }

    /// Build the parts of a new layer without inserting it. Use
    /// `Edit::AddLayer` to insert it undoably.
    pub fn make_layer(&mut self, name: &str, index: usize) -> LayerSnapshot {
        let column = Column::new(self.ids.fresh(), name);
        let peg = Node {
            name: format!("{name}-P"),
            kind: NodeKind::Peg(PegParams::default()),
            enabled: true,
        };
        let node = Node {
            name: name.into(),
            kind: NodeKind::Drawing { column: column.id },
            enabled: true,
        };
        // Reserve graph slots now so the IDs are stable from the start.
        let peg_id = self.graph.add("", NodeKind::Display);
        let node_id = self.graph.add("", NodeKind::Display);
        self.graph.remove(peg_id);
        self.graph.remove(node_id);
        let mut links = vec![Link {
            from: peg_id,
            to: node_id,
            port: 0,
        }];
        if self.custom_wiring {
            let port = (0..)
                .find(|&port| self.graph.input(self.composite, port).is_none())
                .unwrap_or(0);
            links.push(Link {
                from: node_id,
                to: self.composite,
                port,
            });
        }
        LayerSnapshot {
            index,
            layer: Layer {
                column: column.id,
                node: node_id,
                peg: peg_id,
            },
            column,
            node,
            peg,
            links,
        }
    }

    /// Everything needed to remove layer `index` undoably, without removing it.
    pub fn snapshot_layer(&self, index: usize) -> Option<LayerSnapshot> {
        let layer = *self.layers.get(index)?;
        Some(LayerSnapshot {
            index,
            layer,
            column: self.xsheet.column(layer.column)?.clone(),
            node: self.graph.node(layer.node)?.clone(),
            peg: self.graph.node(layer.peg)?.clone(),
            links: self
                .graph
                .links()
                .iter()
                .copied()
                .filter(|link| {
                    [layer.peg, layer.node].contains(&link.from)
                        || [layer.peg, layer.node].contains(&link.to)
                })
                .collect(),
        })
    }

    pub(crate) fn insert_layer(&mut self, s: &LayerSnapshot) {
        let at = s.index.min(self.layers.len());
        self.xsheet
            .columns
            .insert(at.min(self.xsheet.columns.len()), s.column.clone());
        self.graph.restore(s.layer.peg, s.peg.clone(), &[]);
        self.graph.restore(s.layer.node, s.node.clone(), &s.links);
        self.layers.insert(at, s.layer);
        self.rewire();
    }

    pub(crate) fn take_layer(&mut self, index: usize) -> Option<LayerSnapshot> {
        let layer = *self.layers.get(index)?;
        let pos = self
            .xsheet
            .columns
            .iter()
            .position(|c| c.id == layer.column)?;
        let column = self.xsheet.columns.remove(pos);
        let (node, mut links) = self.graph.remove(layer.node)?;
        let (peg, peg_links) = self.graph.remove(layer.peg)?;
        links.extend(peg_links);
        if !self.custom_wiring {
            links.retain(|l| l.to != self.composite);
        }
        links.dedup();
        self.layers.remove(index);
        self.rewire();
        Some(LayerSnapshot {
            index,
            layer,
            column,
            node,
            peg,
            links,
        })
    }

    /// Move the layer at `from` to `to` (both indices in the layer list),
    /// keeping the Xsheet columns in the same order and the Composite
    /// stacking in step.
    pub(crate) fn move_layer(&mut self, from: usize, to: usize) {
        if from >= self.layers.len() || to >= self.layers.len() || from == to {
            return;
        }
        let layer = self.layers.remove(from);
        self.layers.insert(to, layer);
        let order: Vec<Id> = self.layers.iter().map(|l| l.column).collect();
        self.xsheet.columns.sort_by_key(|c| {
            order
                .iter()
                .position(|id| *id == c.id)
                .unwrap_or(usize::MAX)
        });
        self.rewire();
    }

    /// Composite inputs follow layer order (port 0 = front).
    pub fn rewire(&mut self) {
        if self.custom_wiring {
            return;
        }
        self.graph.disconnect_inputs(self.composite);
        for (port, l) in self.layers.iter().enumerate() {
            self.graph.connect(l.node, self.composite, port);
        }
    }

    pub fn column(&self, layer: usize) -> Option<&Column> {
        self.xsheet.column(self.layers.get(layer)?.column)
    }

    pub fn layer_of(&self, column: Id) -> Option<usize> {
        self.layers.iter().position(|l| l.column == column)
    }

    /// Drawing exposed on `layer` at `frame`.
    pub fn exposed(&self, layer: usize, frame: u32) -> Option<Id> {
        self.column(layer)?.get(frame)
    }

    pub fn peg(&self, layer: usize) -> Option<&PegParams> {
        match &self.graph.node(self.layers.get(layer)?.peg)?.kind {
            NodeKind::Peg(p) => Some(p),
            _ => None,
        }
    }

    /// Next free drawing name in a column: "1", "2", ... like Harmony.
    pub fn next_drawing_name(&self, column: Id) -> String {
        let max = self
            .drawings
            .values()
            .filter(|d| d.column == column)
            .filter_map(|d| d.name.parse::<u32>().ok())
            .max()
            .unwrap_or(0);
        (max + 1).to_string()
    }

    /// A fresh empty drawing for `column` (not inserted).
    pub fn make_drawing(&mut self, column: Id) -> Drawing {
        Drawing {
            id: self.ids.fresh(),
            column,
            name: self.next_drawing_name(column),
            strokes: Vec::new(),
            fills: Vec::new(),
            image: None,
            text: Vec::new(),
        }
    }

    pub fn swatch_rgba(&self, swatch: Id) -> [u8; 4] {
        self.palette
            .get(swatch)
            .map(Swatch::rgba)
            .unwrap_or([0, 0, 0, 255])
    }

    pub fn swatch_rgb(&self, swatch: Id) -> [u8; 3] {
        self.palette.get(swatch).map(|s| s.rgb).unwrap_or_default()
    }
}
