//! Drag brush files from other apps onto Symphony to import them as Brush
//! presets. Reading runs on a background thread (DESIGN §17): brush sets
//! can be large.

use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver};

use symphony_brushes::Brush;
use symphony_core::stroke::{StrokeTexture, Tips};

use crate::presets::Preset;
use crate::session::BrushSettings;

/// Where imported tip and grain textures are kept.
pub fn library_dir() -> Option<PathBuf> {
    // Screenshot runs never touch the real library.
    if std::env::var_os("SYMPHONY_SCREENSHOT").is_some() {
        return Some(std::env::temp_dir().join("symphony-shot-brushes"));
    }
    dirs::data_dir().map(|d| d.join("Symphony").join("brushes"))
}

pub struct Outcome {
    pub presets: Vec<Preset>,
    /// Plain-language notes: what came from where, and what didn't import.
    pub messages: Vec<String>,
}

pub struct BrushImport {
    rx: Receiver<Outcome>,
    pub files: usize,
}

impl BrushImport {
    pub fn start(paths: Vec<PathBuf>, ctx: &eframe::egui::Context) -> Self {
        let (tx, rx) = channel();
        let files = paths.len();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let out = run(&paths, library_dir().as_deref());
            let _ = tx.send(out);
            ctx.request_repaint();
        });
        BrushImport { rx, files }
    }

    pub fn poll(&self) -> Option<Outcome> {
        self.rx.try_recv().ok()
    }

    /// Screenshot mode: import on this thread so the shot shows the result.
    pub fn finished(paths: Vec<PathBuf>) -> Self {
        let (tx, rx) = channel();
        let files = paths.len();
        let _ = tx.send(run(&paths, library_dir().as_deref()));
        BrushImport { rx, files }
    }
}

/// Imported sizes are kept, up to the largest size the tools accept.
const MAX_WIDTH: f32 = crate::views::panels::MAX_TOOL_SIZE;

fn settings(b: &Brush, tip: Option<u64>, grain: Option<u64>) -> BrushSettings {
    use symphony_brushes::Texturing;
    use symphony_core::stroke::{GrainMode, PressureCurve};
    let max = b.diameter.clamp(0.5, MAX_WIDTH);
    // The thinnest the size curve goes sets the line's minimum width; the
    // curve's shape between minimum and maximum goes with the texture.
    let lightest = (0..=32)
        .map(|i| b.size_at(i as f32 / 32.0))
        .fold(1.0f32, f32::min)
        .clamp(0.0, 1.0);
    let size_curve = (!b.size_curve.is_empty()).then(|| {
        PressureCurve::sample(|p| {
            if lightest >= 1.0 {
                1.0
            } else {
                (b.size_at(p) - lightest) / (1.0 - lightest)
            }
        })
    });
    let grain_mode = match b.texturing {
        Texturing::Multiply => GrainMode::Multiply,
        Texturing::Subtract => GrainMode::Subtract,
        Texturing::Darken => GrainMode::Darken,
        Texturing::Height => GrainMode::Height,
        Texturing::LinearHeight => GrainMode::LinearHeight,
    };
    BrushSettings {
        min_width: (max * lightest).min(max),
        max_width: max,
        smoothing: b.smoothing.clamp(0.0, 0.95),
        stabilization: Default::default(),
        preserve_corners: false,
        tips: Tips::default(),
        stencil: None,
        // Imported brushes are textured vector brushes, as in Harmony:
        // vector lines drawn with the original tip and grain.
        texture: Some(StrokeTexture {
            tip,
            grain,
            spacing: b.spacing.clamp(0.01, 10.0),
            opacity: b.opacity.clamp(0.0, 1.0),
            angle: b.angle,
            follow: b.follow,
            grain_scale: 1.0,
            flow: b.flow,
            hardness: b.hardness,
            opacity_by_pressure: [
                b.opacity_at(0.0),
                b.opacity_at(1.0 / 3.0),
                b.opacity_at(2.0 / 3.0),
                b.opacity_at(1.0),
            ],
            opacity_curve: (!b.opacity_curve.is_empty())
                .then(|| PressureCurve::sample(|p| b.opacity_at(p))),
            size_curve,
            flow_curve: (!b.flow_curve.is_empty()).then(|| PressureCurve::sample(|p| b.flow_at(p))),
            grain_mode,
            grain_strength: (b.grain_strength < 1.0 || !b.grain_strength_curve.is_empty())
                .then(|| PressureCurve::sample(|p| b.grain_strength_at(p))),
            grain_offset: b.grain_offset,
            auto_spacing: b.auto_spacing,
            dynamics: symphony_core::stroke::Dynamics {
                scatter: b.dynamics.scatter,
                scatter_pressure: b.dynamics.scatter_pressure,
                size_jitter: b.dynamics.size_jitter,
                angle_jitter: b.dynamics.angle_jitter,
                stroke_angle_jitter: b.dynamics.stroke_angle_jitter,
                roundness: b.dynamics.roundness,
                roundness_pressure: b.dynamics.roundness_pressure,
                opacity_jitter: b.dynamics.opacity_jitter,
                flow_jitter: b.dynamics.flow_jitter,
                angle_tilt: b.dynamics.angle_tilt,
                angle_barrel: b.dynamics.angle_barrel,
                roundness_tilt: b.dynamics.roundness_tilt,
            },
            vector: false,
        }),
    }
}

