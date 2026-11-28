//! Dynamic swatches (docs/plans/colour-system.md, Phase 2): a swatch can
//! carry behaviours that vary its colour as it's painted, such as a little
//! natural variation or darkening with pressure. A swatch without dynamics
//! is the plain colour it always was, and never reaches this code.
//!
//! Everything here is deterministic. A stroke's random values come from
//! its own id and its swatch's id through a fixed hash (`splitmix64`, not
//! std's `DefaultHasher`, which may change between Rust versions), so
//! reopening, undoing, rendering or playing back gives the same colours,
//! with nothing extra saved. "Reroll" changes `seed_offset`.

use serde::{Deserialize, Serialize};

use crate::colour::Oklch;
use crate::id::Id;

/// When a swatch's variation picks new values.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Frequency {
    /// One colour per stroke or fill.
    #[default]
    PerStroke,
    /// A new colour for every dab of a stamped brush.
    PerDab,
    /// Drifting smoothly along the stroke.
    Continuous,
}

/// One way a swatch's colour changes. Amounts are in OKLCH: lightness
/// 0..=1, chroma as a fraction of the base chroma, hue in degrees.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Kind {
    /// Random variation up to these amounts either way.
    Variation {
        lightness: f32,
        chroma: f32,
        hue: f32,
        /// How much opacity may drop, 0..=1.
        opacity: f32,
    },
    /// At full pressure the colour moves by these amounts; at none, not at
    /// all.
    Pressure {
        lightness: f32,
        chroma: f32,
        hue: f32,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Behaviour {
    pub kind: Kind,
    /// The one slider most artists need: 0 does nothing, 1 is the
    /// behaviour's full range.
    pub amount: f32,
    #[serde(default = "yes")]
    pub enabled: bool,
}

fn yes() -> bool {
    true
}

impl Behaviour {
    /// Natural variation: a slight, painterly wobble in every channel.
    pub fn natural() -> Self {
        Behaviour {
            kind: Kind::Variation {
                lightness: 0.06,
                chroma: 0.15,
                hue: 6.0,
                opacity: 0.0,
            },
            amount: 0.5,
            enabled: true,
        }
    }

    /// Pressing harder paints darker and a little richer.
    pub fn pressure_darkens() -> Self {
        Behaviour {
            kind: Kind::Pressure {
                lightness: -0.15,
                chroma: 0.1,
                hue: 0.0,
            },
            amount: 0.5,
            enabled: true,
        }
    }
}

/// A swatch's behaviours, applied in order.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Dynamics {
    pub behaviours: Vec<Behaviour>,
    #[serde(default)]
    pub frequency: Frequency,
    /// How far along the stroke (drawing units) Continuous variation takes
    /// to drift to a new value.
    #[serde(default = "default_scale")]
    pub scale: f32,
    /// Changed by "Reroll" for a different set of random colours.
    #[serde(default)]
    pub seed_offset: u32,
}

fn default_scale() -> f32 {
    120.0
}

// By hand: a derived Default would make `scale` 0 instead of the serde
// default, and Continuous variation would flicker dab to dab.
impl Default for Dynamics {
    fn default() -> Self {
        Dynamics {
            behaviours: Vec::new(),
            frequency: Frequency::default(),
            scale: default_scale(),
            seed_offset: 0,
        }
    }
}

impl Dynamics {
    /// Whether anything would change the colour, so static paths can skip
    /// the evaluator.
    pub fn is_active(&self) -> bool {
        self.behaviours.iter().any(|b| b.enabled && b.amount > 0.0)
    }
}

/// What the input was where the colour is wanted.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Sample {
    /// 0..=1 after the pressure curve.
    pub pressure: f32,
    /// Distance along the stroke so far, in drawing units.
    pub distance: f32,
    /// Which dab of a stamped stroke (0 for lines and fills).
    pub dab: u32,
}

/// The seed for a stroke or fill painted with a swatch.
pub fn seed(item: Id, swatch: Id, offset: u32) -> u64 {
    splitmix64(item.0 ^ splitmix64(swatch.0 ^ (u64::from(offset) << 32)))
}

