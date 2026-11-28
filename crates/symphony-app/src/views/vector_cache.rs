//! Cached triangles for plain vector lines, so zooming and panning don't
//! re-tessellate the drawing every frame.
//!
//! Each line is tessellated once per zoom octave, in drawing space, at the
//! power-of-two zoom at or above the current one (so curves stay smooth).
//! Every frame only transforms the cached triangles, and lines outside the
//! view are skipped. A line is rebuilt when it changes (its fingerprint) or
//! when the zoom leaves its octave.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};

use eframe::egui::{self, Color32, Pos2, Rect};
use symphony_core::id::Id;
use symphony_core::math::Affine;
use symphony_core::stroke::Stroke;

struct Entry {
    fingerprint: u64,
    /// The zoom the triangles were made for.
    lod: f32,
    /// Vertices in drawing units, and triangle indices.
    vertices: Vec<[f32; 2]>,
    indices: Vec<u32>,
    /// The scene version the fingerprint was last checked at: lines can
    /// only change when the scene does, so in between it isn't re-hashed.
    checked: u64,
    /// Bounds in drawing units.
    lo: [f32; 2],
    hi: [f32; 2],
    seen: u64,
}

#[derive(Default)]
pub struct VectorMeshes {
    entries: HashMap<Id, Entry>,
    /// Paint fills: triangulated once (a polygon needs no zoom detail).
    fills: HashMap<Id, Entry>,
    /// Imported images on the GPU, by media key.
    images: HashMap<u64, egui::TextureHandle>,
    frame: u64,
    /// Time spent rebuilding this frame; past the budget, lines that
    /// only need a finer zoom level wait a frame (drawing their slightly
    /// coarser triangles), so zooming never hitches.
    spent: std::time::Duration,
    /// Some lines are waiting for a rebuild.
    behind: bool,
    /// The scene's edit counter this frame (see `set_version`).
    version: u64,
    /// Lines tessellated and fills triangulated so far (tests check an
    /// unchanged frame adds none).
    pub built: u64,
}

/// Rebuild time allowed per frame before finer detail waits a frame.
const BUDGET: std::time::Duration = std::time::Duration::from_millis(4);

/// A cheap fingerprint of everything that shapes a line's triangles.
fn fingerprint(st: &Stroke) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    st.min_width.to_bits().hash(&mut h);
    st.max_width.to_bits().hash(&mut h);
    format!("{:?}", st.tips).hash(&mut h);
    st.points.len().hash(&mut h);
    for p in &st.points {
        (
            p.x.to_bits(),
            p.y.to_bits(),
            p.pressure.to_bits(),
            p.width_offset.to_bits(),
        )
            .hash(&mut h);
    }
    st.shape.len().hash(&mut h);
    for c in &st.shape {
        c.len().hash(&mut h);
        for v in c {
            (v[0].to_bits(), v[1].to_bits()).hash(&mut h);
        }
    }
    h.finish()
}

impl VectorMeshes {
    /// Call once per frame before drawing: forget lines not drawn lately.
    pub fn begin_frame(&mut self) {
        self.frame += 1;
        self.spent = std::time::Duration::ZERO;
        self.behind = false;
        let frame = self.frame;
        for map in [&mut self.entries, &mut self.fills] {
            if map.len() > 4096 {
                map.retain(|_, e| frame - e.seen < 2);
            } else if frame.is_multiple_of(120) {
                map.retain(|_, e| frame - e.seen < 600);
            }
        }
    }

    /// The scene's edit counter for this frame. Lines are only re-checked
    /// for changes when it moves.
    pub fn set_version(&mut self, version: u64) {
        self.version = version;
    }

