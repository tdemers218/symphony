//! Textured vector strokes on the canvas. Each stroke's coverage comes
//! from `symphony_vector::stamp` (the same code export uses) and is cached
//! as a texture. Panning moves it, and small zoom changes scale the
//! cached image until the next resolution band.
//!
//! Stamping can take tens of milliseconds for a big brush, so it never
//! blocks a frame (DESIGN §17). Finished strokes are re-stamped on a
//! background thread, and the last image is shown moved to the new
//! position until then. The stroke being drawn is stamped a piece at a
//! time ([`Live`]).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};

use eframe::egui::{self, Color32, Pos2, Rect};
use symphony_core::id::Id;
use symphony_core::math::Affine;
use symphony_core::stroke::{Stroke, StrokeTexture, Texture};
use symphony_vector::stamp::{Coverage, Stamper};

/// A stamped stroke on the GPU, and the drawing -> pixel transform it was
/// stamped with (so it can be shown moved while a new one is made).
#[derive(Clone)]
struct Image {
    handle: egui::TextureHandle,
    /// Pixel rectangle [x, y, w, h] in the space of `px`.
    rect: [f32; 4],
    px: Affine,
}

impl Image {
    fn upload(ctx: &egui::Context, c: &Coverage, px: Affine, name: &str) -> Self {
        let pixels: Vec<Color32> = c
            .data
            .iter()
            .map(|&a| Color32::from_white_alpha(a))
            .collect();
        let image = egui::ColorImage {
            size: [c.width as usize, c.height as usize],
            source_size: egui::vec2(c.width as f32, c.height as f32),
            pixels,
        };
        Image {
            handle: ctx.load_texture(name, image, egui::TextureOptions::LINEAR),
            rect: [c.x as f32, c.y as f32, c.width as f32, c.height as f32],
            px,
        }
    }

    /// Draw it for the transform `now` (drawing -> pixels): exact when
    /// `now` is the transform it was made with, moved otherwise.
    fn draw(&self, painter: &egui::Painter, now: Affine, ppp: f32, colour: Color32) {
        let Some(back) = self.px.inverse() else {
            return;
        };
        let map = now.compose(back);
        let [x, y, w, h] = self.rect;
        let corner = |cx: f32, cy: f32| {
            let (px, py) = map.apply(cx, cy);
            Pos2::new(px / ppp, py / ppp)
        };
        let mut mesh = egui::Mesh::with_texture(self.handle.id());
        for (cx, cy, u, v) in [
            (x, y, 0.0, 0.0),
            (x + w, y, 1.0, 0.0),
            (x + w, y + h, 1.0, 1.0),
            (x, y + h, 0.0, 1.0),
        ] {
            mesh.vertices.push(egui::epaint::Vertex {
                pos: corner(cx, cy),
                uv: Pos2::new(u, v),
                color: colour,
            });
        }
        mesh.indices = vec![0, 1, 2, 0, 2, 3];
        painter.add(egui::Shape::mesh(mesh));
    }

    /// Show the original stamped texture through the surviving outline
    /// while an exact re-stamp is in flight.
    fn draw_clipped(
        &self,
        painter: &egui::Painter,
        shape: &symphony_vector::Mesh,
        screen: Affine,
        colour: Color32,
    ) -> bool {
        let Some(to_drawing) = self.px.inverse() else {
            return false;
        };
        let mapping = screen.compose(to_drawing);
        // Cached images at other pan or zoom levels are usable here when
        // their crop still maps to an axis-aligned rectangle.
        if mapping.b.abs() > 0.001 || mapping.c.abs() > 0.001 {
            return false;
        }
        let [x, y, w, h] = self.rect;
        let rect = Rect::from_two_pos(to_screen(mapping, x, y), to_screen(mapping, x + w, y + h));
        if !rect.intersects(painter.clip_rect()) {
            return false;
        }
        let mut mesh = egui::Mesh::with_texture(self.handle.id());
        mesh.vertices.reserve(shape.vertices.len());
        for &[dx, dy] in &shape.vertices {
            let (sx, sy) = screen.apply(dx, dy);
            let (tx, ty) = self.px.apply(dx, dy);
            mesh.vertices.push(egui::epaint::Vertex {
                pos: Pos2::new(sx, sy),
                uv: Pos2::new((tx - x) / w, (ty - y) / h),
                color: colour,
            });
        }
        mesh.indices = shape.indices.clone();
        painter.with_clip_rect(rect).add(egui::Shape::mesh(mesh));
        true
    }
}

fn to_screen(m: Affine, x: f32, y: f32) -> Pos2 {
    let (x, y) = m.apply(x, y);
    Pos2::new(x, y)
}

struct Job {
    key: u64,
    stroke: Stroke,
    tex: StrokeTexture,
    tip: Option<Arc<Texture>>,
    grain: Option<Arc<Texture>>,
    local: Affine,
    clip: [i32; 4],
}

struct Done {
    key: u64,
    id: Id,
    coverage: Option<Coverage>,
    cancelled: bool,
    local: Affine,
}

struct Worker {
    jobs: Sender<Job>,
    done: Receiver<Done>,
    /// The newest key wanted per stroke; older queued jobs are skipped.
    latest: Arc<Mutex<HashMap<Id, u64>>>,
}

impl Worker {
    fn start(ctx: &egui::Context) -> Self {
        let (jobs, rx) = channel::<Job>();
        let (tx, done) = channel();
        let latest: Arc<Mutex<HashMap<Id, u64>>> = Arc::default();
        // A few workers share the queue, so several strokes stamp at once
        // (each also splits into parallel bands across the cores).
        let rx = Arc::new(Mutex::new(rx));
        for n in 0..STAMP_WORKERS {
            let (rx, tx, wanted, ctx) = (rx.clone(), tx.clone(), latest.clone(), ctx.clone());
            std::thread::Builder::new()
                .name(format!("symphony-stamp-{n}"))
                .spawn(move || loop {
                    let job = match rx.lock().map(|r| r.recv()) {
                        Ok(Ok(job)) => job,
                        _ => break,
                    };
                    let current = wanted
                        .lock()
                        .map(|m| m.get(&job.stroke.id) == Some(&job.key))
                        .unwrap_or(true);
                    let coverage = current
                        .then(|| {
                            symphony_vector::stamp::rasterize(
                                &job.stroke,
                                &job.tex,
                                job.tip.as_deref(),
                                job.grain.as_deref(),
                                job.local,
                                job.clip,
                            )
                        })
                        .flatten();
                    if tx
                        .send(Done {
                            key: job.key,
                            id: job.stroke.id,
                            coverage,
                            cancelled: !current,
                            local: job.local,
                        })
                        .is_err()
                    {
                        break;
                    }
                    ctx.request_repaint();
                })
                .ok();
        }
        Worker { jobs, done, latest }
    }
}

