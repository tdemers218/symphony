//! Vector brushes made from bitmap brushes: the tip image is traced into a
//! shape once, placed at every stamp exactly where the painting engine
//! would stamp it ([`crate::stamp::dabs`]: same spacing, scatter, turn and
//! roundness), and all of them merged into one outline. The result is a
//! clean vector shape with the brush's character in its edge: sharp at any
//! zoom, recolourable, and solid for the eraser and the paint bucket.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use i_overlay::core::fill_rule::FillRule;
use i_overlay::core::overlay_rule::OverlayRule;
use i_overlay::float::single::SingleFloatOverlay;
use symphony_core::stroke::{Stroke, StrokeTexture, Texture};

use crate::shape::Shape;
use crate::stamp::{dabs, Dab};

/// Tips are traced at most this many pixels a side (finer detail is lost
/// in the merge anyway).
const TRACE_SIDE: u32 = 64;
/// A tip pixel is part of the shape from this coverage up.
const SOLID: u8 = 128;
/// The closest vector-brush stamps are placed, in tip diameters.
const MIN_SHAPE_SPACING: f32 = 0.3;
/// Random shrinking a vector brush keeps at most (so it doesn't break up).
const MAX_SHAPE_SIZE_JITTER: f32 = 0.5;
/// The farthest apart flattened-tip stamps are placed, so they join up.
const FLAT_SHAPE_SPACING: f32 = 0.06;
/// Points on a round tip's outline.
const ROUND_SIDES: usize = 20;

/// Contours of a traced tip in its own frame: -1..1 across, the longer
/// side fitted, as the painting engine maps it.
type Traced = Arc<Vec<Vec<[f32; 2]>>>;

static TRACED: Mutex<Option<HashMap<u64, Traced>>> = Mutex::new(None);

fn hash_pixels(t: &Texture) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (t.width, t.height).hash(&mut h);
    t.pixels.hash(&mut h);
    h.finish()
}

/// The tip's outline (traced once per image), or a circle for round tips.
pub fn tip_outline(tip: Option<&Texture>) -> Traced {
    let Some(t) = tip.filter(|t| t.is_valid()) else {
        let circle = (0..ROUND_SIDES)
            .map(|i| {
                let a = i as f32 / ROUND_SIDES as f32 * std::f32::consts::TAU;
                [a.cos(), a.sin()]
            })
            .collect();
        return Arc::new(vec![circle]);
    };
    let key = hash_pixels(t);
    if let Some(found) = TRACED
        .lock()
        .ok()
        .and_then(|g| g.as_ref().and_then(|m| m.get(&key).cloned()))
    {
        return found;
    }
    let traced = Arc::new(trace(t));
    if let Ok(mut g) = TRACED.lock() {
        let m = g.get_or_insert_with(HashMap::new);
        if m.len() > 256 {
            m.clear();
        }
        m.insert(key, traced.clone());
    }
    traced
}

/// Trace a tip: its solid pixels (at a reduced size) as runs of
/// rectangles, merged into outlines, simplified, in the tip frame.
fn trace(t: &Texture) -> Vec<Vec<[f32; 2]>> {
    let k = (TRACE_SIDE as f32 / t.width.max(t.height) as f32).min(1.0);
    let (w, h) = (
        ((t.width as f32 * k).round() as u32).max(1),
        ((t.height as f32 * k).round() as u32).max(1),
    );
    // Area average into the smaller grid.
    let at = |x: u32, y: u32| -> u8 {
        let (x0, y0) = ((x as f32 / k) as u32, (y as f32 / k) as u32);
        let (x1, y1) = (
            (((x + 1) as f32 / k) as u32).clamp(x0 + 1, t.width),
            (((y + 1) as f32 / k) as u32).clamp(y0 + 1, t.height),
        );
        let mut sum = 0u32;
        let mut n = 0u32;
        for yy in y0..y1.min(t.height) {
            for xx in x0..x1.min(t.width) {
                sum += t.pixels[(yy * t.width + xx) as usize] as u32;
                n += 1;
            }
        }
        (sum / n.max(1)) as u8
    };
    let mut rects: Vec<Vec<[f32; 2]>> = Vec::new();
    for y in 0..h {
        let mut x = 0;
        while x < w {
            if at(x, y) < SOLID {
                x += 1;
                continue;
            }
            let start = x;
            while x < w && at(x, y) >= SOLID {
                x += 1;
            }
            let (a, b, yy) = (start as f32, x as f32, y as f32);
            rects.push(vec![[a, yy], [b, yy], [b, yy + 1.0], [a, yy + 1.0]]);
        }
    }
    if rects.is_empty() {
        return Vec::new();
    }
    let empty: Vec<[f32; 2]> = Vec::new();
    let merged = crate::shape::tidy(rects.overlay(&empty, OverlayRule::Subject, FillRule::NonZero));
    // Into the tip frame, as the painting engine maps it: the longer side
    // spans -1..1 and the shorter is centred.
    let side = w.max(h) as f32;
    let (ox, oy) = ((side - w as f32) / 2.0, (side - h as f32) / 2.0);
    merged
        .into_iter()
        .flatten()
        .map(|c| {
            c.into_iter()
                .map(|[x, y]| [(x + ox) / side * 2.0 - 1.0, (y + oy) / side * 2.0 - 1.0])
                .collect()
        })
        .collect()
}

