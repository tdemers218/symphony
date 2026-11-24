//! CPU rendering (ENGINEERING E10.2): a scene frame becomes an RGBA image.
//! Camera, export, and render tests share this graph and projection.
//!
//! Export runs on a background thread with progress and cancel (E5.2).

pub mod checkpoint;
mod effects;
pub mod movie;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use image::RgbaImage;
use symphony_core::doc::Scene;
use symphony_core::graph::{Effect, RenderCommand, RenderItem};
use symphony_core::id::Id;
use symphony_core::math::Affine;
use symphony_core::stroke::{ArtLayer, StrokeKind};
use symphony_vector::tessellate;
use tiny_skia::{FillRule, Paint, PathBuilder, Pixmap, PixmapPaint, Transform};

/// Render `frame` at `width` x `height` pixels (the scene's aspect is
/// kept; the stage fills the image). Background is white paper.
pub fn render_frame(scene: &Scene, frame: u32, width: u32, height: u32) -> Option<RgbaImage> {
    let mut pixmap = render_display(scene, frame, width, height, true, None)?;
    paint_bars(&mut pixmap, &scene.settings);
    RgbaImage::from_raw(pixmap.width(), pixmap.height(), pixmap.take())
}

/// Temporary drawing-space transform for the Select tool's live preview.
#[derive(Clone, Copy)]
pub struct StrokePreview<'a> {
    pub drawing: Id,
    pub strokes: &'a [Id],
    pub transform: Affine,
}

/// One drawing's artwork fitted into a `side` x `side` square with a small
/// margin, on transparent paper (premultiplied RGBA), for thumbnails such
/// as the Node view's hover preview. None when the drawing is empty.
pub fn render_drawing_thumbnail(
    scene: &Scene,
    node: symphony_core::graph::NodeId,
    column: symphony_core::id::Id,
    drawing: symphony_core::id::Id,
    frame: u32,
    side: u32,
) -> Option<Pixmap> {
    let d = scene.drawings.get(&drawing)?;
    let mut lo = [f32::INFINITY; 2];
    let mut hi = [f32::NEG_INFINITY; 2];
    for st in &d.strokes {
        if let Some((a, b)) = symphony_vector::bounds(st) {
            lo = [lo[0].min(a[0]), lo[1].min(a[1])];
            hi = [hi[0].max(b[0]), hi[1].max(b[1])];
        }
    }
    for fill in &d.fills {
        for c in &fill.contours {
            for p in c {
                lo = [lo[0].min(p[0]), lo[1].min(p[1])];
                hi = [hi[0].max(p[0]), hi[1].max(p[1])];
            }
        }
    }
    if !(lo[0] < hi[0] || lo[1] < hi[1]) {
        return None;
    }
    let side = side.clamp(8, 1024);
    let mut pix = Pixmap::new(side, side)?;
    let span = (hi[0] - lo[0]).max(hi[1] - lo[1]).max(1.0);
    let k = side as f32 * 0.86 / span;
    let centre = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0];
    let fit = Affine::translate(side as f32 / 2.0, side as f32 / 2.0)
        .compose(Affine::scale(k, k))
        .compose(Affine::translate(-centre[0], -centre[1]));
    // paint_drawing applies the camera's projection; cancel it so the
    // drawing is shown as drawn.
    let project = scene.project_plane(frame, 0.0)?;
    let view = fit.compose(project.inverse()?);
    let item = RenderItem {
        node,
        column,
        drawing,
        transform: Affine::IDENTITY,
        depth: 0.0,
        matte_pass: false,
        warp: None,
    };
    paint_drawing(&mut pix, scene, frame, view, item, None);
    Some(pix)
}

/// Render the Display graph as a premultiplied RGBA stage with transparent paper.
pub fn render_camera(
    scene: &Scene,
    frame: u32,
    width: u32,
    height: u32,
    preview: Option<StrokePreview<'_>>,
) -> Option<Pixmap> {
    let mut pix = render_display(scene, frame, width, height, false, preview)?;
    paint_bars(&mut pix, &scene.settings);
    Some(pix)
}

/// The camera frame's black bars, over everything: the frame minus the
/// (rounded) picture opening, filled black.
pub fn paint_bars(pix: &mut Pixmap, settings: &symphony_core::doc::Settings) {
    let (sw, sh) = (settings.width.max(1) as f32, settings.height.max(1) as f32);
    let Some(outline) = settings.bars.outline(sw, sh) else {
        return;
    };
    let k = pix.width() as f32 / sw;
    let (cx, cy) = (pix.width() as f32 / 2.0, pix.height() as f32 / 2.0);
    let mut pb = tiny_skia::PathBuilder::new();
    pb.push_rect(
        tiny_skia::Rect::from_xywh(0.0, 0.0, pix.width() as f32, pix.height() as f32).unwrap(),
    );
    for (i, p) in outline.iter().enumerate() {
        let (x, y) = (cx + p[0] * k, cy + p[1] * k);
        if i == 0 {
            pb.move_to(x, y);
        } else {
            pb.line_to(x, y);
        }
    }
    pb.close();
    if let Some(path) = pb.finish() {
        let mut paint = tiny_skia::Paint::default();
        paint.set_color(tiny_skia::Color::BLACK);
        paint.anti_alias = true;
        pix.fill_path(
            &path,
            &paint,
            tiny_skia::FillRule::EvenOdd,
            tiny_skia::Transform::identity(),
            None,
        );
    }
}