    /// Draw an imported image under `m` (uploaded to the GPU once).
    pub fn paint_image(
        &mut self,
        painter: &egui::Painter,
        scene: &symphony_core::doc::Scene,
        img: symphony_core::stroke::PlacedImage,
        m: Affine,
    ) {
        let [x, y, w, h] = img.rect;
        let corners = [(x, y), (x + w, y), (x + w, y + h), (x, y + h)].map(|(px, py)| {
            let (sx, sy) = m.apply(px, py);
            Pos2::new(sx, sy)
        });
        if !Rect::from_points(&corners).intersects(painter.clip_rect()) {
            return;
        }
        let texture = match self.images.entry(img.media) {
            std::collections::hash_map::Entry::Occupied(e) => e.into_mut(),
            std::collections::hash_map::Entry::Vacant(e) => {
                let Some(pix) = symphony_render::decoded_image(scene, img.media) else {
                    return;
                };
                e.insert(painter.ctx().load_texture(
                    format!("imported-{:x}", img.media),
                    egui::ColorImage::from_rgba_premultiplied(
                        [pix.width() as usize, pix.height() as usize],
                        pix.data(),
                    ),
                    egui::TextureOptions::LINEAR,
                ))
            }
        };
        let mut mesh = egui::Mesh::with_texture(texture.id());
        let uv = [
            Pos2::new(0.0, 0.0),
            Pos2::new(1.0, 0.0),
            Pos2::new(1.0, 1.0),
            Pos2::new(0.0, 1.0),
        ];
        for (pos, uv) in corners.into_iter().zip(uv) {
            mesh.vertices.push(egui::epaint::Vertex {
                pos,
                uv,
                color: Color32::WHITE,
            });
        }
        mesh.indices.extend([0, 1, 2, 0, 2, 3]);
        painter.add(egui::Shape::mesh(mesh));
    }

    /// Draw a paint fill under `m`, triangulated once per shape.
    pub fn paint_fill(
        &mut self,
        painter: &egui::Painter,
        fill: &symphony_core::stroke::Fill,
        colour: Color32,
        m: Affine,
    ) {
        // Zones only change when the scene does: between edits the
        // outline isn't re-hashed (imported zones can have many points).
        let version = self.version;
        let unchanged = self
            .fills
            .get(&fill.id)
            .is_some_and(|e| e.checked == version && version != 0 && e.seen != u64::MAX);
        let fp = if unchanged {
            self.fills[&fill.id].fingerprint
        } else {
            let mut h = std::collections::hash_map::DefaultHasher::new();
            for c in &fill.contours {
                c.len().hash(&mut h);
                for v in c {
                    (v[0].to_bits(), v[1].to_bits()).hash(&mut h);
                }
            }
            h.finish()
        };
        let frame = self.frame;
        let e = self
            .fills
            .entry(fill.id)
            .and_modify(|e| {
                if e.fingerprint != fp {
                    e.seen = u64::MAX;
                }
            })
            .or_insert_with(|| Entry {
                fingerprint: 0,
                lod: 1.0,
                vertices: Vec::new(),
                indices: Vec::new(),
                checked: 0,
                lo: [0.0; 2],
                hi: [0.0; 2],
                seen: u64::MAX,
            });
        if e.seen == u64::MAX || e.fingerprint != fp {
            self.built += 1;
            let mesh = symphony_vector::fill::fill_mesh(&fill.contours, Affine::IDENTITY);
            let (mut lo, mut hi) = ([f32::INFINITY; 2], [f32::NEG_INFINITY; 2]);
            for v in &mesh.vertices {
                lo = [lo[0].min(v[0]), lo[1].min(v[1])];
                hi = [hi[0].max(v[0]), hi[1].max(v[1])];
            }
            *e = Entry {
                fingerprint: fp,
                lod: 1.0,
                vertices: mesh.vertices,
                indices: mesh.indices,
                checked: version,
                lo,
                hi,
                seen: frame,
            };
        }
        e.seen = frame;
        e.checked = version;
        draw(painter, e, colour, m);
    }

    /// Draw `st` under `m` (drawing -> screen points).
    /// The stroke being drawn: it isn't in the scene yet, so the scene's
    /// edit counter never moves while it grows. Re-check it every frame,
    /// or the line only appears when the pen lifts.
    pub fn paint_live(&mut self, painter: &egui::Painter, st: &Stroke, colour: Color32, m: Affine) {
        if let Some(e) = self.entries.get_mut(&st.id) {
            e.checked = 0;
        }
        self.paint(painter, st, colour, m);
    }