struct Entry {
    image: Option<Image>,
    used: u64,
}

struct InkPreview {
    image: Image,
    used: u64,
    uncut: bool,
}

#[derive(Default)]
pub struct Cache {
    entries: HashMap<u64, Entry>,
    /// The last image shown for each stroke, to stand in while re-stamping.
    last: HashMap<Id, (Image, u64)>,
    /// Recent texture by ink content, independent of view and stroke id.
    /// Prefer the original, which can cover every surviving piece.
    ink_history: HashMap<u64, InkPreview>,
    pending: HashSet<u64>,
    textures: HashMap<u64, Arc<Texture>>,
    worker: Option<Worker>,
    frame: u64,
    sync_remaining: f32,
    /// Quick low-resolution stamps shown while a stroke's new shape is
    /// stamped in full in the background (editing), by full key.
    low_res: HashMap<u64, (Image, u64)>,
    /// Vector brush strokes: their merged stamp outline, triangulated in
    /// drawing space, by content key.
    stamped: HashMap<u64, (Arc<symphony_vector::Mesh>, u64)>,
    /// The last outline drawn for each vector-brush stroke, to stand in
    /// while its new shape is built.
    stamped_last: HashMap<Id, (Arc<symphony_vector::Mesh>, u64)>,
    outline_worker: Option<OutlineWorker>,
    outlines_pending: HashSet<u64>,
    /// Erased textured strokes' outlines, triangulated, by full key (shown
    /// clipping the old image while the exact stamp is made).
    cut_meshes: HashMap<u64, (Arc<symphony_vector::Mesh>, u64)>,
    /// Vector-brush stamps that may still be merged this frame.
    sync_stamps: f32,
    /// Plain vector lines' cached triangles (they draw through here too).
    pub vectors: super::vector_cache::VectorMeshes,
    /// Work done on the UI thread so far, so tests can check that an
    /// unchanged frame does none (ENGINEERING E6.1).
    pub work: Work,
}

/// Counts of slow work done while drawing (totals since start).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Work {
    /// Textured strokes stamped (full or preview).
    pub stamps: u64,
    /// Vector-brush or cut outlines triangulated.
    pub triangulated: u64,
}

/// Stamped strokes are rebuilt per this many pixels of clip window, so
/// small pans reuse them.
const CLIP_GRID: f32 = 512.0;
/// Most cached strokes; old ones are dropped first.
const MAX_ENTRIES: usize = 512;
/// Total synchronous stamp work per canvas frame, in estimated pixels.
/// (Stamping runs across all cores, so this is a few milliseconds.)
const SYNC_BUDGET: f32 = 2_400_000.0;
/// Background stamping threads (each stroke also splits across cores).
const STAMP_WORKERS: usize = 3;
const UPLOADS_PER_FRAME: usize = 4;
/// Vector-brush stamps merged right away per frame (about 2 ms); longer
/// strokes are built in the background.
const SYNC_STAMPS: f32 = 600.0;
/// The smallest scale a quick edit preview is stamped at; below this it
/// would be mush, so the full stamp is awaited instead.
const MIN_PREVIEW_SCALE: f32 = 0.08;

fn stamp_view(px: Affine, visible: Rect, ppp: f32) -> (Affine, [i32; 4]) {
    let scale = symphony_vector::scale_of(px).max(1e-6);
    let lod = ((scale.log2() * 2.0).round() * 0.5).exp2();
    // Keep the orientation stable as zoom multiplies the affine matrix.
    let step = |v: f32| ((v / scale) * 65_536.0).round() / 65_536.0 * lod;
    let local = Affine {
        a: step(px.a),
        b: step(px.b),
        c: step(px.c),
        d: step(px.d),
        tx: 0.0,
        ty: 0.0,
    };
    let bounds = px
        .inverse()
        .map(|inverse| {
            let project = local.compose(inverse);
            Rect::from_points(
                &[
                    visible.left_top(),
                    visible.right_top(),
                    visible.right_bottom(),
                    visible.left_bottom(),
                ]
                .map(|point| to_screen(project, point.x * ppp, point.y * ppp)),
            )
        })
        .unwrap_or(Rect::ZERO);
    let snap = |v: f32, up: bool| {
        let g = (v / CLIP_GRID).floor() + if up { 2.0 } else { -1.0 };
        (g * CLIP_GRID) as i32
    };
    (
        local,
        [
            snap(bounds.left(), false),
            snap(bounds.top(), false),
            snap(bounds.right(), true),
            snap(bounds.bottom(), true),
        ],
    )
}