/// A dab's tip outline placed in drawing space.
fn place(tip: &[Vec<[f32; 2]>], d: &Dab) -> Vec<Vec<[f32; 2]>> {
    let r = d.d * 0.5;
    let (sin, cos) = d.angle.sin_cos();
    tip.iter()
        .map(|c| {
            c.iter()
                .map(|&[u, v]| {
                    let v = v * d.ratio;
                    [
                        d.c[0] + r * (u * cos - v * sin),
                        d.c[1] + r * (u * sin + v * cos),
                    ]
                })
                .collect()
        })
        .collect()
}

/// The stroke's outline as a vector brush: every stamp's tip shape merged.
/// Empty when nothing would show.
pub fn stamped_outline(stroke: &Stroke, tex: &StrokeTexture, tip: Option<&Texture>) -> Vec<Shape> {
    let tip_shape = tip_outline(tip);
    if tip_shape.is_empty() {
        return Vec::new();
    }
    // Stamps much closer than a third of a tip apart only add work: the
    // merged outline is the same to the eye.
    // A flattened tip is much thinner than its width, so its stamps sit
    // closer (as a fraction of the width) to stay joined up.
    let dy = tex.dynamics;
    let flattened = dy.roundness > 0.0 || dy.roundness_pressure || dy.roundness_tilt;
    let mut tex = *tex;
    tex.spacing = if flattened {
        tex.spacing.clamp(0.01, FLAT_SHAPE_SPACING)
    } else {
        tex.spacing.max(MIN_SHAPE_SPACING)
    };
    tex.auto_spacing = 0.0;
    // Soft bitmap stamps hide stamps that shrink to nothing; solid shapes
    // would break into dashes. Keep the roughness, not the gaps.
    tex.dynamics.size_jitter = tex.dynamics.size_jitter.min(MAX_SHAPE_SIZE_JITTER);
    let polygons: Vec<Vec<[f32; 2]>> = dabs(stroke, &tex)
        .iter()
        .filter(|d| d.d > 0.05)
        .flat_map(|d| place(&tip_shape, d))
        .filter(|c| c.len() >= 3)
        .collect();
    if polygons.is_empty() {
        return Vec::new();
    }
    crate::shape::tidy(crate::shape::merge(polygons))
}

/// Recently built vector-brush outlines by content, shared by the canvas,
/// the eraser and export so a stroke's outline is built once.
static OUTLINES: crate::recent::Shared<Vec<Shape>> = crate::recent::Shared::new(2048);

/// What a vector-brush stroke's stamped outline depends on.
pub fn outline_key(stroke: &Stroke, tex: &StrokeTexture) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for p in &stroke.points {
        (
            p.x.to_bits(),
            p.y.to_bits(),
            p.pressure.to_bits(),
            p.width_offset.to_bits(),
        )
            .hash(&mut h);
        (
            p.pen.tilt.to_bits(),
            p.pen.azimuth.to_bits(),
            p.pen.rotation.to_bits(),
        )
            .hash(&mut h);
    }
    (
        stroke.min_width.to_bits(),
        stroke.max_width.to_bits(),
        // The stroke's random seed (scatter, jitter).
        stroke.id.0,
    )
        .hash(&mut h);
    // The tip is named by its texture key in here.
    format!("{tex:?}").hash(&mut h);
    h.finish()
}