fn render_display(
    scene: &Scene,
    frame: u32,
    width: u32,
    height: u32,
    white_paper: bool,
    preview: Option<StrokePreview<'_>>,
) -> Option<Pixmap> {
    let (width, height) = (width.max(1), height.max(1));
    let mut paper = Pixmap::new(width, height)?;
    if white_paper {
        paper.fill(tiny_skia::Color::WHITE);
    }
    let k = width as f32 / scene.settings.width.max(1) as f32 * scene.camera_scale(frame);
    // Scene units have their origin at the stage centre, y down.
    let view =
        Affine::translate(width as f32 / 2.0, height as f32 / 2.0).compose(Affine::scale(k, k));
    let commands = scene
        .graph
        .render_commands(scene.display, &scene.xsheet, frame)
        .unwrap_or_default();
    let mut layers = vec![paper];
    for command in commands {
        let current = layers.last_mut()?;
        match command {
            RenderCommand::Drawing(item) => {
                paint_drawing(current, scene, frame, view, item, preview)
            }
            RenderCommand::Gradient(gradient) => effects::gradient(current, gradient),
            RenderCommand::BeginEffect => {
                // Bound temporary raster storage, including blur scratch space.
                let bytes = current.data().len().checked_mul(layers.len() + 2)?;
                if bytes > 512 * 1024 * 1024 {
                    return None;
                }
                layers.push(Pixmap::new(width, height)?);
            }
            RenderCommand::EndMask { inverted } => {
                let matte = layers.pop()?;
                let mut image = layers.pop()?;
                effects::mask(&mut image, &matte, inverted);
                layers.last_mut()?.draw_pixmap(
                    0,
                    0,
                    image.as_ref(),
                    &PixmapPaint::default(),
                    Transform::identity(),
                    None,
                );
            }
            RenderCommand::EndEffect(Effect::Blend { mode, amount }) => {
                let image = layers.pop()?;
                effects::blend(layers.last_mut()?, &image, mode, amount);
            }
            RenderCommand::EndEffect(effect) => {
                let mut image = layers.pop()?;
                effects::apply(&mut image, effect, k);
                layers.last_mut()?.draw_pixmap(
                    0,
                    0,
                    image.as_ref(),
                    &PixmapPaint::default(),
                    Transform::identity(),
                    None,
                );
            }
        }
    }
    layers.pop()
}

/// Decoded imported images, by file, shared by every render (premultiplied,
/// ready to draw). Kept to a memory budget; least recently used go first.
static IMAGES: std::sync::Mutex<Vec<(std::path::PathBuf, std::sync::Arc<Pixmap>)>> =
    std::sync::Mutex::new(Vec::new());
const IMAGE_BUDGET: usize = 768 * 1024 * 1024;

/// An imported image, decoded once (premultiplied RGBA).
pub fn decoded_image(scene: &Scene, key: u64) -> Option<std::sync::Arc<Pixmap>> {
    let path = scene.media_path(key)?;
    if let Ok(mut cache) = IMAGES.lock() {
        if let Some(i) = cache.iter().position(|(p, _)| *p == path) {
            let hit = cache.remove(i);
            let pix = hit.1.clone();
            cache.push(hit);
            return Some(pix);
        }
    }
    let rgba = image::open(&path).ok()?.to_rgba8();
    let (w, h) = (rgba.width(), rgba.height());
    let mut pix = Pixmap::new(w, h)?;
    for (dst, src) in pix.pixels_mut().iter_mut().zip(rgba.pixels()) {
        let [r, g, b, a] = src.0;
        *dst = tiny_skia::ColorU8::from_rgba(r, g, b, a).premultiply();
    }
    let pix = std::sync::Arc::new(pix);
    if let Ok(mut cache) = IMAGES.lock() {
        cache.push((path, pix.clone()));
        let mut total: usize = cache.iter().map(|(_, p)| p.data().len()).sum();
        while total > IMAGE_BUDGET && cache.len() > 1 {
            total -= cache.remove(0).1.data().len();
        }
    }
    Some(pix)
}

fn paint_image(
    pix: &mut Pixmap,
    scene: &Scene,
    img: symphony_core::stroke::PlacedImage,
    m: Affine,
) {
    let Some(src) = decoded_image(scene, img.media) else {
        return;
    };
    let [x, y, w, h] = img.rect;
    let (sx, sy) = (
        w / src.width().max(1) as f32,
        h / src.height().max(1) as f32,
    );
    let place = m
        .compose(Affine::translate(x, y))
        .compose(Affine::scale(sx, sy));
    let t = Transform::from_row(place.a, place.b, place.c, place.d, place.tx, place.ty);
    let paint = PixmapPaint {
        quality: tiny_skia::FilterQuality::Bilinear,
        ..PixmapPaint::default()
    };
    pix.draw_pixmap(0, 0, src.as_ref().as_ref(), &paint, t, None);
}

fn paint_drawing(
    pix: &mut Pixmap,
    scene: &Scene,
    frame: u32,
    view: Affine,
    item: RenderItem,
    preview: Option<StrokePreview<'_>>,
) {
    let Some(d) = scene.drawings.get(&item.drawing) else {
        return;
    };
    let Some(project) = scene.project_plane(frame, item.depth) else {
        return;
    };
    let m = view.compose(project).compose(item.transform);
    // An imported image sits under everything else in the drawing.
    if let Some(img) = d.image {
        let transform = item
            .warp
            .as_deref()
            .and_then(|warp| warp.raster_transform())
            .map_or(m, |raster| view.compose(project).compose(raster));
        paint_image(pix, scene, img, transform);
    }
    // Under Mesh Warps: draw the drawing bent (pictures aren't bent).
    let bent;
    let (d, m) = match item.warp.as_deref() {
        Some(warp) => {
            bent = symphony_vector::bend::drawing(d, warp);
            (&bent, view.compose(project).compose(warp.outer()))
        }
        None => (d, m),
    };
    // Art layers back to front; within each, paint under ink.
    let moving = preview.filter(|p| p.drawing == item.drawing);
    for art in ArtLayer::STACK {
        for fill in d.fills.iter().filter(|f| f.art == art) {
            // Selected zones follow the Select tool's live preview.
            let m = match moving {
                Some(p) if p.strokes.contains(&fill.id) => m.compose(p.transform),
                _ => m,
            };
            let mut pb = PathBuilder::new();
            for c in &fill.contours {
                for (k, p) in c.iter().enumerate() {
                    let (x, y) = m.apply(p[0], p[1]);
                    if k == 0 {
                        pb.move_to(x, y);
                    } else {
                        pb.line_to(x, y);
                    }
                }
                pb.close();
            }
            let Some(path) = pb.finish() else { continue };
            let [r, g, b, alpha] = scene.swatch_rgba(fill.swatch);
            let mut paint = Paint::default();
            paint.set_color_rgba8(r, g, b, alpha);
            paint.anti_alias = true;
            pix.fill_path(
                &path,
                &paint,
                FillRule::EvenOdd,
                Transform::identity(),
                None,
            );
        }
        paint_strokes(
            pix,
            scene,
            d,
            art,
            m,
            preview.filter(|p| p.drawing == item.drawing),
        );
    }
}

