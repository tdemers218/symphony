//! Vector strokes and drawings. Geometry work on them (smoothing,
//! tessellation, hit-testing) lives in `symphony-vector` (E1.1).

use serde::{Deserialize, Serialize};

use crate::id::Id;

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Point {
    pub x: f32,
    pub y: f32,
    /// 0..=1 after the pressure curve.
    pub pressure: f32,
    /// How the pen was held here (tilt and rotation), so brushes that
    /// follow them still do after the line is edited. Zero for mice and
    /// for lines drawn before Symphony kept it.
    #[serde(default, skip_serializing_if = "Pen::is_level")]
    pub pen: Pen,
}

/// The pen's attitude at a point, in degrees.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Pen {
    /// How far the pen leans from upright, 0 (upright) ..= 90 (flat).
    #[serde(default)]
    pub tilt: f32,
    /// Which way it leans, counter-clockwise from +x (Krita's "ascension").
    #[serde(default)]
    pub azimuth: f32,
    /// Barrel rotation (pens that report it).
    #[serde(default)]
    pub rotation: f32,
}

impl Pen {
    pub fn is_level(&self) -> bool {
        *self == Pen::default()
    }

    /// From a tablet's tilt along x and y (degrees) and barrel rotation.
    pub fn from_tilt(tilt_x: f32, tilt_y: f32, rotation: f32) -> Self {
        let ok = |v: f32| if v.is_finite() { v } else { 0.0 };
        let (tx, ty) = (ok(tilt_x), ok(tilt_y));
        Pen {
            tilt: tx.hypot(ty).min(90.0),
            azimuth: if tx == 0.0 && ty == 0.0 {
                0.0
            } else {
                ty.atan2(tx).to_degrees()
            },
            rotation: ok(rotation),
        }
    }

    /// Between two pens; angles take the short way round.
    pub fn lerp(a: Pen, b: Pen, t: f32) -> Pen {
        let angle = |x: f32, y: f32| {
            let d = (y - x + 540.0).rem_euclid(360.0) - 180.0;
            x + d * t
        };
        Pen {
            tilt: a.tilt + (b.tilt - a.tilt) * t,
            azimuth: angle(a.azimuth, b.azimuth),
            rotation: angle(a.rotation, b.rotation),
        }
    }
}

/// How a stroke is shaped (DESIGN/RESEARCH: Harmony's pencil vs brush).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum StrokeKind {
    /// Filled contour whose width follows pressure.
    Brush,
    /// Centreline with a thickness profile, editable later.
    Pencil,
    /// Invisible line that only bounds paint zones (Harmony's Stroke and
    /// Close Gap tools). Shown only with Show Strokes.
    Invisible,
}

/// Which art layer of a drawing the stroke is on (Harmony's art layers).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ArtLayer {
    Overlay,
    #[default]
    LineArt,
    ColourArt,
    Underlay,
}

impl ArtLayer {
    /// Back to front.
    pub const STACK: [ArtLayer; 4] = [
        ArtLayer::Underlay,
        ArtLayer::ColourArt,
        ArtLayer::LineArt,
        ArtLayer::Overlay,
    ];
}

/// How a line ends (Harmony's pencil Start and End tips).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Tip {
    #[default]
    Round,
    Flat,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tips {
    /// The end drawn first.
    pub start: Tip,
    pub end: Tip,
}

