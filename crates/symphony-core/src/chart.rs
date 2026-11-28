//! Smart Charts: an animator's timing and spacing chart as live data
//! (docs/plans/smart-charts.md). Points are placed on key poses; a smooth
//! path runs through them; timing says how far along the path the chart
//! is on each frame. A chart describes motion only, so it never changes
//! artwork by itself: other nodes read it through [`ChartCurve::sample`].
//!
//! Space and time are kept apart. The path is one cubic Bézier per
//! segment between neighbouring points. Each segment's timing maps time
//! (0 to 1 between its two points' frames) to the fraction of that
//! segment's arc length covered (0 to 1). Reshaping the path keeps the
//! spacing, and respacing never moves the path. Scale and rotation are
//! their own keyed channels on the same timeline.

use serde::{Deserialize, Serialize};

use crate::anim::Value;
use crate::id::Id;
use crate::math::Affine;

/// A point's id within its chart: stable across edits and saves.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct PointId(pub u32);

/// What a point stands for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Role {
    /// A key pose.
    #[default]
    Key,
    /// A refinement on an in-between frame: the path goes through it, but
    /// it belongs to no drawing.
    Breakdown,
}

/// How the path passes through a point.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum Tangent {
    /// Smooth, worked out from the neighbouring points (centripetal
    /// Catmull-Rom). Adding or moving a point only reshapes the two
    /// segments beside it.
    #[default]
    Auto,
    /// Handles set by the animator, as offsets from the point: `before`
    /// shapes the segment arriving, `after` the one leaving. Automatic
    /// updates never change them.
    Manual { before: [f32; 2], after: [f32; 2] },
    /// A sharp turn.
    Corner,
}

/// The shape of the segment from a point to the next.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum PathKind {
    #[default]
    Curve,
    Line,
}

/// The timing of the segment from a point to the next.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Timing {
    /// Stay on this point until the next point's frame, then jump.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub hold: bool,
    /// 0 leaves at even speed, 1 starts from a stop (slow out).
    #[serde(default)]
    pub ease_out: f32,
    /// 0 arrives at even speed, 1 comes to a stop (slow in).
    #[serde(default)]
    pub ease_in: f32,
    /// Spacing set by the animator: `[time, distance]` pairs strictly
    /// inside the segment, both 0 to 1. Distance never goes back.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub spacing: Vec<[f32; 2]>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChartPoint {
    pub id: PointId,
    /// Timeline frame, 1-based like keys. One point per frame.
    pub frame: u32,
    /// Stage coordinates.
    pub pos: [f32; 2],
    #[serde(default)]
    pub role: Role,
    #[serde(default)]
    pub tangent: Tangent,
    #[serde(default)]
    pub path: PathKind,
    #[serde(default)]
    pub timing: Timing,
    /// The drawing exposed when the point was placed, so retiming can
    /// tell a point tied to a drawing from one tied to a frame.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub drawing: Option<Id>,
}

fn unit_scale() -> [Value; 2] {
    [Value::Constant(1.0), Value::Constant(1.0)]
}