/// Smallest gap between line points when rendering, in pixels. The
/// curve is smooth between them and anti-aliasing blends edges, so finer
/// steps only add triangles to fill.
const RASTER_STEP: f32 = 1.0;

/// Ink for one art layer. Invisible strokes only bound paint zones.
fn paint_strokes(
    pix: &mut Pixmap,
    scene: &Scene,
    d: &symphony_core::stroke::Drawing,
    art: ArtLayer,
    m: Affine,
    preview: Option<StrokePreview<'_>>,
) {
    {
        // Stamp every textured stroke at once across the cores, then lay
        // them down in order (the picture is the same as one at a time).
        let stamped: std::collections::HashMap<
            symphony_core::id::Id,
            symphony_vector::stamp::Coverage,
        > = {
            use rayon::prelude::*;
            let clip = [0, 0, pix.width() as i32, pix.height() as i32];
            d.strokes
                .par_iter()
                .filter(|s| s.art == art && s.kind != StrokeKind::Invisible)
                .filter_map(|st| {
                    let tex = st.texture.filter(|t| !t.vector)?;
                    let m = match preview {
                        Some(p) if p.strokes.contains(&st.id) => m.compose(p.transform),
                        _ => m,
                    };
                    let tip = tex.tip.and_then(|k| scene.textures.get(&k));
                    let grain = tex.grain.and_then(|k| scene.textures.get(&k));
                    symphony_vector::stamp::rasterize(st, &tex, tip, grain, m, clip)
                        .map(|c| (st.id, c))
                })
                .collect()
        };
        for st in d
            .strokes
            .iter()
            .filter(|s| s.art == art && s.kind != StrokeKind::Invisible)
        {
            let m = match preview {
                Some(p) if p.strokes.contains(&st.id) => m.compose(p.transform),
                _ => m,
            };
            if let Some(tex) = st.texture.filter(|t| t.vector) {
                // A vector brush: its merged stamp shapes, filled solid.
                let tip = tex.tip.and_then(|k| scene.textures.get(&k));
                let shapes = symphony_vector::stamped::vector_brush_shapes(st, &tex, tip);
                let mut pb = PathBuilder::new();
                for shape in &shapes {
                    for ring in shape {
                        for (i, &[x, y]) in ring.iter().enumerate() {
                            let (x, y) = m.apply(x, y);
                            if i == 0 {
                                pb.move_to(x, y);
                            } else {
                                pb.line_to(x, y);
                            }
                        }
                        pb.close();
                    }
                }
                if let Some(path) = pb.finish() {
                    let [r, g, b, a] = scene.swatch_rgba(st.swatch);
                    let mut paint = tiny_skia::Paint::default();
                    paint.set_color_rgba8(r, g, b, a);
                    paint.anti_alias = true;
                    pix.fill_path(
                        &path,
                        &paint,
                        tiny_skia::FillRule::EvenOdd,
                        Transform::identity(),
                        None,
                    );
                }
                continue;
            }
            if st.texture.is_some() {
                if let Some(c) = stamped.get(&st.id) {
                    composite_coverage(pix, scene, st, c);
                }
                continue;
            }
            let mesh = tessellate(st, m, RASTER_STEP);
            if mesh.indices.is_empty() {
                continue;
            }
            // One path per stroke with non-zero fill: overlapping triangles
            // merge into one shape, so there are no seams between them.
            let mut pb = PathBuilder::new();
            for t in mesh.indices.as_chunks::<3>().0 {
                let (a, b, c) = (
                    mesh.vertices[t[0] as usize],
                    mesh.vertices[t[1] as usize],
                    mesh.vertices[t[2] as usize],
                );
                // Wind every triangle the same way so non-zero fill unions them.
                let cross = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
                let (b, c) = if cross < 0.0 { (c, b) } else { (b, c) };
                pb.move_to(a[0], a[1]);
                pb.line_to(b[0], b[1]);
                pb.line_to(c[0], c[1]);
                pb.close();
            }
            let Some(path) = pb.finish() else { continue };
            let [r, g, b, alpha] = scene.swatch_rgba(st.swatch);
            let mut paint = Paint::default();
            paint.set_color_rgba8(r, g, b, alpha);
            paint.anti_alias = true;
            pix.fill_path(
                &path,
                &paint,
                FillRule::Winding,
                Transform::identity(),
                None,
            );
        }
    }
}

