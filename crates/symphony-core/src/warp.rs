//! Mesh Warp (Harmony 27's Mesh Warp node): a grid of points over the
//! artwork; moving a point bends everything under the grid with it,
//! smoothly, without changing the drawings themselves (non-destructive).
//!
//! The node sits in the transform chain like a peg. Its grid is placed
//! in the node's own space (`x`, `y` is its centre, `width` x `height` its
//! size, all animatable) and has `rows` x `columns` cells. Each grid point
//! carries an animatable offset from where the grid puts it.
//!
//! Between points the offsets blend smoothly (Catmull-Rom across the
//! grid, so the warp passes through every point with no kinks). Outside
//! the grid, artwork follows the nearest edge of the grid, so nothing
//! tears at the border.

use serde::{Deserialize, Serialize};

use crate::anim::Value;
use crate::math::Affine;

/// How finely deformed artwork is resampled (Harmony's Deformation
/// Quality): straight edges are split into pieces this many times
/// smaller than a grid cell, so they bend smoothly.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum WarpQuality {
    Low,
    #[default]
    Medium,
    High,
    VeryHigh,
    Extreme,
}

impl WarpQuality {
    pub const ALL: [WarpQuality; 5] = [
        WarpQuality::Low,
        WarpQuality::Medium,
        WarpQuality::High,
        WarpQuality::VeryHigh,
        WarpQuality::Extreme,
    ];

    pub fn title(self) -> &'static str {
        match self {
            WarpQuality::Low => "Low",
            WarpQuality::Medium => "Medium",
            WarpQuality::High => "High",
            WarpQuality::VeryHigh => "Very High",
            WarpQuality::Extreme => "Extreme",
        }
    }

    /// Pieces per grid cell side.
    pub fn pieces(self) -> f32 {
        match self {
            WarpQuality::Low => 4.0,
            WarpQuality::Medium => 8.0,
            WarpQuality::High => 16.0,
            WarpQuality::VeryHigh => 32.0,
            WarpQuality::Extreme => 64.0,
        }
    }
}

/// What bends the artwork.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum WarpKind {
    /// A grid of rows x columns over an area.
    #[default]
    Grid,
    /// Points placed on the artwork, with a mesh built over its shape
    /// (like Harmony's Shape-Aware Deformation): parts bend as rigidly
    /// as they can, and only what's connected to a point follows it.
    Points,
}

/// What happens to lines' thickness.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum WarpLines {
    /// Lines follow the warp but keep their thickness (the paths bend).
    #[default]
    Paths,
    /// The whole shape warps, as if seen in perspective: where the warp
    /// stretches, lines get thicker; where it squeezes, thinner.
    Shape,
}

/// A Mesh Warp node's parameters.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MeshWarp {
    pub rows: u32,
    pub columns: u32,
    /// Grid centre (Harmony's (x) Axis and (y) Axis).
    pub x: Value,
    pub y: Value,
    pub width: Value,
    pub height: Value,
    /// Offsets of the grid points, row by row from the top-left:
    /// (rows + 1) x (columns + 1) of them.
    pub points: Vec<[Value; 2]>,
    #[serde(default)]
    pub quality: WarpQuality,
    #[serde(default)]
    pub kind: WarpKind,
    #[serde(default)]
    pub lines: WarpLines,
    /// Points mode: where the points were placed (the warp's space).
    #[serde(default)]
    pub pins: Vec<[f32; 2]>,
    /// Points mode: how far each point has moved (animatable).
    #[serde(default)]
    pub pin_moves: Vec<[Value; 2]>,
    /// Points mode: the mesh built over the artwork once the points are
    /// placed (None while placing them).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mesh: Option<crate::puppet::PuppetMesh>,
    /// Points mode: about how big the mesh's triangles are.
    #[serde(default = "default_density")]
    pub density: f32,
}

fn default_density() -> f32 {
    30.0
}

/// Most rows or columns a grid can have.
pub const MAX_CELLS: u32 = 32;

