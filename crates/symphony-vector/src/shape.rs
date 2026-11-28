//! Brush strokes as outlines, so the Eraser can cut its own shape out of
//! them (Harmony's brush strokes are contours). A stroke keeps its
//! centreline until first erased; from then on its `shape` (outer contour
//! first, then holes) is what is drawn, hit and painted against. Paint
//! zones are cut the same way.

use i_overlay::core::fill_rule::FillRule;
use i_overlay::core::overlay_rule::OverlayRule;
use i_overlay::float::single::SingleFloatOverlay;
use symphony_core::math::Affine;
use symphony_core::stroke::Stroke;

use crate::cut::Disc;
use crate::tessellate;

/// Contours of one shape: the outer edge first, then any holes.
pub type Shape = Vec<Vec<[f32; 2]>>;

/// Sides of the polygon standing in for a round eraser stamp.
const DISC_SIDES: usize = 24;

fn area(c: &[[f32; 2]]) -> f32 {
    let n = c.len();
    (0..n)
        .map(|i| {
            let (a, b) = (c[i], c[(i + 1) % n]);
            a[0] * b[1] - b[0] * a[1]
        })
        .sum::<f32>()
        * 0.5
}

/// Area of a shape (outer minus holes).
pub fn shape_area(s: &Shape) -> f32 {
    s.iter().map(|c| area(c)).sum::<f32>().abs()
}

fn bbox(contours: &[Vec<[f32; 2]>]) -> Option<([f32; 2], [f32; 2])> {
    let mut it = contours.iter().flatten();
    let f = it.next()?;
    let (mut lo, mut hi) = (*f, *f);
    for p in it {
        lo = [lo[0].min(p[0]), lo[1].min(p[1])];
        hi = [hi[0].max(p[0]), hi[1].max(p[1])];
    }
    Some((lo, hi))
}

/// How far (drawing units, a fraction of a pixel at 100%) a point may sit
/// off the straight line between its neighbours and still be dropped.
const SIMPLIFY: f32 = 0.06;
/// Pieces smaller than this (square units) are invisible slivers.
const SLIVER: f32 = 0.25;

/// Drop points that don't change the contour's look (Douglas–Peucker:
/// the result never strays more than `SIMPLIFY` from the original).
/// Without this, every eraser pass leaves more points behind and each
/// later pass gets slower.
fn simplify(c: &[[f32; 2]]) -> Vec<[f32; 2]> {
    let n = c.len();
    if n <= 4 {
        return c.to_vec();
    }
    let off = |a: [f32; 2], b: [f32; 2], p: [f32; 2]| {
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let len2 = dx * dx + dy * dy;
        if len2 < 1e-12 {
            return (p[0] - a[0]).hypot(p[1] - a[1]);
        }
        // Distance to the segment (not the infinite line), so spikes stay.
        let t = (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / len2).clamp(0.0, 1.0);
        (p[0] - a[0] - t * dx).hypot(p[1] - a[1] - t * dy)
    };
    // A closed contour: split at the point farthest from the first.
    let far = (1..n)
        .max_by(|&i, &j| {
            let d = |k: usize| (c[k][0] - c[0][0]).hypot(c[k][1] - c[0][1]);
            d(i).total_cmp(&d(j))
        })
        .unwrap_or(n / 2);
    let mut keep = vec![false; n];
    keep[0] = true;
    keep[far] = true;
    // Spans to check, as (start, end) indices on the ring.
    let mut stack = vec![(0usize, far), (far, n)];
    while let Some((a, b)) = stack.pop() {
        if b <= a + 1 {
            continue;
        }
        let (pa, pb) = (c[a], c[b % n]);
        let (mut worst, mut at) = (0.0f32, a);
        for (k, &p) in c.iter().enumerate().take(b).skip(a + 1) {
            let d = off(pa, pb, p);
            if d > worst {
                worst = d;
                at = k;
            }
        }
        if worst > SIMPLIFY {
            keep[at] = true;
            stack.push((a, at));
            stack.push((at, b));
        }
    }
    c.iter()
        .zip(&keep)
        .filter(|(_, k)| **k)
        .map(|(p, _)| *p)
        .collect()
}