    pub fn paint(&mut self, painter: &egui::Painter, st: &Stroke, colour: Color32, m: Affine) {
        let k = symphony_vector::scale_of(m);
        if !(k.is_finite() && k > 0.0) {
            return;
        }
        let version = self.version;
        let unchanged = self
            .entries
            .get(&st.id)
            .is_some_and(|e| e.checked == version && version != 0);
        let fp = if unchanged {
            self.entries[&st.id].fingerprint
        } else {
            fingerprint(st)
        };
        let frame = self.frame;
        if let Some(e) = self.entries.get_mut(&st.id).filter(|e| e.fingerprint == fp) {
            e.checked = version;
        }
        let entry = self.entries.get(&st.id).filter(|e| e.fingerprint == fp);
        // Cull before any work: from the cached bounds, or the line's own.
        let (lo, hi) = match entry {
            Some(e) => (e.lo, e.hi),
            None => match symphony_vector::bounds(st) {
                Some((lo, hi)) => {
                    let pad = st.max_width.max(st.min_width) * 0.5 + 1.0;
                    ([lo[0] - pad, lo[1] - pad], [hi[0] + pad, hi[1] + pad])
                }
                None => return,
            },
        };
        let corners = [
            m.apply(lo[0], lo[1]),
            m.apply(hi[0], lo[1]),
            m.apply(hi[0], hi[1]),
            m.apply(lo[0], hi[1]),
        ];
        let screen = Rect::from_points(&corners.map(|(x, y)| Pos2::new(x, y)));
        if !screen.intersects(painter.clip_rect()) {
            if let Some(e) = self.entries.get_mut(&st.id) {
                e.seen = frame;
            }
            return;
        }
        // Valid while the zoom is within the octave below its level, so
        // there's always at least as much detail as this zoom needs.
        let fresh = entry.is_some_and(|e| e.lod >= k * 0.999 && e.lod <= k * 2.001);
        let stale_but_usable = entry.is_some() && !fresh;
        let can_wait = stale_but_usable && self.spent >= BUDGET;
        if !fresh && !can_wait {
            let start = std::time::Instant::now();
            let lod = 2f32.powf(k.log2().ceil()).max(1.0 / 64.0);
            self.built += 1;
            let mesh = symphony_vector::tessellate(st, Affine::scale(lod, lod), 0.6);
            let inv = 1.0 / lod;
            let vertices: Vec<[f32; 2]> = mesh
                .vertices
                .iter()
                .map(|v| [v[0] * inv, v[1] * inv])
                .collect();
            let (mut lo, mut hi) = ([f32::INFINITY; 2], [f32::NEG_INFINITY; 2]);
            for v in &vertices {
                lo = [lo[0].min(v[0]), lo[1].min(v[1])];
                hi = [hi[0].max(v[0]), hi[1].max(v[1])];
            }
            self.entries.insert(
                st.id,
                Entry {
                    fingerprint: fp,
                    lod,
                    vertices,
                    indices: mesh.indices,
                    checked: version,
                    lo,
                    hi,
                    seen: frame,
                },
            );
            self.spent += start.elapsed();
        } else if can_wait {
            self.behind = true;
            painter.ctx().request_repaint();
        }
        let Some(e) = self.entries.get_mut(&st.id) else {
            return;
        };
        e.seen = frame;
        draw(painter, e, colour, m);
    }
}

/// Cached triangles to the screen, skipped when outside the view.
fn draw(painter: &egui::Painter, e: &Entry, colour: Color32, m: Affine) {
    if e.indices.is_empty() {
        return;
    }
    let corners = [
        m.apply(e.lo[0], e.lo[1]),
        m.apply(e.hi[0], e.lo[1]),
        m.apply(e.hi[0], e.hi[1]),
        m.apply(e.lo[0], e.hi[1]),
    ];
    if !Rect::from_points(&corners.map(|(x, y)| Pos2::new(x, y))).intersects(painter.clip_rect()) {
        return;
    }
    let mut out = egui::Mesh::default();
    out.vertices.reserve(e.vertices.len());
    for v in &e.vertices {
        let (x, y) = m.apply(v[0], v[1]);
        out.colored_vertex(Pos2::new(x, y), colour);
    }
    out.indices.clone_from(&e.indices);
    painter.add(egui::Shape::mesh(out));
}

#[cfg(test)]
mod tests {
    use super::*;
    use symphony_core::stroke::{Point, StrokeKind};

