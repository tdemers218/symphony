//! Pictures that come in as vector drawings, so every tool works on them
//! (select, transform, erase, paint, recolour):
//!
//! - SVG files: their filled shapes become colour zones (Colour Art), and
//!   their outlined paths become lines (Line Art).
//! - Bitmaps, vectorized on import (Harmony's "Import as Toon Boom Vector
//!   Drawing"): line art traced into lines ([`Vectorize::LineArt`]), or
//!   colour pictures traced into colour zones with the darkest colour as
//!   lines ([`Vectorize::Colour`]).
//!
//! All of this is pure work on a background thread; the session only adds
//! the finished shapes.

use image::RgbaImage;
use symphony_vector::shape::Shape;

/// What the traced pieces are.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    /// A colour zone (Colour Art: the paint bucket repaints it).
    Zone,
    /// A line (Line Art).
    Line,
}

/// One coloured piece, in the source's own units (pixels, or the SVG's
/// user units), y down.
#[derive(Clone, Debug)]
pub struct Piece {
    pub part: Part,
    /// sRGB and opacity.
    pub rgba: [u8; 4],
    pub shape: Shape,
}

/// A picture as vector pieces, back to front.
#[derive(Clone, Debug, Default)]
pub struct VectorArt {
    /// The source's size (to fit it to the scene like a picture).
    pub size: [f32; 2],
    pub pieces: Vec<Piece>,
}

/// How a bitmap becomes vectors.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Vectorize {
    /// Dark marks become lines; the paper goes. `threshold` 0..1: how dark
    /// a pixel must be to count (lower keeps fainter marks).
    LineArt { threshold: f32 },
    /// Up to `colours` colour zones; the darkest colour (if dark) becomes
    /// lines, and white paper goes.
    Colour { colours: usize },
}

impl Default for Vectorize {
    fn default() -> Self {
        Vectorize::LineArt { threshold: 0.5 }
    }
}

/// Simplification, in source pixels: points nearer the line than this go.
const TRACE_TOLERANCE: f32 = 0.35;
/// Pieces smaller than this many square pixels are specks, left out.
const SPECK: f32 = 3.0;
/// Bitmaps are traced at most this many pixels a side.
const TRACE_EDGE: u32 = 4096;
/// Curves in SVG files are split into steps this long (source units,
/// relative to the picture's size).
const SVG_FLATNESS: f32 = 1.0 / 2000.0;

// ---------------------------------------------------------------------
// SVG

/// Read an SVG's shapes as vector pieces.
pub fn from_svg(bytes: &[u8]) -> Result<VectorArt, String> {
    use resvg::usvg;
    let mut options = usvg::Options::default();
    // Text needs the system's fonts (slow to load: once, and only when a
    // file has text).
    if bytes.windows(5).any(|w| w == b"<text") {
        options.fontdb = fonts();
    }
    let tree =
        usvg::Tree::from_data(bytes, &options).map_err(|_| "not an SVG Symphony can read")?;
    let size = tree.size();
    let step = (size.width().max(size.height()) * SVG_FLATNESS).max(1e-3);
    let mut art = VectorArt {
        size: [size.width(), size.height()],
        pieces: Vec::new(),
    };
    let mut skipped = 0usize;
    walk(tree.root(), 1.0, step, &mut art, &mut skipped);
    if art.pieces.is_empty() {
        return Err(if skipped > 0 {
            "it only holds pictures, not shapes".into()
        } else {
            "it has no shapes".into()
        });
    }
    Ok(art)
}