/// Merged shapes, cleaned: points that don't change the look dropped and
/// invisible slivers removed.
pub(crate) fn tidy(shapes: Vec<Vec<Vec<[f32; 2]>>>) -> Vec<Shape> {
    to_f32(shapes)
}

fn to_f32(shapes: Vec<Vec<Vec<[f32; 2]>>>) -> Vec<Shape> {
    shapes
        .into_iter()
        .map(|s| {
            s.iter()
                .map(|c| simplify(c))
                .filter(|c| c.len() >= 3 && area(c).abs() > SLIVER * 0.1)
                .collect::<Shape>()
        })
        .filter(|s| !s.is_empty() && shape_area(s) > SLIVER)
        .collect()
}

/// The outline of a centreline stroke: its triangles merged into one
/// shape (or several, if it crosses itself into separate parts).
pub fn outline(stroke: &Stroke) -> Vec<Shape> {
    if !stroke.shape.is_empty() {
        return vec![stroke.shape.clone()];
    }
    // The eraser asks for the outline of every stroke near it on every
    // move, so it's built once per shape of the stroke.
    OUTLINES
        .get_or_build(outline_key(stroke), || {
            triangles_outline(stroke, OUTLINE_STEP)
        })
        .as_ref()
        .clone()
}

static OUTLINES: crate::recent::Shared<Vec<Shape>> = crate::recent::Shared::new(2048);

/// What a centreline stroke's outline depends on.
fn outline_key(stroke: &Stroke) -> u64 {
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
    (stroke.min_width.to_bits(), stroke.max_width.to_bits()).hash(&mut h);
    format!("{:?}{:?}", stroke.kind, stroke.tips).hash(&mut h);
    h.finish()
}

/// Polygons (filled by the non-zero rule, or even-odd) as clean shapes:
/// overlaps and self-crossings resolved, each an outside with its holes.
/// For importing vector files.
pub fn from_polygons(polygons: Vec<Vec<[f32; 2]>>, even_odd: bool) -> Vec<Shape> {
    let rule = if even_odd {
        FillRule::EvenOdd
    } else {
        FillRule::NonZero
    };
    let empty: Vec<[f32; 2]> = Vec::new();
    to_f32(polygons.overlay(&empty, OverlayRule::Subject, rule))
}

/// Polygons merged per group before the groups are merged together.
pub(crate) const MERGE_GROUP: usize = 128;

/// Merge many polygons into outlines. A union doesn't depend on the order
/// it's done in, so big sets merge in groups across the cores and the
/// groups' outlines (far fewer points) are merged last.
pub(crate) fn merge(polygons: Vec<Vec<[f32; 2]>>) -> Vec<Vec<Vec<[f32; 2]>>> {
    use rayon::prelude::*;
    let empty: Vec<[f32; 2]> = Vec::new();
    let mut polygons = polygons;
    // Merge in groups while that still shrinks the set (pieces that don't
    // touch, like scattered stamps, never shrink: then merge them all).
    while polygons.len() > MERGE_GROUP * 2 {
        let parts: Vec<Vec<[f32; 2]>> = polygons
            .par_chunks(MERGE_GROUP)
            .flat_map_iter(|group| {
                group
                    .overlay(&empty, OverlayRule::Subject, FillRule::NonZero)
                    .into_iter()
                    .flatten()
            })
            .collect();
        let shrank = parts.len() * 2 <= polygons.len();
        polygons = parts;
        if !shrank {
            break;
        }
    }
    polygons.overlay(&empty, OverlayRule::Subject, FillRule::NonZero)
}

/// How finely (drawing units) a stroke is sampled for its outline. The
/// outline is merged from the stroke's triangles, and merging cost grows
/// with their number: 0.05 took ~70 ms per stroke, which made erasing
/// heavy art crawl. 0.4 is still within ~0.01 units on curves, invisible
/// even at 16x zoom.
const OUTLINE_STEP: f32 = 0.4;