/// A stroke's stamped outline if it was built lately (never builds it).
pub fn cached_outline(key: u64) -> Option<Arc<Vec<Shape>>> {
    OUTLINES.get(key)
}

/// The stroke's stamped outline, built once and then shared.
pub fn shared_outline(
    stroke: &Stroke,
    tex: &StrokeTexture,
    tip: Option<&Texture>,
) -> Arc<Vec<Shape>> {
    OUTLINES.get_or_build(outline_key(stroke, tex), || {
        stamped_outline(stroke, tex, tip)
    })
}

/// What a vector-brush stroke covers: its merged stamps, or once the
/// eraser has cut it, its outline.
pub fn vector_brush_shapes(
    stroke: &Stroke,
    tex: &StrokeTexture,
    tip: Option<&Texture>,
) -> Vec<Shape> {
    if !stroke.shape.is_empty() {
        return vec![stroke.shape.clone()];
    }
    shared_outline(stroke, tex, tip).as_ref().clone()
}

/// About how many stamps a vector-brush stroke places (to decide whether
/// building its outline can be done right away).
pub fn estimated_stamps(stroke: &Stroke, tex: &StrokeTexture) -> f32 {
    let length: f32 = stroke
        .points
        .windows(2)
        .map(|w| (w[1].x - w[0].x).hypot(w[1].y - w[0].y))
        .sum();
    let dy = tex.dynamics;
    let spacing = if dy.roundness > 0.0 || dy.roundness_pressure || dy.roundness_tilt {
        tex.spacing.clamp(0.01, FLAT_SHAPE_SPACING)
    } else {
        tex.spacing.max(MIN_SHAPE_SPACING)
    };
    length / (spacing * stroke.min_width.max(stroke.max_width * 0.25).max(0.5))
}

#[cfg(test)]
mod tests {
    use super::*;
    use symphony_core::id::Id;
    use symphony_core::stroke::{Point, StrokeKind};

    fn stroke(width: f32) -> Stroke {
        Stroke {
            id: Id(3),
            kind: StrokeKind::Brush,
            art: Default::default(),
            swatch: Id(0),
            min_width: width,
            max_width: width,
            points: [(0.0, 0.0), (100.0, 0.0)]
                .iter()
                .map(|&(x, y)| Point {
                    x,
                    y,
                    pressure: 1.0,
                    width_offset: 0.0,
                    pen: Default::default(),
                })
                .collect(),
            tips: Default::default(),
            texture: None,
            shape: Vec::new(),
        }
    }

    fn area(shapes: &[Shape]) -> f32 {
        shapes.iter().map(crate::shape::shape_area).sum()
    }

    /// Cost of a long, thin vector-brush stroke (the worst case: many
    /// small stamps). `cargo test --release -p symphony-vector
    /// stamped_cost -- --ignored --nocapture`.
    #[test]
    #[ignore = "benchmark"]
    fn stamped_cost() {
        let mut s = stroke(3.0);
        s.points = (0..400)
            .map(|i| Point {
                x: i as f32 * 3.0,
                y: (i as f32 * 0.05).sin() * 80.0,
                pressure: 0.5 + 0.5 * (i as f32 * 0.03).sin(),
                width_offset: 0.0,
                pen: Default::default(),
            })
            .collect();
        let tex = StrokeTexture {
            spacing: 0.1,
            ..Default::default()
        };
        let _ = stamped_outline(&s, &tex, None);
        let t = std::time::Instant::now();
        let shapes = stamped_outline(&s, &tex, None);
        let grouped = t.elapsed();
        let tip = tip_outline(None);
        let mut wide = tex;
        wide.spacing = MIN_SHAPE_SPACING;
        let polygons: Vec<Vec<[f32; 2]>> = dabs(&s, &wide)
            .iter()
            .flat_map(|d| place(&tip, d))
            .collect();
        let n = polygons.len();
        let empty: Vec<[f32; 2]> = Vec::new();
        let t = std::time::Instant::now();
        let _ = polygons.overlay(&empty, OverlayRule::Subject, FillRule::NonZero);
        println!(
            "stamped_cost: {grouped:?} (one merge {:?}), {n} stamps, {} shapes",
            t.elapsed(),
            shapes.len()
        );
        // scripts/perf.sh: about twice today's time under load.
        if std::env::var_os("SYMPHONY_PERF_GATE").is_some() {
            let ms = grouped.as_secs_f32() * 1000.0;
            assert!(ms < 12.0, "a long vector-brush outline took {ms:.1} ms");
        }
    }