fn walk(
    group: &resvg::usvg::Group,
    opacity: f32,
    step: f32,
    art: &mut VectorArt,
    skipped: &mut usize,
) {
    use resvg::usvg::Node;
    let opacity = opacity * group.opacity().get();
    for node in group.children() {
        match node {
            Node::Group(g) => walk(g, opacity, step, art, skipped),
            Node::Text(t) => walk(t.flattened(), opacity, step, art, skipped),
            Node::Image(_) => *skipped += 1,
            Node::Path(p) if p.is_visible() => {
                let ts = p.abs_transform();
                if let Some(fill) = p.fill() {
                    if let Some(rgb) = paint_colour(fill.paint()) {
                        let even_odd = fill.rule() == resvg::usvg::FillRule::EvenOdd;
                        let rings = flatten(p.data(), ts, step);
                        for shape in clean(rings, even_odd) {
                            art.pieces.push(Piece {
                                part: Part::Zone,
                                rgba: rgba(rgb, opacity * fill.opacity().get()),
                                shape,
                            });
                        }
                    }
                }
                if let Some(stroke) = p.stroke() {
                    if let Some(rgb) = paint_colour(stroke.paint()) {
                        // The stroke's outline (width, caps, joins, dashes)
                        // as a filled shape: a line of exactly that look.
                        let outline = p.data().stroke(&stroke.to_tiny_skia(), ts_scale(ts));
                        if let Some(outline) = outline {
                            let rings = flatten(&outline, ts, step);
                            for shape in clean(rings, false) {
                                art.pieces.push(Piece {
                                    part: Part::Line,
                                    rgba: rgba(rgb, opacity * stroke.opacity().get()),
                                    shape,
                                });
                            }
                        }
                    }
                }
            }
            Node::Path(_) => {}
        }
    }
}

fn ts_scale(ts: resvg::tiny_skia::Transform) -> f32 {
    let (sx, sy) = ts.get_scale();
    sx.max(sy).max(1e-3)
}

/// A paint as one colour: gradients by their middle stop (they become a
/// flat zone you can repaint).
fn paint_colour(paint: &resvg::usvg::Paint) -> Option<[u8; 3]> {
    use resvg::usvg::Paint;
    let stops = match paint {
        Paint::Color(c) => return Some([c.red, c.green, c.blue]),
        Paint::LinearGradient(g) => g.stops(),
        Paint::RadialGradient(g) => g.stops(),
        Paint::Pattern(_) => return Some([128, 128, 128]),
    };
    let s = stops.get(stops.len() / 2)?;
    let c = s.color();
    Some([c.red, c.green, c.blue])
}

fn rgba(rgb: [u8; 3], opacity: f32) -> [u8; 4] {
    [
        rgb[0],
        rgb[1],
        rgb[2],
        (opacity.clamp(0.0, 1.0) * 255.0).round() as u8,
    ]
}

/// A path's subpaths as closed polygons in picture space.
fn flatten(
    path: &resvg::tiny_skia::Path,
    ts: resvg::tiny_skia::Transform,
    step: f32,
) -> Vec<Vec<[f32; 2]>> {
    use resvg::tiny_skia::PathSegment;
    let Some(path) = path.clone().transform(ts) else {
        return Vec::new();
    };
    let mut rings: Vec<Vec<[f32; 2]>> = Vec::new();
    let mut ring: Vec<[f32; 2]> = Vec::new();
    let mut last = [0.0f32; 2];
    let steps = |a: [f32; 2], b: [f32; 2]| {
        (((b[0] - a[0]).hypot(b[1] - a[1]) / step).ceil() as usize).clamp(2, 64)
    };
    for seg in path.segments() {
        match seg {
            PathSegment::MoveTo(p) => {
                if ring.len() >= 3 {
                    rings.push(std::mem::take(&mut ring));
                }
                ring.clear();
                last = [p.x, p.y];
                ring.push(last);
            }
            PathSegment::LineTo(p) => {
                last = [p.x, p.y];
                ring.push(last);
            }
            PathSegment::QuadTo(c, p) => {
                let (c, p) = ([c.x, c.y], [p.x, p.y]);
                let n = steps(last, c) + steps(c, p);
                for i in 1..=n {
                    let t = i as f32 / n as f32;
                    let u = 1.0 - t;
                    ring.push([
                        u * u * last[0] + 2.0 * u * t * c[0] + t * t * p[0],
                        u * u * last[1] + 2.0 * u * t * c[1] + t * t * p[1],
                    ]);
                }
                last = p;
            }
            PathSegment::CubicTo(c1, c2, p) => {
                let (c1, c2, p) = ([c1.x, c1.y], [c2.x, c2.y], [p.x, p.y]);
                let n = steps(last, c1) + steps(c1, c2) + steps(c2, p);
                for i in 1..=n {
                    let t = i as f32 / n as f32;
                    let u = 1.0 - t;
                    let (a, b, c, d) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
                    ring.push([
                        a * last[0] + b * c1[0] + c * c2[0] + d * p[0],
                        a * last[1] + b * c1[1] + c * c2[1] + d * p[1],
                    ]);
                }
                last = p;
            }
            PathSegment::Close => {
                if ring.len() >= 3 {
                    rings.push(std::mem::take(&mut ring));
                }
                ring.clear();
            }
        }
    }
    if ring.len() >= 3 {
        rings.push(ring);
    }
    rings
}