impl MeshWarp {
    /// A flat `rows` x `columns` grid over `width` x `height` at (x, y).
    pub fn new(rows: u32, columns: u32, x: f32, y: f32, width: f32, height: f32) -> Self {
        let (rows, columns) = (rows.clamp(1, MAX_CELLS), columns.clamp(1, MAX_CELLS));
        MeshWarp {
            rows,
            columns,
            x: Value::Constant(x),
            y: Value::Constant(y),
            width: Value::Constant(width),
            height: Value::Constant(height),
            points: vec![[Value::Constant(0.0), Value::Constant(0.0)]; point_count(rows, columns)],
            quality: WarpQuality::default(),
            kind: WarpKind::Grid,
            lines: WarpLines::Paths,
            pins: Vec::new(),
            pin_moves: Vec::new(),
            mesh: None,
            density: default_density(),
        }
    }

    /// Change the grid's resolution. Offsets are kept where the old grid
    /// had a point at the same place (its corners, at least), and the new
    /// points start flat.
    pub fn resize(&mut self, rows: u32, columns: u32) {
        let (rows, columns) = (rows.clamp(1, MAX_CELLS), columns.clamp(1, MAX_CELLS));
        if (rows, columns) == (self.rows, self.columns) {
            return;
        }
        let mut points =
            vec![[Value::Constant(0.0), Value::Constant(0.0)]; point_count(rows, columns)];
        for r in 0..=rows {
            for c in 0..=columns {
                // The same spot on the old grid, if a point is there.
                let (or, oc) = (
                    r as f32 * self.rows as f32 / rows as f32,
                    c as f32 * self.columns as f32 / columns as f32,
                );
                if or.fract() == 0.0 && oc.fract() == 0.0 {
                    let old = or as usize * (self.columns as usize + 1) + oc as usize;
                    if let Some(p) = self.points.get(old) {
                        points[r as usize * (columns as usize + 1) + c as usize] = p.clone();
                    }
                }
            }
        }
        self.rows = rows;
        self.columns = columns;
        self.points = points;
    }

    /// Does any parameter have a key on `frame`?
    pub fn has_key(&self, frame: u32) -> bool {
        [&self.x, &self.y, &self.width, &self.height]
            .into_iter()
            .chain(self.points.iter().flatten())
            .chain(self.pin_moves.iter().flatten())
            .any(|v| v.has_key(frame))
    }

    /// Points mode: where each point is on `frame`.
    pub fn pins_at(&self, frame: u32) -> Vec<[f32; 2]> {
        self.pins
            .iter()
            .enumerate()
            .map(|(i, p)| match self.pin_moves.get(i) {
                Some([x, y]) => [p[0] + x.at(frame), p[1] + y.at(frame)],
                None => *p,
            })
            .collect()
    }

    /// What bends the artwork on `frame` (None: nothing does yet, such
    /// as points placed but no mesh built).
    pub fn field_at(&self, frame: u32) -> Option<WarpField> {
        match self.kind {
            WarpKind::Grid => Some(WarpField::Grid(self.grid_at(frame))),
            WarpKind::Points => {
                let mesh = self.mesh.clone()?;
                let targets = self.pins_at(frame);
                let now = crate::puppet::deform(&mesh, &targets);
                Some(WarpField::Points(std::sync::Arc::new(
                    crate::puppet::PuppetField::new(std::sync::Arc::new(mesh), now),
                )))
            }
        }
    }

    /// The grid as it is on `frame`.
    pub fn grid_at(&self, frame: u32) -> WarpGrid {
        let n = point_count(self.rows, self.columns);
        let mut offsets: Vec<[f32; 2]> = self
            .points
            .iter()
            .map(|[x, y]| [x.at(frame), y.at(frame)])
            .collect();
        offsets.resize(n, [0.0, 0.0]);
        WarpGrid {
            rows: self.rows.max(1),
            columns: self.columns.max(1),
            centre: [self.x.at(frame), self.y.at(frame)],
            size: [
                self.width.at(frame).max(1e-3),
                self.height.at(frame).max(1e-3),
            ],
            offsets,
            quality: self.quality,
        }
    }
}

pub fn point_count(rows: u32, columns: u32) -> usize {
    (rows as usize + 1) * (columns as usize + 1)
}

/// A grid evaluated at one frame.
#[derive(Clone, Debug, PartialEq)]
pub struct WarpGrid {
    pub rows: u32,
    pub columns: u32,
    pub centre: [f32; 2],
    pub size: [f32; 2],
    /// Offset of each grid point from its rest place.
    pub offsets: Vec<[f32; 2]>,
    pub quality: WarpQuality,
}