impl Cache {
    /// Call once per frame before drawing: collect finished stamping and
    /// forget strokes not drawn lately.
    pub fn begin_frame(&mut self, ctx: &egui::Context) {
        self.frame += 1;
        if std::env::var_os("SYMPHONY_PERF").is_some() && self.frame.is_multiple_of(120) {
            let bytes = |img: &Image| (img.rect[2] * img.rect[3] * 4.0) as u64;
            let entries: u64 = self
                .entries
                .values()
                .filter_map(|e| e.image.as_ref())
                .map(bytes)
                .sum();
            let last: u64 = self.last.values().map(|(i, _)| bytes(i)).sum();
            let hist: u64 = self.ink_history.values().map(|p| bytes(&p.image)).sum();
            let low: u64 = self.low_res.values().map(|(i, _)| bytes(i)).sum();
            eprintln!(
                "stroke images: {} cached ({} MB), last {} MB, ink history {} ({} MB), previews {} MB",
                self.entries.len(),
                entries >> 20,
                last >> 20,
                self.ink_history.len(),
                hist >> 20,
                low >> 20
            );
        }
        self.sync_remaining = SYNC_BUDGET;
        self.sync_stamps = SYNC_STAMPS;
        if let Some(w) = &self.outline_worker {
            while let Ok(d) = w.done.try_recv() {
                self.outlines_pending.remove(&d.key);
                let current = w
                    .latest
                    .lock()
                    .map(|l| l.get(&d.id) == Some(&d.key))
                    .unwrap_or(true);
                if let Some(mesh) = d.mesh.filter(|_| current) {
                    self.stamped.insert(d.key, (mesh, self.frame));
                    ctx.request_repaint();
                }
            }
        }
        self.vectors.begin_frame();
        if let Some(w) = &self.worker {
            for _ in 0..UPLOADS_PER_FRAME {
                let Ok(d) = w.done.try_recv() else {
                    break;
                };
                self.pending.remove(&d.key);
                let current = w
                    .latest
                    .lock()
                    .map(|l| l.get(&d.id) == Some(&d.key))
                    .unwrap_or(true);
                if current && !d.cancelled {
                    let image = d
                        .coverage
                        .as_ref()
                        .map(|c| Image::upload(ctx, c, d.local, "stroke"));
                    self.entries.insert(
                        d.key,
                        Entry {
                            image,
                            used: self.frame,
                        },
                    );
                }
                // There may be more completions queued after the upload budget.
                ctx.request_repaint();
            }
        }
        let keep_after = self.frame.saturating_sub(2);
        self.entries.retain(|_, e| e.used >= keep_after);
        self.last.retain(|_, (_, used)| *used >= keep_after);
        self.low_res.retain(|_, (_, used)| *used >= keep_after);
        let keep_stamped_after = self.frame.saturating_sub(120);
        self.stamped
            .retain(|_, (_, used)| *used >= keep_stamped_after);
        self.stamped_last
            .retain(|_, (_, used)| *used >= keep_stamped_after);
        self.cut_meshes.retain(|_, (_, used)| *used >= keep_after);
        let keep_preview_after = self.frame.saturating_sub(16);
        self.ink_history
            .retain(|_, preview| preview.used >= keep_preview_after);
        if self.entries.len() > MAX_ENTRIES {
            let mut ages: Vec<u64> = self.entries.values().map(|e| e.used).collect();
            ages.sort_unstable();
            let cut = ages[ages.len() - MAX_ENTRIES];
            self.entries.retain(|_, e| e.used >= cut);
        }
    }

    fn texture(
        &mut self,
        textures: &BTreeMap<u64, Texture>,
        key: Option<u64>,
    ) -> Option<Arc<Texture>> {
        let key = key?;
        if let Some(t) = self.textures.get(&key) {
            return Some(t.clone());
        }
        let t = Arc::new(textures.get(&key)?.clone());
        self.textures.insert(key, t.clone());
        Some(t)
    }

    /// Hand over the image of a stroke that was just drawn, so it shows
    /// without a gap once it's committed.
    pub fn adopt(&mut self, id: Id, live: Live) {
        if let Some(image) = live.image {
            self.last.insert(id, (image, self.frame));
        }
    }

    fn remember_preview(&mut self, ink: u64, image: Image, uncut: bool) {
        if let Some(previous) = self.ink_history.get_mut(&ink) {
            if uncut || !previous.uncut {
                *previous = InkPreview {
                    image,
                    used: self.frame,
                    uncut,
                };
            }
        } else if self.ink_history.len() < MAX_ENTRIES {
            self.ink_history.insert(
                ink,
                InkPreview {
                    image,
                    used: self.frame,
                    uncut,
                },
            );
        }
    }