fn yes() -> bool {
    true
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Chart {
    /// Sorted by frame, one per frame.
    #[serde(default)]
    pub points: Vec<ChartPoint>,
    #[serde(default = "unit_scale")]
    pub scale: [Value; 2],
    /// Degrees. Past 360 is more turns, not the same angle.
    #[serde(default)]
    pub rotation: Value,
    /// Display colour; `None` uses the chart's place in the scene.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub colour: Option<[u8; 3]>,
    /// Hiding only hides the overlay; what the chart drives is unchanged.
    #[serde(default = "yes")]
    pub visible: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub locked: bool,
    #[serde(default)]
    next_point: u32,
}

impl Default for Chart {
    fn default() -> Self {
        Chart {
            points: Vec::new(),
            scale: unit_scale(),
            rotation: Value::Constant(0.0),
            colour: None,
            visible: true,
            locked: false,
            next_point: 0,
        }
    }
}

impl Chart {
    pub fn point(&self, id: PointId) -> Option<&ChartPoint> {
        self.points.iter().find(|p| p.id == id)
    }

    pub fn point_mut(&mut self, id: PointId) -> Option<&mut ChartPoint> {
        self.points.iter_mut().find(|p| p.id == id)
    }

    pub fn point_on(&self, frame: u32) -> Option<&ChartPoint> {
        self.points.iter().find(|p| p.frame == frame)
    }

    /// The frames the chart spans (first and last point).
    pub fn range(&self) -> Option<(u32, u32)> {
        Some((self.points.first()?.frame, self.points.last()?.frame))
    }

    fn fresh_id(&mut self) -> PointId {
        // Past ids may have come from a newer save; never reuse one.
        let used = self.points.iter().map(|p| p.id.0 + 1).max().unwrap_or(0);
        self.next_point = self.next_point.max(used);
        let id = PointId(self.next_point);
        self.next_point += 1;
        id
    }

    /// Place the chart at `pos` on `frame`. A point already on that frame
    /// moves there (keeping its tangent and timing); otherwise a new point
    /// is added, which extends the chart when past either end.
    pub fn set_point(&mut self, frame: u32, pos: [f32; 2], role: Role) -> PointId {
        let frame = frame.max(1);
        if let Some(p) = self.points.iter_mut().find(|p| p.frame == frame) {
            p.pos = pos;
            return p.id;
        }
        let id = self.fresh_id();
        let at = self.points.partition_point(|p| p.frame < frame);
        // A point added inside a held segment holds too, so the hold the
        // animator set keeps its meaning on both sides.
        let timing = match at.checked_sub(1).and_then(|i| self.points.get(i)) {
            Some(prev) if prev.timing.hold && at < self.points.len() => Timing {
                hold: true,
                ..Timing::default()
            },
            _ => Timing::default(),
        };
        self.points.insert(
            at,
            ChartPoint {
                id,
                frame,
                pos,
                role,
                tangent: Tangent::Auto,
                path: PathKind::Curve,
                timing,
                drawing: None,
            },
        );
        id
    }

    pub fn remove_point(&mut self, id: PointId) -> Option<ChartPoint> {
        let i = self.points.iter().position(|p| p.id == id)?;
        Some(self.points.remove(i))
    }

    /// Add a point on `frame`, between two points, exactly where the chart
    /// already is, without changing the path or the timing: the path is
    /// split where it is on that frame (its neighbours' handles are kept
    /// as they were), and each half's timing follows the old one.
    pub fn insert_on_curve(&mut self, frame: u32, role: Role) -> Option<PointId> {
        let curve = ChartCurve::new(self);
        let seg = curve
            .segments
            .iter()
            .position(|s| s.from < frame && frame < s.to)?;
        let s = &curve.segments[seg];
        let t = (frame - s.from) as f32 / (s.to - s.from) as f32;
        let d = s.timing.at(t);
        let u = s.param_at(d);
        let [c0, c1, c2, c3] = s.c;
        let lerp = |a: [f32; 2], b: [f32; 2]| [a[0] + (b[0] - a[0]) * u, a[1] + (b[1] - a[1]) * u];
        let (a, b, c) = (lerp(c0, c1), lerp(c1, c2), lerp(c2, c3));
        let (ab, bc) = (lerp(a, b), lerp(b, c));
        let mid = lerp(ab, bc);
        let sub = |p: [f32; 2], q: [f32; 2]| [p[0] - q[0], p[1] - q[1]];
        // Keep every other segment as it is: the handles the neighbours
        // had become theirs to keep.
        let handles = curve.handles.clone();
        let (left, right) = (seg, seg + 1);
        let old = self.points[left].timing.clone();
        let line = self.points[left].path == PathKind::Line;
        for (i, (before, after)) in [(left, handles[left]), (right, handles[right])] {
            if self.points[i].tangent != Tangent::Corner {
                self.points[i].tangent = Tangent::Manual { before, after };
            }
        }
        if let Tangent::Manual { after, .. } = &mut self.points[left].tangent {
            *after = sub(a, c0);
        }
        if let Tangent::Manual { before, .. } = &mut self.points[right].tangent {
            *before = sub(c, c3);
        }
        let id = self.fresh_id();
        let timing = |from: f32, to: f32| split_timing(&s.timing, &old, from, to);
        let (first, second) = if old.hold {
            let held = Timing {
                hold: true,
                ..Timing::default()
            };
            (held.clone(), held)
        } else {
            (timing(0.0, t), timing(t, 1.0))
        };
        self.points[left].timing = first;
        self.points.insert(
            right,
            ChartPoint {
                id,
                frame,
                pos: mid,
                role,
                tangent: if line {
                    Tangent::Corner
                } else {
                    Tangent::Manual {
                        before: sub(ab, mid),
                        after: sub(bc, mid),
                    }
                },
                path: self.points[left].path,
                timing: second,
                drawing: None,
            },
        );
        Some(id)
    }

    /// Key the rotation on `frame`, taking the short way round from the
    /// angle it had there (so 350 then 10 is a 20 degree turn). For more
    /// turns on purpose, use [`Chart::set_rotation_turns`].
    pub fn set_rotation(&mut self, frame: u32, degrees: f32) {
        // The first key is what was asked; later ones turn the short way
        // from the angle the keys already give that frame.
        if self.rotation.keys().is_empty() {
            self.rotation.set_key(frame, degrees);
            return;
        }
        let current = self.rotation.at(frame);
        let turn = ((degrees - current) / 360.0).round() * 360.0;
        self.rotation.set_key(frame, degrees - turn);
    }

    /// Key the rotation on `frame` exactly: 720 is two full turns.
    pub fn set_rotation_turns(&mut self, frame: u32, degrees: f32) {
        self.rotation.set_key(frame, degrees);
    }

    pub fn set_scale(&mut self, frame: u32, scale: [f32; 2]) {
        self.scale[0].set_key(frame, scale[0]);
        self.scale[1].set_key(frame, scale[1]);
    }
}

/// The left or right part (time `from` to `to`) of a segment's timing,
/// rescaled to a whole segment of its own.
fn split_timing(map: &TimingMap, old: &Timing, from: f32, to: f32) -> Timing {
    let (d0, d1) = (map.at(from), map.at(to));
    let span = (d1 - d0).max(1e-6);
    const N: usize = 6;
    let spacing = (1..N)
        .map(|i| {
            let t = i as f32 / N as f32;
            [
                t,
                ((map.at(from + (to - from) * t) - d0) / span).clamp(0.0, 1.0),
            ]
        })
        .collect();
    // End slopes, in this part's own units, give its eases.
    let secant = (to - from) / span;
    let ease = |slope: f32| (1.0 - slope * secant).clamp(0.0, 1.0);
    Timing {
        hold: false,
        ease_out: if from == 0.0 {
            old.ease_out
        } else {
            ease(map.slope(from))
        },
        ease_in: if to == 1.0 {
            old.ease_in
        } else {
            ease(map.slope(to))
        },
        spacing,
    }
}

/// A segment's timing, ready to evaluate: time to distance, monotone.
#[derive(Clone, Debug)]
struct TimingMap {
    hold: bool,
    /// (time, distance, slope).
    knots: Vec<[f32; 3]>,
}

impl TimingMap {
    fn new(t: &Timing) -> Self {
        let mut knots: Vec<[f32; 3]> = vec![[0.0, 0.0, 0.0]];
        let mut spacing: Vec<[f32; 2]> = t
            .spacing
            .iter()
            .copied()
            .filter(|[a, b]| a.is_finite() && b.is_finite() && *a > 0.0 && *a < 1.0)
            .collect();
        spacing.sort_by(|a, b| a[0].total_cmp(&b[0]));
        for [time, dist] in spacing {
            let last = *knots.last().unwrap_or(&[0.0; 3]);
            if time - last[0] > 1e-4 {
                knots.push([time, dist.clamp(last[1], 1.0), 0.0]);
            }
        }
        if 1.0 - knots.last().map_or(0.0, |k| k[0]) < 1e-4 && knots.len() > 1 {
            knots.pop();
        }
        knots.push([1.0, 1.0, 0.0]);
        // Fritsch-Carlson: slopes that keep the curve from going back.
        let n = knots.len();
        let delta: Vec<f32> = knots
            .windows(2)
            .map(|w| (w[1][1] - w[0][1]) / (w[1][0] - w[0][0]))
            .collect();
        for k in 1..n - 1 {
            knots[k][2] = if delta[k - 1] * delta[k] <= 0.0 {
                0.0
            } else {
                (delta[k - 1] + delta[k]) * 0.5
            };
        }
        knots[0][2] = delta[0] * (1.0 - t.ease_out.clamp(0.0, 1.0));
        knots[n - 1][2] = delta[n - 2] * (1.0 - t.ease_in.clamp(0.0, 1.0));
        for k in 0..n - 1 {
            if delta[k] == 0.0 {
                knots[k][2] = 0.0;
                knots[k + 1][2] = 0.0;
                continue;
            }
            let (a, b) = (knots[k][2] / delta[k], knots[k + 1][2] / delta[k]);
            let r = a * a + b * b;
            if r > 9.0 {
                let tau = 3.0 / r.sqrt();
                knots[k][2] = tau * a * delta[k];
                knots[k + 1][2] = tau * b * delta[k];
            }
        }
        TimingMap {
            hold: t.hold,
            knots,
        }
    }

    /// Distance (0 to 1) at time `t` (0 to 1).
    fn at(&self, t: f32) -> f32 {
        if self.hold {
            return if t >= 1.0 { 1.0 } else { 0.0 };
        }
        let t = t.clamp(0.0, 1.0);
        let k = self
            .knots
            .windows(2)
            .position(|w| t <= w[1][0])
            .unwrap_or(self.knots.len() - 2);
        let ([t0, s0, m0], [t1, s1, m1]) = (self.knots[k], self.knots[k + 1]);
        let h = t1 - t0;
        let x = (t - t0) / h;
        let (x2, x3) = (x * x, x * x * x);
        let s = (2.0 * x3 - 3.0 * x2 + 1.0) * s0
            + (x3 - 2.0 * x2 + x) * h * m0
            + (-2.0 * x3 + 3.0 * x2) * s1
            + (x3 - x2) * h * m1;
        s.clamp(0.0, 1.0)
    }

    fn slope(&self, t: f32) -> f32 {
        let h = 1e-3;
        (self.at((t + h).min(1.0)) - self.at((t - h).max(0.0)))
            / ((t + h).min(1.0) - (t - h).max(0.0))
    }
}

/// Samples per segment in the arc-length table.
const LUT: usize = 64;

#[derive(Clone, Debug)]
struct Segment {
    from: u32,
    to: u32,
    c: [[f32; 2]; 4],
    /// Arc length from the start at `i / LUT` of the curve parameter.
    lengths: [f32; LUT + 1],
    timing: TimingMap,
}

fn bezier(c: &[[f32; 2]; 4], u: f32) -> [f32; 2] {
    let v = 1.0 - u;
    let (a, b, cc, d) = (v * v * v, 3.0 * v * v * u, 3.0 * v * u * u, u * u * u);
    [
        a * c[0][0] + b * c[1][0] + cc * c[2][0] + d * c[3][0],
        a * c[0][1] + b * c[1][1] + cc * c[2][1] + d * c[3][1],
    ]
}

impl Segment {
    fn length(&self) -> f32 {
        self.lengths[LUT]
    }

    /// The curve parameter at `d` (0 to 1) of the arc length, so even
    /// timing is even speed, however the curve is parameterised.
    fn param_at(&self, d: f32) -> f32 {
        let total = self.length();
        if total <= 1e-6 {
            return d.clamp(0.0, 1.0);
        }
        let want = d.clamp(0.0, 1.0) * total;
        let i = self.lengths.partition_point(|&l| l < want).clamp(1, LUT);
        let (a, b) = (self.lengths[i - 1], self.lengths[i]);
        let f = if b > a { (want - a) / (b - a) } else { 0.0 };
        ((i - 1) as f32 + f) / LUT as f32
    }

    fn at(&self, d: f32) -> [f32; 2] {
        bezier(&self.c, self.param_at(d))
    }
}

/// What a chart says about one moment: everything a consumer may need.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MotionSample {
    pub position: [f32; 2],
    /// Stage units per frame.
    pub velocity: [f32; 2],
    pub speed: f32,
    /// Unit direction of travel; the last known one while still.
    pub direction: [f32; 2],
    pub scale: [f32; 2],
    /// Degrees, turns included.
    pub rotation: f32,
    /// Degrees per frame.
    pub angular_velocity: f32,
    /// 0 to 1 through the chart's frames.
    pub progress: f32,
    /// Stage units travelled along the path since the first point.
    pub distance: f32,
    /// Position, rotation and scale as one transform (scale, then
    /// rotate, then move).
    pub transform: Affine,
}