impl WarpGrid {
    /// Where grid point (row, column) rests (before its offset).
    pub fn rest(&self, row: u32, column: u32) -> [f32; 2] {
        [
            self.centre[0] - self.size[0] / 2.0
                + self.size[0] * column as f32 / self.columns as f32,
            self.centre[1] - self.size[1] / 2.0 + self.size[1] * row as f32 / self.rows as f32,
        ]
    }

    /// Where grid point (row, column) is now.
    pub fn point(&self, row: u32, column: u32) -> [f32; 2] {
        let r = self.rest(row, column);
        let o = self.offset(row as i64, column as i64);
        [r[0] + o[0], r[1] + o[1]]
    }

    fn offset(&self, row: i64, column: i64) -> [f32; 2] {
        let r = row.clamp(0, self.rows as i64) as usize;
        let c = column.clamp(0, self.columns as i64) as usize;
        self.offsets
            .get(r * (self.columns as usize + 1) + c)
            .copied()
            .unwrap_or([0.0, 0.0])
    }

    /// Is the grid flat (nothing moved)?
    pub fn is_flat(&self) -> bool {
        self.offsets.iter().all(|o| o[0] == 0.0 && o[1] == 0.0)
    }

    /// The longest straight piece that still bends smoothly, in the
    /// grid's space.
    pub fn piece(&self) -> f32 {
        (self.size[0] / self.columns as f32)
            .min(self.size[1] / self.rows as f32)
            .max(1e-3)
            / self.quality.pieces()
    }

    /// Bend a point (in the grid's space).
    pub fn apply(&self, p: [f32; 2]) -> [f32; 2] {
        let left = self.centre[0] - self.size[0] / 2.0;
        let top = self.centre[1] - self.size[1] / 2.0;
        // Grid coordinates; outside the grid, the nearest edge's offset.
        let u =
            ((p[0] - left) / self.size[0] * self.columns as f32).clamp(0.0, self.columns as f32);
        let v = ((p[1] - top) / self.size[1] * self.rows as f32).clamp(0.0, self.rows as f32);
        let (c0, r0) = (
            (u.floor() as i64).min(self.columns as i64 - 1),
            (v.floor() as i64).min(self.rows as i64 - 1),
        );
        let (tu, tv) = (u - c0 as f32, v - r0 as f32);
        let wu = catmull_weights(tu);
        let wv = catmull_weights(tv);
        let mut d = [0.0f32; 2];
        for (j, wy) in wv.iter().enumerate() {
            for (i, wx) in wu.iter().enumerate() {
                let o = self.offset(r0 - 1 + j as i64, c0 - 1 + i as i64);
                d[0] += wx * wy * o[0];
                d[1] += wx * wy * o[1];
            }
        }
        [p[0] + d[0], p[1] + d[1]]
    }
}

/// Catmull-Rom weights for the four points around `t` (0..1 between the
/// middle two): the curve passes through each point.
fn catmull_weights(t: f32) -> [f32; 4] {
    let (t2, t3) = (t * t, t * t * t);
    [
        0.5 * (-t3 + 2.0 * t2 - t),
        0.5 * (3.0 * t3 - 5.0 * t2 + 2.0),
        0.5 * (-3.0 * t3 + 4.0 * t2 + t),
        0.5 * (t3 - t2),
    ]
}

/// What one Mesh Warp does on one frame.
#[derive(Clone, Debug, PartialEq)]
pub enum WarpField {
    Grid(WarpGrid),
    Points(std::sync::Arc<crate::puppet::PuppetField>),
    Bone(Box<crate::rig::BoneField>),
}

impl WarpField {
    pub fn apply(&self, p: [f32; 2]) -> [f32; 2] {
        match self {
            WarpField::Grid(g) => g.apply(p),
            WarpField::Points(f) => f.apply(p),
            WarpField::Bone(f) => f.apply(p),
        }
    }

    pub fn is_flat(&self) -> bool {
        match self {
            WarpField::Grid(g) => g.is_flat(),
            WarpField::Points(f) => f.is_flat(),
            WarpField::Bone(f) => {
                f.child == Affine::IDENTITY && (f.radius <= 0.0 || f.parent == Affine::IDENTITY)
            }
        }
    }