    /// Draw `stroke` with its texture under `m` (drawing -> screen points).
    #[allow(clippy::too_many_arguments)]
    pub fn paint(
        &mut self,
        ctx: &egui::Context,
        painter: &egui::Painter,
        textures: &BTreeMap<u64, Texture>,
        stroke: &Stroke,
        tex: &StrokeTexture,
        colour: Color32,
        m: Affine,
    ) {
        let ppp = ctx.pixels_per_point();
        let px = Affine::scale(ppp, ppp).compose(m);
        let (local, clip) = stamp_view(px, painter.clip_rect(), ppp);
        let (key, uncut_key, ink) = keys(stroke, tex, local, clip);
        if let Some(entry) = uncut_key.and_then(|key| self.entries.get_mut(&key)) {
            entry.used = self.frame;
        }
        if let Some(preview) = self.ink_history.get_mut(&ink) {
            preview.used = self.frame;
        }
        let frame = self.frame;
        if let Some(e) = self.entries.get_mut(&key) {
            e.used = frame;
            if let Some(img) = e.image.clone() {
                img.draw(painter, px, ppp, colour);
                self.last.insert(stroke.id, (img.clone(), frame));
                self.remember_preview(ink, img, uncut_key.is_none());
            }
            return;
        }
        let tip = self.texture(textures, tex.tip);
        let grain = self.texture(textures, tex.grain);
        let cost = stroke.points.len() as f32
            * (stroke.max_width * symphony_vector::scale_of(local)).powi(2)
            / (tex.spacing.max(0.01) * 4.0);
        // Small strokes are stamped now, whether new or just edited (the
        // Line Editor reshaping a line), so they never lag behind.
        if cost <= self.sync_remaining {
            self.sync_remaining -= cost;
            self.work.stamps += 1;
            let c = symphony_vector::stamp::rasterize(
                stroke,
                tex,
                tip.as_deref(),
                grain.as_deref(),
                local,
                clip,
            );
            let image = c.as_ref().map(|c| Image::upload(ctx, c, local, "stroke"));
            if let Some(img) = &image {
                img.draw(painter, px, ppp, colour);
                self.last.insert(stroke.id, (img.clone(), frame));
            }
            if let Some(img) = image.as_ref() {
                self.remember_preview(ink, img.clone(), uncut_key.is_none());
            }
            self.entries.insert(key, Entry { image, used: frame });
            return;
        }
        // A shape this cache hasn't seen yet (a stroke being reshaped, or
        // one just opened) gets a quick low-resolution stamp now, so it
        // shows its new shape at once, never a stale or blank one. The full
        // stamp replaces it when the background is done (once editing
        // stops, the stroke looks as drawn).
        let mut previewed = false;
        if uncut_key.is_none() && !self.ink_history.contains_key(&ink) {
            if let Some((img, used)) = self.low_res.get_mut(&key) {
                *used = frame;
                img.draw(painter, px, ppp, colour);
                previewed = true;
            } else {
                let s = (self.sync_remaining / cost.max(1.0)).sqrt().min(0.5);
                if s >= MIN_PREVIEW_SCALE {
                    self.sync_remaining -= cost * s * s;
                    self.work.stamps += 1;
                    let low = Affine::scale(s, s).compose(local);
                    let clip_low = [
                        (clip[0] as f32 * s).floor() as i32,
                        (clip[1] as f32 * s).floor() as i32,
                        (clip[2] as f32 * s).ceil() as i32,
                        (clip[3] as f32 * s).ceil() as i32,
                    ];
                    if let Some(c) = symphony_vector::stamp::rasterize(
                        stroke,
                        tex,
                        tip.as_deref(),
                        grain.as_deref(),
                        low,
                        clip_low,
                    ) {
                        let img = Image::upload(ctx, &c, low, "stroke-preview");
                        img.draw(painter, px, ppp, colour);
                        self.low_res.insert(key, (img, frame));
                        previewed = true;
                    }
                }
            }
        }
        // Expensive strokes stamp in the background. Keep a cached texture
        // visible when available, including a clipped preview for erasing.
        let clipped = previewed
            || uncut_key.is_some_and(|uncut| {
                let image = self
                    .entries
                    .get(&uncut)
                    .and_then(|e| e.image.clone())
                    .or_else(|| self.ink_history.get(&ink).map(|p| p.image.clone()));
                image.is_some_and(|image| {
                    // The cut outline, triangulated once (not every frame
                    // while the exact stamp is made).
                    let shape = match self.cut_meshes.get_mut(&key) {
                        Some((mesh, used)) => {
                            *used = frame;
                            mesh.clone()
                        }
                        None => {
                            self.work.triangulated += 1;
                            let mesh = Arc::new(symphony_vector::fill::fill_mesh(
                                &stroke.shape,
                                Affine::IDENTITY,
                            ));
                            self.cut_meshes.insert(key, (mesh.clone(), frame));
                            mesh
                        }
                    };
                    image.draw_clipped(painter, &shape, m, colour)
                })
            });
        if !clipped {
            match self.last.get_mut(&stroke.id) {
                Some((img, used)) => {
                    *used = frame;
                    img.draw(painter, px, ppp, colour);
                    if uncut_key.is_none() {
                        let image = img.clone();
                        self.remember_preview(ink, image, true);
                    }
                }
                None if stroke.shape.is_empty() => {
                    // A faint plain line while it stamps (cached, not
                    // re-tessellated every frame).
                    self.vectors
                        .paint(painter, stroke, colour.gamma_multiply(0.35), m);
                }
                // A genuinely cold cache has no textured image to show yet.
                None => {}
            }
        }
        let worker = self.worker.get_or_insert_with(|| Worker::start(ctx));
        if let Ok(mut latest) = worker.latest.lock() {
            latest.insert(stroke.id, key);
        }
        if self.pending.insert(key) {
            let _ = worker.jobs.send(Job {
                key,
                stroke: stroke.clone(),
                tex: *tex,
                tip,
                grain,
                local,
                clip,
            });
        }
    }
}

impl Cache {
    /// A vector brush stroke: the tip's shape stamped along the line and
    /// merged into one outline, drawn solid. The outline is worked out once
    /// per shape of the stroke (in drawing space) and only moved after.
    /// Long strokes are worked out in the background (E6.1: the UI thread
    /// only paints); until then the stroke's previous outline, or the
    /// image it was drawn with, stands in.
    pub fn paint_stamped(
        &mut self,
        painter: &egui::Painter,
        textures: &BTreeMap<u64, Texture>,
        stroke: &Stroke,
        tex: &StrokeTexture,
        colour: Color32,
        m: Affine,
    ) {
        let key = stamped_key(stroke, tex);
        let frame = self.frame;
        if let Some((mesh, used)) = self.stamped.get_mut(&key) {
            *used = frame;
            let mesh = mesh.clone();
            self.stamped_last.insert(stroke.id, (mesh.clone(), frame));
            draw_mesh(painter, &mesh, colour, m);
            return;
        }
        let tip = self.texture(textures, tex.tip);
        // Cheap now: an erased stroke (its outline is stored, only
        // triangulated here), an outline built already (by the eraser or
        // export), or a short stroke that fits this frame's budget.
        let cheap = !stroke.shape.is_empty()
            || symphony_vector::stamped::cached_outline(symphony_vector::stamped::outline_key(
                stroke, tex,
            ))
            .is_some()
            || {
                let n = symphony_vector::stamped::estimated_stamps(stroke, tex);
                let fits = n <= self.sync_stamps;
                if fits {
                    self.sync_stamps -= n;
                }
                fits
            };
        if cheap {
            self.work.triangulated += 1;
            let mesh = outline_mesh(stroke, tex, tip.as_deref());
            self.stamped.insert(key, (mesh.clone(), frame));
            self.stamped_last.insert(stroke.id, (mesh.clone(), frame));
            draw_mesh(painter, &mesh, colour, m);
            return;
        }
        let worker = self
            .outline_worker
            .get_or_insert_with(|| OutlineWorker::start(painter.ctx()));
        if let Ok(mut latest) = worker.latest.lock() {
            latest.insert(stroke.id, key);
        }
        if self.outlines_pending.insert(key) {
            let _ = worker.jobs.send(OutlineJob {
                key,
                stroke: stroke.clone(),
                tex: *tex,
                tip,
            });
        }
        // Meanwhile: the outline it had, the image it was drawn with, or
        // a faint plain line.
        if let Some((mesh, used)) = self.stamped_last.get_mut(&stroke.id) {
            *used = frame;
            let mesh = mesh.clone();
            draw_mesh(painter, &mesh, colour, m);
        } else if let Some((img, used)) = self.last.get_mut(&stroke.id) {
            *used = frame;
            let ppp = painter.ctx().pixels_per_point();
            img.draw(painter, Affine::scale(ppp, ppp).compose(m), ppp, colour);
        } else {
            self.vectors
                .paint(painter, stroke, colour.gamma_multiply(0.35), m);
        }
    }
}