/// Polygons (with the SVG's fill rule) into clean shapes: overlaps
/// resolved, each an outside with its holes.
fn clean(rings: Vec<Vec<[f32; 2]>>, even_odd: bool) -> Vec<Shape> {
    symphony_vector::shape::from_polygons(rings, even_odd)
}

// ---------------------------------------------------------------------
// Text (the Text tool)

pub use symphony_core::stroke::{TextAlign, TextStyle};

fn fonts() -> std::sync::Arc<resvg::usvg::fontdb::Database> {
    static FONTS: std::sync::OnceLock<std::sync::Arc<resvg::usvg::fontdb::Database>> =
        std::sync::OnceLock::new();
    FONTS
        .get_or_init(|| {
            let mut db = resvg::usvg::fontdb::Database::new();
            db.load_system_fonts();
            for bytes in [
                include_bytes!("../assets/fonts/fraunces.ttf").as_slice(),
                include_bytes!("../assets/fonts/spacegrotesk.ttf").as_slice(),
                include_bytes!("../assets/fonts/josefinsans.ttf").as_slice(),
                include_bytes!("../assets/fonts/bebasneue.ttf").as_slice(),
                include_bytes!("../assets/fonts/patrickhand.ttf").as_slice(),
                include_bytes!("../assets/fonts/spacemono.ttf").as_slice(),
            ] {
                db.load_font_data(bytes.to_vec());
            }
            std::sync::Arc::new(db)
        })
        .clone()
}

/// The font families on this computer, sorted (loads them the first time:
/// call it off the UI thread).
pub fn font_families() -> Vec<String> {
    let mut names: Vec<String> = fonts()
        .faces()
        .filter_map(|f| f.families.first().map(|(n, _)| n.clone()))
        .collect();
    names.sort_by_key(|n| n.to_lowercase());
    names.dedup();
    names
}

/// `text` (lines split at newlines) as letter shapes, the first line's
/// top at y = 0, aligned at x = 0 (left edge, centre or right edge).
pub fn text_shapes(text: &str, style: &TextStyle) -> Vec<Shape> {
    use resvg::usvg;
    if text.trim().is_empty() {
        return Vec::new();
    }
    let esc = |t: &str| {
        t.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
    };
    let size = style.size.clamp(1.0, 4000.0);
    let anchor = match style.align {
        TextAlign::Left => "start",
        TextAlign::Center => "middle",
        TextAlign::Right => "end",
    };
    let family = if style.family.trim().is_empty() {
        "sans-serif".to_owned()
    } else {
        format!("'{}', sans-serif", esc(&style.family))
    };
    let mut body = String::new();
    for (i, line) in text.split('\n').enumerate() {
        // Each line's baseline: ascent of about 0.8 of the size, lines
        // 1.25 apart.
        let y = size * 0.8 + i as f32 * size * 1.25;
        body.push_str(&format!(
            "<text x=\"0\" y=\"{y}\" xml:space=\"preserve\">{}</text>",
            esc(line)
        ));
    }
    let svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"10\" height=\"10\">\
         <g font-family=\"{family}\" font-size=\"{size}\" font-weight=\"{}\" \
         font-style=\"{}\" text-anchor=\"{anchor}\" fill=\"#000\">{body}</g></svg>",
        if style.bold { "bold" } else { "normal" },
        if style.italic { "italic" } else { "normal" },
    );
    let options = usvg::Options {
        fontdb: fonts(),
        ..Default::default()
    };
    let Ok(tree) = usvg::Tree::from_data(svg.as_bytes(), &options) else {
        return Vec::new();
    };
    let mut art = VectorArt::default();
    let mut skipped = 0;
    walk(tree.root(), 1.0, size / 64.0, &mut art, &mut skipped);
    art.pieces.into_iter().map(|p| p.shape).collect()
}