    /// The longest straight piece that still bends smoothly.
    pub fn piece(&self) -> f32 {
        match self {
            WarpField::Grid(g) => g.piece(),
            WarpField::Points(f) => f.cell() / 4.0,
            WarpField::Bone(f) => {
                if f.radius > 0.0 {
                    (f.radius / 4.0).max(1.0)
                } else {
                    f32::MAX
                }
            }
        }
    }
}

/// One Mesh Warp on the way from a drawing to the stage: what it does,
/// and the plain transform that takes its space onward.
#[derive(Clone, Debug, PartialEq)]
pub struct WarpStep {
    pub outer: Affine,
    pub field: WarpField,
}

/// How a drawing under Mesh Warps reaches the stage: a drawing point p
/// goes `steps[0].outer(steps[0].grid(steps[1].outer(... inner(p))))`.
#[derive(Clone, Debug, PartialEq)]
pub struct Warp {
    pub steps: Vec<WarpStep>,
    pub inner: Affine,
    /// Shape when any of the warps bends lines' thickness too.
    pub lines: WarpLines,
}

impl Warp {
    /// A cheap fingerprint of everything that shapes the bend (to cache
    /// bent artwork by).
    pub fn key(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        let affine = |m: Affine, h: &mut std::collections::hash_map::DefaultHasher| {
            for v in [m.a, m.b, m.c, m.d, m.tx, m.ty] {
                v.to_bits().hash(h);
            }
        };
        affine(self.inner, &mut h);
        (self.lines as u8).hash(&mut h);
        for s in &self.steps {
            affine(s.outer, &mut h);
            match &s.field {
                WarpField::Grid(g) => {
                    (g.rows, g.columns, g.quality as u8).hash(&mut h);
                    for v in g
                        .centre
                        .iter()
                        .chain(&g.size)
                        .chain(g.offsets.iter().flatten())
                    {
                        v.to_bits().hash(&mut h);
                    }
                }
                WarpField::Points(f) => f.key().hash(&mut h),
                WarpField::Bone(f) => {
                    affine(f.rest_inverse, &mut h);
                    affine(f.parent, &mut h);
                    affine(f.child, &mut h);
                    f.radius.to_bits().hash(&mut h);
                }
            }
        }
        h.finish()
    }

    /// A drawing point into the space of the outermost grid's parent
    /// (everything but `steps[0].outer`): the shape the drawing takes, to
    /// draw with `steps[0].outer` as its transform.
    pub fn bend(&self, p: [f32; 2]) -> [f32; 2] {
        let (x, y) = self.inner.apply(p[0], p[1]);
        let mut q = [x, y];
        for (i, step) in self.steps.iter().enumerate().rev() {
            q = step.field.apply(q);
            if i > 0 {
                let (x, y) = step.outer.apply(q[0], q[1]);
                q = [x, y];
            }
        }
        q
    }

    /// The transform to draw bent artwork with.
    pub fn outer(&self) -> Affine {
        self.steps.first().map_or(Affine::IDENTITY, |s| s.outer)
    }

    /// Rigid skin for imported raster layers. Their pixels stay intact
    /// while the vector artwork uses the full joint deformation.
    pub fn raster_transform(&self) -> Option<Affine> {
        let mut changed = false;
        let mut transform = self.inner;
        for step in self.steps.iter().rev() {
            if let WarpField::Bone(bone) = &step.field {
                transform = bone.child.compose(transform);
                changed = true;
            }
            transform = step.outer.compose(transform);
        }
        changed.then_some(transform)
    }

    /// The longest straight piece of drawing-space geometry that still
    /// bends smoothly through every grid.
    pub fn piece(&self) -> f32 {
        // Grid sizes are in their own spaces; drawing space is scaled by
        // the transforms in between (roughly: the inner one).
        let m = self.inner;
        let k = ((m.a * m.d - m.b * m.c).abs().sqrt()).max(1e-6);
        self.steps
            .iter()
            .map(|s| s.field.piece())
            .fold(f32::MAX, f32::min)
            / k
    }

    /// Nothing bends (every grid flat).
    pub fn is_flat(&self) -> bool {
        self.steps.iter().all(|s| s.field.is_flat())
    }
}

