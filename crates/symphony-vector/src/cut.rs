//! Erasing part of a line (Harmony's Eraser on vector art): the eraser's
//! path cuts the stroke's centreline, and what's left becomes one or more
//! strokes. Cut ends get the eraser's tip style.

use symphony_core::stroke::{Point, Stroke, Tip};

/// One eraser stamp: centre and radius, in the stroke's coordinates.
pub type Disc = ([f32; 2], f32);

fn covered(p: [f32; 2], eraser: &[Disc]) -> bool {
    eraser
        .iter()
        .any(|(c, r)| (p[0] - c[0]).hypot(p[1] - c[1]) <= *r)
}

fn lerp(a: &Point, b: &Point, t: f32) -> Point {
    Point {
        x: a.x + (b.x - a.x) * t,
        y: a.y + (b.y - a.y) * t,
        pressure: a.pressure + (b.pressure - a.pressure) * t,
        width_offset: a.width_offset + (b.width_offset - a.width_offset) * t,
        pen: symphony_core::stroke::Pen::lerp(a.pen, b.pen, t),
    }
}

/// Where along a→b the covered state changes, found by bisection.
fn edge(a: &Point, b: &Point, eraser: &[Disc]) -> Point {
    let start = covered([a.x, a.y], eraser);
    let (mut lo, mut hi) = (0.0f32, 1.0f32);
    for _ in 0..12 {
        let mid = (lo + hi) * 0.5;
        let p = lerp(a, b, mid);
        if covered([p.x, p.y], eraser) == start {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    lerp(a, b, (lo + hi) * 0.5)
}

/// Cut `stroke` with the eraser. `None` if the eraser misses it; otherwise
/// the pieces left (empty if it's all erased), in order along the line.
/// Pieces keep the stroke's settings; the caller gives them their IDs.
pub fn cut(stroke: &Stroke, eraser: &[Disc], cut_tip: Tip) -> Option<Vec<Stroke>> {
    if eraser.is_empty() || stroke.points.is_empty() {
        return None;
    }
    // Check finely enough that a small eraser can't slip between points.
    let step = eraser
        .iter()
        .map(|d| d.1)
        .fold(f32::MAX, f32::min)
        .max(0.05)
        * 0.5;
    let mut dense: Vec<Point> = Vec::with_capacity(stroke.points.len());
    for w in stroke.points.windows(2) {
        let (a, b) = (&w[0], &w[1]);
        let len = (b.x - a.x).hypot(b.y - a.y);
        let n = ((len / step).ceil() as usize).clamp(1, 10_000);
        for i in 0..n {
            dense.push(lerp(a, b, i as f32 / n as f32));
        }
    }
    dense.push(*stroke.points.last()?);
    let hit: Vec<bool> = dense.iter().map(|p| covered([p.x, p.y], eraser)).collect();
    if !hit.contains(&true) {
        return None;
    }
    // Runs of kept points, closed off exactly at the eraser's edge.
    let mut pieces: Vec<(Vec<Point>, bool, bool)> = Vec::new();
    let mut run: Vec<Point> = Vec::new();
    let mut run_cut_start = false;
    for i in 0..dense.len() {
        if !hit[i] {
            if run.is_empty() && i > 0 {
                run.push(edge(&dense[i - 1], &dense[i], eraser));
                run_cut_start = true;
            }
            run.push(dense[i]);
        } else if !run.is_empty() {
            run.push(edge(&dense[i - 1], &dense[i], eraser));
            pieces.push((std::mem::take(&mut run), run_cut_start, true));
            run_cut_start = false;
        }
    }
    if !run.is_empty() {
        pieces.push((run, run_cut_start, false));
    }
    let original = stroke.points.len();
    Some(
        pieces
            .into_iter()
            .filter(|(pts, _, _)| {
                // Drop crumbs shorter than the line is wide.
                let len: f32 = pts
                    .windows(2)
                    .map(|w| (w[1].x - w[0].x).hypot(w[1].y - w[0].y))
                    .sum();
                pts.len() >= 2 && len >= stroke.max_width.min(1.0)
            })
            .map(|(pts, cut_start, cut_end)| {
                let mut piece = stroke.clone();
                // Keep the shape light: drop the extra checking points that
                // lie between original points on straight segments.
                piece.points = simplify(pts, original);
                if cut_start {
                    piece.tips.start = cut_tip;
                }
                if cut_end {
                    piece.tips.end = cut_tip;
                }
                piece
            })
            .collect(),
    )
}

/// Split a solid vector stroke's centreline at each cutter crossing.
pub fn split_path(stroke: &Stroke, clip: &crate::shape::Shape) -> Option<Vec<(bool, Stroke)>> {
    if !stroke.shape.is_empty() || stroke.texture.is_some() || stroke.points.len() < 2 {
        return None;
    }
    let cross = |a: [f32; 2], b: [f32; 2]| a[0] * b[1] - a[1] * b[0];
    let edges: Vec<_> = clip
        .iter()
        .flat_map(|ring| {
            ring.iter()
                .copied()
                .zip(ring.iter().copied().cycle().skip(1))
        })
        .collect();
    let points = crate::curve::spline(&stroke.points, 0.5);
    let mut runs: Vec<(bool, Vec<Point>)> = Vec::new();
    for pair in points.windows(2) {
        let (a, b) = (&pair[0], &pair[1]);
        let line = [b.x - a.x, b.y - a.y];
        let mut cuts = vec![0.0_f32, 1.0];
        for &(p, q) in &edges {
            let edge = [q[0] - p[0], q[1] - p[1]];
            let d = cross(line, edge);
            if d.abs() < 1e-7 {
                continue;
            }
            let offset = [p[0] - a.x, p[1] - a.y];
            let t = cross(offset, edge) / d;
            let u = cross(offset, line) / d;
            if t > 0.0 && t < 1.0 && (0.0..=1.0).contains(&u) {
                cuts.push(t);
            }
        }
        cuts.sort_by(f32::total_cmp);
        cuts.dedup_by(|a, b| (*a - *b).abs() < 1e-6);
        for span in cuts.windows(2) {
            let mid = lerp(a, b, (span[0] + span[1]) * 0.5);
            let inside = crate::shape::hits_shape(clip, mid.x, mid.y, 0.0);
            let end = lerp(a, b, span[1]);
            if let Some((_, run)) = runs.last_mut().filter(|(side, _)| *side == inside) {
                run.push(end);
            } else {
                runs.push((inside, vec![lerp(a, b, span[0]), end]));
            }
        }
    }
    if runs.len() < 2 {
        return None;
    }
    let closed = points
        .first()
        .zip(points.last())
        .is_some_and(|(a, b)| (a.x - b.x).hypot(a.y - b.y) < 1e-5);
    if closed && runs.first().map(|run| run.0) == runs.last().map(|run| run.0) {
        let (_, mut tail) = runs.pop()?;
        tail.extend(runs[0].1.iter().copied().skip(1));
        runs[0].1 = tail;
    }
    let count = runs.len();
    let span = stroke.max_width - stroke.min_width;
    Some(
        runs.into_iter()
            .enumerate()
            .map(|(index, (inside, points))| {
                let mut piece = stroke.clone();
                piece.points = crate::curve::simplify(&points, 0.03, span);
                if closed || index > 0 {
                    piece.tips.start = Tip::Flat;
                }
                if closed || index + 1 < count {
                    piece.tips.end = Tip::Flat;
                }
                (inside, piece)
            })
            .collect(),
    )
}

/// Remove points that sit on the straight line between their neighbours
/// (the densified ones), keeping the line's shape and pressure.
fn simplify(pts: Vec<Point>, original: usize) -> Vec<Point> {
    if pts.len() <= 2 || pts.len() <= original {
        return pts;
    }
    let mut out = vec![pts[0]];
    for i in 1..pts.len() - 1 {
        let (a, b, c) = (out[out.len() - 1], pts[i], pts[i + 1]);
        let cross = (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x);
        let len = (c.x - a.x).hypot(c.y - a.y).max(1e-6);
        let pressure_bend = (b.pressure - (a.pressure + c.pressure) * 0.5).abs();
        if (cross / len).abs() > 0.01 || pressure_bend > 0.01 {
            out.push(b);
        }
    }
    out.push(pts[pts.len() - 1]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use symphony_core::id::Id;
    use symphony_core::stroke::StrokeKind;

    fn line() -> Stroke {
        Stroke {
            id: Id(1),
            kind: StrokeKind::Pencil,
            art: Default::default(),
            swatch: Id(0),
            min_width: 2.0,
            max_width: 2.0,
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

    #[test]
    fn erasing_the_middle_leaves_two_pieces_with_cut_tips() {
        let pieces = cut(&line(), &[([50.0, 0.0], 10.0)], Tip::Flat).unwrap();
        assert_eq!(pieces.len(), 2);
        let (a, b) = (&pieces[0], &pieces[1]);
        assert!((a.points.last().unwrap().x - 40.0).abs() < 0.05);
        assert!((b.points[0].x - 60.0).abs() < 0.05);
        assert_eq!((a.tips.start, a.tips.end), (Tip::Round, Tip::Flat));
        assert_eq!((b.tips.start, b.tips.end), (Tip::Flat, Tip::Round));
        // Straight pieces stay two points, not hundreds.
        assert!(a.points.len() <= 3 && b.points.len() <= 3);
    }

    #[test]
    fn missing_erasing_all_and_trimming_an_end() {
        assert!(cut(&line(), &[([50.0, 30.0], 10.0)], Tip::Round).is_none());
        let all: Vec<Disc> = (0..=10).map(|i| ([i as f32 * 10.0, 0.0], 8.0)).collect();
        assert!(cut(&line(), &all, Tip::Round).unwrap().is_empty());
        let end = cut(&line(), &[([100.0, 0.0], 20.0)], Tip::Round).unwrap();
        assert_eq!(end.len(), 1);
        assert!((end[0].points.last().unwrap().x - 80.0).abs() < 0.05);
    }
}
