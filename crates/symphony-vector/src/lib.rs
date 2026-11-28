//! Stroke geometry (ENGINEERING E1.1): turning pen input into clean
//! strokes, and strokes into triangles.
//!
//! - [`pressure_curve`]: the user's pressure response (E4.1).
//! - [`Smoother`]: stroke stabilising as points arrive.
//! - [`tessellate`]: a variable-width stroke becomes a triangle mesh with
//!   round caps, for any renderer.
//! - [`hits`]: does a circle touch a stroke (eraser, selection)?

pub mod bend;
pub mod curve;
pub mod cut;
pub mod fill;
pub mod puppet_mesh;
mod recent;
pub mod shape;
pub mod stamp;
pub mod stamped;
pub mod trace;

use symphony_core::math::Affine;
use symphony_core::stroke::{Point, Stroke, Tip};

/// Map raw pen pressure through a gamma curve. `gamma` < 1 makes light
/// strokes heavier, > 1 makes them lighter. 1.0 is linear.
pub fn pressure_curve(raw: f32, gamma: f32) -> f32 {
    raw.clamp(0.0, 1.0).powf(gamma.clamp(0.2, 5.0))
}

/// Smooths points as they arrive. `amount` 0 = raw, 1 = very smooth.
/// Smoothing trails the pen slightly, so `finish` adds the last raw point
/// and the stroke still ends where the pen lifted.
#[derive(Clone, Debug)]
pub struct Smoother {
    amount: f32,
    last: Option<Point>,
    last_raw: Option<Point>,
    leash: f32,
    stabilized: Option<Point>,
    preserve_corners: bool,
    direction: Option<[f32; 2]>,
}

impl Smoother {
    pub fn new(amount: f32) -> Self {
        Smoother {
            amount: amount.clamp(0.0, 0.95),
            last: None,
            last_raw: None,
            leash: 0.0,
            stabilized: None,
            preserve_corners: false,
            direction: None,
        }
    }

    /// Let the pen lead the drawn point by up to this distance.
    pub fn with_stabilization(mut self, radius: f32) -> Self {
        self.leash = radius.max(0.0);
        self
    }

    pub fn with_corner_preservation(mut self, on: bool) -> Self {
        self.preserve_corners = on;
        self
    }

    /// Feed one raw point. Returns the smoothed point to append, or `None`
    /// if it's too close to the previous one to matter.
    pub fn push(&mut self, raw: Point, min_dist: f32) -> Option<Point> {
        let mut corner = false;
        if self.preserve_corners {
            if let Some(previous) = self.last_raw {
                let direction = [raw.x - previous.x, raw.y - previous.y];
                let length = direction[0].hypot(direction[1]);
                if length > min_dist.max(0.001) {
                    let unit = direction.map(|axis| axis / length);
                    corner = self
                        .direction
                        .is_some_and(|d| d[0] * unit[0] + d[1] * unit[1] < 0.65);
                    self.direction = Some(unit);
                }
            }
        }
        self.last_raw = Some(raw);
        let raw = if self.leash > 0.0 {
            let mut point = raw;
            if let Some(previous) = self.stabilized {
                let distance = (raw.x - previous.x).hypot(raw.y - previous.y);
                let travel = if distance > self.leash {
                    1.0 - self.leash / distance
                } else {
                    0.0
                };
                point.x = previous.x + (raw.x - previous.x) * travel;
                point.y = previous.y + (raw.y - previous.y) * travel;
            }
            self.stabilized = Some(point);
            point
        } else {
            raw
        };
        let Some(prev) = self.last else {
            self.last = Some(raw);
            return Some(raw);
        };
        let a = if corner {
            (1.0 - self.amount).max(0.8)
        } else {
            1.0 - self.amount
        };
        let p = Point {
            x: prev.x + (raw.x - prev.x) * a,
            y: prev.y + (raw.y - prev.y) * a,
            // Pressure is smoothed more lightly, so taper still reacts quickly.
            pressure: prev.pressure + (raw.pressure - prev.pressure) * a.max(0.5),
            pen: symphony_core::stroke::Pen::lerp(prev.pen, raw.pen, a.max(0.5)),
        };
        if (p.x - prev.x).hypot(p.y - prev.y) < min_dist {
            return None;
        }
        self.last = Some(p);
        Some(p)
    }

