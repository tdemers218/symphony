//! Smooth vector lines from few points.
//!
//! A drawn line keeps only the points that matter (`simplify`), and is
//! drawn as a smooth curve through them (`spline`, a centripetal
//! Catmull-Rom curve, which never loops or overshoots on uneven spacing),
//! subdivided as finely as the current zoom needs. So lines are lighter to
//! store and draw, and smooth at any zoom.

use symphony_core::stroke::Point;

/// The perpendicular distance from `p` to segment a-b, and where along it.
fn seg_dist(p: &Point, a: &Point, b: &Point) -> (f32, f32) {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let len2 = dx * dx + dy * dy;
    let t = if len2 > 0.0 {
        (((p.x - a.x) * dx + (p.y - a.y) * dy) / len2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    ((a.x + dx * t - p.x).hypot(a.y + dy * t - p.y), t)
}

/// Keep only the points that change the line by more than `tolerance`
/// (drawing units). Width changes count too: `width_span` is the width
/// difference between no and full pressure, so a pressure swing that would
/// thicken the line visibly keeps its point.
pub fn simplify(points: &[Point], tolerance: f32, width_span: f32) -> Vec<Point> {
    if points.len() <= 2 || tolerance <= 0.0 {
        return points.to_vec();
    }
    let mut keep = vec![false; points.len()];
    keep[0] = true;
    keep[points.len() - 1] = true;
    let mut stack = vec![(0usize, points.len() - 1)];
    while let Some((a, b)) = stack.pop() {
        if b <= a + 1 {
            continue;
        }
        let (mut worst, mut at) = (0.0f32, 0usize);
        for i in a + 1..b {
            let (d, t) = seg_dist(&points[i], &points[a], &points[b]);
            let pressure = points[a].pressure + (points[b].pressure - points[a].pressure) * t;
            let offset =
                points[a].width_offset + (points[b].width_offset - points[a].width_offset) * t;
            let err = d.max(
                ((points[i].pressure - pressure) * width_span + points[i].width_offset - offset)
                    .abs()
                    * 0.5,
            );
            if err > worst {
                worst = err;
                at = i;
            }
        }
        if worst > tolerance {
            keep[at] = true;
            stack.push((a, at));
            stack.push((at, b));
        }
    }
    points
        .iter()
        .zip(keep)
        .filter_map(|(p, k)| k.then_some(*p))
        .collect()
}

/// A point on the centripetal Catmull-Rom segment from `p1` to `p2`.
fn catmull(p0: &Point, p1: &Point, p2: &Point, p3: &Point, u: f32) -> Point {
    let knot = |a: &Point, b: &Point| ((b.x - a.x).hypot(b.y - a.y)).sqrt().max(1e-4);
    let t0 = 0.0;
    let t1 = t0 + knot(p0, p1);
    let t2 = t1 + knot(p1, p2);
    let t3 = t2 + knot(p2, p3);
    let t = t1 + (t2 - t1) * u;
    let lerp = |a: (f32, f32, f32), b: (f32, f32, f32), ta: f32, tb: f32| {
        let w = if (tb - ta).abs() < 1e-6 {
            0.0
        } else {
            (t - ta) / (tb - ta)
        };
        (
            a.0 + (b.0 - a.0) * w,
            a.1 + (b.1 - a.1) * w,
            a.2 + (b.2 - a.2) * w,
        )
    };
    let v = |p: &Point| (p.x, p.y, p.pressure);
    let a1 = lerp(v(p0), v(p1), t0, t1);
    let a2 = lerp(v(p1), v(p2), t1, t2);
    let a3 = lerp(v(p2), v(p3), t2, t3);
    let b1 = lerp(a1, a2, t0, t2);
    let b2 = lerp(a2, a3, t1, t3);
    let c = lerp(b1, b2, t1, t2);
    Point {
        x: c.0,
        y: c.1,
        // Pressure is interpolated linearly: a curve through pressure
        // could dip below zero between points.
        pressure: (p1.pressure + (p2.pressure - p1.pressure) * u).clamp(0.0, 1.0),
        width_offset: p1.width_offset + (p2.width_offset - p1.width_offset) * u,
        pen: symphony_core::stroke::Pen::lerp(p1.pen, p2.pen, u),
    }
}

/// A smooth curve through `points`, with steps no longer than `step`
/// (drawing units). Passes exactly through every point.
pub fn spline(points: &[Point], step: f32) -> Vec<Point> {
    if points.len() < 3 || step.is_nan() || step <= 0.0 {
        return points.to_vec();
    }
    let n = points.len();
    // A point where the line turns sharply is a corner: the curve goes
    // straight into it and out of it, so drawn corners stay sharp and the
    // sides next to them don't bulge.
    let corner = |i: usize| -> bool {
        if i == 0 || i + 1 >= n {
            return true;
        }
        let (a, b, c) = (points[i - 1], points[i], points[i + 1]);
        let (ux, uy) = (b.x - a.x, b.y - a.y);
        let (vx, vy) = (c.x - b.x, c.y - b.y);
        let (lu, lv) = (ux.hypot(uy), vx.hypot(vy));
        if lu < 1e-6 || lv < 1e-6 {
            return true;
        }
        (ux * vx + uy * vy) / (lu * lv) < CORNER_COS
    };
    let mirror = |about: &Point, of: &Point| Point {
        x: 2.0 * about.x - of.x,
        y: 2.0 * about.y - of.y,
        ..*about
    };
    let mut out = Vec::with_capacity(n * 4);
    out.push(points[0]);
    for i in 0..n - 1 {
        let (p1, p2) = (points[i], points[i + 1]);
        let p0 = if corner(i) {
            mirror(&p1, &p2)
        } else {
            points[i - 1]
        };
        let p3 = if corner(i + 1) {
            mirror(&p2, &p1)
        } else {
            points[i + 2]
        };
        let len = (p2.x - p1.x).hypot(p2.y - p1.y);
        let steps = ((len / step).ceil() as usize).clamp(1, 256);
        for s in 1..=steps {
            out.push(catmull(&p0, &p1, &p2, &p3, s as f32 / steps as f32));
        }
    }
    out
}

/// Turning more than this (cosine of about 60 degrees) makes a corner.
const CORNER_COS: f32 = 0.5;

#[cfg(test)]
mod tests {
    use super::*;

    fn p(x: f32, y: f32) -> Point {
        Point {
            x,
            y,
            pressure: 1.0,
            width_offset: 0.0,
            pen: Default::default(),
        }
    }

    /// A shaky circle sampled like a real pen: many close points.
    fn arc() -> Vec<Point> {
        (0..=200)
            .map(|i| {
                let a = i as f32 / 200.0 * std::f32::consts::PI;
                p(100.0 * a.cos(), 100.0 * a.sin())
            })
            .collect()
    }

    fn max_gap(curve: &[Point], original: &[Point]) -> f32 {
        // Every original point is near the drawn curve.
        original
            .iter()
            .map(|o| {
                curve
                    .windows(2)
                    .map(|w| seg_dist(o, &w[0], &w[1]).0)
                    .fold(f32::MAX, f32::min)
            })
            .fold(0.0, f32::max)
    }

    #[test]
    fn straight_lines_keep_two_points() {
        let line: Vec<Point> = (0..50).map(|i| p(i as f32, 0.0)).collect();
        assert_eq!(simplify(&line, 0.5, 0.0).len(), 2);
    }

    #[test]
    fn a_curve_keeps_few_points_but_stays_close() {
        let raw = arc();
        let kept = simplify(&raw, 0.5, 0.0);
        assert!(
            kept.len() < raw.len() / 5,
            "{} of {}",
            kept.len(),
            raw.len()
        );
        // The smooth curve through the kept points stays within the
        // tolerance of what was drawn (a little slack for the curve).
        let drawn = spline(&kept, 0.5);
        assert!(max_gap(&drawn, &raw) < 0.75, "{}", max_gap(&drawn, &raw));
    }

    #[test]
    fn pressure_swings_keep_their_points() {
        let mut line: Vec<Point> = (0..50).map(|i| p(i as f32, 0.0)).collect();
        line[25].pressure = 0.0;
        let kept = simplify(&line, 0.5, 10.0);
        assert!(
            kept.iter().any(|k| k.pressure == 0.0),
            "the thin spot survives"
        );
    }

    #[test]
    fn the_curve_passes_through_every_point_and_is_smooth() {
        // Gentle turns (under the corner angle) get a smooth curve.
        let pts = vec![p(0.0, 0.0), p(50.0, 20.0), p(100.0, 0.0), p(150.0, 20.0)];
        let c = spline(&pts, 1.0);
        for k in &pts {
            assert!(c
                .iter()
                .any(|q| (q.x - k.x).abs() < 1e-3 && (q.y - k.y).abs() < 1e-3));
        }
        // No sharp corner at the middle points: neighbouring steps turn gently.
        let turn = c
            .windows(3)
            .map(|w| {
                let a = (w[1].y - w[0].y).atan2(w[1].x - w[0].x);
                let b = (w[2].y - w[1].y).atan2(w[2].x - w[1].x);
                (b - a).abs()
            })
            .fold(0.0f32, f32::max);
        assert!(turn < 0.3, "{turn}");
    }

    #[test]
    fn drawn_corners_stay_sharp() {
        let sq = vec![
            p(10.0, 10.0),
            p(90.0, 10.0),
            p(90.0, 90.0),
            p(10.0, 90.0),
            p(10.0, 10.0),
        ];
        let c = spline(&sq, 1.0);
        // The sides stay straight: nothing bulges past the square.
        assert!(c
            .iter()
            .all(|q| q.x >= 9.99 && q.x <= 90.01 && q.y >= 9.99 && q.y <= 90.01));
    }

    #[test]
    fn full_detail_changes_nothing() {
        let raw = arc();
        assert_eq!(simplify(&raw, 0.0, 5.0), raw);
        assert_eq!(spline(&raw[..2], 1.0), raw[..2].to_vec());
    }
}
