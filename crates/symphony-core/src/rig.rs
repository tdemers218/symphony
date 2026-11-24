//! Rest-pose bone bindings and analytic two-bone inverse kinematics.

use serde::{Deserialize, Serialize};

use crate::{anim::Value, math::Affine};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Bone {
    pub length: f32,
    pub origin: [f32; 2],
    pub rest_angle: f32,
    pub rotation: Value,
    pub bend_radius: f32,
    pub smart: Option<SmartJoint>,
}

impl Default for Bone {
    fn default() -> Self {
        Self {
            length: 160.0,
            origin: [0.0; 2],
            rest_angle: 0.0,
            rotation: Value::default(),
            bend_radius: 28.0,
            smart: None,
        }
    }
}

impl Bone {
    pub fn length(&self) -> f32 {
        self.length.clamp(1.0, 100_000.0)
    }

    pub fn values(&self) -> impl Iterator<Item = &Value> {
        std::iter::once(&self.rotation)
            .chain(self.smart.iter().flat_map(|joint| joint.target.iter()))
    }

    pub fn values_mut(&mut self) -> impl Iterator<Item = &mut Value> {
        std::iter::once(&mut self.rotation).chain(
            self.smart
                .iter_mut()
                .flat_map(|joint| joint.target.iter_mut()),
        )
    }

    pub fn has_key(&self, frame: u32) -> bool {
        self.values().any(|value| value.has_key(frame))
    }
}

/// Stored on the lower bone. The target is relative to the upper bone's
/// start, in the upper bone's rest axes before either solved rotation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SmartJoint {
    pub target: [Value; 2],
    pub flip: bool,
}

impl SmartJoint {
    pub fn solve(&self, frame: u32, upper: f32, lower: f32) -> [f32; 2] {
        let [x, y] = self.target.each_ref().map(|value| value.at(frame));
        let reach = x.hypot(y).clamp((upper - lower).abs(), upper + lower);
        let cosine = ((reach * reach - upper * upper - lower * lower) / (2.0 * upper * lower))
            .clamp(-1.0, 1.0);
        let bend = cosine.acos() * if self.flip { -1.0 } else { 1.0 };
        let aim = if x == 0.0 && y == 0.0 {
            0.0
        } else {
            y.atan2(x)
        };
        let correction = (lower * bend.sin()).atan2(upper + lower * bend.cos());
        [(aim - correction).to_degrees(), bend.to_degrees()]
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BonePose {
    pub rest: Affine,
    pub posed: Affine,
    pub parent_skin: Affine,
    pub length: f32,
}

impl BonePose {
    pub fn tip(self) -> (f32, f32) {
        self.posed.apply(self.length, 0.0)
    }

    pub fn skin(self) -> Affine {
        if self.posed == self.rest {
            Affine::IDENTITY
        } else {
            self.posed
                .compose(self.rest.inverse().unwrap_or(Affine::IDENTITY))
        }
    }
}

/// Two rigid transformations blended around the selected bone's rest joint.
/// Away from this band each side follows its own bone at full weight.
#[derive(Clone, Debug, PartialEq)]
pub struct BoneField {
    pub rest_inverse: Affine,
    pub parent: Affine,
    pub child: Affine,
    pub radius: f32,
}

impl BoneField {
    pub fn apply(&self, p: [f32; 2]) -> [f32; 2] {
        let (x, _) = self.rest_inverse.apply(p[0], p[1]);
        let (cx, cy) = self.child.apply(p[0], p[1]);
        if self.radius <= 0.0 || x >= self.radius {
            return [cx, cy];
        }
        let t = (0.5 + x / (2.0 * self.radius)).clamp(0.0, 1.0);
        let w = t * t * (3.0 - 2.0 * t);
        let (px, py) = self.parent.apply(p[0], p[1]);
        [px + (cx - px) * w, py + (cy - py) * w]
    }
}