// ---------------------------------------------------------------------
// Bitmaps

/// Trace a bitmap into vector pieces.
pub fn vectorize(img: &RgbaImage, how: Vectorize) -> VectorArt {
    let img = fit(img);
    let (w, h) = (img.width() as usize, img.height() as usize);
    let size = [img.width() as f32, img.height() as f32];
    let pieces = match how {
        Vectorize::LineArt { threshold } => line_art(&img, w, h, threshold),
        Vectorize::Colour { colours } => colour(&img, w, h, colours.clamp(2, 32)),
    };
    // Back to front: zones under lines.
    let mut pieces = pieces;
    pieces.sort_by_key(|p| p.part == Part::Line);
    VectorArt { size, pieces }
}

fn fit(img: &RgbaImage) -> std::borrow::Cow<'_, RgbaImage> {
    let edge = img.width().max(img.height());
    if edge <= TRACE_EDGE {
        return std::borrow::Cow::Borrowed(img);
    }
    let k = TRACE_EDGE as f32 / edge as f32;
    std::borrow::Cow::Owned(image::imageops::resize(
        img,
        ((img.width() as f32 * k).round() as u32).max(1),
        ((img.height() as f32 * k).round() as u32).max(1),
        image::imageops::FilterType::Triangle,
    ))
}

/// How dark a pixel is, 0 (paper or clear) .. 1 (black ink).
fn ink(p: &image::Rgba<u8>) -> f32 {
    let [r, g, b, a] = p.0;
    let lum = (0.2126 * r as f32 + 0.7152 * g as f32 + 0.0722 * b as f32) / 255.0;
    (1.0 - lum) * a as f32 / 255.0
}

fn line_art(img: &RgbaImage, w: usize, h: usize, threshold: f32) -> Vec<Piece> {
    let field: Vec<f32> = img.pixels().map(ink).collect();
    // The ink's own colour: the average of its darkest part.
    let dark: Vec<[u8; 4]> = img
        .pixels()
        .filter(|p| ink(p) >= threshold.max(0.6))
        .map(|p| p.0)
        .collect();
    let rgb = average(&dark).unwrap_or([0, 0, 0]);
    let outlines = symphony_vector::trace::contours(&field, w, h, threshold.clamp(0.02, 0.98));
    symphony_vector::trace::shapes(outlines, TRACE_TOLERANCE, SPECK)
        .into_iter()
        .map(|shape| Piece {
            part: Part::Line,
            rgba: [rgb[0], rgb[1], rgb[2], 255],
            shape,
        })
        .collect()
}

fn average(px: &[[u8; 4]]) -> Option<[u8; 3]> {
    if px.is_empty() {
        return None;
    }
    let mut sum = [0u64; 3];
    for p in px {
        for c in 0..3 {
            sum[c] += p[c] as u64;
        }
    }
    let n = px.len() as u64;
    Some([(sum[0] / n) as u8, (sum[1] / n) as u8, (sum[2] / n) as u8])
}

