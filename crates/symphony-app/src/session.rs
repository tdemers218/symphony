//! The open scene plus everything the UI needs around it: undo history,
//! file path, current frame, selection, tools and onion skin. Every change
//! to the document goes through `history` (ENGINEERING E3.1). The rest is
//! UI state and isn't undoable (E3.5).

use std::path::PathBuf;

use symphony_core::anim::Value;
use symphony_core::doc::{Scene, Settings};
use symphony_core::graph::{NodeKind, PegParams};
use symphony_core::history::{Edit, History};
use symphony_core::id::Id;
use symphony_core::math::Affine;
use symphony_core::stroke::{ArtLayer, Point, Stroke, StrokeKind, Tips};

/// Drawing tools, in Tools-toolbar order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Select,
    Transform,
    Brush,
    Pencil,
    Eraser,
    Paint,
    CloseGap,
    Dropper,
    Hand,
    Zoom,
}

/// Harmony's four Paint tool modes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaintMode {
    /// Paints or repaints zones, whether or not they're filled.
    Paint,
    /// Only fills empty zones.
    PaintUnpainted,
    /// Only recolours zones that already have colour.
    Repaint,
    /// Removes colour from a zone.
    Unpaint,
}

impl PaintMode {
    pub fn title(self) -> &'static str {
        match self {
            PaintMode::Paint => "Paint",
            PaintMode::PaintUnpainted => "Paint Unpainted",
            PaintMode::Repaint => "Repaint",
            PaintMode::Unpaint => "Unpaint",
        }
    }
}

impl Tool {
    pub const ALL: [Tool; 10] = [
        Tool::Select,
        Tool::Transform,
        Tool::Brush,
        Tool::Pencil,
        Tool::Eraser,
        Tool::Paint,
        Tool::CloseGap,
        Tool::Dropper,
        Tool::Hand,
        Tool::Zoom,
    ];

    /// Which milestone brings this tool, if it isn't built yet.
    pub fn planned_for(self) -> Option<&'static str> {
        match self {
            Tool::Brush
            | Tool::Pencil
            | Tool::Eraser
            | Tool::Hand
            | Tool::Zoom
            | Tool::Select
            | Tool::Transform
            | Tool::Paint
            | Tool::CloseGap
            | Tool::Dropper => None,
        }
    }
}

/// What a Brush or Pencil preset holds (Harmony's tool presets).
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct BrushSettings {
    pub min_width: f32,
    pub max_width: f32,
    /// 0 = raw pen, 1 = very smooth.
    pub smoothing: f32,
    /// Pencil start and end tips.
    #[serde(default)]
    pub tips: Tips,
    /// Harmony's thickness stencil: a fixed thickness along the line
    /// (0..=1 between minimum and maximum size) that ignores pen pressure.
    #[serde(default)]
    pub stencil: Option<Stencil>,
    /// Harmony's Brush Type: `None` is a Solid Vector Brush, `Some` a
    /// Textured Vector Brush (tip, grain, spacing, opacity).
    #[serde(default)]
    pub texture: Option<symphony_core::stroke::StrokeTexture>,
}

/// Compare names with runs of digits as numbers, so "2" < "10".
pub fn natural(a: &str, b: &str) -> std::cmp::Ordering {
    let key = |s: &str| -> Vec<(bool, u64, String)> {
        let mut out = Vec::new();
        let mut chars = s.chars().peekable();
        while let Some(&c) = chars.peek() {
            let digit = c.is_ascii_digit();
            let mut part = String::new();
            while let Some(&c) = chars.peek() {
                if c.is_ascii_digit() != digit {
                    break;
                }
                part.push(c);
                chars.next();
            }
            let n = if digit {
                part.parse().unwrap_or(u64::MAX)
            } else {
                0
            };
            out.push((!digit, n, part.to_lowercase()));
        }
        out
    };
    key(a).cmp(&key(b)).then_with(|| a.cmp(b))
}

/// Harmony's names for the art layers.
pub fn art_name(art: ArtLayer) -> &'static str {
    match art {
        ArtLayer::Overlay => "Overlay Art",
        ArtLayer::LineArt => "Line Art",
        ArtLayer::ColourArt => "Colour Art",
        ArtLayer::Underlay => "Underlay Art",
    }
}

impl BrushSettings {
    /// Width for a pen pressure (already through the pressure curve).
    pub fn width_at_pressure(&self, pressure: f32) -> f32 {
        self.min_width + (self.max_width - self.min_width) * pressure.clamp(0.0, 1.0)
    }
}

/// A thickness profile sampled evenly from start to end of a line.
pub type Stencil = [f32; STENCIL_SAMPLES];
pub const STENCIL_SAMPLES: usize = 16;

/// Thickness at `t` (0 at the start of the line, 1 at its end).
pub fn stencil_at(stencil: &Stencil, t: f32) -> f32 {
    let x = t.clamp(0.0, 1.0) * (STENCIL_SAMPLES - 1) as f32;
    let i = (x.floor() as usize).min(STENCIL_SAMPLES - 2);
    let f = x - i as f32;
    stencil[i] * (1.0 - f) + stencil[i + 1] * f
}

/// Replace pen pressure with the stencil, by distance along the line.
pub fn apply_stencil(points: &mut [Point], stencil: &Stencil) {
    let mut along = Vec::with_capacity(points.len());
    let mut total = 0.0;
    for (i, p) in points.iter().enumerate() {
        if i > 0 {
            let q = points[i - 1];
            total += (p.x - q.x).hypot(p.y - q.y);
        }
        along.push(total);
    }
    for (p, d) in points.iter_mut().zip(along) {
        p.pressure = stencil_at(stencil, if total > 0.0 { d / total } else { 0.0 });
    }
}

/// A line's thickness profile, as a stencil (New Thickness Stencil).
pub fn stencil_from(stroke: &Stroke) -> Option<Stencil> {
    let span = stroke.max_width - stroke.min_width;
    if stroke.points.len() < 2 {
        return None;
    }
    let mut lengths = vec![0.0f32];
    for w in stroke.points.windows(2) {
        let d = (w[1].x - w[0].x).hypot(w[1].y - w[0].y);
        lengths.push(lengths.last().unwrap() + d);
    }
    let total = *lengths.last().unwrap();
    if total <= 0.0 {
        return None;
    }
    let width = |i: usize| stroke.width_at(stroke.points[i].pressure);
    let mut out = [0.0; STENCIL_SAMPLES];
    for (k, slot) in out.iter_mut().enumerate() {
        let target = total * k as f32 / (STENCIL_SAMPLES - 1) as f32;
        let i = lengths
            .partition_point(|&l| l < target)
            .clamp(1, lengths.len() - 1);
        let seg = (lengths[i] - lengths[i - 1]).max(1e-6);
        let f = ((target - lengths[i - 1]) / seg).clamp(0.0, 1.0);
        let w = width(i - 1) * (1.0 - f) + width(i) * f;
        // Relative to the line's own sizes, so it scales with any preset.
        *slot = if span > 0.0 {
            ((w - stroke.min_width) / span).clamp(0.0, 1.0)
        } else {
            1.0
        };
    }
    Some(out)
}

/// A peg's values at the current frame, for the Transform tool.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PegValues {
    pub x: f32,
    pub y: f32,
    pub rotation: f32,
    pub scale_x: f32,
    pub scale_y: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct Onion {
    pub on: bool,
    pub before: u32,
    pub after: u32,
}

/// Strokes picked with the Select tool, in one drawing.
#[derive(Clone, Debug, Default)]
pub struct Selection {
    pub drawing: Option<Id>,
    pub strokes: Vec<Id>,
}

pub struct Session {
    pub doc: Scene,
    pub history: History,
    pub path: Option<PathBuf>,
    pub saved_version: u64,
    /// Identifies this session's autosave file (E7.5).
    pub key: String,
    pub frame: u32,
    pub playing: bool,
    pub selected: usize,
    pub tool: Tool,
    pub brush: BrushSettings,
    pub pencil: BrushSettings,
    /// Eraser size (pressure between minimum and maximum) and the tip left
    /// on cut pencil lines (Harmony's Tip Style).
    pub eraser: BrushSettings,
    pub eraser_tip: symphony_core::stroke::Tip,
    /// Eraser options: all art layers (Harmony's Apply to Line and Colour
    /// Art), every visible Timeline layer under the eraser, and every
    /// drawing of the layer (all frames).
    pub erase_all_art: bool,
    pub erase_all_layers: bool,
    pub erase_all_frames: bool,
    pub swatch: Id,
    /// Pressure response, as a gamma (E4.1). 1 = linear.
    pub pressure_gamma: f32,
    pub onion: Onion,
    /// Harmony's Brush Size Cursor preference (DESIGN D8.11).
    pub brush_size_cursor: bool,
    pub animate: bool,
    pub selection: Selection,
    pub paint_mode: PaintMode,
    /// Harmony's Show Strokes (K): reveal invisible strokes.
    pub show_strokes: bool,
    /// Harmony's Respect Protected Colours (Shift+S), on by default.
    pub respect_protected: bool,
    /// The art layer tools draw and edit on (Harmony's Art Layer toolbar).
    pub art: ArtLayer,
    /// Preview All Enabled Art Layers (P): show the other art layers as
    /// reference in the Drawing view. The Camera view always shows all.
    pub art_preview: bool,
    /// Preference: Support Overlay and Underlay Arts (off, as in Harmony).
    pub overlay_underlay: bool,
    /// Preference: tool sizes are screen points, so the brush looks the
    /// same size at any zoom. Off by default: sizes are scene units.
    pub size_follows_zoom: bool,
    /// Select tool options, as in Harmony: Marquee instead of Lasso, and
    /// Easy Drag (drag from anywhere inside the box).
    pub marquee_mode: bool,
    pub easy_drag: bool,
    /// Copied strokes (Cut/Copy/Paste).
    pub clipboard: Vec<Stroke>,
    /// Last action, for the status bar ("Undo Brush stroke").
    pub notice: Option<String>,
}