/// The library key of a texture file name ("0123abcd....png").
pub fn key_of(file: &str) -> Option<u64> {
    u64::from_str_radix(file.strip_suffix(".png")?, 16).ok()
}

thread_local! {
    static LOADED: std::cell::RefCell<std::collections::HashMap<u64, Option<std::sync::Arc<symphony_core::stroke::Texture>>>> =
        Default::default();
}

/// A texture from the brush library, decoded once per run (for previews).
pub fn library_texture(key: u64) -> Option<std::sync::Arc<symphony_core::stroke::Texture>> {
    LOADED.with(|m| {
        m.borrow_mut()
            .entry(key)
            .or_insert_with(|| {
                let path = library_dir()?.join(format!("{key:016x}.png"));
                let g = image::open(path).ok()?.to_luma8();
                let t = symphony_core::stroke::Texture {
                    width: g.width(),
                    height: g.height(),
                    pixels: g.into_raw(),
                };
                t.is_valid().then(|| std::sync::Arc::new(t))
            })
            .clone()
    })
}

/// Load a library texture into the scene's store if it isn't there yet,
/// so the scene carries every texture its strokes use.
pub fn ensure_texture(scene: &mut symphony_core::doc::Scene, key: u64) {
    if scene.textures.contains_key(&key) {
        return;
    }
    if let Some(t) = library_texture(key) {
        scene.textures.insert(key, (*t).clone());
    }
}

/// Save a texture under a name made from its pixels, so the same tip
/// imported twice is stored once.
fn save(img: &image::GrayImage, dir: &Path) -> Option<String> {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (img.width(), img.height()).hash(&mut h);
    img.as_raw().hash(&mut h);
    let name = format!("{:016x}.png", h.finish());
    let path = dir.join(&name);
    if !path.exists() {
        std::fs::create_dir_all(dir).ok()?;
        img.save(&path).ok()?;
    }
    Some(name)
}

// A clean outline needs a substantial opaque core. Screen out soft,
// low-flow tips whose appearance comes primarily from transparency.
fn suitable_vector_conversion(brush: &Brush) -> bool {
    if brush.opacity < 0.6 || brush.flow < 0.4 {
        return false;
    }
    let Some(tip) = &brush.tip else {
        return brush.grain.is_none() && brush.hardness >= 0.6;
    };
    let (visible, opaque) = tip.pixels().fold((0, 0), |(visible, opaque), pixel| {
        (
            visible + u32::from(pixel[0] >= 32),
            opaque + u32::from(pixel[0] >= 192),
        )
    });
    visible > 0 && opaque >= visible.div_ceil(3)
}

