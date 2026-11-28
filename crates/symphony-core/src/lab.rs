//! The Colour Lab (docs/plans/colour-system.md, Phase 3): a sphere or a
//! cube on a floor, lit by a key light and an optional second light, with
//! ambient light, bounce light from the floor, cast shadows and
//! highlights, smooth or in cel bands. Artists pick colours off it, and
//! [`recipe`] works out how to paint each one over the flat base colour
//! with a blend mode and an opacity.
//!
//! It's a tiny ray tracer, deterministic and independent of the GPU. It
//! takes a few milliseconds, and the app runs it off the UI thread.
//! Lighting is computed in linear light and shown in sRGB. Recipes use
//! `colour::composite`, the renderer's own blending, so a recipe painted
//! in a scene gives the colour the Lab showed.

use serde::{Deserialize, Serialize};

use crate::colour::{composite, from_linear, to_linear};
use crate::graph::BlendMode;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Shape {
    #[default]
    Sphere,
    Cube,
}

/// A light far away (like the sun), so its direction is the same
/// everywhere.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Light {
    pub on: bool,
    pub colour: [u8; 3],
    /// 1 is a plain, fully lit light.
    pub strength: f32,
    /// Where it comes from, seen from above, in degrees: 0 from the viewer,
    /// 90 from the right, 180 from behind (a rim light), -90 from the left.
    pub around: f32,
    /// Degrees above the horizon: 0 grazing, 90 straight overhead.
    pub height: f32,
}