/// What a vector-brush stroke's drawn outline depends on: its stamps, or
/// once erased, its stored outline.
fn stamped_key(stroke: &Stroke, tex: &StrokeTexture) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    symphony_vector::stamped::outline_key(stroke, tex).hash(&mut h);
    for ring in &stroke.shape {
        ring.len().hash(&mut h);
        for &[x, y] in ring {
            (x.to_bits(), y.to_bits()).hash(&mut h);
        }
    }
    h.finish()
}

/// A vector-brush stroke's outline, triangulated in drawing space.
fn outline_mesh(
    stroke: &Stroke,
    tex: &StrokeTexture,
    tip: Option<&Texture>,
) -> Arc<symphony_vector::Mesh> {
    let shapes = symphony_vector::stamped::vector_brush_shapes(stroke, tex, tip);
    let mut all = symphony_vector::Mesh::default();
    for shape in &shapes {
        let part = symphony_vector::fill::fill_mesh(shape, Affine::IDENTITY);
        let base = all.vertices.len() as u32;
        all.vertices.extend(part.vertices);
        all.indices.extend(part.indices.iter().map(|i| i + base));
    }
    Arc::new(all)
}

fn draw_mesh(painter: &egui::Painter, mesh: &symphony_vector::Mesh, colour: Color32, m: Affine) {
    if mesh.indices.is_empty() {
        return;
    }
    let mut out = egui::Mesh::default();
    out.vertices.reserve(mesh.vertices.len());
    for &[x, y] in &mesh.vertices {
        let (sx, sy) = m.apply(x, y);
        out.colored_vertex(Pos2::new(sx, sy), colour);
    }
    out.indices = mesh.indices.clone();
    painter.add(egui::Shape::mesh(out));
}

struct OutlineJob {
    key: u64,
    stroke: Stroke,
    tex: StrokeTexture,
    tip: Option<Arc<Texture>>,
}

struct OutlineDone {
    key: u64,
    id: Id,
    /// None when a newer shape of the stroke was wanted first.
    mesh: Option<Arc<symphony_vector::Mesh>>,
}

/// Builds vector-brush outlines off the UI thread (each build also merges
/// across the cores).
struct OutlineWorker {
    jobs: Sender<OutlineJob>,
    done: Receiver<OutlineDone>,
    /// The newest key wanted per stroke; older queued jobs are skipped.
    latest: Arc<Mutex<HashMap<Id, u64>>>,
}

impl OutlineWorker {
    fn start(ctx: &egui::Context) -> Self {
        let (jobs, rx) = channel::<OutlineJob>();
        let (tx, done) = channel();
        let latest: Arc<Mutex<HashMap<Id, u64>>> = Arc::default();
        let (wanted, ctx) = (latest.clone(), ctx.clone());
        std::thread::Builder::new()
            .name("symphony-outline".into())
            .spawn(move || {
                for job in rx {
                    let current = wanted
                        .lock()
                        .map(|m| m.get(&job.stroke.id) == Some(&job.key))
                        .unwrap_or(true);
                    let mesh =
                        current.then(|| outline_mesh(&job.stroke, &job.tex, job.tip.as_deref()));
                    let done = OutlineDone {
                        key: job.key,
                        id: job.stroke.id,
                        mesh,
                    };
                    if tx.send(done).is_err() {
                        break;
                    }
                    ctx.request_repaint();
                }
            })
            .ok();
        OutlineWorker { jobs, done, latest }
    }
}

/// The stroke being drawn, stamped a piece at a time.
pub struct Live {
    px: Affine,
    stamper: Stamper,
    image: Option<Image>,
    points: usize,
}

impl Live {
    /// Stamp what's new in `stroke` and draw it. Starts over if the view
    /// changed mid-stroke.
    #[allow(clippy::too_many_arguments)]
    pub fn paint(
        slot: &mut Option<Live>,
        ctx: &egui::Context,
        painter: &egui::Painter,
        textures: &BTreeMap<u64, Texture>,
        stroke: &Stroke,
        tex: &StrokeTexture,
        colour: Color32,
        m: Affine,
    ) {
        let ppp = ctx.pixels_per_point();
        let px = Affine::scale(ppp, ppp).compose(m);
        let visible = painter.clip_rect();
        let clip = [
            (visible.left() * ppp).floor() as i32,
            (visible.top() * ppp).floor() as i32,
            (visible.right() * ppp).ceil() as i32,
            (visible.bottom() * ppp).ceil() as i32,
        ];
        let restart = slot
            .as_ref()
            .is_none_or(|l| l.px != px || stroke.points.len() < l.points);
        if restart {
            *slot = Some(Live {
                px,
                stamper: Stamper::new(*tex, px, clip),
                image: None,
                points: 0,
            });
        }
        let Some(live) = slot.as_mut() else { return };
        let tip = tex.tip.and_then(|k| textures.get(&k));
        let grain = tex.grain.and_then(|k| textures.get(&k));
        if stroke.points.len() != live.points || live.image.is_none() {
            live.stamper.extend(stroke, tip, grain);
            live.points = stroke.points.len();
            if let Some(c) = live.stamper.coverage() {
                live.image = Some(Image::upload(ctx, &c, px, "live-stroke"));
            }
        }
        if let Some(img) = &live.image {
            img.draw(painter, px, ppp, colour);
        }
    }
}