/// The stroke's triangles at `step`, merged into its outline.
fn triangles_outline(stroke: &Stroke, step: f32) -> Vec<Shape> {
    let mesh = tessellate(stroke, Affine::IDENTITY, step);
    let tris: Vec<Vec<[f32; 2]>> = mesh
        .indices
        .as_chunks::<3>()
        .0
        .iter()
        .map(|t| {
            let (a, b, c) = (
                mesh.vertices[t[0] as usize],
                mesh.vertices[t[1] as usize],
                mesh.vertices[t[2] as usize],
            );
            if area(&[a, b, c]) < 0.0 {
                vec![a, c, b]
            } else {
                vec![a, b, c]
            }
        })
        .filter(|t| area(t).abs() > 1e-6)
        .collect();
    to_f32(merge(tris))
}

/// The eraser's path as polygons. `discs` are the stamps in path order;
/// each pair of neighbours is joined into a capsule (both round ends and
/// the straight sides between), so a drag leaves a clean edge rather than
/// a row of scallops.
pub fn eraser_shape(discs: &[Disc]) -> Vec<Vec<[f32; 2]>> {
    let ok: Vec<Disc> = discs
        .iter()
        .copied()
        .filter(|(c, r)| c[0].is_finite() && c[1].is_finite() && r.is_finite() && *r > 0.0)
        .collect();
    let circle = |c: [f32; 2], r: f32, from: f32, sweep: f32, n: usize| -> Vec<[f32; 2]> {
        (0..=n)
            .map(|i| {
                let a = from + sweep * i as f32 / n as f32;
                [c[0] + r * a.cos(), c[1] + r * a.sin()]
            })
            .collect()
    };
    match ok.as_slice() {
        [] => Vec::new(),
        [(c, r)] => {
            let mut d = circle(*c, *r, 0.0, std::f32::consts::TAU, DISC_SIDES);
            d.pop();
            vec![d]
        }
        _ => ok
            .windows(2)
            .map(|w| {
                let ((a, ra), (b, rb)) = (w[0], w[1]);
                let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
                if dx.hypot(dy) < 1e-4 {
                    let mut d = circle(a, ra.max(rb), 0.0, std::f32::consts::TAU, DISC_SIDES);
                    d.pop();
                    return d;
                }
                // Counter-clockwise: the far half of b, then the back half of a.
                let heading = dy.atan2(dx);
                let half = DISC_SIDES / 2;
                let pi = std::f32::consts::PI;
                let mut poly = circle(b, rb, heading - pi / 2.0, pi, half);
                poly.extend(circle(a, ra, heading + pi / 2.0, pi, half));
                poly
            })
            .collect(),
    }
}

/// `shape` minus the eraser. `None` when the eraser misses it; otherwise
/// what's left as separate shapes (empty if it's all gone).
pub fn erase(shape: &Shape, eraser: &[Vec<[f32; 2]>]) -> Option<Vec<Shape>> {
    let (lo, hi) = bbox(shape)?;
    let (elo, ehi) = bbox(eraser)?;
    if elo[0] > hi[0] || elo[1] > hi[1] || ehi[0] < lo[0] || ehi[1] < lo[1] {
        return None;
    }
    let before = shape_area(shape);
    let clip: Vec<Vec<[f32; 2]>> = eraser.to_vec();
    let left = to_f32(shape.overlay(&clip, OverlayRule::Difference, FillRule::NonZero));
    let after: f32 = left.iter().map(shape_area).sum();
    // The eraser crossed the box but not the shape itself.
    if (before - after).abs() <= before * 1e-4 + 1e-4 {
        return None;
    }
    Some(left)
}

