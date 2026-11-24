//! Finding paint zones (Harmony's Paint tool): the closed area around a
//! point, bounded by the drawing's strokes, including invisible ones.
//!
//! A classic flood fill, no AI (DESIGN D16.5): the strokes are drawn onto a
//! grid, the zone is flooded from the click, and its edges are traced back
//! into vector outlines. If the flood reaches the edge of the grid, the area
//! isn't closed and nothing is painted, as in Harmony.

use std::collections::HashMap;

use symphony_core::math::Affine;
use symphony_core::stroke::Stroke;

use crate::Mesh;

/// Why a zone couldn't be painted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ZoneError {
    /// The area around the point isn't closed by lines.
    Open,
    /// The point is on a line, not inside an area.
    OnLine,
}

/// Grids bigger than this per side are coarsened, to keep painting instant.
const MAX_GRID: f32 = 2048.0;
/// How many cells the zone grows under the surrounding lines, so no paper
/// shows between paint and ink.
const TUCK: usize = 2;

/// The closed zone around (`x`, `y`) in drawing space. `cell` is the grid
/// resolution in drawing units (use about one screen pixel). Returns the
/// zone's outlines: the largest first, holes after (even-odd).
pub fn find_zone(
    strokes: &[&Stroke],
    x: f32,
    y: f32,
    cell: f32,
) -> Result<Vec<Vec<[f32; 2]>>, ZoneError> {
    let Some((lo, hi)) = crate::bounds_of(strokes.iter().copied()) else {
        return Err(ZoneError::Open);
    };
    if x <= lo[0] || y <= lo[1] || x >= hi[0] || y >= hi[1] {
        return Err(ZoneError::Open);
    }
    let PaintGrid {
        ink,
        width: w,
        height: h,
        origin,
        cell,
    } = paint_grid(strokes, lo, hi, cell);
    let seed = (
        ((x - origin[0]) / cell) as usize,
        ((y - origin[1]) / cell) as usize,
    );
    if ink[seed.1 * w + seed.0] {
        return Err(ZoneError::OnLine);
    }

    // Flood fill, 4-connected.
    let mut zone = vec![false; w * h];
    let mut stack = vec![seed];
    zone[seed.1 * w + seed.0] = true;
    while let Some((cx, cy)) = stack.pop() {
        if cx == 0 || cy == 0 || cx == w - 1 || cy == h - 1 {
            return Err(ZoneError::Open);
        }
        for (nx, ny) in [(cx - 1, cy), (cx + 1, cy), (cx, cy - 1), (cx, cy + 1)] {
            let i = ny * w + nx;
            if !zone[i] && !ink[i] {
                zone[i] = true;
                stack.push((nx, ny));
            }
        }
    }

    Ok(trace_paint_zone(zone, &ink, w, h, origin, cell))
}

struct PaintGrid {
    ink: Vec<bool>,
    width: usize,
    height: usize,
    origin: [f32; 2],
    cell: f32,
}

fn paint_grid(strokes: &[&Stroke], lo: [f32; 2], hi: [f32; 2], cell: f32) -> PaintGrid {
    let span = (hi[0] - lo[0]).max(hi[1] - lo[1]);
    let cell = cell.max(span / MAX_GRID).max(1e-4);
    // A one-cell margin of empty space all round: reaching it means "open".
    let origin = [lo[0] - cell, lo[1] - cell];
    let w = ((hi[0] - lo[0]) / cell).ceil() as usize + 3;
    let h = ((hi[1] - lo[1]) / cell).ceil() as usize + 3;

    let mut ink = vec![false; w * h];
    for st in strokes {
        rasterize(st, &mut ink, w, h, origin, cell);
    }
    PaintGrid {
        ink,
        width: w,
        height: h,
        origin,
        cell,
    }
}

pub struct PaintZone {
    pub sample: [f32; 2],
    pub contours: Vec<Vec<[f32; 2]>>,
}