/// A well-mixed 64-bit hash (Vigna's SplitMix64 finaliser). Fixed forever:
/// changing it would change the colours of every saved drawing.
fn splitmix64(x: u64) -> u64 {
    let mut z = x.wrapping_add(0x9e37_79b9_7f4a_7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

/// A value in -1..=1 from a seed and a key.
fn signed(seed: u64, key: u64) -> f32 {
    let bits = splitmix64(seed ^ splitmix64(key)) >> 40;
    bits as f32 / (1u64 << 24) as f32 * 2.0 - 1.0
}

/// Smooth 1D value noise in -1..=1: random values at whole numbers of `x`,
/// eased between, so neighbouring samples are close.
fn noise(seed: u64, channel: u64, x: f32) -> f32 {
    let cell = x.floor();
    let t = x - cell;
    let t = t * t * (3.0 - 2.0 * t);
    let at = |i: f32| signed(seed, channel << 48 ^ (i as i64 as u64));
    at(cell) * (1.0 - t) + at(cell + 1.0) * t
}

/// The colour of `base` (8-bit sRGB with alpha) under `dynamics` at
/// `sample`, for the stroke or fill with `seed`.
pub fn colour_at(base: [u8; 4], dynamics: &Dynamics, sample: &Sample, seed: u64) -> [u8; 4] {
    if !dynamics.is_active() {
        return base;
    }
    let [r, g, b, a] = base;
    let mut lch = Oklch::from_srgb([r, g, b]);
    let mut alpha = f32::from(a) / 255.0;
    for (index, behaviour) in dynamics.behaviours.iter().enumerate() {
        if !behaviour.enabled || behaviour.amount <= 0.0 {
            continue;
        }
        let amount = behaviour.amount.min(1.0);
        match behaviour.kind {
            Kind::Variation {
                lightness,
                chroma,
                hue,
                opacity,
            } => {
                // Each behaviour and channel draws its own value, so adding a
                // behaviour doesn't reshuffle the others.
                let random = |channel: u64| {
                    let channel = (index as u64) << 8 | channel;
                    match dynamics.frequency {
                        Frequency::PerStroke => signed(seed, channel),
                        Frequency::PerDab => signed(seed, channel << 32 ^ u64::from(sample.dab)),
                        Frequency::Continuous => {
                            noise(seed, channel, sample.distance / dynamics.scale.max(1.0))
                        }
                    }
                };
                lch.l += random(0) * lightness * amount;
                lch.c *= 1.0 + random(1) * chroma * amount;
                lch.h += random(2) * hue * amount;
                alpha *= 1.0 - (random(3) * 0.5 + 0.5) * opacity * amount;
            }
            Kind::Pressure {
                lightness,
                chroma,
                hue,
            } => {
                let p = sample.pressure.clamp(0.0, 1.0) * amount;
                lch.l += lightness * p;
                lch.c *= 1.0 + chroma * p;
                lch.h += hue * p;
            }
        }
    }
    lch.h = lch.h.rem_euclid(360.0);
    let [r, g, b] = lch.to_srgb();
    let a = (alpha.clamp(0.0, 1.0) * 255.0).round() as u8;
    [r, g, b, a]
}

#[cfg(test)]
mod tests {
    use super::*;

    const GREEN: [u8; 4] = [70, 140, 60, 255];

    fn natural(frequency: Frequency) -> Dynamics {
        Dynamics {
            behaviours: vec![Behaviour::natural()],
            frequency,
            ..Default::default()
        }
    }

    fn at(d: &Dynamics, distance: f32, dab: u32, seed: u64) -> [u8; 4] {
        let sample = Sample {
            pressure: 1.0,
            distance,
            dab,
        };
        colour_at(GREEN, d, &sample, seed)
    }

    #[test]
    fn no_behaviours_is_the_plain_colour() {
        let none = Dynamics::default();
        assert_eq!(at(&none, 0.0, 0, 1), GREEN);
        let mut off = natural(Frequency::PerStroke);
        off.behaviours[0].enabled = false;
        assert_eq!(at(&off, 0.0, 0, 1), GREEN);
        off.behaviours[0].enabled = true;
        off.behaviours[0].amount = 0.0;
        assert_eq!(at(&off, 0.0, 0, 1), GREEN);
    }

    #[test]
    fn the_same_stroke_always_gets_the_same_colours() {
        let d = natural(Frequency::PerDab);
        let s = seed(Id(41), Id(3), 0);
        assert_eq!(s, seed(Id(41), Id(3), 0));
        for dab in 0..20 {
            assert_eq!(at(&d, 0.0, dab, s), at(&d, 0.0, dab, s));
        }
        // These values are part of saved drawings' look: if this fails, old
        // scenes would repaint differently.
        assert_eq!(
            seed(Id(1), Id(2), 0),
            0xe06d_d043_328b_d285,
            "seed hash changed"
        );
    }

    #[test]
    fn strokes_differ_and_reroll_changes_them() {
        let d = natural(Frequency::PerStroke);
        let colours: std::collections::HashSet<[u8; 4]> = (0..30)
            .map(|i| at(&d, 0.0, 0, seed(Id(i), Id(3), 0)))
            .collect();
        assert!(colours.len() > 20, "{}", colours.len());
        let a = at(&d, 0.0, 0, seed(Id(7), Id(3), 0));
        let b = at(&d, 0.0, 0, seed(Id(7), Id(3), 1));
        assert_ne!(a, b);
    }

    #[test]
    fn per_stroke_holds_and_per_dab_changes_along_a_stroke() {
        let s = seed(Id(9), Id(3), 0);
        let held = natural(Frequency::PerStroke);
        assert!((0..10).all(|dab| at(&held, dab as f32 * 5.0, dab, s) == at(&held, 0.0, 0, s)));
        let dabs = natural(Frequency::PerDab);
        let seen: std::collections::HashSet<[u8; 4]> =
            (0..10).map(|dab| at(&dabs, 0.0, dab, s)).collect();
        assert!(seen.len() > 5);
    }

    #[test]
    fn continuous_variation_drifts_without_jumps() {
        let d = natural(Frequency::Continuous);
        let s = seed(Id(5), Id(3), 0);
        let mut last = at(&d, 0.0, 0, s);
        let mut moved = false;
        for step in 1..400 {
            let now = at(&d, step as f32, 0, s);
            for i in 0..3 {
                assert!(
                    now[i].abs_diff(last[i]) <= 2,
                    "jump at {step}: {last:?} -> {now:?}"
                );
            }
            moved |= now != at(&d, 0.0, 0, s);
            last = now;
        }
        assert!(moved);
    }

    #[test]
    fn variation_stays_close_to_the_base() {
        let d = natural(Frequency::PerStroke);
        let base = Oklch::from_srgb([GREEN[0], GREEN[1], GREEN[2]]);
        for i in 0..200 {
            let [r, g, b, _] = at(&d, 0.0, 0, seed(Id(i), Id(3), 0));
            let lch = Oklch::from_srgb([r, g, b]);
            assert!((lch.l - base.l).abs() <= 0.035, "{lch:?}");
            let dh = (lch.h - base.h + 540.0).rem_euclid(360.0) - 180.0;
            assert!(dh.abs() <= 3.5, "{lch:?}");
        }
    }

    #[test]
    fn pressing_harder_darkens() {
        let d = Dynamics {
            behaviours: vec![Behaviour::pressure_darkens()],
            ..Default::default()
        };
        let light = colour_at(
            GREEN,
            &d,
            &Sample {
                pressure: 0.0,
                ..Default::default()
            },
            0,
        );
        let hard = colour_at(
            GREEN,
            &d,
            &Sample {
                pressure: 1.0,
                ..Default::default()
            },
            0,
        );
        assert_eq!(light, GREEN);
        let l = |c: [u8; 4]| Oklch::from_srgb([c[0], c[1], c[2]]).l;
        assert!(l(hard) < l(GREEN) - 0.05);
    }

    #[test]
    fn dynamics_survive_saving() {
        let d = Dynamics {
            behaviours: vec![Behaviour::natural(), Behaviour::pressure_darkens()],
            frequency: Frequency::Continuous,
            scale: 80.0,
            seed_offset: 3,
        };
        let json = serde_json::to_string(&d).unwrap();
        assert_eq!(serde_json::from_str::<Dynamics>(&json).unwrap(), d);
    }
}