/// Lay a stamped stroke's coverage down in its colour (source-over).
fn composite_coverage(
    pix: &mut Pixmap,
    scene: &Scene,
    st: &symphony_core::stroke::Stroke,
    c: &symphony_vector::stamp::Coverage,
) {
    let [r, g, b, alpha] = scene.swatch_rgba(st.swatch);
    let width = pix.width() as i32;
    let data = pix.data_mut();
    for row in 0..c.height as i32 {
        for col in 0..c.width as i32 {
            let coverage = c.data[(row * c.width as i32 + col) as usize] as u32;
            let a = (coverage * u32::from(alpha) + 127) / 255;
            if a == 0 {
                continue;
            }
            let (x, y) = (c.x + col, c.y + row);
            let i = ((y * width + x) * 4) as usize;
            // Premultiplied source-over.
            let inv = 255 - a;
            for (k, src) in [r, g, b].into_iter().enumerate() {
                data[i + k] = ((src as u32 * a + data[i + k] as u32 * inv) / 255) as u8;
            }
            data[i + 3] = (a + data[i + 3] as u32 * inv / 255) as u8;
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    /// One PNG per frame: name_0001.png, ...
    PngSequence,
    AnimatedGif,
    /// H.264 in MP4, through Cisco's OpenH264 binary (`ExportJob::codec`).
    Mp4Movie,
}

#[derive(Clone, Debug)]
pub struct ExportJob {
    pub format: Format,
    /// For a PNG sequence, the first file's path (the frame number is added).
    pub path: PathBuf,
    pub first: u32,
    pub last: u32,
    /// Output size as a fraction of the scene resolution (1.0, 0.5, ...).
    pub scale: f32,
    /// Path to Cisco's OpenH264 binary, for movies.
    pub codec: Option<PathBuf>,
    /// MP4 rate override. Resamples the scene in time when set.
    pub movie_fps: Option<f32>,
}

/// How an export went.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Exported {
    /// Frames kept from an earlier export of the same scene that didn't
    /// finish (a crash, a full disk, Cancel), so not rendered again.
    pub reused: u32,
}

/// Run an export. `progress` gets (done, total). Stops early, cleanly, when
/// `cancel` is set. Errors are plain sentences (DESIGN D10.3).
///
/// Nothing finished is ever lost (see [`checkpoint`]): each frame is kept
/// on disk as soon as it's rendered, and exporting the same scene again
/// to the same place continues from there.
pub fn export(
    scene: &Scene,
    job: &ExportJob,
    cancel: &AtomicBool,
    progress: &dyn Fn(u32, u32),
) -> Result<Exported, String> {
    let (w, h) = (
        ((scene.settings.width as f32 * job.scale).round() as u32).max(1),
        ((scene.settings.height as f32 * job.scale).round() as u32).max(1),
    );
    let (first, last) = (job.first.max(1), job.last.max(job.first.max(1)));
    let total = last - first + 1;
    match job.format {
        Format::PngSequence => {
            let key = checkpoint::fingerprint(scene, w, h);
            let ck = checkpoint::Checkpoint::sequence(&job.path, &key, w, h)?;
            let frames: Vec<u32> = (first..=last).collect();
            // The frames are the output: they're written (and compressed)
            // across the cores as they render, so there's nothing to do in
            // order but count.
            let reused = in_order(
                scene,
                &frames,
                w,
                h,
                &ck,
                cancel,
                |_| (),
                |i, _, ()| {
                    progress(i as u32 + 1, total);
                    Ok(())
                },
            )?;
            ck.finish();
            Ok(Exported { reused })
        }
        Format::Mp4Movie => {
            let codec = job.codec.as_deref().ok_or(
                "Movie export needs the OpenH264 codec. Download it in Preferences > Video.",
            )?;
            // 4:2:0 video needs even sizes.
            let (w, h) = ((w & !1).max(2), (h & !1).max(2));
            let key = checkpoint::fingerprint(scene, w, h);
            let ck = checkpoint::Checkpoint::folder(&job.path, &key, w, h)?;
            let part = checkpoint::with_suffix(&job.path, ".part");
            let reused = movie::export_mp4(
                scene,
                codec,
                &part,
                &ck,
                first,
                last,
                w,
                h,
                job.movie_fps,
                cancel,
                progress,
            )?;
            finish_file(&part, &job.path)?;
            ck.finish();
            Ok(Exported { reused })
        }
        Format::AnimatedGif => {
            let (gw, gh) = (
                u16::try_from(w).map_err(|_| "GIFs can be at most 65535 pixels wide.")?,
                u16::try_from(h).map_err(|_| "GIFs can be at most 65535 pixels tall.")?,
            );
            let key = checkpoint::fingerprint(scene, w, h);
            let ck = checkpoint::Checkpoint::folder(&job.path, &key, w, h)?;
            let part = checkpoint::with_suffix(&job.path, ".part");
            let file = std::fs::File::create(&part)
                .map_err(|e| format!("Couldn't create {} ({e}).", part.display()))?;
            let mut enc = gif::Encoder::new(std::io::BufWriter::new(file), gw, gh, &[])
                .map_err(|e| format!("Couldn't start the GIF ({e})."))?;
            enc.set_repeat(gif::Repeat::Infinite)
                .map_err(|e| e.to_string())?;
            let delay = (100.0 / scene.settings.fps.max(1.0)).round() as u16;
            let frames: Vec<u32> = (first..=last).collect();
            // Reducing each frame to 256 colours and compressing it are the
            // slow parts; both happen across the cores, ahead of writing.
            let prepare = |img: RgbaImage| {
                let mut raw = img.into_raw();
                let mut frame = gif::Frame::from_rgba_speed(gw, gh, &mut raw, GIF_SPEED);
                frame.delay = delay;
                frame.dispose = gif::DisposalMethod::Background;
                frame.make_lzw_pre_encoded();
                frame
            };
            let reused = in_order(scene, &frames, w, h, &ck, cancel, prepare, |i, _, frame| {
                enc.write_lzw_pre_encoded_frame(&frame)
                    .map_err(|e| format!("Couldn't write the GIF ({e})."))?;
                progress(i as u32 + 1, total);
                Ok(())
            })?;
            let unfinished = |e: String| format!("Couldn't finish the GIF ({e}).");
            let file: std::fs::File = enc
                .into_inner()
                .map_err(|e| unfinished(e.to_string()))?
                .into_inner()
                .map_err(|e| unfinished(e.error().to_string()))?;
            file.sync_all().map_err(|e| unfinished(e.to_string()))?;
            drop(file);
            finish_file(&part, &job.path)?;
            ck.finish();
            Ok(Exported { reused })
        }
    }
}

/// A finished movie or GIF takes its real name only once it's whole.
fn finish_file(part: &Path, out: &Path) -> Result<(), String> {
    std::fs::rename(part, out).map_err(|e| {
        format!(
            "Couldn't name the finished file {} ({e}). It is complete at {}.",
            out.display(),
            part.display()
        )
    })
}

/// Frames rendered at once while exporting: one per core, fewer for huge
/// frames so a batch stays within about 256 MB.
fn batch_len(w: u32, h: u32) -> usize {
    let frame = (w as usize * h as usize * 4).max(1);
    rayon::current_num_threads()
        .min((256usize << 20) / frame)
        .max(1)
}

/// GIF colour reduction speed (1 best .. 30 fastest), as before.
const GIF_SPEED: i32 = 10;

/// Render `frames` (or take them from `ck`, kept from an unfinished
/// export), keep each in `ck`, `prepare` it, and hand them to `each` in
/// order (index, frame, prepared). Batches render, save and prepare ahead
/// across the cores on another thread while `each` runs (writing a movie
/// or GIF has to go in order on one thread). Returns how many frames were
/// reused from `ck`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn in_order<T: Send>(
    scene: &Scene,
    frames: &[u32],
    w: u32,
    h: u32,
    ck: &checkpoint::Checkpoint,
    cancel: &AtomicBool,
    prepare: impl Fn(RgbaImage) -> T + Sync,
    mut each: impl FnMut(usize, u32, T) -> Result<(), String>,
) -> Result<u32, String> {
    use rayon::prelude::*;
    use std::collections::HashMap;
    use std::sync::atomic::AtomicU32;
    let batch = batch_len(w, h);
    let reused = AtomicU32::new(0);
    let kept = AtomicU32::new(0);
    let one = |f: u32| -> Result<RgbaImage, String> {
        if let Some(img) = ck.load(f) {
            reused.fetch_add(1, Ordering::Relaxed);
            kept.fetch_add(1, Ordering::Relaxed);
            return Ok(img);
        }
        let img = render_frame(scene, f, w, h).ok_or("Couldn't render a frame.")?;
        ck.save(f, &img)?;
        kept.fetch_add(1, Ordering::Relaxed);
        Ok(img)
    };
    let stopped = || {
        format!(
            "Export cancelled. The {} frames done are kept: export again to continue.",
            kept.load(Ordering::Relaxed)
        )
    };
    let result = std::thread::scope(|scope| {
        // One batch waiting while the next renders: at most three batches
        // of images exist at once.
        let (tx, rx) = std::sync::mpsc::sync_channel::<Vec<Result<T, String>>>(1);
        let (prepare, one) = (&prepare, &one);
        scope.spawn(move || {
            for chunk in frames.chunks(batch) {
                if cancel.load(Ordering::SeqCst) {
                    break;
                }
                // A movie at a higher rate repeats frames: each is made
                // once (two threads must never write the same file).
                let mut unique = chunk.to_vec();
                unique.sort_unstable();
                unique.dedup();
                let images: HashMap<u32, Result<RgbaImage, String>> =
                    unique.par_iter().map(|&f| (f, one(f))).collect();
                let prepared = chunk
                    .par_iter()
                    .map(|f| images[f].clone().map(prepare))
                    .collect();
                if tx.send(prepared).is_err() {
                    break;
                }
            }
        });
        let mut i = 0;
        for prepared in rx {
            for item in prepared {
                if cancel.load(Ordering::SeqCst) {
                    return Err(stopped());
                }
                each(i, frames[i], item?)?;
                i += 1;
            }
        }
        if i < frames.len() {
            return Err(stopped());
        }
        Ok(())
    });
    result.map(|()| reused.load(Ordering::Relaxed))
}

