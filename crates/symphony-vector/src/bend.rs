//! Artwork bent by Mesh Warps ([`symphony_core::warp`]): a copy of a
//! drawing with every line and zone passed through the warp, ready to draw
//! with the warp's outer transform. Lines are first smoothed into their
//! curve and long edges split, so they bend smoothly. With the warp's
//! Lines set to Paths, lines keep their thickness (scaled only by the
//! plain transforms on the way); with Shape, a plain line is bent as its
//! outline, so it thickens where the warp stretches and thins where it
//! squeezes, like a picture seen in perspective. (Textured lines stamp
//! along their path, so they always bend as paths.) An imported picture
//! isn't bent (it's left out; draw it with the plain transform).

use symphony_core::stroke::{Drawing, Point, Stroke};
use symphony_core::warp::{densify, Warp, WarpLines};

/// `d` bent by `warp`, in the space `warp.outer()` draws from.
pub fn drawing(d: &Drawing, warp: &Warp) -> Drawing {
    let piece = warp.piece();
    let k = width_scale(warp);
    let mut out = d.clone();
    out.image = None;
    for st in &mut out.strokes {
        *st = stroke(st, warp, piece, k);
    }
    for f in &mut out.fills {
        for ring in &mut f.contours {
            *ring = densify(ring, piece, true)
                .into_iter()
                .map(|p| warp.bend(p))
                .collect();
        }
    }
    out
}

/// How much the plain transforms inside the outermost one scale widths.
fn width_scale(warp: &Warp) -> f32 {
    let det = |m: symphony_core::math::Affine| (m.a * m.d - m.b * m.c).abs().sqrt();
    warp.steps
        .iter()
        .skip(1)
        .map(|s| det(s.outer))
        .product::<f32>()
        * det(warp.inner)
}