fn colour(img: &RgbaImage, w: usize, h: usize, k: usize) -> Vec<Piece> {
    use rayon::prelude::*;
    let centres = kmeans(img, k);
    if centres.is_empty() {
        return Vec::new();
    }
    // Every pixel to its nearest colour (clear pixels to none).
    let nearest = |p: &image::Rgba<u8>| -> u8 {
        if p.0[3] < 128 {
            return u8::MAX;
        }
        let mut best = (f32::MAX, 0u8);
        for (i, c) in centres.iter().enumerate() {
            let d = dist(p.0, *c);
            if d < best.0 {
                best = (d, i as u8);
            }
        }
        best.1
    };
    let raw: Vec<u8> = img
        .pixels()
        .collect::<Vec<_>>()
        .par_iter()
        .map(|p| nearest(p))
        .collect();
    // Specks (JPEG noise, the in-between colours along anti-aliased
    // edges) join the colour around them, so nothing is left as a hole.
    let speck = ((w * h) / 40_000).clamp(6, 64);
    let labels = merge_specks(raw, w, h, speck);
    let lum = |c: [f32; 3]| (0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]) / 255.0;
    let mut count = vec![0usize; centres.len()];
    let mut border = vec![0usize; centres.len()];
    for (i, &l) in labels.iter().enumerate() {
        if let Some(c) = count.get_mut(l as usize) {
            *c += 1;
            let (x, y) = (i % w, i / w);
            if x == 0 || y == 0 || x == w - 1 || y == h - 1 {
                border[l as usize] += 1;
            }
        }
    }
    // Paper: a bright colour that runs along the picture's edge goes.
    let paper = (0..centres.len())
        .max_by_key(|&i| border[i])
        .filter(|&i| lum(centres[i]) > 0.85);
    // The darkest colour is the line work, if it's dark and not most of
    // the picture (then it's a dark background, painted as a zone).
    let darkest = (0..centres.len())
        .filter(|&i| count[i] > 0)
        .min_by(|&a, &b| lum(centres[a]).total_cmp(&lum(centres[b])))
        .filter(|&i| lum(centres[i]) < 0.3 && count[i] * 3 < w * h);
    let mut pieces = Vec::new();
    let mut labels = labels;
    if let Some(d) = darkest {
        // Lines are traced from the darkness itself, like line art: the
        // soft edge of anti-aliased ink gives a smooth line, and thin
        // lines (spread over in-between colours) survive.
        // Only colours near the ink count: half way between it and the
        // next darkest colour (dark hair or pupils stay colour zones).
        let dark = lum(centres[d]);
        let next = (0..centres.len())
            .filter(|&i| i != d && count[i] > 0)
            .map(|i| lum(centres[i]))
            .fold(1.0f32, f32::min);
        let span = (next - dark).max(0.05);
        let field: Vec<f32> = img
            .pixels()
            .map(|p| {
                let [r, g, b, a] = p.0;
                let l = (0.2126 * r as f32 + 0.7152 * g as f32 + 0.0722 * b as f32) / 255.0;
                ((next - l) / span).clamp(0.0, 1.0) * a as f32 / 255.0
            })
            .collect();
        let c = centres[d];
        let outlines = symphony_vector::trace::contours(&field, w, h, 0.5);
        pieces.extend(
            symphony_vector::trace::shapes(outlines, TRACE_TOLERANCE, SPECK)
                .into_iter()
                .map(|shape| Piece {
                    part: Part::Line,
                    rgba: [c[0] as u8, c[1] as u8, c[2] as u8, 255],
                    shape,
                }),
        );
        // The zones reach under the lines (as painted cartoons do).
        for (l, &f) in labels.iter_mut().zip(&field) {
            if f >= 0.3 || *l as usize == d {
                *l = u8::MAX - 1;
            }
        }
        grow_into(&mut labels, w, h, u8::MAX - 1);
    }
    let zones: Vec<Piece> = (0..centres.len())
        .into_par_iter()
        .filter(|&i| Some(i) != paper && Some(i) != darkest && count[i] > 0)
        .flat_map_iter(|i| {
            let field: Vec<f32> = labels
                .iter()
                .map(|&l| if l as usize == i { 1.0 } else { 0.0 })
                .collect();
            let c = centres[i];
            // Zones reach a little past their edge, under their
            // neighbours, so no seam of background shows between them.
            let outlines = symphony_vector::trace::contours(&field, w, h, 0.2);
            symphony_vector::trace::shapes(outlines, ZONE_TOLERANCE, 0.0)
                .into_iter()
                .map(move |shape| Piece {
                    part: Part::Zone,
                    rgba: [c[0] as u8, c[1] as u8, c[2] as u8, 255],
                    shape,
                })
                .collect::<Vec<_>>()
        })
        .collect();
    pieces.extend(zones);
    // Paper is only what's connected to the picture's edge: white inside
    // the drawing (eyes, teeth, highlights) stays as white zones.
    if let Some(pi) = paper {
        let mut outside = vec![false; labels.len()];
        let mut queue: Vec<usize> = (0..labels.len())
            .filter(|&i| {
                let (x, y) = (i % w, i / w);
                (x == 0 || y == 0 || x == w - 1 || y == h - 1) && labels[i] as usize == pi
            })
            .collect();
        for &i in &queue {
            outside[i] = true;
        }
        while let Some(i) = queue.pop() {
            let (x, y) = (i % w, i / w);
            for (ok, j) in [
                (x > 0, i.wrapping_sub(1)),
                (x + 1 < w, i + 1),
                (y > 0, i.wrapping_sub(w)),
                (y + 1 < h, i + w),
            ] {
                if ok && !outside[j] && labels[j] as usize == pi {
                    outside[j] = true;
                    queue.push(j);
                }
            }
        }
        let field: Vec<f32> = labels
            .iter()
            .zip(&outside)
            .map(|(&l, &o)| if l as usize == pi && !o { 1.0 } else { 0.0 })
            .collect();
        let c = centres[pi];
        let outlines = symphony_vector::trace::contours(&field, w, h, 0.2);
        pieces.extend(
            symphony_vector::trace::shapes(outlines, ZONE_TOLERANCE, 0.0)
                .into_iter()
                .map(|shape| Piece {
                    part: Part::Zone,
                    rgba: [c[0] as u8, c[1] as u8, c[2] as u8, 255],
                    shape,
                }),
        );
    }
    pieces
}