fn new_key() -> String {
    let n = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("scene-{n:x}")
}

impl Session {
    /// A new scene with one layer called "Drawing", like Harmony's new scene.
    pub fn new_scene() -> Self {
        let mut doc = Scene::new(Settings::default());
        let mut history = History::default();
        let snap = doc.make_layer("Drawing", 0);
        history.apply(&mut doc, "Add layer", vec![Edit::AddLayer(Box::new(snap))]);
        Self::from_doc(doc, None)
    }

    pub fn from_doc(doc: Scene, path: Option<PathBuf>) -> Self {
        let swatch = doc.palette.swatches.first().map(|s| s.id).unwrap_or(Id(0));
        Session {
            saved_version: doc.version,
            doc,
            history: History::default(),
            path,
            key: new_key(),
            frame: 1,
            playing: false,
            selected: 0,
            tool: Tool::Brush,
            brush: BrushSettings {
                min_width: 0.6,
                max_width: 10.0,
                smoothing: 0.4,
                tips: Tips::default(),
                stencil: None,
                texture: None,
            },
            pencil: BrushSettings {
                min_width: 2.0,
                max_width: 3.0,
                smoothing: 0.3,
                tips: Tips::default(),
                stencil: None,
                texture: None,
            },
            eraser: BrushSettings {
                min_width: 8.0,
                max_width: 20.0,
                smoothing: 0.0,
                tips: Tips::default(),
                stencil: None,
                texture: None,
            },
            eraser_tip: symphony_core::stroke::Tip::Round,
            erase_all_art: false,
            erase_all_layers: false,
            erase_all_frames: false,
            swatch,
            pressure_gamma: 1.0,
            onion: Onion {
                on: false,
                before: 1,
                after: 1,
            },
            brush_size_cursor: false,
            selection: Selection::default(),
            paint_mode: PaintMode::Paint,
            show_strokes: false,
            respect_protected: true,
            art: ArtLayer::LineArt,
            art_preview: true,
            overlay_underlay: false,
            size_follows_zoom: false,
            marquee_mode: false,
            easy_drag: false,
            clipboard: Vec::new(),
            animate: true,
            notice: None,
        }
    }

    /// The demo scene used for screenshots and first impressions.
    pub fn demo() -> Self {
        let mut s = Self::new_scene();
        s.doc.settings.length = 48;
        s.rename_layer(0, "Head");
        s.add_layer_named("Body", 1);
        s.add_layer_named("BG", 2);
        let circle = |cx: f32, cy: f32, r: f32| -> Vec<Point> {
            (0..=48)
                .map(|i| {
                    let t = i as f32 / 48.0 * std::f32::consts::TAU;
                    Point {
                        x: cx + r * t.cos(),
                        y: cy + r * t.sin(),
                        pressure: 0.55 + 0.45 * (t * 1.5).sin().abs(),
                    }
                })
                .collect()
        };
        let line = |pts: &[(f32, f32)]| -> Vec<Point> {
            pts.iter()
                .enumerate()
                .map(|(i, &(x, y))| Point {
                    x,
                    y,
                    pressure: 0.3 + 0.7 * ((i as f32) * 0.9).sin().abs(),
                })
                .collect()
        };
        let art: [(usize, Vec<Vec<Point>>, usize); 3] = [
            (
                0,
                vec![
                    circle(0.0, -180.0, 110.0),
                    circle(-38.0, -195.0, 12.0),
                    circle(38.0, -195.0, 12.0),
                ],
                0,
            ),
            (
                1,
                vec![line(&[
                    (-90.0, -60.0),
                    (-120.0, 120.0),
                    (-60.0, 300.0),
                    (60.0, 300.0),
                    (120.0, 120.0),
                    (90.0, -60.0),
                ])],
                0,
            ),
            (
                2,
                vec![line(&[
                    (-900.0, 380.0),
                    (-300.0, 340.0),
                    (300.0, 360.0),
                    (900.0, 330.0),
                ])],
                4,
            ),
        ];
        for (layer, strokes, swatch) in art {
            let swatch = s.doc.palette.swatches[swatch].id;
            for points in strokes {
                let st = Stroke {
                    id: s.doc.ids.fresh(),
                    kind: StrokeKind::Brush,
                    art: Default::default(),
                    swatch,
                    min_width: 1.0,
                    max_width: 9.0,
                    points,
                    tips: Default::default(),
                    texture: None,
                    shape: Vec::new(),
                };
                s.commit_stroke(layer, 1, st);
            }
            let col = s.doc.layers[layer].column;
            if let Some(d) = s.doc.exposed(layer, 1) {
                if let Some(c) = s.doc.xsheet.column_mut(col) {
                    c.expose(d, 1, 48);
                }
            }
        }
        // Paint the head with skin, so the demo shows ink and paint.
        let line = s.swatch;
        s.swatch = s.doc.palette.swatches[1].id;
        s.paint_at(0, 1, 0.0, -130.0, 1.0);
        s.swatch = line;
        let mut peg = s.doc.peg(0).cloned().unwrap_or_default();
        peg.x.set_key(1, 0.0);
        peg.x.set_key(24, 120.0);
        peg.x.set_key(48, 0.0);
        s.set_peg(0, peg);
        s.history.clear();
        s.saved_version = s.doc.version;
        s
    }

    pub fn dirty(&self) -> bool {
        self.doc.version != self.saved_version
    }

    pub fn name(&self) -> String {
        self.path
            .as_deref()
            .map(symphony_io::project_name)
            .unwrap_or_else(|| "Untitled scene".into())
    }

    pub fn length(&self) -> u32 {
        self.doc.settings.length
    }

    pub fn layer_count(&self) -> usize {
        self.doc.layers.len()
    }

    pub fn layer_name(&self, i: usize) -> &str {
        self.doc.column(i).map(|c| c.name.as_str()).unwrap_or("")
    }

    pub fn layer_visible(&self, i: usize) -> bool {
        self.doc.column(i).is_some_and(|c| c.visible)
    }

    /// Name of the drawing exposed on `layer` at `frame`.
    pub fn drawing_name(&self, layer: usize, frame: u32) -> Option<&str> {
        let id = self.doc.exposed(layer, frame)?;
        self.doc.drawings.get(&id).map(|d| d.name.as_str())
    }

    fn note(&mut self, text: impl Into<String>) {
        self.notice = Some(text.into());
    }

    // ---- Undo ------------------------------------------------------------

    pub fn undo(&mut self) {
        match self.history.undo(&mut self.doc) {
            Some(name) => self.note(format!("Undo {name}")),
            None => self.note("Nothing to undo"),
        }
        self.clamp_selection();
    }

    pub fn redo(&mut self) {
        match self.history.redo(&mut self.doc) {
            Some(name) => self.note(format!("Redo {name}")),
            None => self.note("Nothing to redo"),
        }
        self.clamp_selection();
    }

    /// End the current gesture (drag, erase), so it undoes as one step.
    pub fn end_gesture(&mut self) {
        self.history.seal();
    }

    fn clamp_selection(&mut self) {
        self.frame = self.frame.clamp(1, self.length().max(1));
        if self.doc.palette.get(self.swatch).is_none() {
            if let Some(first) = self.doc.palette.swatches.first() {
                self.swatch = first.id;
            }
        }
        self.selected = self.selected.min(self.layer_count().saturating_sub(1));
    }

    // ---- Drawing ---------------------------------------------------------

    /// Add a finished stroke. On an empty cell this creates the next
    /// numbered drawing there first, like Harmony, all in one undo step.
    pub fn commit_stroke(&mut self, layer: usize, frame: u32, stroke: Stroke) {
        let Some(col) = self.doc.layers.get(layer).map(|l| l.column) else {
            return;
        };
        let name = match stroke.kind {
            StrokeKind::Brush => "Brush stroke",
            StrokeKind::Pencil => "Pencil stroke",
            StrokeKind::Invisible => "Close gap",
        };
        let mut edits = Vec::new();
        let drawing = match self.doc.exposed(layer, frame) {
            Some(d) => d,
            None => {
                let d = self.doc.make_drawing(col);
                let id = d.id;
                edits.push(Edit::AddDrawing { drawing: d });
                edits.push(Edit::SetCells {
                    column: col,
                    from: frame,
                    before: vec![None],
                    after: vec![Some(id)],
                });
                id
            }
        };
        edits.push(Edit::AddStroke { drawing, stroke });
        self.history.apply(&mut self.doc, name, edits);
    }