/// Every region enclosed by these strokes, with an interior sample for
/// matching existing paint. Rasterize once even when the selection contains
/// multiple disconnected or concave regions.
pub fn enclosed_zones(strokes: &[&Stroke], cell: f32) -> Vec<PaintZone> {
    let Some((lo, hi)) = crate::bounds_of(strokes.iter().copied()) else {
        return Vec::new();
    };
    let PaintGrid {
        ink,
        width: w,
        height: h,
        origin,
        cell,
    } = paint_grid(strokes, lo, hi, cell);
    let mut visited = ink.clone();
    let mut zones = Vec::new();
    let mut region = Vec::new();
    for seed in 0..visited.len() {
        if visited[seed] {
            continue;
        }
        let (x, y) = (seed % w, seed / w);
        let (mut left, mut top, mut right, mut bottom) = (x, y, x, y);
        let mut open = false;
        visited[seed] = true;
        region.clear();
        region.push((x, y));
        let mut i = 0;
        while let Some(&(x, y)) = region.get(i) {
            i += 1;
            left = left.min(x);
            right = right.max(x);
            top = top.min(y);
            bottom = bottom.max(y);
            open |= x == 0 || y == 0 || x == w - 1 || y == h - 1;
            for (nx, ny) in [
                (x.saturating_sub(1), y),
                ((x + 1).min(w - 1), y),
                (x, y.saturating_sub(1)),
                (x, (y + 1).min(h - 1)),
            ] {
                let index = ny * w + nx;
                if !visited[index] {
                    visited[index] = true;
                    region.push((nx, ny));
                }
            }
        }
        if open {
            continue;
        }
        left = left.saturating_sub(TUCK + 1);
        top = top.saturating_sub(TUCK + 1);
        right = (right + TUCK + 1).min(w - 1);
        bottom = (bottom + TUCK + 1).min(h - 1);
        let width = right - left + 1;
        let height = bottom - top + 1;
        let local_ink: Vec<_> = (top..=bottom)
            .flat_map(|row| ink[row * w + left..=row * w + right].iter().copied())
            .collect();
        let mut mask = vec![false; width * height];
        for &(x, y) in &region {
            mask[(y - top) * width + x - left] = true;
        }
        let local_origin = [
            origin[0] + left as f32 * cell,
            origin[1] + top as f32 * cell,
        ];
        let sample = [
            origin[0] + (x as f32 + 0.5) * cell,
            origin[1] + (y as f32 + 0.5) * cell,
        ];
        let contours = trace_paint_zone(mask, &local_ink, width, height, local_origin, cell);
        zones.push(PaintZone { sample, contours });
    }
    zones
}

fn trace_paint_zone(
    mut zone: Vec<bool>,
    ink: &[bool],
    w: usize,
    h: usize,
    origin: [f32; 2],
    cell: f32,
) -> Vec<Vec<[f32; 2]>> {
    // Tuck the zone under the surrounding ink.
    for _ in 0..TUCK {
        let prev = zone.clone();
        for cy in 1..h - 1 {
            for cx in 1..w - 1 {
                let i = cy * w + cx;
                if !prev[i] && ink[i] && (prev[i - 1] || prev[i + 1] || prev[i - w] || prev[i + w])
                {
                    zone[i] = true;
                }
            }
        }
    }

    let mut loops = trace(&zone, w, h);
    for l in loops.iter_mut() {
        // Reconstruct diagonals from the raster staircase before simplifying.
        // The half-cell displacement stays safely inside the ink tuck.
        let midpoints: Vec<_> = l
            .iter()
            .zip(l.iter().cycle().skip(1))
            .map(|(a, b)| ((a.0 + b.0) * 0.5, (a.1 + b.1) * 0.5))
            .collect();
        *l = simplify(&midpoints, 0.55);
    }
    loops.retain(|l| l.len() >= 3);
    loops.sort_by(|a, b| area(b).abs().total_cmp(&area(a).abs()));
    loops
        .into_iter()
        .map(|l| {
            l.into_iter()
                .map(|(gx, gy)| [origin[0] + gx * cell, origin[1] + gy * cell])
                .collect()
        })
        .collect()
}