    fn sketch() -> Vec<Stroke> {
        (0..600)
            .map(|i| Stroke {
                id: Id(i),
                kind: StrokeKind::Pencil,
                art: Default::default(),
                swatch: Id(0),
                min_width: 1.0,
                max_width: 4.0,
                points: (0..60)
                    .map(|k| Point {
                        x: -900.0 + (i % 30) as f32 * 60.0 + k as f32 * 3.0,
                        y: -500.0 + (i / 30) as f32 * 50.0 + (k as f32 * 0.4).sin() * 12.0,
                        pressure: 0.5 + 0.5 * (k as f32 * 0.2).sin(),
                        width_offset: 0.0,
                        pen: Default::default(),
                    })
                    .collect(),
                tips: Default::default(),
                texture: None,
                shape: Vec::new(),
            })
            .collect()
    }

    /// Frame cost of drawing a dense sketch while zooming, on a 2560x1440
    /// screen: `cargo test --release -p symphony-app zoom_cost -- --ignored
    /// --nocapture`. Not pass/fail.
    #[test]
    #[ignore]
    fn zoom_cost() {
        let strokes = sketch();
        let ctx = egui::Context::default();
        let mut cache = VectorMeshes::default();
        let screen = Rect::from_min_size(Pos2::ZERO, egui::vec2(2560.0, 1440.0));
        for zoom in [1.0f32, 4.0, 16.0] {
            let m = Affine::translate(1280.0, 720.0).compose(Affine::scale(zoom, zoom));
            let mut times = Vec::new();
            for step in 0..3 {
                // Zooming within the octave: a slightly different scale each frame.
                let m = m.compose(Affine::scale(
                    1.0 - step as f32 * 0.1,
                    1.0 - step as f32 * 0.1,
                ));
                let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
                    let painter = ui.painter().with_clip_rect(screen);
                    cache.begin_frame();
                    let start = std::time::Instant::now();
                    for st in &strokes {
                        cache.paint(&painter, st, Color32::BLACK, m);
                    }
                    times.push(start.elapsed());
                });
                out.textures_delta.clear();
            }
            println!(
                "zoom {zoom}x: first frame {:?}, then {:?}, {:?}",
                times[0], times[1], times[2]
            );
        }
    }

    /// The line being drawn grows on screen every frame, even though the
    /// scene (and its edit counter) doesn't change until the pen lifts.
    #[test]
    fn the_line_being_drawn_grows_every_frame() {
        let mut st = sketch().remove(0);
        st.kind = StrokeKind::Brush;
        let full = st.points.clone();
        let ctx = egui::Context::default();
        let mut cache = VectorMeshes::default();
        cache.set_version(7);
        let screen = Rect::from_min_size(Pos2::ZERO, egui::vec2(2000.0, 1200.0));
        let m = Affine::translate(1000.0, 600.0);
        let mut widths = Vec::new();
        for n in [5, 20, 60] {
            st.points = full[..n].to_vec();
            let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
                let painter = ui.painter().with_clip_rect(screen);
                cache.begin_frame();
                cache.paint_live(&painter, &st, Color32::BLACK, m);
            });
            out.textures_delta.clear();
            let width = out
                .shapes
                .iter()
                .map(|s| s.shape.visual_bounding_rect())
                .filter(|r| r.is_finite())
                .fold(Rect::NOTHING, |a, r| a.union(r))
                .width();
            widths.push(width);
        }
        assert!(widths[0] < widths[1] && widths[1] < widths[2], "{widths:?}");
    }

    #[test]
    fn edited_lines_are_rebuilt_and_far_ones_skipped() {
        let mut strokes = sketch();
        strokes.truncate(2);
        let ctx = egui::Context::default();
        let mut cache = VectorMeshes::default();
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            let painter = ui
                .painter()
                .with_clip_rect(Rect::from_min_size(Pos2::ZERO, egui::vec2(4000.0, 4000.0)));
            // In view: built and cached.
            let near = Affine::translate(2000.0, 2000.0);
            cache.paint(&painter, &strokes[0], Color32::BLACK, near);
            let before = cache.entries[&strokes[0].id].fingerprint;
            // Edited: rebuilt.
            strokes[0].points[3].x += 5.0;
            cache.paint(&painter, &strokes[0], Color32::BLACK, near);
            assert_ne!(cache.entries[&strokes[0].id].fingerprint, before);
            // Far out of view: never even tessellated.
            cache.paint(
                &painter,
                &strokes[1],
                Color32::BLACK,
                Affine::translate(1.0e6, 0.0),
            );
            assert!(!cache.entries.contains_key(&strokes[1].id));
        });
        out.textures_delta.clear();
    }
}