/// A chart made ready to evaluate. Cheap to build (a few hundred curve
/// evaluations for a long chart); callers that sample often keep one per
/// chart and scene version.
#[derive(Clone, Debug)]
pub struct ChartCurve {
    first: Option<(u32, [f32; 2])>,
    last: u32,
    segments: Vec<Segment>,
    /// Each point's handles as used (before, after), auto ones included.
    handles: Vec<([f32; 2], [f32; 2])>,
    scale: [Value; 2],
    rotation: Value,
}

impl ChartCurve {
    pub fn new(chart: &Chart) -> Self {
        // Defend against hand-edited files: sorted, one point per frame.
        let mut pts: Vec<&ChartPoint> = chart.points.iter().collect();
        pts.sort_by_key(|p| p.frame);
        pts.dedup_by_key(|p| p.frame);
        let handles = auto_handles(&pts);
        let segments = pts
            .windows(2)
            .enumerate()
            .map(|(i, w)| {
                let (a, b) = (w[0], w[1]);
                let c = if a.path == PathKind::Line {
                    let third = |k: f32| {
                        [
                            a.pos[0] + (b.pos[0] - a.pos[0]) * k,
                            a.pos[1] + (b.pos[1] - a.pos[1]) * k,
                        ]
                    };
                    [a.pos, third(1.0 / 3.0), third(2.0 / 3.0), b.pos]
                } else {
                    let (out, inn) = (handles[i].1, handles[i + 1].0);
                    [
                        a.pos,
                        [a.pos[0] + out[0], a.pos[1] + out[1]],
                        [b.pos[0] + inn[0], b.pos[1] + inn[1]],
                        b.pos,
                    ]
                };
                let mut lengths = [0.0; LUT + 1];
                let mut prev = c[0];
                for k in 1..=LUT {
                    let p = bezier(&c, k as f32 / LUT as f32);
                    lengths[k] = lengths[k - 1] + (p[0] - prev[0]).hypot(p[1] - prev[1]);
                    prev = p;
                }
                Segment {
                    from: a.frame,
                    to: b.frame,
                    c,
                    lengths,
                    timing: TimingMap::new(&a.timing),
                }
            })
            .collect();
        ChartCurve {
            first: pts.first().map(|p| (p.frame, p.pos)),
            last: pts.last().map_or(0, |p| p.frame),
            segments,
            handles,
            scale: chart.scale.clone(),
            rotation: chart.rotation.clone(),
        }
    }