/// Split outlines along a cutter boundary, returning the inside and outside.
pub fn split(shapes: &[Shape], clip: &Shape) -> Option<(Vec<Shape>, Vec<Shape>)> {
    let (lo, hi) = bbox(clip)?;
    let mut inside = Vec::new();
    let mut outside = Vec::new();
    for shape in shapes {
        let Some((a, b)) = bbox(shape) else { continue };
        if lo[0] > b[0] || lo[1] > b[1] || hi[0] < a[0] || hi[1] < a[1] {
            outside.push(shape.clone());
            continue;
        }
        let parts = to_f32(shape.overlay(clip, OverlayRule::Intersect, FillRule::NonZero));
        if parts.is_empty() {
            outside.push(shape.clone());
        } else {
            inside.extend(parts);
            outside.extend(to_f32(shape.overlay(
                clip,
                OverlayRule::Difference,
                FillRule::NonZero,
            )));
        }
    }
    (!inside.is_empty()).then_some((inside, outside))
}

/// Is (`x`, `y`) inside the shape, or within `r` of its edge?
pub fn hits_shape(shape: &Shape, x: f32, y: f32, r: f32) -> bool {
    let mut inside = false;
    for c in shape {
        let n = c.len();
        for i in 0..n {
            let (a, b) = (c[i], c[(i + n - 1) % n]);
            if (a[1] > y) != (b[1] > y) && x < (b[0] - a[0]) * (y - a[1]) / (b[1] - a[1]) + a[0] {
                inside = !inside;
            }
        }
    }
    if inside {
        return true;
    }
    shape.iter().any(|c| {
        let n = c.len();
        (0..n).any(|i| {
            let (a, b) = (c[i], c[(i + 1) % n]);
            let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
            let len2 = dx * dx + dy * dy;
            let t = if len2 > 0.0 {
                (((x - a[0]) * dx + (y - a[1]) * dy) / len2).clamp(0.0, 1.0)
            } else {
                0.0
            };
            (a[0] + dx * t - x).hypot(a[1] + dy * t - y) <= r
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use symphony_core::id::Id;
    use symphony_core::stroke::{Point, StrokeKind};

    fn bar() -> Stroke {
        Stroke {
            id: Id(1),
            kind: StrokeKind::Brush,
            art: Default::default(),
            swatch: Id(0),
            min_width: 10.0,
            max_width: 10.0,
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

    /// A pressure-varying stroke through `pts`.
    fn line(pts: &[(f32, f32)]) -> Stroke {
        let mut s = bar();
        s.min_width = 4.0;
        s.max_width = 30.0;
        s.points = pts
            .iter()
            .enumerate()
            .map(|(i, &(x, y))| Point {
                x,
                y,
                pressure: 0.3 + 0.7 * ((i as f32 * 0.7).sin() * 0.5 + 0.5),
                width_offset: 0.0,
                pen: Default::default(),
            })
            .collect();
        s
    }

    /// The coarser, fast outline covers what the finely sampled one does,
    /// including strokes that loop over themselves and turn sharply.
    #[test]
    fn the_fast_outline_matches_a_fine_one() {
        let wave: Vec<(f32, f32)> = (0..60)
            .map(|k| (k as f32 * 4.0, (k as f32 * 0.3).sin() * 20.0))
            .collect();
        let lp: Vec<(f32, f32)> = (0..80)
            .map(|k| {
                let a = k as f32 / 80.0 * 9.0;
                (a * 12.0 + a.cos() * 40.0, a.sin() * 40.0)
            })
            .collect();
        let zig: Vec<(f32, f32)> = (0..20)
            .map(|k| (k as f32 * 6.0, if k % 2 == 0 { 0.0 } else { 40.0 }))
            .collect();
        let back = vec![(0.0, 0.0), (100.0, 0.0), (2.0, 3.0)];
        for (name, pts) in [
            ("wave", wave),
            ("loops", lp),
            ("zigzag", zig),
            ("doubling back", back),
        ] {
            let s = line(&pts);
            let fast: f32 = outline(&s).iter().map(shape_area).sum();
            let slow: f32 = triangles_outline(&s, 0.05).iter().map(shape_area).sum();
            assert!(
                (fast - slow).abs() < slow * 0.01,
                "{name}: fast {fast} vs fine {slow}"
            );
        }
    }

    #[test]
    fn pieces_that_never_touch_still_merge() {
        // Far apart squares: grouping can't shrink them (this once
        // recursed forever).
        let squares: Vec<Vec<[f32; 2]>> = (0..MERGE_GROUP * 5)
            .map(|i| {
                let x = i as f32 * 10.0;
                vec![[x, 0.0], [x + 2.0, 0.0], [x + 2.0, 2.0], [x, 2.0]]
            })
            .collect();
        assert_eq!(merge(squares).len(), MERGE_GROUP * 5);
    }

    #[test]
    fn outline_matches_the_drawn_stroke() {
        let o = outline(&bar());
        assert_eq!(o.len(), 1);
        // 100 x 10 body plus two round caps of radius 5.
        let want = 100.0 * 10.0 + std::f32::consts::PI * 25.0;
        assert!(
            (shape_area(&o[0]) - want).abs() < want * 0.02,
            "{}",
            shape_area(&o[0])
        );
    }

    #[test]
    fn erasing_the_middle_splits_with_round_bites() {
        let o = outline(&bar()).remove(0);
        let left = erase(&o, &eraser_shape(&[([50.0, 0.0], 8.0)])).unwrap();
        assert_eq!(left.len(), 2, "two pieces");
        // The cut follows the eraser's circle: the edge point nearest the
        // centre is about 8 away, on the curve, not a flat end at x=42.
        for piece in &left {
            let nearest = piece[0]
                .iter()
                .map(|p| (p[0] - 50.0).hypot(p[1]))
                .fold(f32::MAX, f32::min);
            assert!((nearest - 8.0).abs() < 0.3, "{nearest}");
            assert!(!hits_shape(piece, 50.0, 0.0, 0.0));
        }
    }

    #[test]
    fn shaving_an_edge_keeps_one_thinner_piece() {
        let o = outline(&bar()).remove(0);
        let left = erase(&o, &eraser_shape(&[([50.0, 9.0], 6.0)])).unwrap();
        assert_eq!(left.len(), 1, "a bite, not a cut");
        assert!(hits_shape(&left[0], 50.0, -3.0, 0.0));
        assert!(!hits_shape(&left[0], 50.0, 4.0, 0.0));
    }

    #[test]
    fn a_dragged_eraser_leaves_a_straight_clean_edge() {
        // Dragging straight down through the bar: the cut edge is the
        // capsule's straight side at x = 50 - 6, with no scallops.
        let o = outline(&bar()).remove(0);
        let path: Vec<Disc> = (0..=10)
            .map(|i| ([50.0, -20.0 + i as f32 * 4.0], 6.0))
            .collect();
        let left = erase(&o, &eraser_shape(&path)).unwrap();
        assert_eq!(left.len(), 2);
        let edge_x: Vec<f32> = left[0]
            .iter()
            .chain(&left[1])
            .flatten()
            .filter(|p| p[1].abs() <= 5.01 && (p[0] - 50.0).abs() < 10.0)
            .map(|p| (p[0] - 50.0).abs())
            .collect();
        assert!(!edge_x.is_empty());
        assert!(edge_x.iter().all(|d| (d - 6.0).abs() < 0.05), "{edge_x:?}");
    }

    #[test]
    fn misses_and_whole_erases() {
        let o = outline(&bar()).remove(0);
        assert!(erase(&o, &eraser_shape(&[([50.0, 40.0], 5.0)])).is_none());
        // Box overlaps but the shape doesn't.
        assert!(erase(&o, &eraser_shape(&[([-12.0, 12.0], 5.0)])).is_none());
        let all: Vec<Disc> = (0..=20).map(|i| ([i as f32 * 5.0, 0.0], 12.0)).collect();
        assert!(erase(&o, &eraser_shape(&all)).unwrap().is_empty());
    }
}