    /// Merging in parallel groups covers the same area as one merge.
    #[test]
    fn grouped_union_matches_one_merge() {
        let mut s = stroke(4.0);
        s.points = (0..300)
            .map(|i| Point {
                x: i as f32 * 2.0,
                y: (i as f32 * 0.07).sin() * 40.0,
                pressure: 1.0,
                width_offset: 0.0,
                pen: Default::default(),
            })
            .collect();
        let tex = StrokeTexture {
            spacing: 0.05,
            ..Default::default()
        };
        let tip = tip_outline(None);
        let polygons: Vec<Vec<[f32; 2]>> =
            dabs(&s, &tex).iter().flat_map(|d| place(&tip, d)).collect();
        assert!(
            polygons.len() > crate::shape::MERGE_GROUP * 2,
            "{}",
            polygons.len()
        );
        let empty: Vec<[f32; 2]> = Vec::new();
        let one =
            crate::shape::tidy(polygons.overlay(&empty, OverlayRule::Subject, FillRule::NonZero));
        let grouped = crate::shape::tidy(crate::shape::merge(polygons));
        let (a, b) = (area(&one), area(&grouped));
        assert!((a - b).abs() < a * 1e-3, "{a} vs {b}");
        assert_eq!(one.len(), grouped.len());
    }

    /// A built outline is shared, not built again.
    #[test]
    fn outlines_are_built_once() {
        let tex = StrokeTexture {
            spacing: 0.2,
            ..Default::default()
        };
        let mut s = stroke(6.0);
        s.id = Id(987_654);
        let key = outline_key(&s, &tex);
        let first = shared_outline(&s, &tex, None);
        assert!(cached_outline(key).is_some_and(|c| Arc::ptr_eq(&c, &first)));
        assert!(Arc::ptr_eq(&shared_outline(&s, &tex, None), &first));
    }

    /// A round tip stamped densely is a capsule: about width x length plus
    /// round ends.
    #[test]
    fn a_round_vector_brush_is_its_line() {
        let tex = StrokeTexture {
            spacing: 0.1,
            ..Default::default()
        };
        let shapes = stamped_outline(&stroke(10.0), &tex, None);
        let want = 100.0 * 10.0 + std::f32::consts::PI * 25.0;
        assert!(
            (area(&shapes) - want).abs() < want * 0.05,
            "{}",
            area(&shapes)
        );
    }

    /// A square tip turned 45 degrees and flattened gives a different,
    /// narrower outline: the tip's shape and dynamics carry over.
    #[test]
    fn the_tip_shape_and_dynamics_carry_over() {
        let square = Texture {
            width: 8,
            height: 8,
            pixels: vec![255; 64],
        };
        let mut tex = StrokeTexture {
            spacing: 0.25,
            ..Default::default()
        };
        let full = area(&stamped_outline(&stroke(10.0), &tex, Some(&square)));
        assert!(
            full > 1000.0,
            "a square tip covers at least the round one: {full}"
        );
        tex.dynamics.roundness = 0.3;
        let flat = area(&stamped_outline(&stroke(10.0), &tex, Some(&square)));
        assert!(flat < full * 0.5, "flattened: {flat} vs {full}");
        // A tip with a gap in it keeps a ragged, not solid, edge.
        let mut comb = Texture {
            width: 8,
            height: 8,
            pixels: vec![0; 64],
        };
        for y in 0..8 {
            for x in 0..8 {
                if x % 4 < 2 {
                    comb.pixels[y * 8 + x] = 255;
                }
            }
        }
        let stripes = tip_outline(Some(&comb));
        assert!(
            stripes.len() >= 2,
            "a comb tip traces as separate teeth: {}",
            stripes.len()
        );
    }
}