    pub fn range(&self) -> Option<(u32, u32)> {
        self.first.map(|(f, _)| (f, self.last))
    }

    /// Where the chart is at `frame` (fractional frames work), holding
    /// before its first point and after its last.
    pub fn position(&self, frame: f32) -> Option<[f32; 2]> {
        let (first, start) = self.first?;
        if frame <= first as f32 || self.segments.is_empty() {
            return Some(start);
        }
        let i = self
            .segments
            .partition_point(|s| (s.to as f32) < frame)
            .min(self.segments.len() - 1);
        let s = &self.segments[i];
        let t = ((frame - s.from as f32) / (s.to - s.from) as f32).clamp(0.0, 1.0);
        Some(s.at(s.timing.at(t)))
    }

    fn distance(&self, frame: f32) -> f32 {
        let mut total = 0.0;
        for s in &self.segments {
            if frame >= s.to as f32 {
                total += s.length();
            } else if frame > s.from as f32 {
                let t = (frame - s.from as f32) / (s.to - s.from) as f32;
                // Arc length is the table's, by parameter.
                let u = s.param_at(s.timing.at(t));
                let x = u * LUT as f32;
                let i = (x.floor() as usize).min(LUT - 1);
                total += s.lengths[i] + (s.lengths[i + 1] - s.lengths[i]) * (x - i as f32);
                break;
            } else {
                break;
            }
        }
        total
    }