    /// Erase strokes by ID from `drawing`. Repeated calls during one eraser
    /// gesture undo as a single step.
    /// Harmony's Eraser on vector art, over discs in drawing space.
    /// Brush strokes and paint lose exactly the eraser's shape (they become
    /// outlines); pencil lines, textured strokes and invisible lines are cut
    /// along their centreline with the eraser's tip style. Works on the
    /// current art layer, or all of them with "Apply to all art layers".
    /// Pieces keep their depth; a whole drag is one undo step (call
    /// `end_gesture`).
    pub fn erase(&mut self, drawing: Id, eraser: &[symphony_vector::cut::Disc]) {
        use symphony_vector::shape;
        let all_art = self.erase_all_art;
        let art = self.art;
        let tip = self.eraser_tip;
        let on = |a: ArtLayer| all_art || a == art;
        let Some(d) = self.doc.drawings.get(&drawing) else {
            return;
        };
        let poly = shape::eraser_shape(eraser);
        // The eraser's reach, to skip everything nowhere near it quickly.
        let Some(reach) = eraser
            .iter()
            .map(|(c, r)| ([c[0] - r, c[1] - r], [c[0] + r, c[1] + r]))
            .reduce(|(a0, a1), (b0, b1)| {
                (
                    [a0[0].min(b0[0]), a0[1].min(b0[1])],
                    [a1[0].max(b1[0]), a1[1].max(b1[1])],
                )
            })
        else {
            return;
        };
        let near = |lo: [f32; 2], hi: [f32; 2]| {
            lo[0] <= reach.1[0] && lo[1] <= reach.1[1] && hi[0] >= reach.0[0] && hi[1] >= reach.0[1]
        };
        // What changes, worked out first, then applied one edit at a time.
        let mut strokes: Vec<(Id, Vec<Stroke>)> = Vec::new();
        for st in d.strokes.iter().filter(|st| on(st.art)) {
            if !symphony_vector::bounds(st).is_some_and(|(lo, hi)| near(lo, hi)) {
                continue;
            }
            let by_shape = st.kind == StrokeKind::Brush && st.texture.is_none();
            if by_shape {
                let outlines = shape::outline(st);
                let mut hit = false;
                let mut left: Vec<shape::Shape> = Vec::new();
                for o in &outlines {
                    match shape::erase(o, &poly) {
                        Some(parts) => {
                            hit = true;
                            left.extend(parts);
                        }
                        None => left.push(o.clone()),
                    }
                }
                if hit {
                    let pieces = left
                        .into_iter()
                        .map(|sh| Stroke {
                            shape: sh,
                            ..st.clone()
                        })
                        .collect();
                    strokes.push((st.id, pieces));
                }
            } else if let Some(pieces) = symphony_vector::cut::cut(st, eraser, tip) {
                strokes.push((st.id, pieces));
            }
        }
        let mut fills: Vec<(Id, Vec<symphony_core::stroke::Fill>)> = Vec::new();
        for f in d.fills.iter().filter(|f| on(f.art)) {
            if let Some(parts) = shape::erase(&f.contours, &poly) {
                let pieces = parts
                    .into_iter()
                    .map(|contours| symphony_core::stroke::Fill {
                        contours,
                        ..f.clone()
                    })
                    .collect();
                fills.push((f.id, pieces));
            }
        }
        for (id, mut pieces) in strokes {
            let Some(index) = self
                .doc
                .drawings
                .get(&drawing)
                .and_then(|d| d.strokes.iter().position(|x| x.id == id))
            else {
                continue;
            };
            let before = self.doc.drawings[&drawing].strokes[index].clone();
            for p in &mut pieces {
                p.id = self.doc.ids.fresh();
            }
            self.history.apply_merged(
                &mut self.doc,
                "Erase",
                "erase",
                Edit::ReplaceStroke {
                    drawing,
                    index,
                    before,
                    after: pieces,
                },
            );
        }
        for (id, mut pieces) in fills {
            let Some(index) = self
                .doc
                .drawings
                .get(&drawing)
                .and_then(|d| d.fills.iter().position(|x| x.id == id))
            else {
                continue;
            };
            let before = self.doc.drawings[&drawing].fills[index].clone();
            for p in &mut pieces {
                p.id = self.doc.ids.fresh();
            }
            self.history.apply_merged(
                &mut self.doc,
                "Erase",
                "erase",
                Edit::ReplaceFill {
                    drawing,
                    index,
                    before,
                    after: pieces,
                },
            );
        }
    }

    /// Harmony's Create Empty Drawing: a new drawing in the current cell.
    pub fn new_drawing(&mut self) {
        let Some(col) = self.doc.layers.get(self.selected).map(|l| l.column) else {
            return;
        };
        let before = self.doc.exposed(self.selected, self.frame);
        let d = self.doc.make_drawing(col);
        let (id, name) = (d.id, d.name.clone());
        self.history.apply(
            &mut self.doc,
            "Create empty drawing",
            vec![
                Edit::AddDrawing { drawing: d },
                Edit::SetCells {
                    column: col,
                    from: self.frame,
                    before: vec![before],
                    after: vec![Some(id)],
                },
            ],
        );
        self.note(format!("Created drawing {name}"));
    }

    // ---- Ink and paint (M5) ---------------------------------------------

    /// The Paint tool at drawing-local (`x`, `y`). `cell` is about one
    /// screen pixel in drawing units, the precision of zone finding.
    pub fn paint_at(&mut self, layer: usize, frame: u32, x: f32, y: f32, cell: f32) {
        use symphony_vector::fill::{find_zone, ZoneError};
        let Some(drawing) = self.doc.exposed(layer, frame) else {
            self.note("There's no drawing here to paint");
            return;
        };
        let Some(d) = self.doc.drawings.get(&drawing) else {
            return;
        };
        let mode = self.paint_mode;
        let swatch = self.swatch;
        let art = self.art;
        // Topmost existing zone under the point, on the current art layer.
        let existing = d
            .fills
            .iter()
            .enumerate()
            .rev()
            .find(|(_, f)| f.art == art && f.contains(x, y))
            .map(|(i, f)| (i, f.clone()));
        if let Some((_, fill)) = &existing {
            let changes = match mode {
                PaintMode::Unpaint => true,
                PaintMode::PaintUnpainted => false,
                _ => fill.swatch != swatch,
            };
            if changes && self.is_locked(fill.swatch) {
                self.note_protected(fill.swatch);
                return;
            }
        }
        match (mode, existing) {
            (PaintMode::Unpaint, Some((index, fill))) => {
                self.history.apply(
                    &mut self.doc,
                    "Unpaint",
                    vec![Edit::RemoveFill {
                        drawing,
                        index,
                        fill,
                    }],
                );
            }
            (PaintMode::Paint | PaintMode::Repaint, Some((_, fill))) => {
                if fill.swatch != swatch {
                    self.history.apply(
                        &mut self.doc,
                        "Repaint",
                        vec![Edit::SetFillSwatch {
                            drawing,
                            fill: fill.id,
                            before: fill.swatch,
                            after: swatch,
                        }],
                    );
                }
            }
            (PaintMode::PaintUnpainted, Some(_))
            | (PaintMode::Unpaint | PaintMode::Repaint, None) => {
                // Harmony leaves these alone.
            }
            (PaintMode::Paint | PaintMode::PaintUnpainted, None) => {
                // Zones are closed by the current art layer's own lines, as
                // in Harmony (Colour Art uses strokes made from Line Art).
                let strokes: Vec<&Stroke> = d.strokes.iter().filter(|st| st.art == art).collect();
                match find_zone(&strokes, x, y, cell) {
                    Ok(contours) => {
                        let fill = symphony_core::stroke::Fill {
                            id: self.doc.ids.fresh(),
                            swatch,
                            art,
                            contours,
                        };
                        self.history.apply(
                            &mut self.doc,
                            "Paint",
                            vec![Edit::AddFill { drawing, fill }],
                        );
                    }
                    Err(ZoneError::Open) => self.note(
                        "This area isn't closed. Close the gap (Close Gap tool) and paint again.",
                    ),
                    Err(ZoneError::OnLine) => self.paint_line(drawing, x, y, cell),
                }
            }
        }
    }

    /// Paint on a line recolours the line itself, as in Harmony.
    fn paint_line(&mut self, drawing: Id, x: f32, y: f32, cell: f32) {
        let swatch = self.swatch;
        let Some(d) = self.doc.drawings.get(&drawing) else {
            return;
        };
        let art = self.art;
        let Some(st) = d.strokes.iter().rev().find(|st| {
            st.art == art
                && st.kind != StrokeKind::Invisible
                && symphony_vector::hits(st, x, y, cell * 2.0)
        }) else {
            return;
        };
        if st.swatch == swatch {
            return;
        }
        if self.is_locked(st.swatch) {
            let locked = st.swatch;
            self.note_protected(locked);
            return;
        }
        let before = st.clone();
        let mut after = st.clone();
        after.swatch = swatch;
        self.history.apply(
            &mut self.doc,
            "Paint line",
            vec![Edit::SetStrokes {
                drawing,
                before: vec![before],
                after: vec![after],
            }],
        );
    }

    /// Harmony's Close Gap: from a rough stroke near a gap, join the two
    /// nearest line ends with an invisible stroke. `reach` is how far from
    /// the rough stroke to look for line ends, in drawing units.
    pub fn close_gap(&mut self, layer: usize, frame: u32, rough: &[Point], reach: f32) {
        let Some(drawing) = self.doc.exposed(layer, frame) else {
            self.note("There's no drawing here to close a gap in");
            return;
        };
        let Some(d) = self.doc.drawings.get(&drawing) else {
            return;
        };
        let near = |p: &Point| {
            rough
                .iter()
                .map(|r| (r.x - p.x).hypot(r.y - p.y))
                .fold(f32::MAX, f32::min)
        };
        // Line ends close to the rough stroke, on the current art layer.
        // Colour Art lines are invisible strokes made from Line Art, so
        // they count there.
        let art = self.art;
        let ends: Vec<(usize, Point)> = d
            .strokes
            .iter()
            .enumerate()
            .filter(|(_, st)| st.art == art)
            .filter(|(_, st)| art != ArtLayer::LineArt || st.kind != StrokeKind::Invisible)
            .flat_map(|(i, st)| {
                st.points
                    .first()
                    .into_iter()
                    .chain(st.points.last())
                    .map(move |p| (i, *p))
            })
            .filter(|(_, p)| near(p) <= reach)
            .collect();
        let mut best: Option<(f32, Point, Point)> = None;
        for a in 0..ends.len() {
            for b in a + 1..ends.len() {
                let (pa, pb) = (ends[a].1, ends[b].1);
                let dist = (pa.x - pb.x).hypot(pa.y - pb.y);
                if dist > 1e-3 && best.is_none_or(|(bd, _, _)| dist < bd) {
                    best = Some((dist, pa, pb));
                }
            }
        }
        let Some((_, a, b)) = best else {
            self.note("No line ends near that stroke. Draw it closer to the gap.");
            return;
        };
        let stroke = Stroke {
            id: self.doc.ids.fresh(),
            kind: StrokeKind::Invisible,
            art,
            swatch: self.swatch,
            min_width: 0.0,
            max_width: 0.0,
            points: vec![Point { pressure: 1.0, ..a }, Point { pressure: 1.0, ..b }],
            tips: Default::default(),
            texture: None,
            shape: Vec::new(),
        };
        self.history.apply(
            &mut self.doc,
            "Close gap",
            vec![Edit::AddStroke { drawing, stroke }],
        );
        self.note("Gap closed");
    }