/// Pixels labelled `gap` take a neighbour's label, pass by pass, until
/// none are left (or nothing borders them).
fn grow_into(labels: &mut [u8], w: usize, h: usize, gap: u8) {
    loop {
        let mut changed = false;
        let mut left = false;
        let before = labels.to_vec();
        for i in 0..labels.len() {
            if before[i] != gap {
                continue;
            }
            let (x, y) = (i % w, i / w);
            let mut take = None;
            for (ok, j) in [
                (x > 0, i.wrapping_sub(1)),
                (x + 1 < w, i + 1),
                (y > 0, i.wrapping_sub(w)),
                (y + 1 < h, i + w),
            ] {
                if ok && before[j] != gap {
                    take = Some(before[j]);
                    break;
                }
            }
            match take {
                Some(l) => {
                    labels[i] = l;
                    changed = true;
                }
                None => left = true,
            }
        }
        if !changed || !left {
            break;
        }
    }
}

/// Simplification for colour zones: finer than for lines, so neighbouring
/// zones (simplified separately) stay overlapped.
const ZONE_TOLERANCE: f32 = 0.2;

/// Connected areas of one label smaller than `smallest` pixels take the
/// label most common around them.
fn merge_specks(mut labels: Vec<u8>, w: usize, h: usize, smallest: usize) -> Vec<u8> {
    let mut seen = vec![false; labels.len()];
    let mut area: Vec<usize> = Vec::new();
    let mut queue: Vec<usize> = Vec::new();
    for start in 0..labels.len() {
        if seen[start] {
            continue;
        }
        let own = labels[start];
        area.clear();
        queue.clear();
        queue.push(start);
        seen[start] = true;
        let mut around = [0u32; 256];
        while let Some(i) = queue.pop() {
            area.push(i);
            let (x, y) = (i % w, i / w);
            let mut visit = |j: usize| {
                if labels[j] == own {
                    if !seen[j] {
                        seen[j] = true;
                        queue.push(j);
                    }
                } else {
                    around[labels[j] as usize] += 1;
                }
            };
            if x > 0 {
                visit(i - 1);
            }
            if x + 1 < w {
                visit(i + 1);
            }
            if y > 0 {
                visit(i - w);
            }
            if y + 1 < h {
                visit(i + w);
            }
        }
        if area.len() < smallest {
            if let Some((to, _)) = around
                .iter()
                .enumerate()
                .filter(|(_, n)| **n > 0)
                .max_by_key(|(_, n)| **n)
            {
                for &i in &area {
                    labels[i] = to as u8;
                }
            }
        }
    }
    labels
}

fn dist(p: [u8; 4], c: [f32; 3]) -> f32 {
    (p[0] as f32 - c[0]).powi(2) + (p[1] as f32 - c[1]).powi(2) + (p[2] as f32 - c[2]).powi(2)
}

