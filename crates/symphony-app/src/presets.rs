//! Brush and Pencil presets (Harmony's tool presets). Like Harmony, they
//! live in the user's preferences, not in a scene, and can be exported to
//! a file and imported on another machine. See
//! docs/plans/artist-essentials.md §1.

use std::path::Path;

use serde::{Deserialize, Serialize};
use symphony_core::stroke::{Tip, Tips};

use crate::session::{BrushSettings, Stencil, STENCIL_SAMPLES};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PresetTool {
    Brush,
    Pencil,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Preset {
    pub name: String,
    pub settings: BrushSettings,
    /// The app it was imported from, if any ("Krita", "Procreate"...).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// Tip and grain textures, as file names in the brush library folder
    /// (kept for textured brushes).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tip: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grain: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vector_conversion: Option<bool>,
}

impl Preset {
    pub fn new(name: impl Into<String>, settings: BrushSettings) -> Self {
        Preset {
            name: name.into(),
            settings,
            source: None,
            tip: None,
            grain: None,
            vector_conversion: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Presets {
    pub brush: Vec<Preset>,
    #[serde(default)]
    brush_library_version: u8,
    pub pencil: Vec<Preset>,
    /// The preset last picked for each tool, by name, for highlighting.
    #[serde(default)]
    pub current_brush: Option<String>,
    #[serde(default)]
    pub current_pencil: Option<String>,
    /// Show presets as a grid of tiles instead of a list.
    #[serde(default)]
    pub grid: bool,
}

/// The exchange file: our own documented format, JSON.
#[derive(Serialize, Deserialize)]
struct PresetFile {
    format: String,
    version: u32,
    #[serde(default)]
    brush: Vec<Preset>,
    #[serde(default)]
    pencil: Vec<Preset>,
}

const FORMAT: &str = "symphony-presets";
pub const EXTENSION: &str = "symphonypresets";
/// Preset files are tiny; anything bigger is not one of ours.
const MAX_FILE: u64 = 4 * 1024 * 1024;

fn settings(min: f32, max: f32, smoothing: f32) -> BrushSettings {
    BrushSettings {
        min_width: min,
        max_width: max,
        smoothing,
        stabilization: Default::default(),
        preserve_corners: false,
        tips: Tips::default(),
        stencil: None,
        texture: None,
    }
}

/// Swell in the middle, thin at both ends.
fn tapered() -> Stencil {
    let mut s = [0.0; STENCIL_SAMPLES];
    for (i, v) in s.iter_mut().enumerate() {
        let t = i as f32 / (STENCIL_SAMPLES - 1) as f32;
        *v = (t * std::f32::consts::PI).sin().max(0.0).powf(0.6);
    }
    s
}

fn expressive_brushes() -> [Preset; 3] {
    let mut nib = settings(1.2, 14.0, 0.3);
    nib.tips = Tips {
        start: Tip::Flat,
        end: Tip::Flat,
    };
    let mut taper = settings(0.25, 11.0, 0.4);
    taper.stencil = Some(tapered());
    let mut cleanup = settings(0.6, 7.0, 0.4);
    cleanup.stabilization = crate::session::Stabilization::Light;
    [
        Preset::new("Square ink", nib),
        Preset::new("Tapered ink", taper),
        Preset::new("Steady cleanup", cleanup),
    ]
}

impl Default for Presets {
    /// Symphony's own starter set.
    fn default() -> Self {
        let p = |name: &str, settings| Preset::new(name, settings);
        let mut taper = settings(0.5, 4.0, 0.3);
        taper.stencil = Some(tapered());
        let mut flat = settings(2.0, 3.0, 0.3);
        flat.tips = Tips {
            start: Tip::Flat,
            end: Tip::Flat,
        };
        Presets {
            brush_library_version: 1,
            brush: vec![
                p("Fine detail", settings(0.4, 3.0, 0.5)),
                p("Inking", settings(0.6, 10.0, 0.4)),
                p("Bold", settings(2.0, 20.0, 0.3)),
                p("Even marker", settings(6.0, 6.0, 0.3)),
            ]
            .into_iter()
            .chain(expressive_brushes())
            .collect(),
            pencil: vec![
                p("Clean line", settings(2.0, 3.0, 0.3)),
                p("Thin line", settings(1.0, 1.5, 0.3)),
                p("Tapered", taper),
                p("Flat ends", flat),
            ],
            current_brush: None,
            current_pencil: None,
            grid: false,
        }
    }
}

impl Presets {
    pub fn upgrade_builtin_brushes(&mut self) {
        if self.brush_library_version < 1 {
            for preset in expressive_brushes() {
                if self.brush.iter().all(|old| old.name != preset.name) {
                    self.brush.push(preset);
                }
            }
            self.brush_library_version = 1;
        }
    }

    pub fn list(&self, tool: PresetTool) -> &Vec<Preset> {
        match tool {
            PresetTool::Brush => &self.brush,
            PresetTool::Pencil => &self.pencil,
        }
    }

    pub fn list_mut(&mut self, tool: PresetTool) -> &mut Vec<Preset> {
        match tool {
            PresetTool::Brush => &mut self.brush,
            PresetTool::Pencil => &mut self.pencil,
        }
    }

    pub fn current(&self, tool: PresetTool) -> Option<&str> {
        match tool {
            PresetTool::Brush => self.current_brush.as_deref(),
            PresetTool::Pencil => self.current_pencil.as_deref(),
        }
    }

    pub fn set_current(&mut self, tool: PresetTool, name: Option<String>) {
        match tool {
            PresetTool::Brush => self.current_brush = name,
            PresetTool::Pencil => self.current_pencil = name,
        }
    }

    /// A name not yet used in that tool's list: "Inking", "Inking 2"...
    pub fn unique_name(&self, tool: PresetTool, wanted: &str) -> String {
        let wanted = wanted.trim();
        let wanted = if wanted.is_empty() { "Preset" } else { wanted };
        let taken = |n: &str| self.list(tool).iter().any(|p| p.name == n);
        if !taken(wanted) {
            return wanted.to_owned();
        }
        (2..)
            .map(|i| format!("{wanted} {i}"))
            .find(|n| !taken(n))
            .unwrap_or_default()
    }

    /// Add a preset at the end of the list and make it current.
    pub fn add(&mut self, tool: PresetTool, name: &str, settings: BrushSettings) -> String {
        let name = self.unique_name(tool, name);
        self.list_mut(tool)
            .push(Preset::new(name.clone(), settings));
        self.set_current(tool, Some(name.clone()));
        name
    }

    pub fn rename(&mut self, tool: PresetTool, from: &str, to: &str) {
        let to = to.trim();
        if to.is_empty() || to == from {
            return;
        }
        let to = self.unique_name(tool, to);
        if let Some(p) = self.list_mut(tool).iter_mut().find(|p| p.name == from) {
            p.name = to.clone();
        }
        if self.current(tool) == Some(from) {
            self.set_current(tool, Some(to));
        }
    }

    /// Remove a preset. Returns it with its place, so it can be restored.
    pub fn remove(&mut self, tool: PresetTool, name: &str) -> Option<(usize, Preset)> {
        let list = self.list_mut(tool);
        let i = list.iter().position(|p| p.name == name)?;
        let p = list.remove(i);
        if self.current(tool) == Some(name) {
            self.set_current(tool, None);
        }
        Some((i, p))
    }

    pub fn restore(&mut self, tool: PresetTool, index: usize, mut preset: Preset) {
        preset.name = self.unique_name(tool, &preset.name);
        let list = self.list_mut(tool);
        list.insert(index.min(list.len()), preset);
    }

    /// Write both lists to a preset file.
    pub fn export(&self, path: &Path) -> std::io::Result<()> {
        let file = PresetFile {
            format: FORMAT.into(),
            version: 1,
            brush: self.brush.clone(),
            pencil: self.pencil.clone(),
        };
        let json = serde_json::to_string_pretty(&file).map_err(std::io::Error::other)?;
        std::fs::write(path, json)
    }

    /// Add the presets from a file, renaming any that clash. Returns how
    /// many were added.
    pub fn import(&mut self, path: &Path) -> Result<usize, String> {
        let too_big = std::fs::metadata(path)
            .map(|m| m.len() > MAX_FILE)
            .map_err(|e| format!("Couldn't read the file: {e}"))?;
        if too_big {
            return Err("This file is too large to be a preset file.".into());
        }
        let text =
            std::fs::read_to_string(path).map_err(|e| format!("Couldn't read the file: {e}"))?;
        let file: PresetFile = serde_json::from_str(&text)
            .map_err(|_| "This isn't a Symphony preset file.".to_string())?;
        if file.format != FORMAT {
            return Err("This isn't a Symphony preset file.".into());
        }
        if file.version > 1 {
            return Err(
                "These presets come from a newer Symphony. Update Symphony to use them.".into(),
            );
        }
        let mut n = 0;
        for (tool, list) in [
            (PresetTool::Brush, file.brush),
            (PresetTool::Pencil, file.pencil),
        ] {
            for p in list {
                let s = sanitised(p.settings);
                let name = self.unique_name(tool, &p.name);
                // Texture names must be plain library file names.
                let plain = |f: &String| {
                    f.len() <= 64
                        && f.ends_with(".png")
                        && f.chars()
                            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
                };
                self.list_mut(tool).push(Preset {
                    name,
                    settings: s,
                    source: p.source.filter(|s| s.len() <= 40),
                    tip: p.tip.filter(plain),
                    grain: p.grain.filter(plain),
                    vector_conversion: p.vector_conversion,
                });
                n += 1;
            }
        }
        Ok(n)
    }
}

/// Keep imported values in the ranges the tools allow.
fn sanitised(mut s: BrushSettings) -> BrushSettings {
    let clean = |v: f32, lo: f32, hi: f32, default: f32| {
        if v.is_finite() {
            v.clamp(lo, hi)
        } else {
            default
        }
    };
    s.max_width = clean(s.max_width, 0.5, crate::views::panels::MAX_TOOL_SIZE, 4.0);
    s.min_width = clean(s.min_width, 0.0, s.max_width, 0.0);
    s.smoothing = clean(s.smoothing, 0.0, 0.95, 0.3);
    if let Some(t) = &mut s.texture {
        t.spacing = clean(t.spacing, 0.01, 10.0, 0.1);
        t.opacity = clean(t.opacity, 0.0, 1.0, 1.0);
        t.angle = clean(t.angle, -360.0, 360.0, 0.0);
        t.grain_scale = clean(t.grain_scale, 0.01, 100.0, 1.0);
    }
    if let Some(st) = &mut s.stencil {
        for v in st.iter_mut() {
            *v = clean(*v, 0.0, 1.0, 1.0);
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starter_presets_are_finite() {
        let p = Presets::default();
        for preset in p.brush.iter().chain(&p.pencil) {
            let s = preset.settings;
            assert!(s.min_width.is_finite() && s.max_width.is_finite());
            if let Some(st) = s.stencil {
                assert!(st.iter().all(|v| v.is_finite() && (0.0..=1.0).contains(v)));
            }
        }
    }

    #[test]
    fn names_stay_unique() {
        let mut p = Presets::default();
        let a = p.add(PresetTool::Brush, "Inking", settings(1.0, 2.0, 0.1));
        assert_eq!(a, "Inking 2");
        p.rename(PresetTool::Brush, "Inking 2", "Bold");
        assert!(p.brush.iter().any(|x| x.name == "Bold 2"));
        assert_eq!(p.current(PresetTool::Brush), Some("Bold 2"));
        let (i, gone) = p.remove(PresetTool::Brush, "Bold 2").unwrap();
        assert_eq!(p.current(PresetTool::Brush), None);
        p.restore(PresetTool::Brush, i, gone);
        assert_eq!(p.brush[i].name, "Bold 2");
    }

    #[test]
    fn export_then_import_round_trips_and_rejects_strangers() {
        let dir = std::env::temp_dir().join(format!("sym-presets-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("mine.symphonypresets");
        let ours = Presets::default();
        ours.export(&file).unwrap();
        let mut other = Presets {
            brush_library_version: 1,
            brush: Vec::new(),
            pencil: Vec::new(),
            current_brush: None,
            current_pencil: None,
            grid: false,
        };
        assert_eq!(
            other.import(&file).unwrap(),
            ours.brush.len() + ours.pencil.len()
        );
        assert_eq!(other.brush, ours.brush);
        assert_eq!(other.pencil, ours.pencil);
        // Importing again keeps both copies.
        other.import(&file).unwrap();
        assert!(other.pencil.iter().any(|p| p.name == "Tapered 2"));

        let junk = dir.join("junk.json");
        std::fs::write(&junk, "{\"format\":\"other\",\"version\":1}").unwrap();
        assert!(other.import(&junk).is_err());
        std::fs::write(&junk, "not json at all").unwrap();
        assert!(other.import(&junk).is_err());
        let hostile = r#"{"format":"symphony-presets","version":1,"brush":[{"name":"x","settings":{"min_width":1e30,"max_width":-5,"smoothing":2}}]}"#;
        std::fs::write(&junk, hostile).unwrap();
        other.import(&junk).unwrap();
        let x = &other.brush.last().unwrap().settings;
        assert!(x.max_width >= 0.5 && x.min_width <= x.max_width && x.smoothing <= 0.95);
        std::fs::remove_dir_all(&dir).ok();
    }
}