    /// The Dropper: pick up the swatch under the point (a zone or a line).
    pub fn pick_colour(&mut self, layer: usize, frame: u32, x: f32, y: f32, cell: f32) {
        let Some(d) = self
            .doc
            .exposed(layer, frame)
            .and_then(|d| self.doc.drawings.get(&d))
        else {
            return;
        };
        let line = d
            .strokes
            .iter()
            .rev()
            .find(|st| {
                st.kind != StrokeKind::Invisible && symphony_vector::hits(st, x, y, cell * 2.0)
            })
            .map(|st| st.swatch);
        let zone = d
            .fills
            .iter()
            .rev()
            .find(|f| f.contains(x, y))
            .map(|f| f.swatch);
        if let Some(sw) = line.or(zone) {
            self.swatch = sw;
            let name = self
                .doc
                .palette
                .get(sw)
                .map(|w| w.name.clone())
                .unwrap_or_default();
            self.note(format!("Picked {name}"));
        }
    }

    // ---- Selection (Select tool) ------------------------------------------

    /// Drop the selection if it no longer points at the current drawing.
    pub fn validate_selection(&mut self) {
        let current = self.doc.exposed(self.selected, self.frame);
        if self.selection.drawing != current || current.is_none() {
            self.selection = Selection {
                drawing: current,
                strokes: Vec::new(),
            };
            return;
        }
        if let Some(d) = current.and_then(|d| self.doc.drawings.get(&d)) {
            self.selection
                .strokes
                .retain(|id| d.strokes.iter().any(|st| st.id == *id));
        }
    }

    pub fn selected_strokes(&self) -> Vec<&Stroke> {
        let Some(d) = self
            .selection
            .drawing
            .and_then(|d| self.doc.drawings.get(&d))
        else {
            return Vec::new();
        };
        d.strokes
            .iter()
            .filter(|st| self.selection.strokes.contains(&st.id))
            .collect()
    }

    pub fn select_all(&mut self) {
        self.validate_selection();
        if let Some(d) = self
            .selection
            .drawing
            .and_then(|d| self.doc.drawings.get(&d))
        {
            let art = self.art;
            self.selection.strokes = d
                .strokes
                .iter()
                .filter(|st| st.art == art)
                .map(|st| st.id)
                .collect();
        }
    }

    pub fn deselect(&mut self) {
        self.selection.strokes.clear();
    }

    /// Apply `m` (drawing-local) to the selected strokes as one undo step.
    pub fn transform_selection(&mut self, m: Affine, name: &str) {
        let Some(drawing) = self.selection.drawing else {
            return;
        };
        let before: Vec<Stroke> = self.selected_strokes().into_iter().cloned().collect();
        if before.is_empty() {
            return;
        }
        let after = before
            .iter()
            .map(|st| symphony_vector::transformed(st, m))
            .collect();
        self.history.apply(
            &mut self.doc,
            name,
            vec![Edit::SetStrokes {
                drawing,
                before,
                after,
            }],
        );
    }