    /// Everything about `frame` (fractional frames work).
    pub fn sample(&self, frame: f32) -> Option<MotionSample> {
        let position = self.position(frame)?;
        let h = 0.01;
        let (a, b) = (self.position(frame - h)?, self.position(frame + h)?);
        let velocity = [(b[0] - a[0]) / (2.0 * h), (b[1] - a[1]) / (2.0 * h)];
        let speed = velocity[0].hypot(velocity[1]);
        let direction = if speed > 1e-4 {
            [velocity[0] / speed, velocity[1] / speed]
        } else {
            self.last_direction(frame)
        };
        let scale = [self.scale[0].at_f(frame), self.scale[1].at_f(frame)];
        let rotation = self.rotation.at_f(frame);
        let angular_velocity =
            (self.rotation.at_f(frame + h) - self.rotation.at_f(frame - h)) / (2.0 * h);
        let (first, last) = self.range()?;
        let progress = if last > first {
            ((frame - first as f32) / (last - first) as f32).clamp(0.0, 1.0)
        } else {
            1.0
        };
        let transform = Affine::translate(position[0], position[1])
            .compose(Affine::rotate_deg(rotation))
            .compose(Affine::scale(scale[0], scale[1]));
        Some(MotionSample {
            position,
            velocity,
            speed,
            direction,
            scale,
            rotation,
            angular_velocity,
            progress,
            distance: self.distance(frame),
            transform,
        })
    }

    /// The way the chart was last going at or before `frame`.
    fn last_direction(&self, frame: f32) -> [f32; 2] {
        for s in self.segments.iter().rev() {
            if (s.from as f32) < frame || s.from == self.first.map_or(0, |f| f.0) {
                let (a, b) = (s.at(0.98), s.at(1.0));
                let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
                let len = dx.hypot(dy);
                if len > 1e-6 {
                    return [dx / len, dy / len];
                }
            }
        }
        [1.0, 0.0]
    }

    /// The whole path as a polyline (for drawing), `per_segment` points a
    /// segment.
    pub fn path(&self, per_segment: usize) -> Vec<[f32; 2]> {
        let n = per_segment.max(2);
        let mut out = Vec::with_capacity(self.segments.len() * n + 1);
        if let Some((_, p)) = self.first {
            out.push(p);
        }
        for s in &self.segments {
            out.extend((1..=n).map(|i| bezier(&s.c, i as f32 / n as f32)));
        }
        out
    }

    /// Each point's Bézier handles as offsets (before, after), automatic
    /// ones as computed. For drawing handles and for freezing a shape.
    pub fn handles(&self) -> &[([f32; 2], [f32; 2])] {
        &self.handles
    }
}