/// Split a polyline so no piece is longer than `piece` (bent artwork then
/// curves instead of staying straight between far-apart points).
pub fn densify(points: &[[f32; 2]], piece: f32, closed: bool) -> Vec<[f32; 2]> {
    if points.len() < 2 || piece.is_nan() || piece <= 0.0 {
        return points.to_vec();
    }
    let n = points.len();
    let segments = if closed { n } else { n - 1 };
    let mut out = Vec::with_capacity(n * 2);
    for i in 0..segments {
        let (a, b) = (points[i], points[(i + 1) % n]);
        out.push(a);
        let len = (b[0] - a[0]).hypot(b[1] - a[1]);
        let k = ((len / piece).ceil() as usize).clamp(1, 512);
        for s in 1..k {
            let t = s as f32 / k as f32;
            out.push([a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]);
        }
    }
    if !closed {
        out.push(points[n - 1]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_flat_grid_changes_nothing() {
        let w = MeshWarp::new(3, 4, 10.0, -5.0, 200.0, 100.0);
        let g = w.grid_at(1);
        assert!(g.is_flat());
        for p in [[0.0, 0.0], [95.0, 40.0], [-500.0, 300.0]] {
            assert_eq!(g.apply(p), p);
        }
    }

    #[test]
    fn moving_a_point_moves_artwork_there_and_fades_away() {
        let mut w = MeshWarp::new(2, 2, 0.0, 0.0, 100.0, 100.0);
        // The centre point (row 1, column 1).
        w.points[4] = [Value::Constant(10.0), Value::Constant(-20.0)];
        let g = w.grid_at(1);
        let at = g.apply([0.0, 0.0]);
        assert!(
            (at[0] - 10.0).abs() < 1e-4 && (at[1] + 20.0).abs() < 1e-4,
            "{at:?}"
        );
        // The corners don't move; in between moves part way, smoothly.
        assert_eq!(g.apply([-50.0, -50.0]), [-50.0, -50.0]);
        let mid = g.apply([25.0, 0.0]);
        assert!(mid[0] > 25.0 && mid[0] < 35.0, "{mid:?}");
        // Outside the grid: the nearest edge's offset (none here).
        assert_eq!(g.apply([200.0, 0.0]), [200.0, 0.0]);
    }

    #[test]
    fn points_animate_and_resizing_keeps_corners() {
        let mut w = MeshWarp::new(1, 1, 0.0, 0.0, 10.0, 10.0);
        w.points[3][0].set_key(1, 0.0);
        w.points[3][0].set_key(11, 10.0);
        assert!(
            (w.grid_at(6).point(1, 1)[0] - 10.0).abs() < 1e-4,
            "half way: 5 + 5"
        );
        assert!(w.has_key(11) && !w.has_key(6));
        w.resize(2, 2);
        assert_eq!(w.points.len(), 9);
        assert_eq!(w.points[8][0].at(11), 10.0, "the corner kept its keys");
        assert_eq!(w.points[4][0].at(11), 0.0, "new points start flat");
    }

    #[test]
    fn chained_warps_bend_in_order() {
        let mut w = MeshWarp::new(1, 1, 0.0, 0.0, 100.0, 100.0);
        for p in &mut w.points {
            p[0] = Value::Constant(5.0);
        }
        let warp = Warp {
            steps: vec![
                WarpStep {
                    outer: Affine::translate(1000.0, 0.0),
                    field: WarpField::Grid(w.grid_at(1)),
                },
                WarpStep {
                    outer: Affine::scale(2.0, 2.0),
                    field: WarpField::Grid(w.grid_at(1)),
                },
            ],
            inner: Affine::IDENTITY,
            lines: WarpLines::Paths,
        };
        // inner, inner grid (+5), its outer (x2), outer grid (+5).
        let b = warp.bend([1.0, 0.0]);
        assert!((b[0] - 17.0).abs() < 1e-4 && b[1].abs() < 1e-4, "{b:?}");
        assert_eq!(warp.outer(), Affine::translate(1000.0, 0.0));
    }

    #[test]
    fn densify_splits_long_edges_only() {
        let sq = [[0.0, 0.0], [10.0, 0.0], [10.0, 1.0]];
        let d = densify(&sq, 2.5, true);
        assert_eq!(d.len(), 4 + 1 + 5);
        assert_eq!(densify(&sq, 100.0, false), sq.to_vec());
    }
}