    /// The final raw point, if smoothing left it behind.
    pub fn finish(&mut self) -> Option<Point> {
        let (last, raw) = (self.last?, self.last_raw?);
        ((last.x - raw.x).hypot(last.y - raw.y) > 0.01).then_some(raw)
    }
}

/// Triangles for one stroke, in the coordinates of `transform`'s output.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Mesh {
    pub vertices: Vec<[f32; 2]>,
    pub indices: Vec<u32>,
}

/// Uniform scale of a transform, for widths.
pub fn scale_of(m: Affine) -> f32 {
    (m.a * m.d - m.b * m.c).abs().sqrt()
}

const CAP_SEGMENTS: usize = 8;

/// Build the stroke outline as triangles: a strip along the centreline with
/// width from pressure, and round caps at both ends. Points closer than
/// `min_step` (output units) are skipped to keep the strip clean.
pub fn tessellate(stroke: &Stroke, transform: Affine, min_step: f32) -> Mesh {
    // An erased brush stroke is its outline.
    if !stroke.shape.is_empty() {
        return fill::fill_mesh(&stroke.shape, transform);
    }
    let k = scale_of(transform);
    let mut pts: Vec<([f32; 2], f32)> = Vec::with_capacity(stroke.points.len());
    // A smooth curve through the stored points, as fine as this zoom needs.
    let curve = curve::spline(&stroke.points, (min_step * 2.0 / k.max(1e-6)).max(1e-3));
    for p in &curve {
        let (x, y) = transform.apply(p.x, p.y);
        let w = stroke.width_at(p.pressure) * k * 0.5;
        if let Some((last, lw)) = pts.last_mut() {
            if (x - last[0]).hypot(y - last[1]) < min_step {
                *lw = lw.max(w);
                continue;
            }
        }
        pts.push(([x, y], w));
    }
    let mut mesh = Mesh::default();
    match pts.as_slice() {
        [] => {}
        [(c, r)] => disc(&mut mesh, *c, *r),
        _ => {
            strip(&mut mesh, &pts);
            let (first, last) = (pts[0], pts[pts.len() - 1]);
            if stroke.tips.start == Tip::Round {
                cap(&mut mesh, first.0, sub(first.0, pts[1].0), first.1);
            }
            if stroke.tips.end == Tip::Round {
                cap(&mut mesh, last.0, sub(last.0, pts[pts.len() - 2].0), last.1);
            }
        }
    }
    mesh
}

fn sub(a: [f32; 2], b: [f32; 2]) -> [f32; 2] {
    [a[0] - b[0], a[1] - b[1]]
}

fn normalise(v: [f32; 2]) -> [f32; 2] {
    let l = v[0].hypot(v[1]);
    if l < 1e-6 {
        [1.0, 0.0]
    } else {
        [v[0] / l, v[1] / l]
    }
}

fn strip(mesh: &mut Mesh, pts: &[([f32; 2], f32)]) {
    let n = pts.len();
    for i in 0..n {
        let prev = pts[i.saturating_sub(1)].0;
        let next = pts[(i + 1).min(n - 1)].0;
        let t = normalise(sub(next, prev));
        let nrm = [-t[1], t[0]];
        let (c, r) = pts[i];
        mesh.vertices.push([c[0] + nrm[0] * r, c[1] + nrm[1] * r]);
        mesh.vertices.push([c[0] - nrm[0] * r, c[1] - nrm[1] * r]);
    }
    for i in 0..n as u32 - 1 {
        let (a, b, c, d) = (2 * i, 2 * i + 1, 2 * i + 2, 2 * i + 3);
        mesh.indices.extend_from_slice(&[a, b, c, b, d, c]);
    }
}

/// Half-disc cap facing `dir`.
fn cap(mesh: &mut Mesh, c: [f32; 2], dir: [f32; 2], r: f32) {
    let d = normalise(dir);
    let base = d[1].atan2(d[0]) - std::f32::consts::FRAC_PI_2;
    let centre = mesh.vertices.len() as u32;
    mesh.vertices.push(c);
    for i in 0..=CAP_SEGMENTS {
        let a = base + std::f32::consts::PI * i as f32 / CAP_SEGMENTS as f32;
        mesh.vertices.push([c[0] + a.cos() * r, c[1] + a.sin() * r]);
    }
    for i in 0..CAP_SEGMENTS as u32 {
        mesh.indices
            .extend_from_slice(&[centre, centre + 1 + i, centre + 2 + i]);
    }
}