/// Every point's handles: manual ones as set, corners flat, automatic
/// ones from centripetal Catmull-Rom over the neighbours on curved
/// segments (a straight segment or a corner ends the smooth run).
fn auto_handles(pts: &[&ChartPoint]) -> Vec<([f32; 2], [f32; 2])> {
    let n = pts.len();
    let dist = |a: [f32; 2], b: [f32; 2]| (b[0] - a[0]).hypot(b[1] - a[1]);
    // Centripetal knot spacing; never zero, so coincident points are safe.
    let knot = |a: [f32; 2], b: [f32; 2]| dist(a, b).sqrt().max(1e-3);
    (0..n)
        .map(|i| {
            let p = pts[i].pos;
            match pts[i].tangent {
                Tangent::Manual { before, after } => (before, after),
                Tangent::Corner => ([0.0; 2], [0.0; 2]),
                Tangent::Auto => {
                    let prev =
                        (i > 0 && pts[i - 1].path == PathKind::Curve).then(|| pts[i - 1].pos);
                    let next =
                        (i + 1 < n && pts[i].path == PathKind::Curve).then(|| pts[i + 1].pos);
                    // An open end mirrors its one neighbour, so the path
                    // leaves straight toward it.
                    let (a, b) = match (prev, next) {
                        (Some(a), Some(b)) => (a, b),
                        (Some(a), None) => (a, [2.0 * p[0] - a[0], 2.0 * p[1] - a[1]]),
                        (None, Some(b)) => ([2.0 * p[0] - b[0], 2.0 * p[1] - b[1]], b),
                        (None, None) => return ([0.0; 2], [0.0; 2]),
                    };
                    let (d0, d1) = (knot(a, p), knot(p, b));
                    // Barry-Goldman tangent at p, per unit of knot.
                    let m = |k: usize| {
                        (p[k] - a[k]) / d0 - (b[k] - a[k]) / (d0 + d1) + (b[k] - p[k]) / d1
                    };
                    let m = [m(0), m(1)];
                    // A Bézier handle is a third of the tangent times the
                    // segment's knot span.
                    let before = [-m[0] * d0 / 3.0, -m[1] * d0 / 3.0];
                    let after = [m[0] * d1 / 3.0, m[1] * d1 / 3.0];
                    (before, after)
                }
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: [f32; 2], b: [f32; 2], tol: f32) -> bool {
        (a[0] - b[0]).abs() <= tol && (a[1] - b[1]).abs() <= tol
    }

    fn three() -> Chart {
        let mut c = Chart::default();
        c.set_point(1, [0.0, 0.0], Role::Key);
        c.set_point(12, [100.0, -60.0], Role::Key);
        c.set_point(24, [200.0, 0.0], Role::Key);
        c
    }

    #[test]
    fn the_chart_is_on_each_point_at_its_frame_and_holds_outside() {
        let c = three();
        let curve = ChartCurve::new(&c);
        for p in &c.points {
            assert!(close(curve.position(p.frame as f32).unwrap(), p.pos, 1e-3));
        }
        assert_eq!(curve.position(-5.0), Some([0.0, 0.0]));
        assert!(close(curve.position(90.0).unwrap(), [200.0, 0.0], 1e-3));
        assert_eq!(ChartCurve::new(&Chart::default()).position(3.0), None);
    }

    #[test]
    fn the_path_is_smooth_through_a_middle_point() {
        let c = three();
        let curve = ChartCurve::new(&c);
        let (before, after) = curve.handles()[1];
        // Handles on one line through the point, pointing apart.
        let cross = before[0] * after[1] - before[1] * after[0];
        let dot = before[0] * after[0] + before[1] * after[1];
        assert!(cross.abs() < 1e-3 && dot < 0.0, "{before:?} {after:?}");
        // At the top of an arc the path runs level.
        assert!(before[1].abs() < 1e-3);
        // And it arcs: halfway between the first two keys, it's above
        // the straight line between them.
        let mid = curve.position(6.5).unwrap();
        assert!(mid[1] < -30.0 - 1.0, "{mid:?}");
    }

    #[test]
    fn even_timing_is_even_speed_along_the_arc() {
        let mut c = Chart::default();
        c.set_point(1, [0.0, 0.0], Role::Key);
        c.set_point(11, [100.0, 0.0], Role::Key);
        c.points[0].tangent = Tangent::Manual {
            before: [0.0; 2],
            after: [90.0, 80.0],
        };
        let curve = ChartCurve::new(&c);
        let steps: Vec<f32> = (1..11)
            .map(|f| curve.sample(f as f32 + 0.5).unwrap().speed)
            .collect();
        let (lo, hi) = steps
            .iter()
            .fold((f32::MAX, 0.0f32), |(a, b), &s| (a.min(s), b.max(s)));
        assert!(hi - lo < hi * 0.05, "uneven: {steps:?}");
    }

    #[test]
    fn uneven_frame_gaps_keep_each_segment_on_its_own_schedule() {
        let mut c = Chart::default();
        c.set_point(1, [0.0, 0.0], Role::Key);
        c.set_point(3, [10.0, 0.0], Role::Key);
        c.set_point(43, [20.0, 0.0], Role::Key);
        c.points[1].tangent = Tangent::Corner;
        let curve = ChartCurve::new(&c);
        assert!((curve.position(2.0).unwrap()[0] - 5.0).abs() < 0.1);
        assert!((curve.position(23.0).unwrap()[0] - 15.0).abs() < 0.1);
    }

    #[test]
    fn adding_a_point_extends_the_chart_and_only_reshapes_its_neighbours() {
        let mut c = Chart::default();
        for (f, x, y) in [
            (1, 0.0, 0.0),
            (10, 100.0, -50.0),
            (20, 200.0, 0.0),
            (30, 300.0, -50.0),
        ] {
            c.set_point(f, [x, y], Role::Key);
        }
        let before = ChartCurve::new(&c);
        c.set_point(40, [400.0, 80.0], Role::Key);
        let after = ChartCurve::new(&c);
        assert_eq!(after.range(), Some((1, 40)));
        // Far from the new point: the same path.
        for f in [2.0, 5.0, 9.0, 12.0, 18.0] {
            assert!(close(
                before.position(f).unwrap(),
                after.position(f).unwrap(),
                1e-3
            ));
        }
        // Next to it: the last old segment now bends toward it.
        assert!(!close(
            before.position(25.0).unwrap(),
            after.position(25.0).unwrap(),
            0.5
        ));
    }

    #[test]
    fn manual_handles_survive_automatic_updates() {
        let mut c = three();
        let id = c.points[1].id;
        let set = Tangent::Manual {
            before: [-5.0, 20.0],
            after: [30.0, 0.0],
        };
        c.point_mut(id).unwrap().tangent = set;
        c.set_point(36, [300.0, 50.0], Role::Key);
        c.set_point(12, [110.0, -60.0], Role::Key);
        assert_eq!(c.point(id).unwrap().tangent, set);
        let curve = ChartCurve::new(&c);
        assert_eq!(curve.handles()[1], ([-5.0, 20.0], [30.0, 0.0]));
    }

    #[test]
    fn inserting_on_the_curve_keeps_path_and_timing() {
        let mut c = three();
        c.points[0].timing.ease_out = 0.8;
        c.points[1].timing.ease_in = 0.5;
        let before = ChartCurve::new(&c);
        let id = c.insert_on_curve(5, Role::Breakdown).unwrap();
        let p = c.point(id).unwrap();
        assert_eq!((p.frame, p.role), (5, Role::Breakdown));
        let after = ChartCurve::new(&c);
        for f in 1..=24 {
            let (a, b) = (
                before.position(f as f32).unwrap(),
                after.position(f as f32).unwrap(),
            );
            assert!(close(a, b, 0.6), "frame {f}: {a:?} vs {b:?}");
        }
        // Not on a point's frame or outside the chart.
        assert_eq!(c.insert_on_curve(12, Role::Key), None);
        assert_eq!(c.insert_on_curve(30, Role::Key), None);
    }

    #[test]
    fn coincident_points_stay_finite() {
        let mut c = Chart::default();
        c.set_point(1, [5.0, 5.0], Role::Key);
        c.set_point(6, [5.0, 5.0], Role::Key);
        c.set_point(12, [5.0, 5.0], Role::Key);
        c.set_point(18, [50.0, 5.0], Role::Key);
        let curve = ChartCurve::new(&c);
        for f in 0..=40 {
            let s = curve.sample(f as f32 * 0.5).unwrap();
            assert!(s
                .position
                .iter()
                .chain(&s.velocity)
                .chain(&s.direction)
                .all(|v| v.is_finite()));
        }
        assert!(close(curve.position(9.0).unwrap(), [5.0, 5.0], 1e-3));
    }

    #[test]
    fn one_point_per_frame() {
        let mut c = three();
        let id = c.points[1].id;
        assert_eq!(
            c.set_point(12, [1.0, 2.0], Role::Key),
            id,
            "it moves the point there"
        );
        assert_eq!(c.points.len(), 3);
        // A broken file with two points on a frame still evaluates.
        let mut broken = three();
        broken.points.push(broken.points[1].clone());
        assert!(ChartCurve::new(&broken).position(12.0).is_some());
        // Ids are never reused, even after a delete.
        let gone = c.points[2].id;
        c.remove_point(gone);
        assert_ne!(c.set_point(30, [0.0, 0.0], Role::Key), gone);
    }

    #[test]
    fn a_hold_stays_then_jumps() {
        let mut c = three();
        c.points[0].timing.hold = true;
        let curve = ChartCurve::new(&c);
        assert_eq!(curve.position(11.9), Some([0.0, 0.0]));
        assert_eq!(curve.sample(6.0).unwrap().speed, 0.0);
        assert!(close(curve.position(12.0).unwrap(), [100.0, -60.0], 1e-3));
        // A point added inside the hold holds too.
        c.set_point(6, [0.0, 0.0], Role::Breakdown);
        assert!(c.point_on(6).unwrap().timing.hold);
    }

    #[test]
    fn timing_and_path_edit_independently() {
        let c = three();
        let even = ChartCurve::new(&c);
        // Easing: same path, different spacing.
        let mut eased = c.clone();
        eased.points[0].timing.ease_out = 1.0;
        eased.points[0].timing.spacing = vec![[0.5, 0.2]];
        let e = ChartCurve::new(&eased);
        assert_eq!(e.path(16), even.path(16), "timing never moves the path");
        assert!(e.sample(2.0).unwrap().distance < even.sample(2.0).unwrap().distance);
        // The spacing knot is met: halfway in time, a fifth of the way.
        let seg_len = e.segments[0].length();
        assert!((e.sample(6.5).unwrap().distance / seg_len - 0.2).abs() < 1e-3);
        // Reshaping: different path, same fractions of the way.
        let mut bent = eased.clone();
        bent.points[1].pos = [100.0, -90.0];
        let b = ChartCurve::new(&bent);
        assert_ne!(b.path(16), e.path(16));
        let fraction =
            |curve: &ChartCurve, f| curve.sample(f).unwrap().distance / curve.segments[0].length();
        assert!((fraction(&b, 4.0) - fraction(&e, 4.0)).abs() < 1e-3);
    }

    #[test]
    fn spacing_never_goes_back() {
        let t = Timing {
            spacing: vec![[0.3, 0.6], [0.5, 0.4], [0.7, 0.9], [2.0, 0.1], [0.7, 0.2]],
            ..Timing::default()
        };
        let map = TimingMap::new(&t);
        let mut last = 0.0;
        for i in 0..=100 {
            let d = map.at(i as f32 / 100.0);
            assert!(d + 1e-6 >= last, "went back at {i}: {d} < {last}");
            last = d;
        }
        assert_eq!(map.at(1.0), 1.0);
    }

    #[test]
    fn scale_and_rotation_are_their_own_channels() {
        let mut c = three();
        c.set_scale(1, [1.0, 1.0]);
        c.set_scale(24, [2.0, 0.5]);
        c.set_rotation(1, 0.0);
        c.set_rotation(24, 90.0);
        let curve = ChartCurve::new(&c);
        let s = curve.sample(12.5).unwrap();
        assert!((s.scale[0] - 1.5).abs() < 1e-3 && (s.scale[1] - 0.75).abs() < 1e-3);
        assert!((s.rotation - 45.0).abs() < 1e-3);
        assert!((s.angular_velocity - 90.0 / 23.0).abs() < 1e-2);
        // The path is untouched by them.
        assert_eq!(curve.path(8), ChartCurve::new(&three()).path(8));
        let (x, y) = s.transform.apply(0.0, 0.0);
        assert!(close([x, y], s.position, 1e-3));
    }

    #[test]
    fn rotation_takes_the_short_way_unless_told_to_turn() {
        let mut c = three();
        c.set_rotation(1, 350.0);
        c.set_rotation(12, 10.0);
        assert_eq!(c.rotation.at(12), 370.0, "20 degrees on, not 340 back");
        c.set_rotation_turns(24, 370.0 + 720.0);
        let curve = ChartCurve::new(&c);
        assert_eq!(curve.sample(24.0).unwrap().rotation, 1090.0);
    }

    #[test]
    fn fractional_frames_sit_between_whole_ones() {
        let c = three();
        let curve = ChartCurve::new(&c);
        let (a, m, b) = (
            curve.position(5.0).unwrap(),
            curve.position(5.5).unwrap(),
            curve.position(6.0).unwrap(),
        );
        assert!(m[0] > a[0] && m[0] < b[0]);
        let s = curve.sample(5.5).unwrap();
        assert!((s.progress - 4.5 / 23.0).abs() < 1e-5);
        assert!(s.distance > 0.0);
    }

    #[test]
    fn charts_save_and_load_unchanged() {
        let mut c = three();
        c.points[0].timing.ease_out = 0.4;
        c.points[0].timing.spacing = vec![[0.25, 0.1]];
        c.points[1].tangent = Tangent::Corner;
        c.points[2].role = Role::Breakdown;
        c.set_rotation(12, 400.0);
        c.colour = Some([200, 90, 40]);
        let json = serde_json::to_string(&c).unwrap();
        let back: Chart = serde_json::from_str(&json).unwrap();
        assert_eq!(back, c);
        let (a, b) = (ChartCurve::new(&c), ChartCurve::new(&back));
        for f in 0..50 {
            assert_eq!(a.sample(f as f32 * 0.5), b.sample(f as f32 * 0.5));
        }
        // A minimal chart (as a future or older file might hold) loads
        // with defaults.
        let bare: Chart =
            serde_json::from_str(r#"{"points":[{"id":0,"frame":1,"pos":[1,2]}]}"#).unwrap();
        assert!(bare.visible && bare.scale[0] == Value::Constant(1.0));
    }
}
