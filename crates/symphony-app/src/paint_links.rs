//! A bucket fill keeps a sample inside its region and the ids of the
//! lines that enclosed it. Refit on commit, without running a flood fill
//! for every pointer event while the line is being edited.

use symphony_core::{
    history::Edit,
    id::Id,
    math::Affine,
    stroke::{Drawing, FillLink, Stroke},
};
use symphony_vector::{bounds_of, fill::find_zone};

pub fn link(
    strokes: &[&Stroke],
    contours: &[Vec<[f32; 2]>],
    seed: [f32; 2],
    cell: f32,
) -> Option<FillLink> {
    let outer = contours.first()?;
    let first = *outer.first()?;
    let (lo, hi) = outer.iter().fold((first, first), |(lo, hi), p| {
        (
            [lo[0].min(p[0]), lo[1].min(p[1])],
            [hi[0].max(p[0]), hi[1].max(p[1])],
        )
    });
    // Include lines around holes as well as the outer edge. Long,
    // simplified boundary segments are sampled instead of relying on vertices.
    let reach = (cell * 4.0).max((hi[0] - lo[0]).max(hi[1] - lo[1]) / 256.0);
    let nearby: Vec<_> = strokes
        .iter()
        .copied()
        .filter(|stroke| {
            symphony_vector::bounds(stroke).is_some_and(|(a, b)| {
                (0..2).all(|axis| b[axis] + reach >= lo[axis] && a[axis] - reach <= hi[axis])
            })
        })
        .collect();
    let mut boundary = Vec::new();
    for stroke in nearby {
        let hits = contours.iter().any(|ring| {
            ring.iter().zip(ring.iter().cycle().skip(1)).any(|(a, b)| {
                let count =
                    (((b[0] - a[0]).hypot(b[1] - a[1]) / reach).ceil() as u32).clamp(1, 256);
                (0..=count).any(|i| {
                    let t = i as f32 / count as f32;
                    let x = a[0] + (b[0] - a[0]) * t;
                    let y = a[1] + (b[1] - a[1]) * t;
                    symphony_vector::hits(stroke, x, y, reach)
                })
            })
        });
        if hits {
            boundary.push(stroke);
        }
    }
    Some(FillLink {
        seed,
        cell,
        bounds: bounds_of(boundary.iter().copied())?,
        strokes: boundary.into_iter().map(|stroke| stroke.id).collect(),
    })
}

pub fn refit(
    drawing: &Drawing,
    changed: &[Stroke],
    explicitly_moved: &[Id],
    transform: Option<Affine>,
) -> Vec<Edit> {
    if changed.is_empty() {
        return Vec::new();
    }
    drawing
        .fills
        .iter()
        .enumerate()
        .filter_map(|(index, fill)| {
            let linked = fill.linked.as_ref()?;
            if explicitly_moved.contains(&fill.id)
                || !changed
                    .iter()
                    .any(|stroke| linked.strokes.contains(&stroke.id))
            {
                return None;
            }
            let strokes: Vec<_> = linked
                .strokes
                .iter()
                .filter_map(|id| {
                    changed
                        .iter()
                        .find(|stroke| stroke.id == *id)
                        .or_else(|| drawing.strokes.iter().find(|stroke| stroke.id == *id))
                })
                .collect();
            if strokes.len() != linked.strokes.len() {
                return None;
            }
            let bounds = bounds_of(strokes.iter().copied())?;
            let mut after = fill.clone();
            let link = after.linked.as_mut()?;
            if let Some(m) = transform.filter(|_| {
                linked
                    .strokes
                    .iter()
                    .all(|id| changed.iter().any(|s| s.id == *id))
            }) {
                let map = |[x, y]: [f32; 2]| {
                    let (x, y) = m.apply(x, y);
                    [x, y]
                };
                for p in after.contours.iter_mut().flatten() {
                    *p = map(*p);
                }
                link.seed = map(link.seed);
            } else {
                let (old_lo, old_hi) = linked.bounds;
                let (lo, hi) = bounds;
                let remapped = [0, 1].map(|axis| {
                    let t = (linked.seed[axis] - old_lo[axis])
                        / (old_hi[axis] - old_lo[axis]).max(0.001);
                    lo[axis] + t * (hi[axis] - lo[axis])
                });
                let candidates = [remapped, linked.seed];
                let (seed, contours) = candidates.into_iter().find_map(|[x, y]| {
                    find_zone(&strokes, x, y, linked.cell)
                        .ok()
                        .map(|contours| ([x, y], contours))
                })?;
                after.contours = contours;
                link.seed = seed;
            }
            link.bounds = bounds;
            (*fill != after).then(|| Edit::ReplaceFill {
                drawing: drawing.id,
                index,
                before: fill.clone(),
                after: vec![after],
            })
        })
        .collect()
}