fn keys(
    stroke: &Stroke,
    tex: &StrokeTexture,
    m: Affine,
    clip: [i32; 4],
) -> (u64, Option<u64>, u64) {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for p in &stroke.points {
        (
            p.x.to_bits(),
            p.y.to_bits(),
            p.pressure.to_bits(),
            p.width_offset.to_bits(),
        )
            .hash(&mut h);
    }
    (stroke.min_width.to_bits(), stroke.max_width.to_bits()).hash(&mut h);
    (
        tex.tip,
        tex.grain,
        tex.spacing.to_bits(),
        tex.opacity.to_bits(),
    )
        .hash(&mut h);
    (tex.angle.to_bits(), tex.follow, tex.grain_scale.to_bits()).hash(&mut h);
    (tex.flow.to_bits(), tex.hardness.to_bits()).hash(&mut h);
    for v in tex.opacity_by_pressure {
        v.to_bits().hash(&mut h);
    }
    for curve in [
        tex.opacity_curve,
        tex.size_curve,
        tex.flow_curve,
        tex.grain_strength,
    ] {
        curve.map(|curve| curve.0.map(f32::to_bits)).hash(&mut h);
    }
    (
        tex.grain_mode as u8,
        tex.grain_offset.map(f32::to_bits),
        tex.auto_spacing.to_bits(),
    )
        .hash(&mut h);
    // How the pen was held, and the brush's dynamics: both change the
    // stamps. With dynamics on, the stroke's own random seed (its id) does
    // too, so look-alike strokes don't share one picture.
    for p in &stroke.points {
        if !p.pen.is_level() {
            (
                p.pen.tilt.to_bits(),
                p.pen.azimuth.to_bits(),
                p.pen.rotation.to_bits(),
            )
                .hash(&mut h);
        }
    }
    if !tex.dynamics.is_still() {
        format!("{:?}", tex.dynamics).hash(&mut h);
        stroke.id.0.hash(&mut h);
    }
    tex.vector.hash(&mut h);
    let ink = h.finish();
    for v in [m.a, m.b, m.c, m.d, m.tx, m.ty] {
        v.to_bits().hash(&mut h);
    }
    clip.hash(&mut h);
    let base = h.finish();
    if stroke.shape.is_empty() {
        return (base, None, ink);
    }
    for ring in &stroke.shape {
        ring.len().hash(&mut h);
        for &[x, y] in ring {
            (x.to_bits(), y.to_bits()).hash(&mut h);
        }
    }
    (h.finish(), Some(base), ink)
}