fn disc(mesh: &mut Mesh, c: [f32; 2], r: f32) {
    cap(mesh, c, [1.0, 0.0], r);
    cap(mesh, c, [-1.0, 0.0], r);
}

/// Does a circle at (`x`, `y`) with radius `r` touch the stroke (in the
/// stroke's own coordinates)?
pub fn hits(stroke: &Stroke, x: f32, y: f32, r: f32) -> bool {
    if !stroke.shape.is_empty() {
        return shape::hits_shape(&stroke.shape, x, y, r);
    }
    let near = |p: &Point| r + stroke.width_at(p.pressure) * 0.5;
    match stroke.points.as_slice() {
        [] => false,
        [p] => (p.x - x).hypot(p.y - y) <= near(p),
        pts => pts.windows(2).any(|w| {
            let (a, b) = (w[0], w[1]);
            let (dx, dy) = (b.x - a.x, b.y - a.y);
            let len2 = dx * dx + dy * dy;
            let t = if len2 > 0.0 {
                (((x - a.x) * dx + (y - a.y) * dy) / len2).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let (px, py) = (a.x + dx * t, a.y + dy * t);
            (px - x).hypot(py - y) <= near(&a).max(near(&b))
        }),
    }
}

/// Bounding box of a stroke including its width, as (min, max).
pub fn bounds(stroke: &Stroke) -> Option<([f32; 2], [f32; 2])> {
    if !stroke.shape.is_empty() {
        let mut it = stroke.shape.iter().flatten();
        let f = it.next()?;
        let (mut lo, mut hi) = (*f, *f);
        for p in it {
            lo = [lo[0].min(p[0]), lo[1].min(p[1])];
            hi = [hi[0].max(p[0]), hi[1].max(p[1])];
        }
        return Some((lo, hi));
    }
    let mut it = stroke.points.iter();
    let first = it.next()?;
    let r0 = stroke.width_at(first.pressure) * 0.5;
    let (mut lo, mut hi) = ([first.x - r0, first.y - r0], [first.x + r0, first.y + r0]);
    for p in it {
        let r = stroke.width_at(p.pressure) * 0.5;
        lo = [lo[0].min(p.x - r), lo[1].min(p.y - r)];
        hi = [hi[0].max(p.x + r), hi[1].max(p.y + r)];
    }
    Some((lo, hi))
}

/// Bounding box of several strokes.
pub fn bounds_of<'a>(
    strokes: impl IntoIterator<Item = &'a Stroke>,
) -> Option<([f32; 2], [f32; 2])> {
    strokes
        .into_iter()
        .filter_map(bounds)
        .reduce(|(a0, a1), (b0, b1)| {
            (
                [a0[0].min(b0[0]), a0[1].min(b0[1])],
                [a1[0].max(b1[0]), a1[1].max(b1[1])],
            )
        })
}

/// The stroke moved by `m`. Widths scale with the transform's average
/// scale, so a scaled-up drawing gets thicker lines, as on paper.
pub fn transformed(stroke: &Stroke, m: Affine) -> Stroke {
    let k = scale_of(m);
    let mut out = stroke.clone();
    for p in out.points.iter_mut() {
        let (x, y) = m.apply(p.x, p.y);
        p.x = x;
        p.y = y;
    }
    for p in out.shape.iter_mut().flatten() {
        let (x, y) = m.apply(p[0], p[1]);
        *p = [x, y];
    }
    // A mirror flips the outline's winding; even-odd filling doesn't mind.
    out.min_width *= k;
    out.max_width *= k;
    out
}

/// Is a point inside a closed polygon (even-odd rule)? For lasso selection.
pub fn inside_polygon(x: f32, y: f32, poly: &[[f32; 2]]) -> bool {
    let mut inside = false;
    let n = poly.len();
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + n - 1) % n]);
        if (a[1] > y) != (b[1] > y) && x < (b[0] - a[0]) * (y - a[1]) / (b[1] - a[1]) + a[0] {
            inside = !inside;
        }
    }
    inside
}

#[cfg(test)]
mod tests {
    use super::*;
    use symphony_core::id::Id;
    use symphony_core::stroke::{ArtLayer, StrokeKind};