fn stroke(st: &Stroke, warp: &Warp, piece: f32, k: f32) -> Stroke {
    let mut out = st.clone();
    // The whole shape warps: a plain line as its outline.
    if warp.lines == WarpLines::Shape
        && st.shape.is_empty()
        && st.texture.is_none()
        && st.kind != symphony_core::stroke::StrokeKind::Invisible
    {
        let outline: Vec<Vec<[f32; 2]>> = crate::shape::outline(st).into_iter().flatten().collect();
        if !outline.is_empty() {
            out.shape = outline;
        }
    }
    if !out.shape.is_empty() {
        for ring in &mut out.shape {
            *ring = densify(ring, piece, true)
                .into_iter()
                .map(|p| warp.bend(p))
                .collect();
        }
    }
    // The line as drawn (its smooth curve), fine enough to bend smoothly.
    let curve: Vec<Point> = if st.points.len() >= 3 {
        crate::curve::spline(&st.points, piece)
    } else {
        // Two points: split the straight line.
        let mut pts = Vec::new();
        for w in st.points.windows(2) {
            let (a, b) = (w[0], w[1]);
            let len = (b.x - a.x).hypot(b.y - a.y);
            let n = ((len / piece).ceil() as usize).clamp(1, 512);
            for s in 0..n {
                let t = s as f32 / n as f32;
                pts.push(Point {
                    x: a.x + (b.x - a.x) * t,
                    y: a.y + (b.y - a.y) * t,
                    pressure: a.pressure + (b.pressure - a.pressure) * t,
                    pen: symphony_core::stroke::Pen::lerp(a.pen, b.pen, t),
                });
            }
        }
        pts.extend(st.points.last().copied());
        pts
    };
    out.points = curve
        .into_iter()
        .map(|p| {
            let [x, y] = warp.bend([p.x, p.y]);
            Point { x, y, ..p }
        })
        .collect();
    out.min_width *= k;
    out.max_width *= k;
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use symphony_core::anim::Value;
    use symphony_core::id::Id;
    use symphony_core::math::Affine;
    use symphony_core::stroke::{Fill, StrokeKind};
    use symphony_core::warp::{MeshWarp, WarpField, WarpStep};

    /// A straight line across a grid whose middle is pulled down comes out
    /// curved: its middle follows the point, its ends stay put.
    #[test]
    fn a_line_bends_with_the_grid() {
        let mut w = MeshWarp::new(2, 2, 0.0, 0.0, 200.0, 200.0);
        w.points[4] = [Value::Constant(0.0), Value::Constant(30.0)];
        let warp = Warp {
            steps: vec![WarpStep {
                outer: Affine::translate(500.0, 0.0),
                field: WarpField::Grid(w.grid_at(1)),
            }],
            inner: Affine::IDENTITY,
            lines: WarpLines::Paths,
        };
        let p = |x: f32| Point {
            x,
            y: 0.0,
            pressure: 1.0,
            pen: Default::default(),
        };
        let line = Stroke {
            id: Id(1),
            kind: StrokeKind::Brush,
            art: Default::default(),
            swatch: Id(0),
            min_width: 4.0,
            max_width: 4.0,
            points: vec![p(-100.0), p(100.0)],
            tips: Default::default(),
            texture: None,
            shape: Vec::new(),
        };
        let d = Drawing {
            id: Id(2),
            column: Id(3),
            name: "d".into(),
            strokes: vec![line],
            fills: vec![Fill {
                linked: None,
                id: Id(4),
                swatch: Id(0),
                art: Default::default(),
                contours: vec![vec![
                    [-50.0, -50.0],
                    [50.0, -50.0],
                    [50.0, 50.0],
                    [-50.0, 50.0],
                ]],
            }],
            image: None,
            text: Vec::new(),
        };
        let bent = drawing(&d, &warp);
        let pts = &bent.strokes[0].points;
        assert!(pts.len() > 10, "split to bend smoothly");
        let mid = pts
            .iter()
            .min_by(|a, b| a.x.abs().total_cmp(&b.x.abs()))
            .unwrap();
        assert!((mid.y - 30.0).abs() < 1.0, "the middle follows: {}", mid.y);
        assert!(pts[0].y.abs() < 1e-3 && pts.last().unwrap().y.abs() < 1e-3);
        assert_eq!(bent.strokes[0].min_width, 4.0);
        // The zone's bottom edge bulges down too.
        let ring = &bent.fills[0].contours[0];
        assert!(ring.len() > 4);
        assert!(ring.iter().any(|p| p[1] > 60.0), "the zone bends");

        // Shape: the line becomes its outline, thicker where the grid
        // stretches it (pull the middle column apart vertically).
        let mut stretch = MeshWarp::new(2, 2, 0.0, 0.0, 200.0, 200.0);
        stretch.points[1] = [Value::Constant(0.0), Value::Constant(-60.0)];
        stretch.points[7] = [Value::Constant(0.0), Value::Constant(60.0)];
        let thick = Warp {
            steps: vec![WarpStep {
                outer: Affine::IDENTITY,
                field: WarpField::Grid(stretch.grid_at(1)),
            }],
            inner: Affine::IDENTITY,
            lines: WarpLines::Shape,
        };
        let shaped = drawing(&d, &thick);
        let outline = &shaped.strokes[0].shape;
        assert!(!outline.is_empty(), "the line warps as a shape");
        // Its thickness at the middle: was 4, now stretched.
        let span: Vec<f32> = outline
            .iter()
            .flatten()
            .filter(|p| p[0].abs() < 3.0)
            .map(|p| p[1])
            .collect();
        let thickness = span.iter().fold(f32::MIN, |a, &b| a.max(b))
            - span.iter().fold(f32::MAX, |a, &b| a.min(b));
        assert!(thickness > 6.0, "thicker where stretched: {thickness}");
        let paths = drawing(
            &d,
            &Warp {
                lines: WarpLines::Paths,
                ..thick.clone()
            },
        );
        assert!(paths.strokes[0].shape.is_empty(), "Paths keeps it a line");
        assert_eq!(paths.strokes[0].max_width, 4.0);
    }
}