/// A sample stroke drawn with a textured brush, for Tool Properties and
/// the preset list. Uses the library textures, so it works before any
/// stroke is drawn.
pub fn sample(
    painter: &egui::Painter,
    rect: Rect,
    tex: &StrokeTexture,
    min_width: f32,
    max_width: f32,
    ink: Color32,
) {
    use symphony_core::stroke::{Point, StrokeKind};
    let ctx = painter.ctx();
    let ppp = ctx.pixels_per_point();
    let n = 48;
    let max_width = max_width.min(rect.height() - 6.0).max(0.5);
    let min_width = min_width.min(max_width);
    let wave = (rect.height() * 0.15).min(8.0);
    let points: Vec<Point> = (0..=n)
        .map(|i| {
            let t = i as f32 / n as f32;
            Point {
                x: 10.0 + t * (rect.width() - 20.0),
                y: rect.height() * 0.5 + (t * std::f32::consts::TAU).sin() * wave,
                pressure: (t * std::f32::consts::PI).sin(),
                width_offset: 0.0,
                pen: Default::default(),
            }
        })
        .collect();
    let stroke = Stroke {
        id: symphony_core::id::Id(0),
        kind: StrokeKind::Brush,
        art: Default::default(),
        swatch: symphony_core::id::Id(0),
        min_width,
        max_width,
        points,
        tips: Default::default(),
        texture: Some(*tex),
        shape: Vec::new(),
    };
    let mut h = std::collections::hash_map::DefaultHasher::new();
    keys(&stroke, tex, Affine::scale(ppp, ppp), [0, 0, 0, 0])
        .0
        .hash(&mut h);
    (rect.width().to_bits(), rect.height().to_bits()).hash(&mut h);
    let id = egui::Id::unique(("brush-sample", h.finish()));
    // A vector brush: its merged stamp outline, drawn solid.
    if tex.vector {
        let mesh = ctx
            .data(|d| d.get_temp::<Arc<symphony_vector::Mesh>>(id))
            .unwrap_or_else(|| {
                let tip = tex.tip.and_then(crate::brush_import::library_texture);
                let mut all = symphony_vector::Mesh::default();
                for shape in
                    symphony_vector::stamped::vector_brush_shapes(&stroke, tex, tip.as_deref())
                {
                    let part = symphony_vector::fill::fill_mesh(&shape, Affine::IDENTITY);
                    let base = all.vertices.len() as u32;
                    all.vertices.extend(part.vertices);
                    all.indices.extend(part.indices.iter().map(|i| i + base));
                }
                let mesh = Arc::new(all);
                ctx.data_mut(|d| d.insert_temp(id, mesh.clone()));
                mesh
            });
        let mut out = egui::Mesh::default();
        for &[x, y] in &mesh.vertices {
            out.colored_vertex(rect.min + egui::vec2(x, y), ink);
        }
        out.indices = mesh.indices.clone();
        painter.add(egui::Shape::mesh(out));
        return;
    }
    let handle = ctx
        .data(|d| d.get_temp::<egui::TextureHandle>(id))
        .or_else(|| {
            let tip = tex.tip.and_then(crate::brush_import::library_texture);
            let grain = tex.grain.and_then(crate::brush_import::library_texture);
            let clip = [
                0,
                0,
                (rect.width() * ppp) as i32,
                (rect.height() * ppp) as i32,
            ];
            let c = symphony_vector::stamp::rasterize(
                &stroke,
                tex,
                tip.as_deref(),
                grain.as_deref(),
                Affine::scale(ppp, ppp),
                clip,
            )?;
            let mut pixels = vec![Color32::TRANSPARENT; (clip[2] * clip[3]).max(0) as usize];
            for row in 0..c.height as i32 {
                for col in 0..c.width as i32 {
                    let (x, y) = (c.x + col, c.y + row);
                    if x < clip[2] && y < clip[3] {
                        pixels[(y * clip[2] + x) as usize] = Color32::from_white_alpha(
                            c.data[(row * c.width as i32 + col) as usize],
                        );
                    }
                }
            }
            let image = egui::ColorImage {
                size: [clip[2] as usize, clip[3] as usize],
                source_size: egui::vec2(clip[2] as f32, clip[3] as f32),
                pixels,
            };
            let handle = ctx.load_texture("brush-sample", image, egui::TextureOptions::LINEAR);
            ctx.data_mut(|d| d.insert_temp(id, handle.clone()));
            Some(handle)
        });
    if let Some(handle) = handle {
        painter.image(
            handle.id(),
            rect,
            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
            ink,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use symphony_core::stroke::StrokeKind;

    #[test]
    fn stamped_view_reuses_a_band_across_pan_and_zoom() {
        let viewport = Rect::from_min_max(Pos2::new(10.0, 10.0), Pos2::new(810.0, 510.0));
        let at = |zoom, x, y| Affine::translate(x, y).compose(Affine::scale(zoom, zoom));
        let start = stamp_view(at(1.0, 70.0, 60.0), viewport, 1.0);
        assert_eq!(start, stamp_view(at(1.0, 70.35, 61.7), viewport, 1.0));
        assert_eq!(start, stamp_view(at(1.02, 70.35, 61.7), viewport, 1.0));
        assert_ne!(start.0, stamp_view(at(1.4, 70.0, 60.0), viewport, 1.0).0);
    }

    /// A line of `n` points with `texture`, for the work tests.
    fn line(id: u64, y: f32, n: usize, width: f32, texture: Option<StrokeTexture>) -> Stroke {
        Stroke {
            id: Id(id),
            kind: StrokeKind::Brush,
            art: Default::default(),
            swatch: Id(0),
            min_width: width,
            max_width: width,
            points: (0..n)
                .map(|i| symphony_core::stroke::Point {
                    x: 20.0 + i as f32 * 1.5,
                    y: y + (i as f32 * 0.1).sin() * 10.0,
                    pressure: 1.0,
                    width_offset: 0.0,
                    pen: Default::default(),
                })
                .collect(),
            tips: Default::default(),
            texture,
            shape: Vec::new(),
        }
    }

    /// The rule that keeps frames smooth (ENGINEERING E6.1): once a
    /// scene is drawn, drawing it again, or panning a little, rebuilds
    /// nothing. Every kind of stroke is here: plain lines, small and big
    /// textured strokes, short and long vector-brush strokes, and an
    /// erased textured stroke. (A stand-in that was rebuilt every frame
    /// once made a real scene stutter; this catches that kind of bug.)
    #[test]
    fn an_unchanged_frame_rebuilds_nothing() {
        let ctx = egui::Context::default();
        let viewport = Rect::from_min_size(Pos2::ZERO, egui::vec2(900.0, 700.0));
        let textured = StrokeTexture::default();
        let vector = StrokeTexture {
            vector: true,
            ..Default::default()
        };
        let mut cut = cut_stroke();
        cut.texture = Some(textured);
        cut.id = Id(99);
        let strokes = vec![
            line(1, 40.0, 40, 3.0, None),
            line(2, 100.0, 12, 4.0, Some(textured)),
            line(3, 180.0, 400, 60.0, Some(textured)),
            line(4, 300.0, 12, 4.0, Some(vector)),
            line(5, 400.0, 400, 2.0, Some(vector)),
            cut,
        ];
        let textures = BTreeMap::new();
        let mut cache = Cache::default();
        let mut draw = |m: Affine| {
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                cache.begin_frame(&ctx);
                let painter = ui.painter().with_clip_rect(viewport);
                for st in &strokes {
                    let tex = st.texture;
                    match tex {
                        Some(t) if t.vector => {
                            cache.paint_stamped(&painter, &textures, st, &t, Color32::WHITE, m)
                        }
                        Some(t) => {
                            cache.paint(&ctx, &painter, &textures, st, &t, Color32::WHITE, m)
                        }
                        None => cache.vectors.paint(&painter, st, Color32::WHITE, m),
                    }
                }
            });
            output.textures_delta.clear();
            let idle = cache.pending.is_empty() && cache.outlines_pending.is_empty();
            (cache.work, cache.vectors.built, idle)
        };
        let at = |dx: f32| Affine::translate(30.0 + dx, 20.0);
        // Let the background finish everything.
        let mut settled = draw(at(0.0));
        for _ in 0..400 {
            std::thread::sleep(std::time::Duration::from_millis(5));
            let now = draw(at(0.0));
            if now == settled && now.2 {
                break;
            }
            settled = now;
        }
        for frame in 0..6 {
            assert_eq!(
                draw(at(frame as f32 * 0.7)),
                settled,
                "frame {frame} rebuilt something"
            );
        }
    }

    #[test]
    #[ignore = "CPU navigation benchmark"]
    fn textured_navigation_cost() {
        use std::time::{Duration, Instant};
        let ctx = egui::Context::default();
        let viewport = Rect::from_min_size(Pos2::ZERO, egui::vec2(960.0, 640.0));
        let strokes: Vec<_> = (0..24)
            .map(|index| {
                let x = (index % 4) as f32 * 150.0;
                let y = (index / 4) as f32 * 60.0;
                Stroke {
                    id: Id(index + 1),
                    kind: StrokeKind::Brush,
                    art: Default::default(),
                    swatch: Id(0),
                    min_width: 21.0,
                    max_width: 28.0,
                    points: (0..60)
                        .map(|i| {
                            let t = i as f32;
                            symphony_core::stroke::Point {
                                x: x + t * 1.7,
                                y: y + (t * 0.15).sin() * 12.0,
                                pressure: 0.8,
                                width_offset: 0.0,
                                pen: Default::default(),
                            }
                        })
                        .collect(),
                    tips: Default::default(),
                    texture: None,
                    shape: Vec::new(),
                }
            })
            .collect();
        let tex = StrokeTexture::default();
        let textures = BTreeMap::new();
        let mut cache = Cache::default();
        let mut draw = |m| {
            let start = Instant::now();
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                cache.begin_frame(&ctx);
                let painter = ui.painter().with_clip_rect(viewport);
                for stroke in &strokes {
                    cache.paint(&ctx, &painter, &textures, stroke, &tex, Color32::WHITE, m);
                }
            });
            output.textures_delta.clear();
            start.elapsed()
        };
        let pose = |zoom, dx| Affine::translate(72.0 + dx, 55.0).compose(Affine::scale(zoom, zoom));
        for _ in 0..24 {
            draw(pose(1.0, 0.0));
            std::thread::sleep(Duration::from_millis(5));
        }
        for (label, moving) in [("pan", false), ("zoom", true)] {
            let mut timings: Vec<_> = (0..40)
                .map(|frame| {
                    let zoom = if moving {
                        1.0 + frame as f32 * 0.012
                    } else {
                        1.0
                    };
                    let time = draw(pose(zoom, frame as f32 * 0.41));
                    std::thread::sleep(Duration::from_millis(4));
                    time
                })
                .collect();
            timings.sort_unstable();
            println!(
                "textured {label}: median {:?}, p95 {:?}",
                timings[20], timings[37]
            );
        }
    }

    fn cut_stroke() -> Stroke {
        Stroke {
            id: Id(1),
            kind: StrokeKind::Brush,
            art: Default::default(),
            swatch: Id(0),
            min_width: 20.0,
            max_width: 20.0,
            points: Vec::new(),
            tips: Default::default(),
            texture: None,
            shape: vec![
                vec![[10.0, 10.0], [30.0, 10.0], [30.0, 30.0], [10.0, 30.0]],
                vec![[15.0, 15.0], [15.0, 25.0], [25.0, 25.0], [25.0, 15.0]],
            ],
        }
    }

    /// A big stroke whose shape is new (being reshaped, or just opened)
    /// shows a quick low-resolution stamp at once, textured and in its new
    /// shape, while the full one is stamped in the background.
    #[test]
    fn a_reshaped_big_stroke_previews_at_low_resolution() {
        let ctx = egui::Context::default();
        let mut stroke = cut_stroke();
        stroke.shape.clear();
        stroke.max_width = 120.0;
        stroke.min_width = 120.0;
        stroke.points = (0..200)
            .map(|i| symphony_core::stroke::Point {
                x: i as f32 * 4.0,
                y: (i as f32 * 0.1).sin() * 60.0,
                pressure: 1.0,
                width_offset: 0.0,
                pen: Default::default(),
            })
            .collect();
        let mut cache = Cache::default();
        cache.begin_frame(&ctx);
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            cache.paint(
                &ctx,
                ui.painter(),
                &BTreeMap::new(),
                &stroke,
                &StrokeTexture::default(),
                Color32::WHITE,
                Affine::IDENTITY,
            );
        });
        output.textures_delta.clear();
        assert_eq!(cache.pending.len(), 1, "the full stamp is queued");
        let textured = output.shapes.iter().any(|s| {
            matches!(&s.shape, egui::Shape::Mesh(m) if m.texture_id != egui::TextureId::default())
        });
        assert!(textured, "a textured preview shows right away");
        let (img, _) = cache.low_res.values().next().expect("a low-res preview");
        // Full size here is 1 pixel per unit; the preview is at most half.
        let scale = symphony_vector::scale_of(img.px);
        assert!(scale <= 0.51, "stamped smaller than full size: {scale}");
    }

    #[test]
    fn uncached_cut_waits_for_its_texture() {
        let ctx = egui::Context::default();
        let mut stroke = cut_stroke();
        stroke.max_width = 100.0;
        stroke.points = vec![
            symphony_core::stroke::Point {
                x: 20.0,
                y: 20.0,
                pressure: 1.0,
                width_offset: 0.0,
                pen: Default::default(),
            };
            128
        ];
        let mut cache = Cache::default();
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            cache.paint(
                &ctx,
                ui.painter(),
                &BTreeMap::new(),
                &stroke,
                &StrokeTexture::default(),
                Color32::WHITE,
                Affine::IDENTITY,
            );
        });
        output.textures_delta.clear();
        assert_eq!(cache.pending.len(), 1);
        assert!(!output
            .shapes
            .iter()
            .any(|shape| matches!(shape.shape, egui::Shape::Mesh(_))));
    }

    #[test]
    fn a_recent_texture_survives_a_new_stroke_id_and_zoom_level() {
        let ctx = egui::Context::default();
        let mut stroke = cut_stroke();
        stroke.points = vec![symphony_core::stroke::Point {
            x: 20.0,
            y: 20.0,
            pressure: 1.0,
            width_offset: 0.0,
            pen: Default::default(),
        }];

        let tex = StrokeTexture::default();
        let mut cache = Cache::default();
        let (_, _, ink) = keys(&stroke, &tex, Affine::IDENTITY, [0; 4]);
        let image = Image::upload(
            &ctx,
            &Coverage {
                x: 10,
                y: 10,
                width: 20,
                height: 20,
                data: vec![180; 400],
            },
            Affine::IDENTITY,
            "recent erase preview",
        );
        let texture_id = image.handle.id();
        cache.remember_preview(ink, image, true);
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            cache.paint(
                &ctx,
                ui.painter(),
                &BTreeMap::new(),
                &stroke,
                &tex,
                Color32::WHITE,
                Affine::scale(1.4, 1.4),
            );
        });
        output.textures_delta.clear();
        assert!(output.shapes.iter().any(|shape| {
            matches!(&shape.shape, egui::Shape::Mesh(mesh) if mesh.texture_id == texture_id)
        }));
    }

    #[test]
    fn cached_erase_preview_keeps_the_texture_and_excludes_the_hole() {
        let ctx = egui::Context::default();
        let cut = cut_stroke();
        let coverage = Coverage {
            x: 10,
            y: 10,
            width: 20,
            height: 20,
            data: vec![180; 400],
        };
        let image = Image::upload(&ctx, &coverage, Affine::IDENTITY, "erase preview test");
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            let shape = symphony_vector::fill::fill_mesh(&cut.shape, Affine::IDENTITY);
            image.draw_clipped(ui.painter(), &shape, Affine::IDENTITY, Color32::WHITE);
        });
        output.textures_delta.clear();
        let mesh = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Mesh(mesh) if mesh.texture_id == image.handle.id() => Some(mesh),
                _ => None,
            })
            .expect("cut geometry must use the original texture");
        assert!(mesh
            .vertices
            .iter()
            .any(|v| v.pos == Pos2::new(15.0, 15.0) && v.uv == Pos2::new(0.25, 0.25)));
        let area: f32 = mesh
            .indices
            .as_chunks::<3>()
            .0
            .iter()
            .map(|ids| {
                let [a, b, c] = ids.map(|i| mesh.vertices[i as usize].pos);
                ((b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x)).abs() * 0.5
            })
            .sum();
        assert_eq!(area, 300.0);
    }
}