impl Light {
    /// The direction towards the light (y up, the viewer towards +z).
    pub fn direction(&self) -> [f32; 3] {
        let (sa, ca) = self.around.to_radians().sin_cos();
        let (sh, ch) = self.height.clamp(0.0, 90.0).to_radians().sin_cos();
        [ch * sa, sh, ch * ca]
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Lab {
    pub shape: Shape,
    /// The flat base colour of the object.
    pub object: [u8; 3],
    pub key: Light,
    pub second: Light,
    /// Light from all around (the sky): fills the shadows.
    pub ambient: [u8; 3],
    pub ambient_strength: f32,
    pub floor: [u8; 3],
    /// How much light the floor throws back up onto the object.
    pub bounce: f32,
    pub shadows: bool,
    /// Highlight brightness, 0..=1.
    pub highlight: f32,
    /// Highlight tightness, 0 (broad, matte) ..= 1 (small, glossy).
    pub gloss: f32,
    /// 0 is smooth shading; 2 or more is that many cel tones.
    pub bands: u8,
}

impl Default for Lab {
    fn default() -> Self {
        Lab {
            shape: Shape::Sphere,
            object: [214, 150, 120],
            key: Light {
                on: true,
                colour: [255, 238, 214],
                strength: 1.0,
                around: -40.0,
                height: 40.0,
            },
            second: Light {
                on: true,
                colour: [130, 165, 255],
                strength: 0.7,
                around: 140.0,
                height: 20.0,
            },
            ambient: [150, 165, 195],
            ambient_strength: 0.25,
            floor: [140, 130, 120],
            bounce: 0.4,
            shadows: true,
            highlight: 0.5,
            gloss: 0.5,
            bands: 0,
        }
    }
}

/// What a pixel of the Lab image shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Surface {
    Object,
    Floor,
}

/// A rendered Lab: `size` × `size` straight sRGB pixels, row by row, and
/// which surface each pixel shows.
#[derive(Clone, Debug)]
pub struct Image {
    pub size: u32,
    pub pixels: Vec<[u8; 3]>,
    pub surfaces: Vec<Surface>,
}

impl Image {
    pub fn at(&self, x: u32, y: u32) -> Option<([u8; 3], Surface)> {
        if x >= self.size || y >= self.size {
            return None;
        }
        let i = (y * self.size + x) as usize;
        Some((self.pixels[i], self.surfaces[i]))
    }
}

type V3 = [f32; 3];

fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn scale(a: V3, k: f32) -> V3 {
    [a[0] * k, a[1] * k, a[2] * k]
}
fn mul(a: V3, b: V3) -> V3 {
    [a[0] * b[0], a[1] * b[1], a[2] * b[2]]
}
fn dot(a: V3, b: V3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn normalise(a: V3) -> V3 {
    let len = dot(a, a).sqrt().max(1e-9);
    scale(a, 1.0 / len)
}
fn linear(c: [u8; 3]) -> V3 {
    c.map(to_linear)
}

/// The floor's height; both shapes rest on it.
const FLOOR: f32 = -1.0;
/// The cube's half size, and its turn so three faces show.
const CUBE_HALF: f32 = 0.78;
const CUBE_TURN: f32 = 35.0;
/// How far the camera looks down.
const PITCH: f32 = 22.0;

struct Scene {
    shape: Shape,
    turn: (f32, f32),
}

impl Scene {
    fn new(shape: Shape) -> Self {
        Scene {
            shape,
            turn: CUBE_TURN.to_radians().sin_cos(),
        }
    }

    fn cube_centre() -> V3 {
        [0.0, FLOOR + CUBE_HALF, 0.0]
    }

    /// Into and out of the cube's own axes (turned about y).
    fn to_cube(&self, v: V3) -> V3 {
        let (s, c) = self.turn;
        [c * v[0] - s * v[2], v[1], s * v[0] + c * v[2]]
    }
    fn out_of_cube(&self, v: V3) -> V3 {
        let (s, c) = self.turn;
        [c * v[0] + s * v[2], v[1], -s * v[0] + c * v[2]]
    }

    /// The nearest hit on the object: distance and normal.
    fn hit(&self, origin: V3, dir: V3) -> Option<(f32, V3)> {
        match self.shape {
            Shape::Sphere => {
                let b = dot(origin, dir);
                let c = dot(origin, origin) - 1.0;
                let disc = b * b - c;
                if disc < 0.0 {
                    return None;
                }
                let t = -b - disc.sqrt();
                (t > 1e-4).then(|| (t, normalise(add(origin, scale(dir, t)))))
            }
            Shape::Cube => {
                let o = self.to_cube(sub(origin, Self::cube_centre()));
                let d = self.to_cube(dir);
                let (mut near, mut far) = (f32::NEG_INFINITY, f32::INFINITY);
                let mut axis = 0;
                for i in 0..3 {
                    if d[i].abs() < 1e-9 {
                        if o[i].abs() > CUBE_HALF {
                            return None;
                        }
                        continue;
                    }
                    let a = (-CUBE_HALF - o[i]) / d[i];
                    let b = (CUBE_HALF - o[i]) / d[i];
                    let (a, b) = if a < b { (a, b) } else { (b, a) };
                    if a > near {
                        near = a;
                        axis = i;
                    }
                    far = far.min(b);
                }
                if near > far || near <= 1e-4 {
                    return None;
                }
                let mut n = [0.0; 3];
                n[axis] = -d[axis].signum();
                Some((near, self.out_of_cube(n)))
            }
        }
    }

    /// Distance from a point to the object's surface (0 on it).
    fn distance(&self, p: V3) -> f32 {
        match self.shape {
            Shape::Sphere => dot(p, p).sqrt() - 1.0,
            Shape::Cube => {
                let q = self
                    .to_cube(sub(p, Self::cube_centre()))
                    .map(|v| (v.abs() - CUBE_HALF).max(0.0));
                dot(q, q).sqrt()
            }
        }
    }
}

/// Shading tones: smooth, or snapped to `bands` cel tones (the lit side is
/// always fully lit, as in cel animation).
fn band(x: f32, bands: u8) -> f32 {
    let x = x.max(0.0);
    if bands < 2 {
        return x;
    }
    if x <= 0.0 {
        return 0.0;
    }
    let steps = f32::from(bands - 1);
    (x * steps).ceil().min(steps) / steps
}

/// Render the Lab at `size` × `size`, 2×2 samples per pixel.
pub fn render(lab: &Lab, size: u32) -> Image {
    let scene = Scene::new(lab.shape);
    let (sp, cp) = PITCH.to_radians().sin_cos();
    let forward = [0.0, -sp, -cp];
    let up = [0.0, cp, -sp];
    let centre = [0.0, -0.2, 0.0];
    let half = 1.9;
    let lights: Vec<(V3, V3)> = [lab.key, lab.second]
        .iter()
        .filter(|l| l.on && l.strength > 0.0)
        .map(|l| (l.direction(), scale(linear(l.colour), l.strength)))
        .collect();
    let object = linear(lab.object);
    let floor = linear(lab.floor);
    let ambient = scale(linear(lab.ambient), lab.ambient_strength);
    // Light reaching the floor, which it throws back up as bounce.
    let floor_light = lights.iter().fold([0.0; 3], |acc, (dir, col)| {
        add(acc, scale(*col, dir[1].max(0.0)))
    });
    let bounce = scale(mul(floor, add(floor_light, ambient)), lab.bounce);
    let shininess = 6.0 + lab.gloss.clamp(0.0, 1.0).powi(2) * 300.0;

    let shade = |origin: V3, dir: V3| -> (V3, Surface) {
        let view = scale(dir, -1.0);
        let lit = |p: V3, n: V3| -> Vec<f32> {
            lights
                .iter()
                .map(|(l, _)| {
                    if lab.shadows && scene.hit(add(p, scale(n, 1e-3)), *l).is_some() {
                        0.0
                    } else {
                        1.0
                    }
                })
                .collect()
        };
        if let Some((t, n)) = scene.hit(origin, dir) {
            let p = add(origin, scale(dir, t));
            let visible = lit(p, n);
            // Sky light from above; flat in cel bands.
            let sky = if lab.bands >= 2 {
                0.8
            } else {
                0.6 + 0.4 * n[1]
            };
            let mut light = scale(ambient, sky);
            let mut shine = [0.0; 3];
            for ((l, col), vis) in lights.iter().zip(&visible) {
                let ndl = dot(n, *l);
                light = add(light, scale(*col, band(ndl, lab.bands) * vis));
                if ndl > 0.0 && lab.highlight > 0.0 {
                    let h = normalise(add(*l, view));
                    let s = dot(n, h).max(0.0).powf(shininess);
                    let s = if lab.bands >= 2 {
                        if s > 0.5 {
                            1.0
                        } else {
                            0.0
                        }
                    } else {
                        s
                    };
                    shine = add(shine, scale(*col, s * lab.highlight * vis));
                }
            }
            light = add(light, scale(bounce, band(-n[1], lab.bands)));
            return (add(mul(object, light), shine), Surface::Object);
        }
        // Everything else is floor (the camera looks down at it).
        let t = if dir[1] < 0.0 {
            (FLOOR - origin[1]) / dir[1]
        } else {
            1e6
        };
        let p = add(origin, scale(dir, t));
        let n = [0.0, 1.0, 0.0];
        let visible = lit(p, n);
        // Contact shadow: the floor right next to the object sees less sky.
        let near = (1.0 - scene.distance(p) / 0.7).clamp(0.0, 1.0);
        let occlusion = 1.0 - 0.6 * near * near;
        let mut light = scale(ambient, occlusion);
        for ((l, col), vis) in lights.iter().zip(&visible) {
            light = add(light, scale(*col, band(l[1], lab.bands) * vis));
        }
        (mul(floor, light), Surface::Floor)
    };

    let mut pixels = Vec::with_capacity((size * size) as usize);
    let mut surfaces = Vec::with_capacity((size * size) as usize);
    let size_f = size as f32;
    for y in 0..size {
        for x in 0..size {
            let mut sum = [0.0; 3];
            let mut middle = Surface::Floor;
            for (k, (dx, dy)) in [(0.25, 0.25), (0.75, 0.25), (0.25, 0.75), (0.75, 0.75)]
                .into_iter()
                .enumerate()
            {
                let sx = ((x as f32 + dx) / size_f * 2.0 - 1.0) * half;
                let sy = (1.0 - (y as f32 + dy) / size_f * 2.0) * half;
                let on_plane = add(centre, add([sx, 0.0, 0.0], scale(up, sy)));
                let origin = sub(on_plane, scale(forward, 20.0));
                let (c, surface) = shade(origin, forward);
                sum = add(sum, c);
                if k == 3 {
                    middle = surface;
                }
            }
            pixels.push(scale(sum, 0.25).map(from_linear));
            surfaces.push(middle);
        }
    }
    Image {
        size,
        pixels,
        surfaces,
    }
}

/// How to paint `target` over a flat `base` with one blend mode.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Recipe {
    pub mode: BlendMode,
    /// The colour to paint with.
    pub colour: [u8; 3],
    pub opacity: f32,
    /// False when this mode can't reach the target at this opacity; then
    /// `colour` gives the closest it can.
    pub exact: bool,
}

/// The modes the Lab suggests recipes in: the ones artists shade with, and
/// whose result rises steadily with the paint colour, so the paint colour
/// for a target can be solved exactly.
pub const RECIPE_MODES: [BlendMode; 8] = [
    BlendMode::Multiply,
    BlendMode::Screen,
    BlendMode::Add,
    BlendMode::Overlay,
    BlendMode::SoftLight,
    BlendMode::LinearBurn,
    BlendMode::ColorDodge,
    BlendMode::Normal,
];

/// What one channel of paint `s` over `b` gives in `mode` at `opacity`.
fn result(mode: BlendMode, b: f32, s: f32, opacity: f32) -> f32 {
    composite(mode, [b; 3], 1.0, [s; 3], opacity).0[0]
}

/// The paint colour that turns `base` into `target` with `mode` at
/// `opacity` (0..=1): the inverse of the renderer's blending, solved per
/// channel. If no colour reaches the target, the closest one, marked
/// inexact.
pub fn recipe(mode: BlendMode, base: [u8; 3], target: [u8; 3], opacity: f32) -> Recipe {
    let opacity = opacity.clamp(0.01, 1.0);
    let colour: [u8; 3] = std::array::from_fn(|i| {
        let b = f32::from(base[i]) / 255.0;
        let want = f32::from(target[i]) / 255.0;
        let (lo_out, hi_out) = (result(mode, b, 0.0, opacity), result(mode, b, 1.0, opacity));
        let rising = hi_out >= lo_out;
        let (mut lo, mut hi) = (0.0_f32, 1.0_f32);
        for _ in 0..24 {
            let mid = (lo + hi) / 2.0;
            let below = result(mode, b, mid, opacity) < want;
            if below == rising {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        ((lo + hi) / 2.0 * 255.0).round() as u8
    });
    let made = crate::colour::over_opaque(mode, base, colour, opacity);
    let exact = made.iter().zip(target).all(|(m, t)| m.abs_diff(t) <= 1);
    Recipe {
        mode,
        colour,
        opacity,
        exact,
    }
}

/// The lowest opacity (in whole percent) at which `mode` reaches `target`
/// over `base`, if it can at all.
pub fn lowest_opacity(mode: BlendMode, base: [u8; 3], target: [u8; 3]) -> Option<u8> {
    if !recipe(mode, base, target, 1.0).exact {
        return None;
    }
    // Reaching gets easier as opacity rises, so search for the edge.
    let (mut lo, mut hi) = (1_u8, 100_u8);
    while lo < hi {
        let mid = (lo + hi) / 2;
        if recipe(mode, base, target, f32::from(mid) / 100.0).exact {
            hi = mid;
        } else {
            lo = mid + 1;
        }
    }
    Some(lo)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::colour::Oklch;

    fn lightness(c: [u8; 3]) -> f32 {
        Oklch::from_srgb(c).l
    }

    fn lab() -> Lab {
        Lab {
            second: Light {
                on: false,
                ..Lab::default().second
            },
            ..Lab::default()
        }
    }

    #[test]
    fn the_lit_side_is_lighter_than_the_shadow_side() {
        let mut l = lab();
        l.key.around = -90.0; // from the left
        l.key.height = 10.0;
        let image = render(&l, 64);
        let (left, s1) = image.at(18, 32).unwrap();
        let (right, s2) = image.at(46, 32).unwrap();
        assert_eq!((s1, s2), (Surface::Object, Surface::Object));
        assert!(
            lightness(left) > lightness(right) + 0.15,
            "{left:?} {right:?}"
        );
    }

    #[test]
    fn the_object_casts_a_shadow_on_the_floor() {
        let mut l = lab();
        l.key.around = 0.0; // from the viewer: the shadow falls behind
        l.key.height = 30.0;
        l.ambient_strength = 0.0;
        let with = render(&l, 64);
        l.shadows = false;
        let without = render(&l, 64);
        let darker = (0..64 * 64)
            .filter(|&i| with.surfaces[i] == Surface::Floor)
            .filter(|&i| lightness(with.pixels[i]) + 0.1 < lightness(without.pixels[i]))
            .count();
        assert!(darker > 40, "{darker} shadowed floor pixels");
    }

    #[test]
    fn bounce_light_lifts_the_underside() {
        let mut l = lab();
        l.floor = [255, 120, 60];
        let warm = render(&l, 64);
        l.bounce = 0.0;
        let none = render(&l, 64);
        // Low on the sphere, facing the floor.
        let (a, _) = warm.at(32, 44).unwrap();
        let (b, _) = none.at(32, 44).unwrap();
        assert!(a[0] > b[0], "{a:?} {b:?}");
    }

    #[test]
    fn cel_bands_give_few_tones() {
        let mut l = lab();
        l.bands = 2;
        l.highlight = 0.0;
        l.shadows = false;
        let image = render(&l, 48);
        // Apart from antialiased edges, the object is a handful of flat
        // tones: lit, shadow, and each with bounce.
        let mut counts = std::collections::HashMap::<[u8; 3], usize>::new();
        let mut total = 0;
        for (c, s) in image.pixels.iter().zip(&image.surfaces) {
            if *s == Surface::Object {
                *counts.entry(*c).or_default() += 1;
                total += 1;
            }
        }
        let mut sizes: Vec<usize> = counts.into_values().collect();
        sizes.sort_unstable_by(|a, b| b.cmp(a));
        let main: usize = sizes.iter().take(4).sum();
        assert!(main * 100 >= total * 85, "{main} of {total}");
    }

    #[test]
    fn a_cube_shows_three_differently_lit_faces() {
        let mut l = lab();
        l.shape = Shape::Cube;
        l.highlight = 0.0;
        let image = render(&l, 96);
        let mut tones: Vec<[u8; 3]> = Vec::new();
        for &(x, y) in &[(48, 22), (32, 52), (64, 52)] {
            let (c, s) = image.at(x, y).unwrap();
            assert_eq!(s, Surface::Object, "({x}, {y})");
            tones.push(c);
        }
        assert!(tones[0] != tones[1] && tones[1] != tones[2] && tones[0] != tones[2]);
    }

    #[test]
    fn multiply_recipes_make_the_shadow_exactly() {
        let base = [214, 150, 120];
        let shadow = [120, 70, 80];
        let r = recipe(BlendMode::Multiply, base, shadow, 1.0);
        assert!(r.exact, "{r:?}");
        let made = crate::colour::over_opaque(BlendMode::Multiply, base, r.colour, 1.0);
        assert!(
            made.iter().zip(shadow).all(|(a, b)| a.abs_diff(b) <= 1),
            "{made:?}"
        );
    }

    #[test]
    fn multiply_cannot_lighten_and_screen_cannot_darken() {
        let base = [100, 100, 100];
        assert!(!recipe(BlendMode::Multiply, base, [180, 180, 180], 1.0).exact);
        assert!(!recipe(BlendMode::Screen, base, [40, 40, 40], 1.0).exact);
        assert!(recipe(BlendMode::Screen, base, [180, 170, 160], 1.0).exact);
        assert_eq!(
            lowest_opacity(BlendMode::Multiply, base, [180, 180, 180]),
            None
        );
    }

    #[test]
    fn lower_opacity_needs_a_stronger_colour_until_it_cant() {
        let base = [220, 200, 180];
        let target = [150, 120, 110];
        let full = recipe(BlendMode::Multiply, base, target, 1.0);
        let half = recipe(BlendMode::Multiply, base, target, 0.5);
        assert!(full.exact);
        // At half opacity the paint must be darker to get there.
        assert!(half.colour.iter().zip(full.colour).all(|(h, f)| *h <= f));
        let lowest = lowest_opacity(BlendMode::Multiply, base, target).unwrap();
        assert!((20..=60).contains(&lowest), "{lowest}");
        assert!(recipe(BlendMode::Multiply, base, target, f32::from(lowest) / 100.0).exact);
        assert!(
            !recipe(
                BlendMode::Multiply,
                base,
                target,
                f32::from(lowest - 1) / 100.0
            )
            .exact
        );
    }

    #[test]
    fn every_recipe_mode_solves_a_reachable_target() {
        // Pick a target by painting a known colour, then solve it back.
        let base = [150, 120, 100];
        for mode in RECIPE_MODES {
            let painted = crate::colour::over_opaque(mode, base, [90, 160, 200], 0.8);
            let r = recipe(mode, base, painted, 0.8);
            assert!(r.exact, "{mode:?}: {painted:?} -> {r:?}");
        }
    }
}