fn run(paths: &[PathBuf], library: Option<&Path>) -> Outcome {
    let mut out = Outcome {
        presets: Vec::new(),
        messages: Vec::new(),
    };
    for path in paths {
        let file = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        match symphony_brushes::import(path) {
            Ok(got) => {
                let mut added = 0;
                let mut erasers = 0;
                let mut source = None;
                for b in &got.brushes {
                    if b.eraser {
                        erasers += 1;
                        continue;
                    }
                    source = Some(b.source.title());
                    let tip = b.tip.as_ref().zip(library).and_then(|(t, d)| save(t, d));
                    let grain = b.grain.as_ref().zip(library).and_then(|(g, d)| save(g, d));
                    let keys = (
                        tip.as_deref().and_then(key_of),
                        grain.as_deref().and_then(key_of),
                    );
                    out.presets.push(Preset {
                        name: b.name.clone(),
                        settings: settings(b, keys.0, keys.1),
                        source: Some(b.source.title().to_owned()),
                        tip,
                        grain,
                        vector_conversion: Some(suitable_vector_conversion(b)),
                    });
                    added += 1;
                }
                if added > 0 {
                    out.messages.push(format!(
                        "{added} brush{} from {file}{}",
                        if added == 1 { "" } else { "es" },
                        source.map_or(String::new(), |s| format!(" ({s})"))
                    ));
                }
                if erasers > 0 {
                    out.messages.push(format!(
                        "{erasers} eraser preset{} skipped: eraser presets aren't supported yet",
                        if erasers == 1 { "" } else { "s" }
                    ));
                }
                out.messages
                    .extend(got.problems.iter().map(|p| format!("{file}: {p}")));
            }
            Err(e) => out.messages.push(format!("{file}: {e}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Design review: a real brush file's brushes drawn as Bitmap (left)
    /// and as Vector (right), rendered by the export renderer.
    /// `SYMPHONY_BRUSH_FILE=<file> SYMPHONY_BRUSH_SHEET=<out.png> cargo test
    /// -p symphony-app brush_sheet_bitmap_vs_vector -- --ignored`.
    #[test]
    #[ignore = "needs a real brush file"]
    fn brush_sheet_bitmap_vs_vector() {
        let (Some(file), Some(out)) = (
            std::env::var_os("SYMPHONY_BRUSH_FILE"),
            std::env::var_os("SYMPHONY_BRUSH_SHEET"),
        ) else {
            return;
        };
        let lib = std::env::temp_dir().join(format!("symphony-sheet-{}", std::process::id()));
        std::fs::create_dir_all(&lib).unwrap();
        let got = run(&[PathBuf::from(&file)], Some(&lib));
        let mut s = crate::session::Session::new_scene();
        s.add_layer();
        let layer = s.selected;
        for entry in std::fs::read_dir(&lib).unwrap().flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if let (Some(key), Ok(img)) = (key_of(&name), image::open(entry.path())) {
                let g = img.to_luma8();
                s.doc.textures.insert(
                    key,
                    symphony_core::stroke::Texture {
                        width: g.width(),
                        height: g.height(),
                        pixels: g.into_raw(),
                    },
                );
            }
        }
        let rows = got.presets.iter().take(10).count().max(1) as f32;
        for (row, p) in got.presets.iter().take(10).enumerate() {
            let y = -480.0 + (row as f32 + 0.5) * 960.0 / rows;
            for (col, vector) in [(0.0f32, false), (1.0, true)] {
                let mut tex = p.settings.texture;
                if let Some(t) = &mut tex {
                    t.vector = vector;
                }
                let width = p.settings.max_width.clamp(4.0, 40.0);
                let points = (0..=80)
                    .map(|i| {
                        let t = i as f32 / 80.0;
                        symphony_core::stroke::Point {
                            x: -880.0 + col * 900.0 + t * 780.0,
                            y: y + (t * std::f32::consts::TAU).sin() * 20.0,
                            pressure: (t * std::f32::consts::PI).sin().max(0.15),
                            pen: Default::default(),
                        }
                    })
                    .collect();
                let st = symphony_core::stroke::Stroke {
                    id: s.doc.ids.fresh(),
                    kind: symphony_core::stroke::StrokeKind::Brush,
                    art: Default::default(),
                    swatch: s.swatch,
                    min_width: width * (p.settings.min_width / p.settings.max_width.max(0.01)),
                    max_width: width,
                    points,
                    tips: Default::default(),
                    texture: tex,
                    shape: Vec::new(),
                };
                s.commit_stroke(layer, 1, st);
            }
        }
        let img = symphony_render::render_frame(&s.doc, 1, 1920, 1080).unwrap();
        img.save(&out).unwrap();
        for p in got.presets.iter().take(10) {
            println!("{}", p.name);
        }
        std::fs::remove_dir_all(&lib).ok();
    }

    #[test]
    fn sizes_fit_symphony_tools() {
        let mut b = symphony_brushes::import_bytes(
            "myb",
            "Pen",
            br#"{"settings":{"radius_logarithmic":{"base_value":6.0,"inputs":{}}}}"#,
        )
        .unwrap()
        .brushes
        .remove(0);
        b.min_fraction = 0.25;
        // A big stamp keeps its size (no 60-point cap any more)...
        let s = settings(&b, None, None);
        assert!((s.max_width - b.diameter).abs() < 0.01);
        assert!((s.min_width - b.diameter * 0.25).abs() < 0.01);
        // ...up to the tools' typed maximum.
        b.diameter = 1.0e6;
        assert_eq!(settings(&b, None, None).max_width, MAX_WIDTH);
    }

    #[test]
    fn bad_files_report_instead_of_failing() {
        let dir = std::env::temp_dir().join(format!("sym-bi-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let bad = dir.join("broken.kpp");
        std::fs::write(&bad, b"nope").unwrap();
        let out = run(&[bad, dir.join("missing.myb")], None);
        assert!(out.presets.is_empty());
        assert_eq!(out.messages.len(), 2);
        std::fs::remove_dir_all(&dir).ok();
    }
}

#[cfg(test)]
#[path = "brush_sheet.rs"]
mod brush_sheet;