/// Harmony's Textured Vector Brush: the stroke stays a vector line, and
/// its look comes from stamping a bitmap tip along it, multiplied by a
/// paper grain. It's redrawn from the line, so editing or transforming the
/// line re-samples the texture instead of stretching pixels.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct StrokeTexture {
    /// Tip image in `Scene::textures` (none = a hard round tip).
    #[serde(default)]
    pub tip: Option<u64>,
    /// Paper grain in `Scene::textures`, tiled in drawing space.
    #[serde(default)]
    pub grain: Option<u64>,
    /// Distance between stamps as a fraction of the tip size.
    pub spacing: f32,
    /// Most opacity the stroke can reach (non-cumulative), 0..=1.
    pub opacity: f32,
    /// Tip rotation in degrees, counter-clockwise.
    #[serde(default)]
    pub angle: f32,
    /// Turn the tip with the stroke's direction (Harmony's Heading).
    #[serde(default)]
    pub follow: bool,
    /// Grain pixels per drawing unit.
    #[serde(default = "one")]
    pub grain_scale: f32,
    /// How much each stamp lays down, 0..=1. Below 1, overlapping stamps
    /// build up (Krita, Harmony and Procreate flow).
    #[serde(default = "one")]
    pub flow: f32,
    /// Edge of the round tip (no tip image): 1 is hard, lower is softer.
    #[serde(default = "one")]
    pub hardness: f32,
    /// Stamp opacity at pen pressures 0, 1/3, 2/3 and 1: how pressure
    /// fades the stroke. All 1 means pressure doesn't change opacity.
    #[serde(default = "full")]
    pub opacity_by_pressure: [f32; 4],
    /// A finer opacity curve, used instead of `opacity_by_pressure` when
    /// set (imported brushes).
    #[serde(default)]
    pub opacity_curve: Option<PressureCurve>,
    /// Pressure to size, 0 = the stroke's minimum width .. 1 = maximum.
    /// None means straight between them.
    #[serde(default)]
    pub size_curve: Option<PressureCurve>,
    /// Pressure to flow, multiplying `flow`.
    #[serde(default)]
    pub flow_curve: Option<PressureCurve>,
    /// How the grain shapes each stamp (Krita's texturing modes).
    #[serde(default)]
    pub grain_mode: GrainMode,
    /// Pressure to grain strength, 0..=1 (Krita's texture Strength).
    #[serde(default)]
    pub grain_strength: Option<PressureCurve>,
    /// Grain offset in grain pixels (Krita's texture Offset X/Y).
    #[serde(default)]
    pub grain_offset: [f32; 2],
    /// Krita's auto spacing: stamps are `coefficient * sqrt(size)` apart
    /// instead of `spacing * size`. 0 is off.
    #[serde(default)]
    pub auto_spacing: f32,
    /// Randomness and pressure shaping per stamp (Krita's Scatter,
    /// Rotation and Ratio options, Photoshop's Shape Dynamics, Scattering
    /// and Transfer). Seeded by the stroke, so a stroke looks the same on
    /// every redraw.
    #[serde(default, skip_serializing_if = "Dynamics::is_still")]
    pub dynamics: Dynamics,
    /// A vector brush made from this bitmap brush: the tip's shape is
    /// stamped along the line and merged into one clean outline (no
    /// pixels), instead of painting the tip image.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub vector: bool,
}

/// Per-stamp variation of a textured brush. All zero is a steady brush.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Dynamics {
    /// Each stamp lands up to this many diameters from the line, in a
    /// random direction (Krita Scatter, Photoshop Scattering).
    #[serde(default)]
    pub scatter: f32,
    /// Scatter grows with pen pressure (Krita's pressure sensor on Scatter).
    #[serde(default)]
    pub scatter_pressure: bool,
    /// Each stamp is up to this fraction smaller, at random, 0..=1.
    #[serde(default)]
    pub size_jitter: f32,
    /// Each stamp turns up to this many degrees either way, at random.
    #[serde(default)]
    pub angle_jitter: f32,
    /// The whole stroke turns up to this many degrees either way, at random
    /// (Krita's "fuzzy stroke" rotation).
    #[serde(default)]
    pub stroke_angle_jitter: f32,
    /// Tip roundness, height over width, 0..=1; 0 means round (1).
    #[serde(default)]
    pub roundness: f32,
    /// Roundness follows pen pressure (Krita's pressure sensor on Ratio):
    /// light pressure flattens the tip towards `roundness`.
    #[serde(default)]
    pub roundness_pressure: bool,
    /// Each stamp's opacity and flow drop by up to these fractions.
    #[serde(default)]
    pub opacity_jitter: f32,
    #[serde(default)]
    pub flow_jitter: f32,
    /// The tip turns to the way the pen leans (Krita's "ascension").
    #[serde(default)]
    pub angle_tilt: bool,
    /// The tip turns with the pen's barrel rotation.
    #[serde(default)]
    pub angle_barrel: bool,
    /// Leaning the pen flattens the tip towards `roundness` (Krita's
    /// "declination" on Ratio): upright is round, flat is flattest.
    #[serde(default)]
    pub roundness_tilt: bool,
}

impl Dynamics {
    pub fn is_still(&self) -> bool {
        *self == Dynamics::default()
    }
}

/// How the grain combines with each stamp. Formulas follow Krita 5.0's
/// masking composite ops (alpha `a`, grain `m`, strength `s`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum GrainMode {
    /// `a * m * s`: dark grain removes paint.
    #[default]
    Multiply,
    /// `a - (m + 1 - s)`: light grain removes paint, harder as strength
    /// drops.
    Subtract,
    /// `min(m, a * s)`.
    Darken,
    /// Paper height: light grain is high and catches paint first.
    Height,
    /// Height blended with multiply.
    LinearHeight,
}