/// Mark grid cells covered by the stroke (its width, or at least a cell).
fn rasterize(st: &Stroke, ink: &mut [bool], w: usize, h: usize, origin: [f32; 2], cell: f32) {
    if !st.shape.is_empty() {
        // Erased brush strokes: every cell whose centre is in the outline,
        // plus the outline's own edge so thin parts still close zones.
        let Some((lo, hi)) = crate::bounds(st) else {
            return;
        };
        let gx0 = (((lo[0] - origin[0]) / cell).floor().max(0.0) as usize).min(w);
        let gy0 = (((lo[1] - origin[1]) / cell).floor().max(0.0) as usize).min(h);
        let gx1 = (((hi[0] - origin[0]) / cell).ceil().max(0.0) as usize + 1).min(w);
        let gy1 = (((hi[1] - origin[1]) / cell).ceil().max(0.0) as usize + 1).min(h);
        for gy in gy0..gy1 {
            for gx in gx0..gx1 {
                let x = origin[0] + (gx as f32 + 0.5) * cell;
                let y = origin[1] + (gy as f32 + 0.5) * cell;
                if crate::shape::hits_shape(&st.shape, x, y, cell * 0.5) {
                    ink[gy * w + gx] = true;
                }
            }
        }
        return;
    }
    // Zones close along the same smooth curve that is drawn.
    let curved = Stroke {
        points: crate::curve::spline(&st.points, cell),
        ..st.clone()
    };
    let st = &curved;
    let to_grid = |px: f32, py: f32| ((px - origin[0]) / cell, (py - origin[1]) / cell);
    let radius = |p: f32| (st.width_at(p) * 0.5 / cell).max(0.75);
    let mut mark = |ax: f32, ay: f32, ar: f32, bx: f32, by: f32, br: f32| {
        let r = ar.max(br);
        let (x0, x1) = (
            (ax.min(bx) - r).floor().max(0.0) as usize,
            ((ax.max(bx) + r).ceil() as usize).min(w - 1),
        );
        let (y0, y1) = (
            (ay.min(by) - r).floor().max(0.0) as usize,
            ((ay.max(by) + r).ceil() as usize).min(h - 1),
        );
        let (dx, dy) = (bx - ax, by - ay);
        let len2 = dx * dx + dy * dy;
        for gy in y0..=y1 {
            for gx in x0..=x1 {
                let (px, py) = (gx as f32 + 0.5, gy as f32 + 0.5);
                let t = if len2 > 0.0 {
                    (((px - ax) * dx + (py - ay) * dy) / len2).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                let (qx, qy) = (ax + dx * t, ay + dy * t);
                if (px - qx).hypot(py - qy) <= ar + (br - ar) * t {
                    ink[gy * w + gx] = true;
                }
            }
        }
    };
    match st.points.as_slice() {
        [] => {}
        [p] => {
            let (gx, gy) = to_grid(p.x, p.y);
            let r = radius(p.pressure);
            mark(gx, gy, r, gx, gy, r);
        }
        pts => {
            for s in pts.windows(2) {
                let (ax, ay) = to_grid(s[0].x, s[0].y);
                let (bx, by) = to_grid(s[1].x, s[1].y);
                mark(ax, ay, radius(s[0].pressure), bx, by, radius(s[1].pressure));
            }
        }
    }
}

/// Trace the zone's boundary into closed loops of grid-corner points.
/// Each zone cell contributes the sides it shares with non-zone cells,
/// directed clockwise, and the sides are then chained into loops.
fn trace(zone: &[bool], w: usize, h: usize) -> Vec<Vec<(f32, f32)>> {
    let at = |x: isize, y: isize| {
        x >= 0
            && y >= 0
            && (x as usize) < w
            && (y as usize) < h
            && zone[y as usize * w + x as usize]
    };
    let mut next: HashMap<(isize, isize), Vec<(isize, isize)>> = HashMap::new();
    for y in 0..h as isize {
        for x in 0..w as isize {
            if !at(x, y) {
                continue;
            }
            let mut edge =
                |a: (isize, isize), b: (isize, isize)| next.entry(a).or_default().push(b);
            if !at(x, y - 1) {
                edge((x, y), (x + 1, y));
            }
            if !at(x + 1, y) {
                edge((x + 1, y), (x + 1, y + 1));
            }
            if !at(x, y + 1) {
                edge((x + 1, y + 1), (x, y + 1));
            }
            if !at(x - 1, y) {
                edge((x, y + 1), (x, y));
            }
        }
    }
    let mut loops = Vec::new();
    while let Some(&start) = next.keys().next() {
        let mut lp = vec![(start.0 as f32, start.1 as f32)];
        let mut cur = start;
        while let Some(outs) = next.get_mut(&cur) {
            let Some(n) = outs.pop() else { break };
            if outs.is_empty() {
                next.remove(&cur);
            }
            if n == start {
                break;
            }
            lp.push((n.0 as f32, n.1 as f32));
            cur = n;
        }
        loops.push(lp);
    }
    loops
}

/// Ramer-Douglas-Peucker on a closed loop: drop points closer than `tol`
/// to the line between their neighbours, so staircase edges become straight.
fn simplify(lp: &[(f32, f32)], tol: f32) -> Vec<(f32, f32)> {
    if lp.len() < 4 {
        return lp.to_vec();
    }
    // Split the loop at its two farthest-apart points and simplify each half.
    let far = (1..lp.len())
        .max_by(|&a, &b| dist2(lp[0], lp[a]).total_cmp(&dist2(lp[0], lp[b])))
        .unwrap_or(lp.len() / 2);
    let mut first: Vec<(f32, f32)> = lp[..=far].to_vec();
    let mut second: Vec<(f32, f32)> = lp[far..].to_vec();
    second.push(lp[0]);
    first = rdp(&first, tol);
    second = rdp(&second, tol);
    first.pop();
    second.pop();
    first.extend(second);
    first
}

fn rdp(pts: &[(f32, f32)], tol: f32) -> Vec<(f32, f32)> {
    if pts.len() < 3 {
        return pts.to_vec();
    }
    let (a, b) = (pts[0], pts[pts.len() - 1]);
    let (idx, d) = pts[1..pts.len() - 1]
        .iter()
        .enumerate()
        .map(|(i, &p)| (i + 1, seg_dist(p, a, b)))
        .max_by(|x, y| x.1.total_cmp(&y.1))
        .unwrap_or((0, 0.0));
    if d > tol {
        let mut left = rdp(&pts[..=idx], tol);
        let right = rdp(&pts[idx..], tol);
        left.pop();
        left.extend(right);
        left
    } else {
        vec![a, b]
    }
}

fn dist2(a: (f32, f32), b: (f32, f32)) -> f32 {
    (a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)
}

fn seg_dist(p: (f32, f32), a: (f32, f32), b: (f32, f32)) -> f32 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len2 = dx * dx + dy * dy;
    let t = if len2 > 0.0 {
        (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / len2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    dist2(p, (a.0 + dx * t, a.1 + dy * t)).sqrt()
}

fn area(lp: &[(f32, f32)]) -> f32 {
    let n = lp.len();
    (0..n)
        .map(|i| lp[i].0 * lp[(i + 1) % n].1 - lp[(i + 1) % n].0 * lp[i].1)
        .sum::<f32>()
        / 2.0
}

/// Triangles for a fill (outside plus holes), in `transform`'s output space.
pub fn fill_mesh(contours: &[Vec<[f32; 2]>], transform: Affine) -> Mesh {
    let mut verts: Vec<f32> = Vec::new();
    let mut holes = Vec::new();
    for (i, c) in contours.iter().enumerate() {
        if i > 0 {
            holes.push(verts.len() / 2);
        }
        for p in c {
            let (x, y) = transform.apply(p[0], p[1]);
            verts.push(x);
            verts.push(y);
        }
    }
    let indices = earcutr::earcut(&verts, &holes, 2).unwrap_or_default();
    Mesh {
        vertices: verts.as_chunks::<2>().0.to_vec(),
        indices: indices.into_iter().map(|i| i as u32).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use symphony_core::id::Id;
    use symphony_core::stroke::{ArtLayer, Fill, Point, StrokeKind};

    fn line(pts: &[(f32, f32)]) -> Stroke {
        Stroke {
            id: Id(1),
            kind: StrokeKind::Brush,
            art: ArtLayer::LineArt,
            swatch: Id(2),
            min_width: 2.0,
            max_width: 2.0,
            points: pts
                .iter()
                .map(|&(x, y)| Point {
                    x,
                    y,
                    pressure: 1.0,
                    pen: Default::default(),
                })
                .collect(),
            tips: Default::default(),
            texture: None,
            shape: Vec::new(),
        }
    }

    fn square(c: f32, r: f32) -> Stroke {
        line(&[
            (c - r, c - r),
            (c + r, c - r),
            (c + r, c + r),
            (c - r, c + r),
            (c - r, c - r),
        ])
    }

    fn as_fill(contours: Vec<Vec<[f32; 2]>>) -> Fill {
        Fill {
            linked: None,
            id: Id(9),
            swatch: Id(2),
            art: ArtLayer::LineArt,
            contours,
        }
    }

    #[test]
    fn finds_each_separate_closed_region_in_a_selection() {
        let left = square(-30.0, 8.0);
        let right = square(30.0, 8.0);
        let open = line(&[(-10.0, 1.0), (10.0, 1.0)]);
        let zones = enclosed_zones(&[&left, &right, &open], 1.0);
        assert_eq!(zones.len(), 2);
        let fills: Vec<_> = zones
            .into_iter()
            .map(|zone| {
                let fill = as_fill(zone.contours);
                assert!(fill.contains(zone.sample[0], zone.sample[1]));
                fill
            })
            .collect();
        assert!(fills.iter().any(|fill| fill.contains(-30.0, -30.0)));
        assert!(fills.iter().any(|fill| fill.contains(30.0, 30.0)));
    }

    #[test]
    fn fills_a_closed_square() {
        let sq = square(50.0, 40.0);
        let zone = find_zone(&[&sq], 50.0, 50.0, 1.0).unwrap();
        let f = as_fill(zone);
        assert!(f.contains(50.0, 50.0));
        assert!(f.contains(15.0, 50.0), "reaches close to the line");
        assert!(!f.contains(5.0, 50.0), "stops at the line");
    }

    #[test]
    fn open_shape_is_refused() {
        let open = line(&[(10.0, 10.0), (90.0, 10.0), (90.0, 90.0), (10.0, 90.0)]);
        assert_eq!(find_zone(&[&open], 50.0, 50.0, 1.0), Err(ZoneError::Open));
    }

    #[test]
    fn invisible_stroke_closes_the_gap() {
        let open = line(&[(10.0, 10.0), (90.0, 10.0), (90.0, 90.0), (10.0, 90.0)]);
        let mut gap = line(&[(10.0, 90.0), (10.0, 10.0)]);
        gap.kind = StrokeKind::Invisible;
        gap.min_width = 0.0;
        gap.max_width = 0.0;
        assert!(find_zone(&[&open, &gap], 50.0, 50.0, 1.0).is_ok());
    }

    #[test]
    fn ring_keeps_its_hole() {
        let (outer, inner) = (square(50.0, 40.0), square(50.0, 15.0));
        let zone = find_zone(&[&outer, &inner], 20.0, 50.0, 1.0).unwrap();
        assert_eq!(zone.len(), 2, "outside plus one hole");
        let f = as_fill(zone.clone());
        assert!(f.contains(20.0, 50.0));
        assert!(!f.contains(50.0, 50.0), "the middle stays unpainted");
        assert!(!fill_mesh(&zone, Affine::IDENTITY).indices.is_empty());
    }

    #[test]
    fn clicking_a_line_is_not_a_zone() {
        let sq = square(50.0, 40.0);
        assert_eq!(find_zone(&[&sq], 10.0, 50.0, 1.0), Err(ZoneError::OnLine));
    }
}