    fn stroke(points: &[(f32, f32)]) -> Stroke {
        Stroke {
            id: Id(1),
            kind: StrokeKind::Brush,
            art: ArtLayer::LineArt,
            swatch: Id(2),
            min_width: 2.0,
            max_width: 2.0,
            points: points
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

    #[test]
    fn flat_tips_drop_the_round_caps() {
        let round = stroke(&[(0.0, 0.0), (10.0, 0.0), (20.0, 0.0)]);
        let mut flat = round.clone();
        flat.tips = symphony_core::stroke::Tips {
            start: Tip::Flat,
            end: Tip::Flat,
        };
        let a = tessellate(&round, Affine::IDENTITY, 0.1);
        let b = tessellate(&flat, Affine::IDENTITY, 0.1);
        let reach = |m: &Mesh| m.vertices.iter().map(|v| v[0]).fold(f32::MIN, f32::max);
        assert!(reach(&a) > 20.5, "round end sticks out past the centreline");
        assert!(reach(&b) <= 20.01, "flat end stops at the centreline");
    }

    #[test]
    fn pressure_curve_ends_are_fixed() {
        assert_eq!(pressure_curve(0.0, 2.0), 0.0);
        assert_eq!(pressure_curve(1.0, 0.5), 1.0);
        assert!(pressure_curve(0.5, 0.5) > 0.5);
    }

    #[test]
    fn stabilization_filters_small_moves_and_reaches_the_lift() {
        let mut smoother = Smoother::new(0.0).with_stabilization(3.0);
        let point = |x| Point {
            x,
            y: 0.0,
            pressure: 1.0,
            pen: Default::default(),
        };
        assert_eq!(smoother.push(point(0.0), 0.1), Some(point(0.0)));
        assert_eq!(smoother.push(point(2.0), 0.1), None);
        let stabilized = smoother.push(point(7.0), 0.1).unwrap();
        assert!((stabilized.x - 4.0).abs() < 1e-5);
        assert_eq!(smoother.finish(), Some(point(7.0)));
    }

    #[test]
    fn smoother_ends_where_the_pen_lifted() {
        let mut s = Smoother::new(0.8);
        let mut out = vec![];
        for x in 0..10 {
            out.extend(s.push(
                Point {
                    x: x as f32 * 10.0,
                    y: 0.0,
                    pressure: 1.0,
                    pen: Default::default(),
                },
                0.1,
            ));
        }
        out.extend(s.finish());
        assert_eq!(out.last().unwrap().x, 90.0);
        // Smoothed points lag the raw ones.
        assert!(out[3].x < 30.0);
    }

    #[test]
    fn tessellation_is_well_formed() {
        let m = tessellate(
            &stroke(&[(0.0, 0.0), (10.0, 0.0), (20.0, 5.0)]),
            Affine::IDENTITY,
            0.1,
        );
        assert!(m.indices.len().is_multiple_of(3) && !m.indices.is_empty());
        assert!(m.indices.iter().all(|&i| (i as usize) < m.vertices.len()));
        // Width 2 means the strip spans y = -1..1 at the start.
        assert!(m.vertices.iter().any(|v| (v[1] - 1.0).abs() < 1e-4));
    }

    #[test]
    fn single_point_is_a_dot() {
        let m = tessellate(&stroke(&[(5.0, 5.0)]), Affine::IDENTITY, 0.1);
        assert!(!m.indices.is_empty());
    }

    #[test]
    fn bounds_include_width_and_transform_scales_it() {
        let s = stroke(&[(0.0, 0.0), (10.0, 0.0)]);
        assert_eq!(bounds(&s), Some(([-1.0, -1.0], [11.0, 1.0])));
        let t = transformed(&s, Affine::scale(2.0, 2.0));
        assert_eq!(t.points[1].x, 20.0);
        assert_eq!(t.max_width, 4.0);
    }

    #[test]
    fn lasso_contains_points() {
        let sq = [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]];
        assert!(inside_polygon(5.0, 5.0, &sq));
        assert!(!inside_polygon(15.0, 5.0, &sq));
    }

    #[test]
    fn hit_test_respects_width() {
        let s = stroke(&[(0.0, 0.0), (10.0, 0.0)]);
        assert!(hits(&s, 5.0, 1.5, 1.0));
        assert!(!hits(&s, 5.0, 3.0, 1.0));
    }
}