impl GrainMode {
    /// One stamp's alpha `a` after the grain `m` at strength `s`.
    pub fn apply(self, a: f32, m: f32, s: f32) -> f32 {
        let r = match self {
            GrainMode::Multiply => a * m * s,
            GrainMode::Subtract => a - (m + 1.0 - s),
            GrainMode::Darken => m.min(a * s),
            GrainMode::Height => {
                let s = 0.99 * s;
                let inv = 1.0 - s;
                a / inv - (m + inv)
            }
            GrainMode::LinearHeight => {
                let s = 0.99 * s;
                let inv = 1.0 - s;
                let dst = a / inv - inv;
                (dst * (1.0 - m)).max(dst - m)
            }
        };
        r.clamp(0.0, 1.0)
    }
}

/// A pressure response sampled at nine even pressures, 0 to 1.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PressureCurve(pub [f32; 9]);

impl PressureCurve {
    /// Sample any curve function at the nine pressures.
    pub fn sample(f: impl Fn(f32) -> f32) -> Self {
        let mut v = [0.0; 9];
        for (i, slot) in v.iter_mut().enumerate() {
            let y = f(i as f32 / 8.0);
            *slot = if y.is_finite() {
                y.clamp(0.0, 1.0)
            } else {
                1.0
            };
        }
        PressureCurve(v)
    }

    pub fn at(&self, pressure: f32) -> f32 {
        let x = if pressure.is_finite() {
            pressure.clamp(0.0, 1.0)
        } else {
            1.0
        } * 8.0;
        let i = (x.floor() as usize).min(7);
        let f = x - i as f32;
        (self.0[i] * (1.0 - f) + self.0[i + 1] * f).clamp(0.0, 1.0)
    }
}

impl Default for StrokeTexture {
    fn default() -> Self {
        StrokeTexture {
            tip: None,
            grain: None,
            spacing: 0.1,
            opacity: 1.0,
            angle: 0.0,
            follow: false,
            grain_scale: 1.0,
            flow: 1.0,
            hardness: 1.0,
            opacity_by_pressure: [1.0; 4],
            opacity_curve: None,
            size_curve: None,
            flow_curve: None,
            grain_mode: GrainMode::Multiply,
            grain_strength: None,
            grain_offset: [0.0, 0.0],
            auto_spacing: 0.0,
            dynamics: Dynamics::default(),
            vector: false,
        }
    }
}

fn full() -> [f32; 4] {
    [1.0; 4]
}

impl StrokeTexture {
    /// Stamp opacity for a pen pressure.
    pub fn opacity_at(&self, pressure: f32) -> f32 {
        if let Some(c) = &self.opacity_curve {
            return c.at(pressure);
        }
        let x = pressure.clamp(0.0, 1.0) * 3.0;
        let i = (x.floor() as usize).min(2);
        let f = x - i as f32;
        let c = &self.opacity_by_pressure;
        (c[i] * (1.0 - f) + c[i + 1] * f).clamp(0.0, 1.0)
    }
}

impl StrokeTexture {
    /// Stamp flow for a pen pressure.
    pub fn flow_at(&self, pressure: f32) -> f32 {
        let f = if self.flow.is_finite() {
            self.flow.clamp(0.0, 1.0)
        } else {
            1.0
        };
        f * self.flow_curve.map_or(1.0, |c| c.at(pressure))
    }

    /// Grain strength for a pen pressure.
    pub fn grain_strength_at(&self, pressure: f32) -> f32 {
        self.grain_strength.map_or(1.0, |c| c.at(pressure))
    }

    /// Where between the stroke's minimum and maximum width a pressure
    /// lands, 0..=1.
    pub fn size_at(&self, pressure: f32) -> f32 {
        match &self.size_curve {
            Some(c) => c.at(pressure),
            None => pressure.clamp(0.0, 1.0),
        }
    }
}

fn one() -> f32 {
    1.0
}

/// A greyscale image used by textured strokes: 255 paints fully.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Texture {
    pub width: u32,
    pub height: u32,
    #[serde(with = "crate::b64")]
    pub pixels: Vec<u8>,
}