    /// Flip the selection around its centre.
    pub fn flip_selection(&mut self, horizontal: bool) {
        let Some((lo, hi)) = symphony_vector::bounds_of(self.selected_strokes()) else {
            return;
        };
        let (cx, cy) = ((lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0);
        let (sx, sy) = if horizontal { (-1.0, 1.0) } else { (1.0, -1.0) };
        let m = Affine::translate(cx, cy)
            .compose(Affine::scale(sx, sy))
            .compose(Affine::translate(-cx, -cy));
        self.transform_selection(
            m,
            if horizontal {
                "Flip horizontal"
            } else {
                "Flip vertical"
            },
        );
    }

    pub fn delete_selection(&mut self) {
        let Some(drawing) = self.selection.drawing else {
            return;
        };
        let ids = std::mem::take(&mut self.selection.strokes);
        let Some(d) = self.doc.drawings.get(&drawing) else {
            return;
        };
        // Remove from the back so earlier indices stay valid on undo.
        let mut edits: Vec<Edit> = d
            .strokes
            .iter()
            .enumerate()
            .filter(|(_, st)| ids.contains(&st.id))
            .map(|(index, st)| Edit::RemoveStroke {
                drawing,
                index,
                stroke: st.clone(),
            })
            .collect();
        edits.reverse();
        self.history.apply(&mut self.doc, "Delete", edits);
    }

    pub fn copy_selection(&mut self) {
        self.clipboard = self.selected_strokes().into_iter().cloned().collect();
        let n = self.clipboard.len();
        if n > 0 {
            self.note(format!(
                "Copied {n} stroke{}",
                if n == 1 { "" } else { "s" }
            ));
        }
    }

    pub fn cut_selection(&mut self) {
        self.copy_selection();
        self.delete_selection();
    }

    /// Paste into the current cell (creating a drawing there if it's empty),
    /// with fresh IDs, and select what was pasted.
    pub fn paste(&mut self) {
        if self.clipboard.is_empty() {
            return;
        }
        let Some(col) = self.doc.layers.get(self.selected).map(|l| l.column) else {
            return;
        };
        let mut edits = Vec::new();
        let drawing = match self.doc.exposed(self.selected, self.frame) {
            Some(d) => d,
            None => {
                let d = self.doc.make_drawing(col);
                let id = d.id;
                edits.push(Edit::AddDrawing { drawing: d });
                edits.push(Edit::SetCells {
                    column: col,
                    from: self.frame,
                    before: vec![None],
                    after: vec![Some(id)],
                });
                id
            }
        };
        let mut ids = Vec::new();
        for st in self.clipboard.clone() {
            let mut st = st;
            if self.doc.palette.get(st.swatch).is_none() {
                st.swatch = self.swatch;
            }
            st.id = self.doc.ids.fresh();
            ids.push(st.id);
            edits.push(Edit::AddStroke {
                drawing,
                stroke: st,
            });
        }
        self.history.apply(&mut self.doc, "Paste", edits);
        self.selection = Selection {
            drawing: Some(drawing),
            strokes: ids,
        };
    }

    // ---- Transform tool (layers, through their pegs) ----------------------

    /// Change the selected layer's peg in one gesture step. In Animate mode
    /// values are keyed on the current frame; otherwise constants change.
    pub fn adjust_peg(&mut self, layer: usize, f: impl FnOnce(&mut PegValues)) {
        let Some(mut p) = self.doc.peg(layer).cloned() else {
            return;
        };
        let frame = self.frame;
        let mut v = PegValues {
            x: p.x.at(frame),
            y: p.y.at(frame),
            rotation: p.rotation.at(frame),
            scale_x: p.scale_x.as_ref().map_or(1.0, |s| s.at(frame)),
            scale_y: p.scale_y.as_ref().map_or(1.0, |s| s.at(frame)),
        };
        let old = v;
        f(&mut v);
        let animate = self.animate;
        let set = |value: &mut Value, n: f32| match value {
            Value::Constant(c) if !animate => *c = n,
            _ => value.set_key(frame, n),
        };
        if v.x != old.x {
            set(&mut p.x, v.x);
        }
        if v.y != old.y {
            set(&mut p.y, v.y);
        }
        if v.rotation != old.rotation {
            set(&mut p.rotation, v.rotation);
        }
        if v.scale_x != old.scale_x {
            set(p.scale_x.get_or_insert(Value::Constant(1.0)), v.scale_x);
        }
        if v.scale_y != old.scale_y {
            set(p.scale_y.get_or_insert(Value::Constant(1.0)), v.scale_y);
        }
        self.set_peg(layer, p);
    }

    /// Put the peg's pivot at `pivot` (drawing space) if the peg has never
    /// been rotated or scaled, so turning a drawing happens around its middle.
    pub fn ensure_pivot(&mut self, layer: usize, pivot: (f32, f32)) {
        let Some(p) = self.doc.peg(layer).cloned() else {
            return;
        };
        let untouched = p.pivot == (0.0, 0.0)
            && matches!(p.rotation, Value::Constant(r) if r == 0.0)
            && p.scale_x.is_none()
            && p.scale_y.is_none();
        if untouched {
            let mut p = p;
            p.pivot = pivot;
            self.set_peg(layer, p);
        }
    }

    // ---- Timing ----------------------------------------------------------

    /// Hold the current drawing one frame longer (+1) or shorter (-1),
    /// shifting everything after it.
    pub fn change_exposure(&mut self, delta: i32) {
        let Some(col) = self.doc.layers.get(self.selected).map(|l| l.column) else {
            return;
        };
        let Some(d) = self.doc.exposed(self.selected, self.frame) else {
            self.note("There's no drawing in this cell to extend");
            return;
        };
        if delta > 0 {
            if let Some(e) = Edit::splice(&self.doc, col, self.frame, 0, vec![Some(d)]) {
                self.history
                    .apply(&mut self.doc, "Increase exposure", vec![e]);
            }
        } else {
            let column = self.doc.xsheet.column(col);
            let run = column
                .map(|c| c.run_end(self.frame) - c.run_start(self.frame) + 1)
                .unwrap_or(1);
            if run <= 1 {
                self.note("This drawing is only exposed for one frame");
                return;
            }
            if let Some(e) = Edit::splice(&self.doc, col, self.frame, 1, vec![]) {
                self.history
                    .apply(&mut self.doc, "Decrease exposure", vec![e]);
            }
        }
    }

    /// The selected layer's drawings (its element), in Harmony's order:
    /// by name, with numbers compared as numbers ("2" before "10").
    pub fn element_drawings(&self, layer: usize) -> Vec<(Id, String)> {
        let Some(col) = self.doc.layers.get(layer).map(|l| l.column) else {
            return Vec::new();
        };
        let mut list: Vec<(Id, String)> = self
            .doc
            .drawings
            .values()
            .filter(|d| d.column == col)
            .map(|d| (d.id, d.name.clone()))
            .collect();
        list.sort_by(|a, b| natural(&a.1, &b.1));
        list
    }

    /// Harmony's drawing substitution: expose `drawing` from the current
    /// frame to the end of the current exposure (splitting it if the frame
    /// is mid-way through).
    pub fn swap_drawing(&mut self, layer: usize, frame: u32, drawing: Id) {
        let Some(col_id) = self.doc.layers.get(layer).map(|l| l.column) else {
            return;
        };
        let Some(col) = self.doc.xsheet.column(col_id) else {
            return;
        };
        let Some(current) = col.get(frame) else {
            self.note("There's no drawing in this cell to swap");
            return;
        };
        if current == drawing {
            return;
        }
        let end = col.run_end(frame);
        let before: Vec<Option<Id>> = (frame..=end).map(|f| col.get(f)).collect();
        let after = vec![Some(drawing); before.len()];
        self.history.apply_merged(
            &mut self.doc,
            "Swap drawing",
            &format!("swap-{}-{frame}", col_id.0),
            Edit::SetCells {
                column: col_id,
                from: frame,
                before,
                after,
            },
        );
    }

    /// Substitute Drawing Next (]) or Previous ([).
    pub fn substitute(&mut self, delta: i32) {
        let (layer, frame) = (self.selected, self.frame);
        let Some(current) = self.doc.exposed(layer, frame) else {
            self.note("There's no drawing in this cell to swap");
            return;
        };
        let list = self.element_drawings(layer);
        let Some(i) = list.iter().position(|(id, _)| *id == current) else {
            return;
        };
        let Some((next, name)) = i
            .checked_add_signed(delta as isize)
            .and_then(|j| list.get(j))
            .cloned()
        else {
            self.note(if delta > 0 {
                "This is the layer's last drawing"
            } else {
                "This is the layer's first drawing"
            });
            return;
        };
        self.swap_drawing(layer, frame, next);
        self.end_gesture();
        self.note(format!("Drawing {name}"));
    }

    /// Go to Next Drawing (G) or Previous Drawing (F): jump to the start of
    /// the next or previous exposure on the selected layer.
    pub fn go_to_drawing(&mut self, forward: bool) {
        let Some(col) = self.doc.column(self.selected) else {
            return;
        };
        let start = col.run_start(self.frame);
        let target = if forward {
            let mut f = col.run_end(self.frame) + 1;
            while f <= col.len() && col.get(f).is_none() {
                f += 1;
            }
            (f <= col.len()).then_some(f)
        } else {
            let mut f = start.saturating_sub(1);
            while f >= 1 && col.get(f).is_none() {
                f -= 1;
            }
            (f >= 1).then(|| col.run_start(f))
        };
        match target {
            Some(f) => self.frame = f,
            None => self.note(if forward {
                "No more drawings after this one"
            } else {
                "No drawings before this one"
            }),
        }
    }

    pub fn set_frame_rate(&mut self, fps: f32) {
        let before = self.doc.settings.fps;
        if before != fps {
            self.history.apply(
                &mut self.doc,
                "Change frame rate",
                vec![Edit::SetFrameRate { before, after: fps }],
            );
        }
    }

    /// Make room for new poses after the playhead, holding each visible
    /// exposure until the animator creates a new drawing in an inserted cell.
    pub fn insert_inbetweens(&mut self, count: u32) {
        let before = self.length();
        if count == 0 || count > 100_000u32.saturating_sub(before) {
            return;
        }
        let from = self.frame + 1;
        let mut edits = vec![Edit::SetLength {
            before,
            after: before + count,
        }];
        for column in &self.doc.xsheet.columns {
            let held = column.get(self.frame);
            if held.is_some() || column.len() >= from {
                edits.extend(Edit::splice(
                    &self.doc,
                    column.id,
                    from,
                    0,
                    vec![held; count as usize],
                ));
            }
        }
        for (id, node) in self.doc.graph.nodes() {
            let NodeKind::Peg(before) = &node.kind else {
                continue;
            };
            let mut after = before.clone();
            for value in [&mut after.x, &mut after.y, &mut after.rotation]
                .into_iter()
                .chain(after.scale_x.iter_mut())
                .chain(after.scale_y.iter_mut())
            {
                if let Value::Keys(keys) = value {
                    for key in keys.iter_mut().filter(|key| key.frame >= from) {
                        key.frame = key.frame.saturating_add(count);
                    }
                }
            }
            if &after != before {
                edits.push(Edit::SetPeg {
                    node: id,
                    before: before.clone(),
                    after,
                });
            }
        }
        self.history
            .apply(&mut self.doc, "Insert in-betweens", edits);
        self.frame = from;
    }

    pub fn set_length(&mut self, length: u32) {
        let before = self.doc.settings.length;
        if before != length {
            self.history.apply_merged(
                &mut self.doc,
                "Change scene length",
                "length",
                Edit::SetLength {
                    before,
                    after: length,
                },
            );
        }
    }

    /// Distinct drawings before and after the current one on the selected
    /// layer, nearest first, for onion skin.
    pub fn onion_drawings(&self) -> (Vec<Id>, Vec<Id>) {
        let Some(col) = self.doc.column(self.selected) else {
            return (vec![], vec![]);
        };
        let current = col.get(self.frame);
        let walk = |frames: &mut dyn Iterator<Item = u32>, n: u32| {
            let mut out: Vec<Id> = Vec::new();
            for f in frames {
                if let Some(d) = col.get(f) {
                    if Some(d) != current && !out.contains(&d) {
                        out.push(d);
                        if out.len() as u32 >= n {
                            break;
                        }
                    }
                }
            }
            out
        };
        let before = walk(&mut (1..self.frame).rev(), self.onion.before);
        let after = walk(&mut (self.frame + 1..=self.length()), self.onion.after);
        (before, after)
    }

    /// Add or remove one earlier (`before`) or later drawing from onion skin.
    pub fn onion_step(&mut self, before: bool, delta: i32) {
        let n = if before {
            &mut self.onion.before
        } else {
            &mut self.onion.after
        };
        *n = (*n as i32 + delta).clamp(0, 9) as u32;
        self.onion.on = true;
    }

    /// Harmony's "Onion Skin: N Previous/Next" shortcuts set a count and show it.
    pub fn onion_set(&mut self, before: bool, n: u32) {
        if before {
            self.onion.before = n;
        } else {
            self.onion.after = n;
        }
        self.onion.on = true;
    }

    // ---- Layers ----------------------------------------------------------

    fn add_layer_named(&mut self, name: &str, index: usize) {
        let snap = self.doc.make_layer(name, index);
        self.history.apply(
            &mut self.doc,
            "Add layer",
            vec![Edit::AddLayer(Box::new(snap))],
        );
    }

    /// Add a layer above the selected one, like Harmony's Add Drawing Layer.
    pub fn add_layer(&mut self) {
        let n = (1..)
            .find(|n| (0..self.layer_count()).all(|i| self.layer_name(i) != format!("Drawing {n}")))
            .unwrap_or(1);
        let index = self.selected.min(self.layer_count());
        self.add_layer_named(&format!("Drawing {n}"), index);
        self.selected = index;
    }

    pub fn delete_layer(&mut self, i: usize) {
        if self.layer_count() <= 1 {
            self.note("A scene needs at least one layer");
            return;
        }
        if let Some(snap) = self.doc.snapshot_layer(i) {
            let name = self.layer_name(i).to_owned();
            self.history.apply(
                &mut self.doc,
                "Delete layer",
                vec![Edit::RemoveLayer(Box::new(snap))],
            );
            self.clamp_selection();
            self.note(format!("Deleted {name}. Undo brings it back."));
        }
    }

    pub fn rename_layer(&mut self, i: usize, name: &str) {
        let name = name.trim();
        let Some(col) = self.doc.column(i) else {
            return;
        };
        if name.is_empty() || name == col.name {
            return;
        }
        let (column, before) = (col.id, col.name.clone());
        self.history.apply(
            &mut self.doc,
            "Rename layer",
            vec![Edit::Rename {
                column,
                before,
                after: name.into(),
            }],
        );
    }

    pub fn toggle_visible(&mut self, i: usize) {
        let Some(col) = self.doc.column(i) else {
            return;
        };
        let (column, before) = (col.id, col.visible);
        let name = if before { "Hide layer" } else { "Show layer" };
        self.history.apply(
            &mut self.doc,
            name,
            vec![Edit::SetVisible {
                column,
                before,
                after: !before,
            }],
        );
    }

    // ---- Pegs and colour -------------------------------------------------

    /// Set a peg's parameters. Repeated calls during one drag merge.
    pub fn set_peg(&mut self, layer: usize, after: PegParams) {
        let Some(l) = self.doc.layers.get(layer).copied() else {
            return;
        };
        let Some(before) = self.doc.peg(layer).cloned() else {
            return;
        };
        if before != after {
            self.history.apply_merged(
                &mut self.doc,
                "Move peg",
                &format!("peg-{}", l.peg.0),
                Edit::SetPeg {
                    node: l.peg,
                    before,
                    after,
                },
            );
        }
    }

    /// Change one peg value at the current frame: a key in Animate mode,
    /// otherwise the constant value.
    pub fn set_peg_value(&mut self, layer: usize, pick: fn(&mut PegParams) -> &mut Value, v: f32) {
        let Some(mut p) = self.doc.peg(layer).cloned() else {
            return;
        };
        let value = pick(&mut p);
        match value {
            Value::Constant(c) if !self.animate => *c = v,
            _ => value.set_key(self.frame, v),
        }
        self.set_peg(layer, p);
    }

    fn apply_palette(
        &mut self,
        name: &str,
        palette: symphony_core::palette::Palette,
        mut edits: Vec<Edit>,
    ) {
        edits.push(Edit::SetPalette {
            before: self.doc.palette.clone(),
            after: palette,
        });
        self.history.apply(&mut self.doc, name, edits);
    }

    pub fn add_swatch(&mut self) {
        let rgb = self.doc.palette.get(self.swatch).map_or([0; 3], |s| s.rgb);
        let id = self.doc.ids.fresh();
        let mut palette = self.doc.palette.clone();
        palette.swatches.push(symphony_core::palette::Swatch {
            id,
            name: "New colour".into(),
            rgb,
            protected: false,
        });
        self.apply_palette("Add colour", palette, Vec::new());
        self.swatch = id;
    }

    pub fn rename_swatch(&mut self, id: Id, name: &str) {
        let mut palette = self.doc.palette.clone();
        let Some(swatch) = palette.get_mut(id) else {
            return;
        };
        let name = name.trim();
        if name.is_empty() || swatch.name == name {
            return;
        }
        swatch.name = name.into();
        self.apply_palette("Rename colour", palette, Vec::new());
    }

    pub fn move_swatch(&mut self, id: Id, delta: isize) {
        let mut palette = self.doc.palette.clone();
        let Some(index) = palette.swatches.iter().position(|sw| sw.id == id) else {
            return;
        };
        let Some(next) = index
            .checked_add_signed(delta)
            .filter(|i| *i < palette.swatches.len())
        else {
            return;
        };
        palette.swatches.swap(index, next);
        self.apply_palette("Move colour", palette, Vec::new());
    }

    pub fn delete_swatch(&mut self, id: Id, replacement: Id) {
        let mut palette = self.doc.palette.clone();
        if id == replacement || palette.get(id).is_none() || palette.get(replacement).is_none() {
            return;
        }
        palette.swatches.retain(|sw| sw.id != id);
        let mut edits = Vec::new();
        for (&drawing, d) in &self.doc.drawings {
            for st in d.strokes.iter().filter(|s| s.swatch == id) {
                edits.push(Edit::SetStrokeSwatch {
                    drawing,
                    stroke: st.id,
                    before: id,
                    after: replacement,
                });
            }
            for fill in d.fills.iter().filter(|f| f.swatch == id) {
                edits.push(Edit::SetFillSwatch {
                    drawing,
                    fill: fill.id,
                    before: id,
                    after: replacement,
                });
            }
        }
        self.apply_palette("Remove colour", palette, edits);
        if self.swatch == id {
            self.swatch = replacement;
        }
    }

    /// Harmony's L: switch between Line Art and Colour Art.
    pub fn switch_line_colour_art(&mut self) {
        self.art = match self.art {
            ArtLayer::LineArt => ArtLayer::ColourArt,
            _ => ArtLayer::LineArt,
        };
        self.after_art_change();
    }

    /// Harmony's colon: switch between Overlay and Underlay (when enabled).
    pub fn switch_overlay_underlay(&mut self) {
        if !self.overlay_underlay {
            self.note("Overlay and Underlay are off. Turn on Support Overlay and Underlay Arts in Preferences.");
            return;
        }
        self.art = match self.art {
            ArtLayer::Overlay => ArtLayer::Underlay,
            _ => ArtLayer::Overlay,
        };
        self.after_art_change();
    }

    pub fn set_art(&mut self, art: ArtLayer) {
        if self.art != art {
            self.art = art;
            self.after_art_change();
        }
    }

    /// Tools work on one art layer, so a selection on another is dropped.
    fn after_art_change(&mut self) {
        self.selection.strokes.clear();
        self.note(format!("Drawing on {}", art_name(self.art)));
    }

    /// Harmony's Create Colour Art from Line Art (*): copy the selected
    /// Line Art lines, or all of them, into Colour Art as invisible strokes
    /// that close paint zones there. One undo step.
    pub fn create_colour_art(&mut self, layer: usize, frame: u32) {
        let Some(drawing) = self.doc.exposed(layer, frame) else {
            self.note("There's no drawing here");
            return;
        };
        let Some(d) = self.doc.drawings.get(&drawing) else {
            return;
        };
        let chosen: Vec<Stroke> = d
            .strokes
            .iter()
            .filter(|st| st.art == ArtLayer::LineArt)
            .filter(|st| {
                self.selection.drawing != Some(drawing)
                    || self.selection.strokes.is_empty()
                    || self.selection.strokes.contains(&st.id)
            })
            .cloned()
            .collect();
        if chosen.is_empty() {
            self.note("There's no Line Art to create Colour Art from");
            return;
        }
        let n = chosen.len();
        let mut edits = Vec::with_capacity(n);
        for mut st in chosen {
            st.id = self.doc.ids.fresh();
            st.art = ArtLayer::ColourArt;
            st.kind = StrokeKind::Invisible;
            st.texture = None;
            edits.push(Edit::AddStroke {
                drawing,
                stroke: st,
            });
        }
        self.history
            .apply(&mut self.doc, "Create Colour Art from Line Art", edits);
        self.note(format!(
            "Created {n} Colour Art stroke{} from Line Art",
            if n == 1 { "" } else { "s" }
        ));
    }

    /// The thickness profile of the one selected pencil line, for Harmony's
    /// New Thickness Stencil.
    pub fn selected_stencil(&self) -> Option<Stencil> {
        let [id] = self.selection.strokes.as_slice() else {
            return None;
        };
        let d = self.doc.drawings.get(&self.selection.drawing?)?;
        let st = d.strokes.iter().find(|st| st.id == *id)?;
        (st.kind == StrokeKind::Pencil)
            .then(|| stencil_from(st))
            .flatten()
    }

    /// Does painting have to leave this swatch's zones and lines alone?
    pub fn is_locked(&self, swatch: Id) -> bool {
        self.respect_protected && self.doc.palette.get(swatch).is_some_and(|sw| sw.protected)
    }

    fn note_protected(&mut self, swatch: Id) {
        let name = self
            .doc
            .palette
            .get(swatch)
            .map_or_else(String::new, |sw| sw.name.clone());
        self.note(format!(
            "{name} is protected. Unprotect it in the Colour view, or turn off Respect Protected Colours (Shift+S)."
        ));
    }

    /// Harmony's Protect Colour, from the Colour view. Undoable, like any
    /// palette change.
    pub fn toggle_protect_swatch(&mut self, id: Id) {
        let mut palette = self.doc.palette.clone();
        let Some(sw) = palette.get_mut(id) else {
            return;
        };
        sw.protected = !sw.protected;
        let name = if sw.protected {
            "Protect colour"
        } else {
            "Unprotect colour"
        };
        self.apply_palette(name, palette, Vec::new());
    }

    pub fn toggle_respect_protected(&mut self) {
        self.respect_protected = !self.respect_protected;
        self.note(if self.respect_protected {
            "Respecting protected colours"
        } else {
            "Protected colours can be painted over"
        });
    }

    /// Recolour a swatch. Every stroke using it changes (E2.3).
    pub fn set_swatch(&mut self, swatch: Id, rgb: [u8; 3]) {
        let before = self.doc.swatch_rgb(swatch);
        if before != rgb {
            self.history.apply_merged(
                &mut self.doc,
                "Edit swatch",
                &format!("swatch-{}", swatch.0),
                Edit::SetSwatch {
                    swatch,
                    before,
                    after: rgb,
                },
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inserted_inbetweens_hold_exposures_and_move_keys_at_any_rate() {
        let mut s = Session::new_scene();
        let first = line(&mut s, 0.0, 10.0);
        let second = line(&mut s, 10.0, 20.0);
        s.commit_stroke(0, 1, first);
        s.commit_stroke(0, 2, second);
        let a = s.doc.exposed(0, 1).unwrap();
        let b = s.doc.exposed(0, 2).unwrap();
        let mut peg = s.doc.peg(0).unwrap().clone();
        peg.x.set_key(1, 0.0);
        peg.x.set_key(2, 20.0);
        s.set_peg(0, peg);
        s.set_frame_rate(12.0);
        s.insert_inbetweens(3);
        assert_eq!(s.doc.settings.fps, 12.0);
        assert_eq!(s.length(), 51);
        assert_eq!(s.frame, 2);
        assert_eq!(s.doc.exposed(0, 4), Some(a));
        assert_eq!(s.doc.exposed(0, 5), Some(b));
        assert_eq!(s.doc.peg(0).unwrap().x.at(3), 10.0);
        s.undo();
        assert_eq!(s.length(), 48);
        assert_eq!(s.doc.exposed(0, 2), Some(b));
        assert_eq!(s.doc.peg(0).unwrap().x.at(2), 20.0);
        s.undo();
        assert_eq!(s.doc.settings.fps, 24.0);
    }

    fn line(s: &mut Session, x0: f32, x1: f32) -> Stroke {
        let swatch = s.swatch;
        Stroke {
            id: s.doc.ids.fresh(),
            kind: StrokeKind::Brush,
            art: Default::default(),
            swatch,
            min_width: 2.0,
            max_width: 2.0,
            points: vec![
                Point {
                    x: x0,
                    y: 0.0,
                    pressure: 1.0,
                },
                Point {
                    x: x1,
                    y: 0.0,
                    pressure: 1.0,
                },
            ],
            tips: Default::default(),
            texture: None,
            shape: Vec::new(),
        }
    }

    #[test]
    fn flip_selection_undoes_in_one_step() {
        let mut s = Session::new_scene();
        let st = line(&mut s, 0.0, 10.0);
        s.commit_stroke(0, 1, st);
        s.select_all();
        assert_eq!(s.selection.strokes.len(), 1);
        s.flip_selection(true);
        let d = s.doc.exposed(0, 1).unwrap();
        assert_eq!(s.doc.drawings[&d].strokes[0].points[0].x, 10.0);
        s.undo();
        assert_eq!(s.doc.drawings[&d].strokes[0].points[0].x, 0.0);
    }

    #[test]
    fn copy_paste_into_empty_cell_creates_a_drawing() {
        let mut s = Session::new_scene();
        let st = line(&mut s, 0.0, 10.0);
        s.commit_stroke(0, 1, st);
        s.select_all();
        s.copy_selection();
        s.frame = 2;
        s.paste();
        let d2 = s.doc.exposed(0, 2).expect("paste created a drawing");
        assert_ne!(Some(d2), s.doc.exposed(0, 1));
        assert_eq!(s.doc.drawings[&d2].strokes.len(), 1);
        // Pasted strokes get fresh IDs and are selected.
        assert_eq!(s.selection.strokes, vec![s.doc.drawings[&d2].strokes[0].id]);
        assert_ne!(s.doc.drawings[&d2].strokes[0].id, s.clipboard[0].id);
    }

    #[test]
    fn delete_selection_restores_order_on_undo() {
        let mut s = Session::new_scene();
        for x in [0.0, 20.0, 40.0] {
            let st = line(&mut s, x, x + 5.0);
            s.commit_stroke(0, 1, st);
        }
        let d = s.doc.exposed(0, 1).unwrap();
        let before: Vec<Id> = s.doc.drawings[&d].strokes.iter().map(|st| st.id).collect();
        s.validate_selection();
        s.selection.strokes = vec![before[0], before[2]];
        s.delete_selection();
        assert_eq!(s.doc.drawings[&d].strokes.len(), 1);
        s.undo();
        let after: Vec<Id> = s.doc.drawings[&d].strokes.iter().map(|st| st.id).collect();
        assert_eq!(after, before);
    }

    fn square_drawing(s: &mut Session) {
        let pts = [
            (10.0, 10.0),
            (90.0, 10.0),
            (90.0, 90.0),
            (10.0, 90.0),
            (10.0, 10.0),
        ];
        let swatch = s.swatch;
        let st = Stroke {
            id: s.doc.ids.fresh(),
            kind: StrokeKind::Brush,
            art: Default::default(),
            swatch,
            min_width: 2.0,
            max_width: 2.0,
            points: pts
                .iter()
                .map(|&(x, y)| Point {
                    x,
                    y,
                    pressure: 1.0,
                })
                .collect(),
            tips: Default::default(),
            texture: None,
            shape: Vec::new(),
        };
        s.commit_stroke(0, 1, st);
    }

    fn fills(s: &Session) -> Vec<symphony_core::stroke::Fill> {
        let d = s.doc.exposed(0, 1).unwrap();
        s.doc.drawings[&d].fills.clone()
    }

    #[test]
    fn palette_edits_preserve_and_restore_art_references() {
        let mut s = Session::new_scene();
        let original = s.swatch;
        square_drawing(&mut s);
        s.paint_mode = PaintMode::Paint;
        s.paint_at(0, 1, 50.0, 50.0, 1.0);
        s.add_swatch();
        let replacement = s.swatch;
        s.rename_swatch(replacement, "Shadow ink");
        s.move_swatch(replacement, -1);
        let before = s.doc.palette.clone();
        s.delete_swatch(original, replacement);
        let d = s.doc.exposed(0, 1).unwrap();
        assert_eq!(s.doc.drawings[&d].strokes[0].swatch, replacement);
        assert_eq!(fills(&s)[0].swatch, replacement);
        assert!(s.doc.palette.get(original).is_none());
        s.undo();
        assert_eq!(s.doc.palette, before);
        assert_eq!(s.doc.drawings[&d].strokes[0].swatch, original);
        assert_eq!(fills(&s)[0].swatch, original);
        s.redo();
        assert_eq!(fills(&s)[0].swatch, replacement);
    }

    #[test]
    fn paint_modes_follow_harmony() {
        let mut s = Session::new_scene();
        square_drawing(&mut s);
        let (a, b) = (s.doc.palette.swatches[1].id, s.doc.palette.swatches[2].id);

        s.paint_mode = PaintMode::Repaint;
        s.swatch = a;
        s.paint_at(0, 1, 50.0, 50.0, 1.0);
        assert!(fills(&s).is_empty(), "Repaint leaves empty zones alone");

        s.paint_mode = PaintMode::Paint;
        s.paint_at(0, 1, 50.0, 50.0, 1.0);
        assert_eq!(fills(&s).len(), 1);
        assert_eq!(fills(&s)[0].swatch, a);

        s.paint_mode = PaintMode::PaintUnpainted;
        s.swatch = b;
        s.paint_at(0, 1, 50.0, 50.0, 1.0);
        assert_eq!(
            fills(&s)[0].swatch,
            a,
            "Paint Unpainted leaves painted zones alone"
        );

        s.paint_mode = PaintMode::Repaint;
        s.paint_at(0, 1, 50.0, 50.0, 1.0);
        assert_eq!(fills(&s)[0].swatch, b);
        s.undo();
        assert_eq!(fills(&s)[0].swatch, a, "repaint undoes");

        s.paint_mode = PaintMode::Unpaint;
        s.paint_at(0, 1, 50.0, 50.0, 1.0);
        assert!(fills(&s).is_empty());
    }

    #[test]
    fn thickness_stencils_ignore_pressure_and_round_trip() {
        let line = |pressure: fn(f32) -> f32| Stroke {
            id: Id(1),
            kind: StrokeKind::Pencil,
            art: Default::default(),
            swatch: Id(0),
            min_width: 1.0,
            max_width: 5.0,
            points: (0..=20)
                .map(|i| {
                    let t = i as f32 / 20.0;
                    Point {
                        x: t * 100.0,
                        y: 0.0,
                        pressure: pressure(t),
                    }
                })
                .collect(),
            tips: Default::default(),
            texture: None,
            shape: Vec::new(),
        };
        // A line thick in the middle gives a stencil thick in the middle.
        let swell = stencil_from(&line(|t| (t * std::f32::consts::PI).sin())).unwrap();
        assert!(swell[0] < 0.05 && swell[STENCIL_SAMPLES - 1] < 0.05);
        assert!(swell[STENCIL_SAMPLES / 2] > 0.9);
        // Applying it to a flat-pressure line copies the profile.
        let mut flat = line(|_| 1.0);
        apply_stencil(&mut flat.points, &swell);
        assert!(flat.points[0].pressure < 0.05);
        assert!(flat.points[10].pressure > 0.9);
        assert!(stencil_from(&Stroke {
            points: vec![],
            ..line(|_| 1.0)
        })
        .is_none());
    }

    #[test]
    fn substitution_swaps_to_the_end_of_the_exposure() {
        let mut s = Session::new_scene();
        square_drawing(&mut s); // drawing 1 on frame 1
        let one = s.doc.exposed(0, 1).unwrap();
        // Hold drawing 1 for frames 1-4, then add drawings 2 and 10 to the element.
        for _ in 0..3 {
            s.change_exposure(1);
        }
        let col = s.doc.layers[0].column;
        let two = s.doc.make_drawing(col);
        let mut ten = s.doc.make_drawing(col);
        ten.name = "10".into();
        let (two_id, ten_id) = (two.id, ten.id);
        s.history.apply(
            &mut s.doc,
            "Add",
            vec![
                Edit::AddDrawing { drawing: two },
                Edit::AddDrawing { drawing: ten },
            ],
        );
        let names: Vec<String> = s.element_drawings(0).into_iter().map(|d| d.1).collect();
        assert_eq!(names, ["1", "2", "10"], "numbers sort as numbers");

        // ] on frame 3 swaps frames 3-4 only, splitting the exposure.
        s.frame = 3;
        s.substitute(1);
        let cells: Vec<_> = (1..=4).map(|f| s.doc.exposed(0, f)).collect();
        assert_eq!(cells, [Some(one), Some(one), Some(two_id), Some(two_id)]);
        s.substitute(1);
        assert_eq!(s.doc.exposed(0, 4), Some(ten_id));
        s.substitute(1);
        assert_eq!(
            s.doc.exposed(0, 4),
            Some(ten_id),
            "stops at the last drawing"
        );
        s.undo();
        assert_eq!(s.doc.exposed(0, 3), Some(two_id));

        // G and F jump between exposures.
        s.frame = 1;
        s.go_to_drawing(true);
        assert_eq!(s.frame, 3);
        s.go_to_drawing(false);
        assert_eq!(s.frame, 1);
    }

    #[test]
    fn eraser_cuts_lines_and_one_drag_undoes_at_once() {
        let mut s = Session::new_scene();
        square_drawing(&mut s); // one closed brush stroke: a ring
        let d = s.doc.exposed(0, 1).unwrap();
        let original = s.doc.drawings[&d].strokes.clone();
        let strokes = |s: &Session| s.doc.drawings[&d].strokes.clone();
        // Through the top edge: the ring opens into one C-shaped outline.
        s.erase(d, &[([50.0, 10.0], 5.0)]);
        assert_eq!(strokes(&s).len(), 1);
        assert!(!strokes(&s)[0].shape.is_empty(), "now an outline");
        assert!(!symphony_vector::hits(&strokes(&s)[0], 50.0, 10.0, 0.0));
        assert!(symphony_vector::hits(&strokes(&s)[0], 30.0, 10.0, 0.0));
        // Through the bottom edge too: two separate pieces.
        s.erase(d, &[([50.0, 90.0], 5.0)]);
        assert_eq!(strokes(&s).len(), 2);
        s.end_gesture();
        // Other art layers are untouched by the eraser.
        s.art = ArtLayer::ColourArt;
        s.erase(d, &[([10.0, 50.0], 5.0)]);
        assert_eq!(strokes(&s).len(), 2);
        s.undo();
        assert_eq!(strokes(&s), original, "one drag, one undo");
    }

    #[test]
    fn pencil_lines_are_cut_along_their_centreline() {
        let mut s = Session::new_scene();
        let swatch = s.swatch;
        let st = Stroke {
            id: s.doc.ids.fresh(),
            kind: StrokeKind::Pencil,
            art: Default::default(),
            swatch,
            min_width: 2.0,
            max_width: 2.0,
            points: [(0.0, 0.0), (100.0, 0.0)]
                .iter()
                .map(|&(x, y)| Point {
                    x,
                    y,
                    pressure: 1.0,
                })
                .collect(),
            tips: Default::default(),
            texture: None,
            shape: Vec::new(),
        };
        s.commit_stroke(0, 1, st);
        let d = s.doc.exposed(0, 1).unwrap();
        s.eraser_tip = symphony_core::stroke::Tip::Flat;
        s.erase(d, &[([50.0, 0.0], 10.0)]);
        let pieces = &s.doc.drawings[&d].strokes;
        assert_eq!(pieces.len(), 2);
        assert!(
            pieces.iter().all(|p| p.shape.is_empty()),
            "still centrelines"
        );
        assert_eq!(pieces[0].tips.end, symphony_core::stroke::Tip::Flat);
    }

    #[test]
    fn eraser_removes_paint_and_can_reach_every_art_layer() {
        let mut s = Session::new_scene();
        square_drawing(&mut s);
        s.paint_mode = PaintMode::Paint;
        s.swatch = s.doc.palette.swatches[1].id;
        s.paint_at(0, 1, 50.0, 50.0, 1.0);
        let d = s.doc.exposed(0, 1).unwrap();
        let area = |s: &Session| -> f32 {
            s.doc.drawings[&d]
                .fills
                .iter()
                .map(|f| symphony_vector::shape::shape_area(&f.contours))
                .sum()
        };
        let before = area(&s);
        s.erase(d, &[([50.0, 50.0], 10.0)]);
        let after = area(&s);
        let hole = std::f32::consts::PI * 100.0;
        assert!(
            (before - after - hole).abs() < hole * 0.05,
            "{before} -> {after}"
        );
        assert!(!s.doc.drawings[&d].fills[0].contains(50.0, 50.0));
        s.end_gesture();

        // Paint on Colour Art isn't touched from Line Art...
        s.undo();
        s.doc.drawings.get_mut(&d).unwrap().fills[0].art = ArtLayer::ColourArt;
        s.erase(d, &[([50.0, 50.0], 10.0)]);
        assert!(s.doc.drawings[&d].fills[0].contains(50.0, 50.0));
        // ...unless the eraser applies to all art layers.
        s.erase_all_art = true;
        s.erase(d, &[([50.0, 50.0], 10.0)]);
        assert!(!s.doc.drawings[&d].fills[0].contains(50.0, 50.0));
    }

    #[test]
    fn art_layers_keep_line_and_colour_apart() {
        let mut s = Session::new_scene();
        square_drawing(&mut s);
        let fill_swatch = s.doc.palette.swatches[1].id;
        s.swatch = fill_swatch;
        s.paint_mode = PaintMode::Paint;

        // Colour Art has no lines yet, so there's no zone to paint.
        s.switch_line_colour_art();
        assert_eq!(s.art, ArtLayer::ColourArt);
        s.paint_at(0, 1, 50.0, 50.0, 1.0);
        assert!(fills(&s).is_empty());

        // Create Colour Art from Line Art, then paint: the fill is Colour Art.
        s.create_colour_art(0, 1);
        let d = s.doc.exposed(0, 1).unwrap();
        let colour_lines: Vec<_> = s.doc.drawings[&d]
            .strokes
            .iter()
            .filter(|st| st.art == ArtLayer::ColourArt)
            .collect();
        assert_eq!(colour_lines.len(), 1);
        assert_eq!(colour_lines[0].kind, StrokeKind::Invisible);
        s.paint_at(0, 1, 50.0, 50.0, 1.0);
        assert_eq!(fills(&s).len(), 1);
        assert_eq!(fills(&s)[0].art, ArtLayer::ColourArt);

        // Tools only see the current art layer.
        s.select_all();
        assert_eq!(s.selection.strokes.len(), 1);
        s.switch_line_colour_art();
        assert_eq!(s.art, ArtLayer::LineArt);
        assert!(
            s.selection.strokes.is_empty(),
            "switching drops the selection"
        );
        s.paint_at(0, 1, 50.0, 50.0, 1.0);
        assert_eq!(fills(&s).len(), 2, "Line Art gets its own zone");

        // One undo removes the whole Create Colour Art step.
        s.undo();
        s.undo();
        s.undo();
        assert!(s.doc.drawings[&d]
            .strokes
            .iter()
            .all(|st| st.art == ArtLayer::LineArt));

        // Overlay and Underlay need the preference.
        s.switch_overlay_underlay();
        assert_eq!(s.art, ArtLayer::LineArt);
        s.overlay_underlay = true;
        s.switch_overlay_underlay();
        assert_eq!(s.art, ArtLayer::Overlay);
    }

    #[test]
    fn protected_colours_are_left_alone_while_respected() {
        let mut s = Session::new_scene();
        let ink = s.swatch;
        square_drawing(&mut s);
        let (a, b) = (s.doc.palette.swatches[1].id, s.doc.palette.swatches[2].id);
        s.paint_mode = PaintMode::Paint;
        s.swatch = a;
        s.paint_at(0, 1, 50.0, 50.0, 1.0);
        s.toggle_protect_swatch(a);
        s.toggle_protect_swatch(ink);
        assert!(s.doc.palette.get(a).unwrap().protected);

        s.swatch = b;
        s.paint_at(0, 1, 50.0, 50.0, 1.0);
        assert_eq!(fills(&s)[0].swatch, a, "Paint skips a protected zone");
        s.paint_mode = PaintMode::Unpaint;
        s.paint_at(0, 1, 50.0, 50.0, 1.0);
        assert_eq!(fills(&s).len(), 1, "Unpaint skips a protected zone");
        s.paint_mode = PaintMode::Paint;
        s.paint_at(0, 1, 10.0, 50.0, 1.0);
        let d = s.doc.exposed(0, 1).unwrap();
        assert_eq!(s.doc.drawings[&d].strokes[0].swatch, ink, "protected line");

        s.toggle_respect_protected();
        s.paint_at(0, 1, 50.0, 50.0, 1.0);
        assert_eq!(fills(&s)[0].swatch, b, "not respected: repaints");

        s.undo();
        s.undo();
        assert!(!s.doc.palette.get(ink).unwrap().protected, "protect undoes");
    }

    #[test]
    fn close_gap_makes_an_open_shape_paintable() {
        let mut s = Session::new_scene();
        let swatch = s.swatch;
        let open = Stroke {
            id: s.doc.ids.fresh(),
            kind: StrokeKind::Brush,
            art: Default::default(),
            swatch,
            min_width: 2.0,
            max_width: 2.0,
            points: [(10.0, 10.0), (90.0, 10.0), (90.0, 90.0), (10.0, 90.0)]
                .iter()
                .map(|&(x, y)| Point {
                    x,
                    y,
                    pressure: 1.0,
                })
                .collect(),
            tips: Default::default(),
            texture: None,
            shape: Vec::new(),
        };
        s.commit_stroke(0, 1, open);
        s.paint_at(0, 1, 50.0, 50.0, 1.0);
        assert!(fills(&s).is_empty(), "an open shape can't be painted");
        // A rough stroke near the gap on the left side.
        let rough = [
            Point {
                x: 14.0,
                y: 20.0,
                pressure: 1.0,
            },
            Point {
                x: 14.0,
                y: 80.0,
                pressure: 1.0,
            },
        ];
        s.close_gap(0, 1, &rough, 40.0);
        s.paint_at(0, 1, 50.0, 50.0, 1.0);
        assert_eq!(fills(&s).len(), 1, "after Close Gap it can");
    }

    #[test]
    fn transform_tool_move_keys_the_peg_in_animate_mode() {
        let mut s = Session::new_scene();
        s.animate = true;
        s.frame = 5;
        s.adjust_peg(0, |v| v.x += 30.0);
        s.end_gesture();
        let p = s.doc.peg(0).unwrap();
        assert!(p.has_key(5));
        assert_eq!(p.x.at(5), 30.0);
        s.undo();
        assert_eq!(s.doc.peg(0).unwrap().x.at(5), 0.0);
    }
}