/// The picture's main colours (k-means on a sample of its pixels).
fn kmeans(img: &RgbaImage, k: usize) -> Vec<[f32; 3]> {
    let all: Vec<[u8; 4]> = img.pixels().map(|p| p.0).filter(|p| p[3] >= 128).collect();
    if all.is_empty() {
        return Vec::new();
    }
    let stride = (all.len() / 60_000).max(1);
    let sample: Vec<[u8; 4]> = all.iter().step_by(stride).copied().collect();
    // Start from the most common colours (flat fills are many pixels of
    // one colour; anti-aliased edges are few of each), skipping ones too
    // close to a colour already chosen.
    let mut bins: std::collections::HashMap<u16, (u32, [u32; 3])> = Default::default();
    for p in &sample {
        let key = ((p[0] as u16 >> 3) << 10) | ((p[1] as u16 >> 3) << 5) | (p[2] as u16 >> 3);
        let e = bins.entry(key).or_default();
        e.0 += 1;
        for c in 0..3 {
            e.1[c] += p[c] as u32;
        }
    }
    let mut bins: Vec<(u32, [f32; 3])> = bins
        .into_values()
        .map(|(n, sum)| (n, sum.map(|v| v as f32 / n as f32)))
        .collect();
    bins.sort_by(|a, b| b.0.cmp(&a.0).then(a.1[0].total_cmp(&b.1[0])));
    let mut centres: Vec<[f32; 3]> = Vec::new();
    let close = |a: [f32; 3], b: [f32; 3]| {
        (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2) < 28.0 * 28.0
    };
    for (_, c) in &bins {
        if centres.len() >= k {
            break;
        }
        if !centres.iter().any(|o| close(*o, *c)) {
            centres.push(*c);
        }
    }
    for _ in 0..12 {
        let mut sum = vec![[0f64; 4]; centres.len()];
        for p in &sample {
            let (i, _) = centres
                .iter()
                .enumerate()
                .map(|(i, c)| (i, dist(*p, *c)))
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .unwrap_or((0, 0.0));
            for c in 0..3 {
                sum[i][c] += p[c] as f64;
            }
            sum[i][3] += 1.0;
        }
        for (c, s) in centres.iter_mut().zip(&sum) {
            if s[3] > 0.0 {
                *c = [
                    (s[0] / s[3]) as f32,
                    (s[1] / s[3]) as f32,
                    (s[2] / s[3]) as f32,
                ];
            }
        }
    }
    centres
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area(art: &VectorArt, part: Part) -> f32 {
        art.pieces
            .iter()
            .filter(|p| p.part == part)
            .map(|p| symphony_vector::shape::shape_area(&p.shape).abs())
            .sum()
    }

    #[test]
    fn svg_fills_become_zones_and_strokes_become_lines() {
        let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="100">
            <rect x="10" y="10" width="80" height="80" fill="#ff0000"/>
            <path d="M 120 50 L 190 50" stroke="#0000ff" stroke-width="10" fill="none"/>
            <circle cx="150" cy="20" r="10" fill="#00ff00" fill-opacity="0.5"/>
        </svg>"##;
        let art = from_svg(svg).unwrap();
        assert_eq!(art.size, [200.0, 100.0]);
        let zones: Vec<_> = art.pieces.iter().filter(|p| p.part == Part::Zone).collect();
        assert_eq!(zones.len(), 2);
        assert_eq!(zones[0].rgba, [255, 0, 0, 255]);
        assert_eq!(zones[1].rgba[3], 128, "fill opacity carries over");
        assert!(
            (area(&art, Part::Line) - 700.0).abs() < 20.0,
            "a 70 x 10 line"
        );
        assert!(area(&art, Part::Zone) > 6400.0);
    }

    /// Text comes out as letter shapes: more letters cover more, a bigger
    /// size scales them, and alignment moves them about x = 0.
    #[test]
    fn text_becomes_letter_shapes() {
        let style = TextStyle {
            size: 50.0,
            ..Default::default()
        };
        let one = text_shapes("I", &style);
        if one.is_empty() {
            eprintln!("No fonts on this machine: skipping");
            return;
        }
        let total = |s: &[Shape]| -> f32 {
            s.iter()
                .map(|x| symphony_vector::shape::shape_area(x).abs())
                .sum()
        };
        let word = text_shapes("HELLO", &style);
        assert!(total(&word) > total(&one) * 3.0);
        let big = text_shapes(
            "HELLO",
            &TextStyle {
                size: 100.0,
                ..style.clone()
            },
        );
        assert!((total(&big) / total(&word) - 4.0).abs() < 0.5);
        let xs = |s: &[Shape]| {
            s.iter()
                .flatten()
                .flatten()
                .map(|p| p[0])
                .fold(f32::MIN, f32::max)
        };
        let right = text_shapes(
            "HELLO",
            &TextStyle {
                align: TextAlign::Right,
                ..style.clone()
            },
        );
        assert!(xs(&right) <= 0.5, "right-aligned text ends at x = 0");
        assert!(text_shapes("  ", &style).is_empty());
        // Two lines go down the page.
        let two = text_shapes("A\nA", &style);
        let ys = two
            .iter()
            .flatten()
            .flatten()
            .map(|p| p[1])
            .fold(0.0f32, f32::max);
        assert!(ys > 90.0, "{ys}");
    }

    #[test]
    fn svg_holes_follow_the_fill_rule() {
        let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100">
            <path fill-rule="evenodd" fill="#000"
              d="M 0 0 H 100 V 100 H 0 Z M 25 25 H 75 V 75 H 25 Z"/>
        </svg>"##;
        let art = from_svg(svg).unwrap();
        assert!((area(&art, Part::Zone) - 7500.0).abs() < 1.0);
    }

    /// Scanned-style line art (dark strokes on off-white paper) traces to
    /// lines only, roughly the ink's area, and the paper is gone.
    #[test]
    fn line_art_traces_to_lines() {
        let mut img = RgbaImage::from_pixel(120, 80, image::Rgba([245, 242, 235, 255]));
        for x in 10..110 {
            for y in 38..42 {
                img.put_pixel(x, y, image::Rgba([20, 20, 20, 255]));
            }
        }
        let art = vectorize(&img, Vectorize::LineArt { threshold: 0.5 });
        assert!(art.pieces.iter().all(|p| p.part == Part::Line));
        assert!(
            (area(&art, Part::Line) - 400.0).abs() < 30.0,
            "{}",
            area(&art, Part::Line)
        );
    }

    /// White inside a drawing (an eye) stays as a white zone; only the
    /// paper around it goes.
    #[test]
    fn white_inside_the_drawing_stays() {
        let mut img = RgbaImage::from_pixel(100, 100, image::Rgba([255, 255, 255, 255]));
        for y in 20..80 {
            for x in 20..80 {
                let edge = !(24..76).contains(&x) || !(24..76).contains(&y);
                let eye = (40..60).contains(&x) && (40..60).contains(&y);
                let c = match (edge, eye) {
                    (true, _) => [0, 0, 0, 255],
                    (_, true) => [255, 255, 255, 255],
                    _ => [230, 120, 40, 255],
                };
                img.put_pixel(x, y, image::Rgba(c));
            }
        }
        let art = vectorize(&img, Vectorize::Colour { colours: 4 });
        let white: f32 = art
            .pieces
            .iter()
            .filter(|p| p.rgba[0] > 240 && p.rgba[1] > 240)
            .map(|p| symphony_vector::shape::shape_area(&p.shape).abs())
            .sum();
        assert!((white - 400.0).abs() < 60.0, "the eye: {white}");
    }

    /// A flat-coloured picture traces into zones of its colours, with the
    /// black outline as lines and the white paper left out.
    #[test]
    fn colour_pictures_trace_to_zones_and_lines() {
        let mut img = RgbaImage::from_pixel(100, 100, image::Rgba([255, 255, 255, 255]));
        for y in 20..80 {
            for x in 20..80 {
                let edge = !(24..76).contains(&x) || !(24..76).contains(&y);
                let c = if edge {
                    [0, 0, 0, 255]
                } else {
                    [230, 120, 40, 255]
                };
                img.put_pixel(x, y, image::Rgba(c));
            }
        }
        let art = vectorize(&img, Vectorize::Colour { colours: 4 });
        let orange = art
            .pieces
            .iter()
            .find(|p| p.part == Part::Zone)
            .expect("an orange zone");
        assert!(orange.rgba[0] > 200 && orange.rgba[2] < 80);
        // The zone reaches under the outline, as painted cartoons do.
        let zone = area(&art, Part::Zone);
        assert!(zone > 52.0 * 52.0 && zone < 61.0 * 61.0, "{zone}");
        assert!(area(&art, Part::Line) > 700.0);
        assert!(
            art.pieces.iter().all(|p| p.rgba[..3] != [255, 255, 255]),
            "the paper around it is gone"
        );
    }
}