impl Texture {
    /// Sizes match and nothing is absurd (hostile files, E7.6).
    pub fn is_valid(&self) -> bool {
        self.width > 0
            && self.height > 0
            && self.width <= 4096
            && self.height <= 4096
            && self.pixels.len() == (self.width * self.height) as usize
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Stroke {
    pub id: Id,
    pub kind: StrokeKind,
    #[serde(default)]
    pub art: ArtLayer,
    /// Palette swatch, never a raw colour (E2.3).
    pub swatch: Id,
    pub min_width: f32,
    pub max_width: f32,
    pub points: Vec<Point>,
    #[serde(default)]
    pub tips: Tips,
    /// Set for textured vector brush strokes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub texture: Option<StrokeTexture>,
    /// Once a brush stroke is erased, its outline (outer contour first,
    /// then holes, even-odd): drawn, hit and painted against instead of
    /// the centreline, so the eraser's shape shows exactly.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub shape: Vec<Vec<[f32; 2]>>,
}

impl Stroke {
    pub fn width_at(&self, pressure: f32) -> f32 {
        self.min_width + (self.max_width - self.min_width) * pressure.clamp(0.0, 1.0)
    }
}

/// One drawing in a column, like Harmony's numbered drawings.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Drawing {
    pub id: Id,
    /// The column (element) this drawing belongs to.
    pub column: Id,
    pub name: String,
    pub strokes: Vec<Stroke>,
    /// Painted colour zones, drawn under the strokes of their art layer.
    #[serde(default)]
    pub fills: Vec<Fill>,
    /// An imported picture (or one frame of a sequence or video), under the
    /// fills and lines.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<PlacedImage>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub text: Vec<TextObject>,
}

/// Editable source for the vector shapes generated by the Text tool.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TextObject {
    pub id: Id,
    pub text: String,
    pub style: TextStyle,
    pub transform: crate::math::Affine,
    pub glyphs: Vec<Id>,
}

/// How text looks (the Text tool's properties).
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TextStyle {
    /// A font family installed on this computer ("" = the system's sans).
    pub family: String,
    /// Height in drawing units.
    pub size: f32,
    pub bold: bool,
    pub italic: bool,
    pub align: TextAlign,
}

impl Default for TextStyle {
    fn default() -> Self {
        TextStyle {
            family: String::new(),
            size: 72.0,
            bold: false,
            italic: false,
            align: TextAlign::Left,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum TextAlign {
    #[default]
    Left,
    Center,
    Right,
}

/// An imported image placed in a drawing.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlacedImage {
    /// Key into `Scene::media`.
    pub media: u64,
    /// Where it sits, in drawing units: left, top, width, height.
    pub rect: [f32; 4],
}

/// A painted zone (Harmony's Paint tool): the closed area found from the
/// strokes, filled with a palette swatch by ID (E2.3).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Fill {
    pub id: Id,
    pub swatch: Id,
    #[serde(default)]
    pub art: ArtLayer,
    /// Closed outlines in drawing space. The first is the outside, any
    /// others are holes (even-odd).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub linked: Option<FillLink>,
    pub contours: Vec<Vec<[f32; 2]>>,
}

/// A sample and the strokes that bounded its bucket-filled region.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FillLink {
    pub seed: [f32; 2],
    pub cell: f32,
    pub strokes: Vec<Id>,
    pub bounds: ([f32; 2], [f32; 2]),
}

impl Fill {
    /// Is the point inside the painted area (even-odd over all contours)?
    pub fn contains(&self, x: f32, y: f32) -> bool {
        let mut inside = false;
        for c in &self.contours {
            let n = c.len();
            for i in 0..n {
                let (a, b) = (c[i], c[(i + n - 1) % n]);
                if (a[1] > y) != (b[1] > y) && x < (b[0] - a[0]) * (y - a[1]) / (b[1] - a[1]) + a[0]
                {
                    inside = !inside;
                }
            }
        }
        inside
    }
}

#[cfg(test)]
mod grain_tests {
    use super::*;

    #[test]
    fn grain_modes_follow_krita() {
        // Subtract: light grain removes paint; strength below 1 removes more.
        assert_eq!(GrainMode::Subtract.apply(1.0, 1.0, 1.0), 0.0);
        assert_eq!(GrainMode::Subtract.apply(1.0, 0.0, 1.0), 1.0);
        assert!((GrainMode::Subtract.apply(1.0, 0.5, 0.92) - 0.42).abs() < 1e-6);
        // Multiply scales by grain and strength.
        assert!((GrainMode::Multiply.apply(0.8, 0.5, 1.0) - 0.4).abs() < 1e-6);
        assert!((GrainMode::Darken.apply(1.0, 0.3, 1.0) - 0.3).abs() < 1e-6);
        for m in [
            GrainMode::Multiply,
            GrainMode::Subtract,
            GrainMode::Darken,
            GrainMode::Height,
            GrainMode::LinearHeight,
        ] {
            for a in [0.0, 0.5, 1.0] {
                for g in [0.0, 0.5, 1.0] {
                    let v = m.apply(a, g, 0.7);
                    assert!((0.0..=1.0).contains(&v), "{m:?}");
                }
            }
        }
    }

    #[test]
    fn old_scenes_load_with_the_new_texture_fields() {
        let old = r#"{"tip":null,"grain":null,"spacing":0.1,"opacity":1.0}"#;
        let t: StrokeTexture = serde_json::from_str(old).unwrap();
        assert_eq!(t.grain_mode, GrainMode::Multiply);
        assert_eq!(t.flow_at(0.3), 1.0);
        assert_eq!(t.size_at(0.3), 0.3);
    }
}