/// `shot.png` + frame 7 -> `shot_0007.png`.
pub fn numbered(path: &Path, frame: u32) -> PathBuf {
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "frame".into());
    let dir = path.parent().unwrap_or(Path::new("."));
    dir.join(format!("{stem}_{frame:04}.png"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use symphony_core::doc::Settings;
    use symphony_core::history::{Edit, History};
    use symphony_core::stroke::{Point, Stroke, StrokeKind};

    fn scene_with_line() -> Scene {
        let mut s = Scene::new(Settings {
            width: 200,
            height: 100,
            fps: 12.0,
            length: 3,
            ..Default::default()
        });
        let mut h = History::default();
        let snap = s.make_layer("Drawing", 0);
        h.apply(&mut s, "Add layer", vec![Edit::AddLayer(Box::new(snap))]);
        let col = s.layers[0].column;
        let d = s.make_drawing(col);
        let did = d.id;
        let swatch = s.palette.swatches[0].id;
        let st = Stroke {
            id: s.ids.fresh(),
            kind: StrokeKind::Brush,
            art: Default::default(),
            swatch,
            min_width: 10.0,
            max_width: 10.0,
            points: vec![
                Point {
                    x: -50.0,
                    y: 0.0,
                    pressure: 1.0,
                    pen: Default::default(),
                },
                Point {
                    x: 50.0,
                    y: 0.0,
                    pressure: 1.0,
                    pen: Default::default(),
                },
            ],
            tips: Default::default(),
            texture: None,
            shape: Vec::new(),
        };
        let cells = Edit::splice(&s, col, 1, 0, vec![Some(did), Some(did), Some(did)]).unwrap();
        h.apply(
            &mut s,
            "Draw",
            vec![
                Edit::AddDrawing { drawing: d },
                cells,
                Edit::AddStroke {
                    drawing: did,
                    stroke: st,
                },
            ],
        );
        s
    }

    #[test]
    fn renders_textured_strokes_with_their_grain() {
        let mut s = scene_with_line();
        // Grain: alternating black and white columns, one pixel each.
        s.textures.insert(
            1,
            symphony_core::stroke::Texture {
                width: 2,
                height: 1,
                pixels: vec![0, 255],
            },
        );
        let d = *s.drawings.keys().next().unwrap();
        s.drawings.get_mut(&d).unwrap().strokes[0].texture =
            Some(symphony_core::stroke::StrokeTexture {
                tip: None,
                grain: Some(1),
                spacing: 0.1,
                opacity: 1.0,
                angle: 0.0,
                follow: false,
                grain_scale: 1.0,
                flow: 1.0,
                hardness: 1.0,
                opacity_by_pressure: [1.0; 4],
                ..Default::default()
            });
        let img = render_frame(&s, 1, 200, 100).unwrap();
        let row: Vec<u8> = (90..110).map(|x| img.get_pixel(x, 50).0[0]).collect();
        let ink = s.palette.swatches[0].rgb[0];
        assert!(row.contains(&255), "paper shows through the grain: {row:?}");
        assert!(row.contains(&ink), "ink where the grain is white: {row:?}");
    }

    #[test]
    fn mask_keeps_or_cuts_alpha_and_converted_drawing_stays_hidden_in_composite() {
        use symphony_core::graph::{Effect, NodeKind};
        let mut scene = scene_with_line();
        let image = scene.layers[0].node;
        let mut stroke = scene.drawings[&scene.exposed(0, 1).unwrap()].strokes[0].clone();
        stroke.id = scene.ids.fresh();
        stroke.points[0].x = -8.0;
        stroke.points[1].x = 8.0;
        let mut history = History::default();
        let layer = scene.make_layer("Matte", 1);
        history.apply(
            &mut scene,
            "Add layer",
            vec![Edit::AddLayer(Box::new(layer))],
        );
        let layer = scene.layers[1];
        let mut drawing = scene.make_drawing(layer.column);
        drawing.strokes.push(stroke);
        let cells = Edit::splice(&scene, layer.column, 1, 0, vec![Some(drawing.id)]).unwrap();
        history.apply(
            &mut scene,
            "Draw matte",
            vec![Edit::AddDrawing { drawing }, cells],
        );
        history.apply(
            &mut scene,
            "Convert to mask drawing",
            vec![Edit::SetDrawingMatte {
                id: layer.node,
                before: false,
                after: true,
            }],
        );
        let mask = scene
            .graph
            .add("Mask", NodeKind::Effect(Effect::Mask { inverted: false }));
        assert!(scene.graph.can_connect_to(layer.node, mask, 1));
        assert!(!scene.graph.can_connect_to(layer.node, mask, 0));
        scene.graph.connect(image, mask, 0);
        scene.graph.connect(layer.node, mask, 1);
        scene.graph.connect(mask, scene.composite, 0);

        let keep = render_camera(&scene, 1, 200, 100, None).unwrap();
        assert_eq!(keep.pixel(100, 50).unwrap().alpha(), 255);
        assert_eq!(keep.pixel(140, 50).unwrap().alpha(), 0);
        scene.graph.node_mut(mask).unwrap().kind =
            NodeKind::Effect(Effect::Mask { inverted: true });
        let cut = render_camera(&scene, 1, 200, 100, None).unwrap();
        assert_eq!(cut.pixel(100, 50).unwrap().alpha(), 0);
        assert_eq!(cut.pixel(140, 50).unwrap().alpha(), 255);

        history.undo(&mut scene);
        assert_eq!(scene.graph.input(mask, 1), Some(layer.node));
        assert!(scene.exposed(1, 1).is_some());
        assert_eq!(
            render_camera(&scene, 1, 200, 100, None)
                .unwrap()
                .pixel(100, 50)
                .unwrap()
                .alpha(),
            255
        );
    }

    #[test]
    fn camera_and_export_render_posed_artwork_without_bone_controls() {
        use symphony_core::{anim::Value, graph::NodeKind, rig::Bone};
        let mut scene = scene_with_line();
        let layer = scene.layers[0];
        let root = scene
            .graph
            .add("Skeleton", NodeKind::Skeleton(Default::default()));
        let upper = scene.graph.add(
            "Upper",
            NodeKind::Bone(Bone {
                origin: [-50.0, 0.0],
                length: 50.0,
                ..Bone::default()
            }),
        );
        let lower = scene.graph.add(
            "Lower",
            NodeKind::Bone(Bone {
                length: 50.0,
                rotation: Value::Constant(90.0),
                bend_radius: 12.0,
                ..Bone::default()
            }),
        );
        scene.graph.connect(root, upper, 0);
        scene.graph.connect(upper, lower, 0);
        scene.graph.connect(lower, layer.peg, 0);
        let camera = render_camera(&scene, 1, 200, 100, None).unwrap();
        let export = render_frame(&scene, 1, 200, 100).unwrap();
        assert_eq!(camera.pixel(75, 50).unwrap().alpha(), 255);
        assert_eq!(camera.pixel(100, 78).unwrap().alpha(), 255);
        assert_eq!(camera.pixel(128, 50).unwrap().alpha(), 0);
        assert_eq!(camera.pixel(80, 72).unwrap().alpha(), 0);
        assert_eq!(&export[(100, 78)].0[..3], &scene.palette.swatches[0].rgb);

        scene.graph.disconnect_inputs(layer.peg);
        scene.graph.connect(upper, layer.peg, 0);
        let export_with_empty_lower = render_frame(&scene, 1, 200, 100).unwrap();
        assert_eq!(export_with_empty_lower[(100, 78)].0, [255; 4]);
    }

    #[test]
    fn camera_pan_moves_foreground_faster_than_background() {
        use symphony_core::anim::Value;
        use symphony_core::graph::NodeKind;
        let mut scene = scene_with_line();
        let near = scene.layers[0];
        let drawing = scene.exposed(0, 1).unwrap();
        let near_stroke = &mut scene.drawings.get_mut(&drawing).unwrap().strokes[0];
        near_stroke.points[0].x = -10.0;
        near_stroke.points[1].x = 10.0;
        for point in &mut near_stroke.points {
            point.y = -12.0;
        }
        let mut far_stroke = near_stroke.clone();
        far_stroke.id = scene.ids.fresh();
        for point in &mut far_stroke.points {
            point.y = 48.0;
        }

        let mut history = History::default();
        let layer = scene.make_layer("Far", 1);
        history.apply(
            &mut scene,
            "Add layer",
            vec![Edit::AddLayer(Box::new(layer))],
        );
        let far = scene.layers[1];
        let mut drawing = scene.make_drawing(far.column);
        drawing.strokes.push(far_stroke);
        let cells = Edit::splice(&scene, far.column, 1, 0, vec![Some(drawing.id)]).unwrap();
        history.apply(
            &mut scene,
            "Add drawing",
            vec![Edit::AddDrawing { drawing }, cells],
        );
        for (node, z) in [(near.peg, -100.0), (far.peg, 200.0)] {
            let NodeKind::Peg(peg) = &mut scene.graph.node_mut(node).unwrap().kind else {
                unreachable!()
            };
            peg.z = Value::Constant(z);
        }

        let centre = |image: &Pixmap, row| {
            let ink: Vec<_> = (0..image.width())
                .filter(|&x| image.pixel(x, row).unwrap().alpha() > 200)
                .collect();
            (ink.first().unwrap() + ink.last().unwrap() + 1) as f32 * 0.5
        };
        let before = render_camera(&scene, 1, 200, 100, None).unwrap();
        scene.camera_motion.x = Value::Constant(20.0);
        let after = render_camera(&scene, 1, 200, 100, None).unwrap();
        assert_eq!(centre(&before, 26) - centre(&after, 26), 40.0);
        assert_eq!(centre(&before, 74) - centre(&after, 74), 10.0);

        let exported = render_frame(&scene, 1, 200, 100).unwrap();
        let ink = scene.palette.swatches[0].rgb;
        assert_eq!(&exported[(60, 26)].0[..3], &ink);
        assert_eq!(&exported[(90, 74)].0[..3], &ink);
    }

    #[test]
    fn field_of_view_changes_the_framing_without_changing_image_size() {
        let mut scene = scene_with_line();
        let standard = render_camera(&scene, 1, 200, 100, None).unwrap();
        scene.camera_fov = symphony_core::anim::Value::Constant(90.0);
        let wide = render_camera(&scene, 1, 200, 100, None).unwrap();
        assert_eq!(standard.width(), wide.width());
        assert_eq!(standard.height(), wide.height());
        assert_eq!(standard.pixel(58, 50).unwrap().alpha(), 255);
        assert_eq!(wide.pixel(58, 50).unwrap().alpha(), 0);
        assert_eq!(wide.pixel(100, 50).unwrap().alpha(), 255);
    }

    #[test]
    fn swatch_opacity_limits_textured_brush_coverage() {
        let mut scene = scene_with_line();
        let drawing = scene.drawings.values_mut().next().unwrap();
        let stroke = &mut drawing.strokes[0];
        stroke.texture = Some(symphony_core::stroke::StrokeTexture {
            tip: None,
            grain: None,
            spacing: 0.1,
            opacity: 1.0,
            angle: 0.0,
            follow: false,
            grain_scale: 1.0,
            flow: 1.0,
            hardness: 1.0,
            opacity_by_pressure: [0.0, 0.33, 0.66, 1.0],
            ..Default::default()
        });
        let brush_swatch = scene.palette.get_mut(stroke.swatch).unwrap();
        brush_swatch.opacity = 128;
        let image = render_camera(&scene, 1, 200, 100, None).unwrap();
        assert_eq!(
            image.pixels().iter().map(|pixel| pixel.alpha()).max(),
            Some(128)
        );
    }

    #[test]
    fn renders_ink_on_paper() {
        let s = scene_with_line();
        let img = render_frame(&s, 1, 200, 100).unwrap();
        let [r, g, b] = s.palette.swatches[0].rgb;
        assert_eq!(
            img.get_pixel(100, 50).0,
            [r, g, b, 255],
            "centre of the line is ink"
        );
        assert_eq!(
            img.get_pixel(100, 10).0,
            [255, 255, 255, 255],
            "above it is paper"
        );
    }

    #[test]
    fn renders_effect_chain_with_gradient_behind_the_drawing() {
        use symphony_core::graph::{Effect, Gradient, NodeKind};

        let mut scene = scene_with_line();
        let drawing = scene.layers[0].node;
        let gradient = scene.graph.add(
            "Gradient",
            NodeKind::Gradient(Gradient {
                start: [240, 20, 10, 255],
                end: [10, 20, 240, 255],
                angle: 0.0,
            }),
        );
        let opacity = scene
            .graph
            .add("Opacity", NodeKind::Effect(Effect::Opacity { amount: 0.0 }));
        let blur = scene.graph.add(
            "Box Blur",
            NodeKind::Effect(Effect::Blur {
                radius: 16.0,
                mode: symphony_core::graph::BlurMode::Box,
            }),
        );
        let grain = scene.graph.add(
            "Grain",
            NodeKind::Effect(Effect::Grain {
                amount: 0.25,
                seed: 42,
            }),
        );
        scene.graph.connect(drawing, opacity, 0);
        scene.graph.connect(opacity, blur, 0);
        scene.graph.connect(blur, scene.composite, 0);
        scene.graph.connect(gradient, scene.composite, 1);
        let background = render_frame(&scene, 1, 200, 100).unwrap();
        assert!(background[(5, 50)][0] > background[(5, 50)][2]);
        assert!(background[(195, 50)][0] < background[(195, 50)][2]);

        scene.graph.node_mut(opacity).unwrap().enabled = false;
        let blurred = render_frame(&scene, 1, 200, 100).unwrap();
        assert_ne!(blurred[(100, 45)], background[(100, 45)]);
        assert_eq!(blurred[(100, 10)], background[(100, 10)]);

        scene.graph.connect(scene.composite, grain, 0);
        scene.graph.connect(grain, scene.display, 0);
        let textured = render_frame(&scene, 1, 200, 100).unwrap();
        assert_ne!(textured, blurred);
        assert_eq!(textured, render_frame(&scene, 1, 200, 100).unwrap());
    }

    #[test]
    fn exports_gif_and_png_sequence() {
        let s = scene_with_line();
        let dir = std::env::temp_dir().join(format!("symphony-render-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let cancel = AtomicBool::new(false);
        let gif = ExportJob {
            format: Format::AnimatedGif,
            path: dir.join("a.gif"),
            first: 1,
            last: 3,
            scale: 0.5,
            codec: None,
            movie_fps: None,
        };
        export(&s, &gif, &cancel, &|_, _| {}).unwrap();
        assert!(std::fs::metadata(dir.join("a.gif")).unwrap().len() > 0);
        let png = ExportJob {
            format: Format::PngSequence,
            path: dir.join("shot.png"),
            first: 2,
            last: 3,
            scale: 1.0,
            codec: None,
            movie_fps: None,
        };
        export(&s, &png, &cancel, &|_, _| {}).unwrap();
        assert!(dir.join("shot_0002.png").is_file() && dir.join("shot_0003.png").is_file());
        // Movie export runs when a Cisco binary is available:
        // SYMPHONY_TEST_OPENH264=/path/to/libopenh264-2.7.0-<platform>.
        if let Some(codec) = std::env::var_os("SYMPHONY_TEST_OPENH264") {
            let mp4 = ExportJob {
                format: Format::Mp4Movie,
                path: dir.join("m.mp4"),
                first: 1,
                last: 3,
                scale: 1.0,
                codec: Some(codec.into()),
                movie_fps: Some(48.0),
            };
            export(&s, &mp4, &cancel, &|_, _| {}).unwrap();
            let bytes = std::fs::read(dir.join("m.mp4")).unwrap();
            assert_eq!(&bytes[4..8], b"ftyp", "an MP4 file starts with ftyp");
            let file = std::io::Cursor::new(&bytes);
            let reader = mp4::Mp4Reader::read_header(file, bytes.len() as u64).unwrap();
            let track = &reader.tracks()[&1];
            assert_eq!(track.sample_count(), 12);
            assert_eq!(track.frame_rate(), 48.0);
            assert_eq!(track.duration(), std::time::Duration::from_millis(250));
            if let Some(keep) = std::env::var_os("SYMPHONY_TEST_MP4_OUT") {
                std::fs::write(keep, &bytes).unwrap();
            }
        }
        cancel.store(true, Ordering::SeqCst);
        assert!(export(&s, &png, &cancel, &|_, _| {}).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// An export that stops part way (Cancel here; a crash or a full disk
    /// leave the same files) keeps every finished frame, leaves no broken
    /// file under the real name, and exporting again reuses those frames.
    /// A damaged frame is rendered again, and a changed scene starts over.
    #[test]
    fn an_interrupted_export_loses_nothing() {
        // Batches of at least three frames on any machine, so all three
        // are done when the Cancel lands.
        rayon::ThreadPoolBuilder::new()
            .num_threads(4)
            .build()
            .unwrap()
            .install(interrupted_export);
    }

    fn interrupted_export() {
        let mut s = scene_with_line();
        let dir = std::env::temp_dir().join(format!("symphony-resume-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for (format, name) in [
            (Format::AnimatedGif, "a.gif"),
            (Format::PngSequence, "f.png"),
        ] {
            let out = dir.join(name);
            let job = ExportJob {
                format,
                path: out.clone(),
                first: 1,
                last: 3,
                scale: 0.5,
                codec: None,
                movie_fps: None,
            };
            let cancel = AtomicBool::new(false);
            let stop = |_: u32, _: u32| cancel.store(true, Ordering::SeqCst);
            let err = export(&s, &job, &cancel, &stop).unwrap_err();
            assert!(err.contains("kept"), "{err}");
            let frame = |f: u32| match format {
                Format::PngSequence => numbered(&out, f),
                _ => checkpoint::with_suffix(&out, ".partial").join(format!("frame_{f:05}.png")),
            };
            if format == Format::AnimatedGif {
                assert!(!out.exists(), "no unfinished GIF under the real name");
            }
            assert!(frame(1).is_file() && frame(3).is_file());
            // A frame cut short (a power cut while writing).
            std::fs::write(frame(2), b"\x89PNG").unwrap();
            let go = AtomicBool::new(false);
            let done = export(&s, &job, &go, &|_, _| {}).unwrap();
            assert_eq!(done.reused, 2, "{name}: frames 1 and 3 reused, 2 redone");
            assert!(out.is_file() || numbered(&out, 2).is_file());
            assert!(image::open(frame(2)).is_ok() || format == Format::AnimatedGif);
            if format == Format::AnimatedGif {
                assert!(!checkpoint::with_suffix(&out, ".partial").exists());
                assert!(!checkpoint::with_suffix(&out, ".part").exists());
            }
        }
        // A changed scene doesn't reuse old frames.
        let png = ExportJob {
            format: Format::PngSequence,
            path: dir.join("f.png"),
            first: 1,
            last: 3,
            scale: 0.5,
            codec: None,
            movie_fps: None,
        };
        let cancel = AtomicBool::new(false);
        let _ = export(&s, &png, &cancel, &|_, _| {
            cancel.store(true, Ordering::SeqCst)
        });
        s.settings.fps = 24.0;
        let go = AtomicBool::new(false);
        assert_eq!(export(&s, &png, &go, &|_, _| {}).unwrap().reused, 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Black bars are part of the exported picture: a 2.39 shape blacks out
    /// the top and bottom, rounded corners black out the corners, and the
    /// middle is untouched.
    #[test]
    fn exports_draw_the_black_bars() {
        let mut scene = Scene::new(Settings {
            width: 200,
            height: 100,
            fps: 12.0,
            length: 1,
            ..Default::default()
        });
        scene.settings.bars = symphony_core::doc::Bars {
            on: true,
            aspect: 2.39,
            radius: 10.0,
        };
        let img = render_frame(&scene, 1, 200, 100).unwrap();
        let px = |x, y| img.get_pixel(x, y).0;
        assert_eq!(px(100, 2), [0, 0, 0, 255], "top bar");
        assert_eq!(px(100, 97), [0, 0, 0, 255], "bottom bar");
        assert_eq!(px(1, 50 - 41 + 1), [0, 0, 0, 255], "rounded corner");
        assert_eq!(px(100, 50), [255, 255, 255, 255], "the picture");
        scene.settings.bars.on = false;
        let img = render_frame(&scene, 1, 200, 100).unwrap();
        assert_eq!(
            img.get_pixel(100, 2).0,
            [255, 255, 255, 255],
            "off: no bars"
        );
    }
}
