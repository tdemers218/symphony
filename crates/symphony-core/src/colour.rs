//! Colour maths shared by the picker, the renderer and (later) dynamic
//! swatches, so a preview and the final render compute the same colour
//! (docs/plans/colour-system.md).
//!
//! OKLab and its polar form OKLCH (Björn Ottosson, 2020) separate perceived
//! lightness, chroma and hue far better than HSV: darkening a colour in
//! OKLCH keeps its hue, and two colours with the same L look about equally
//! light. Swatches stay stored as 8-bit sRGB; OKLCH is for choosing and
//! varying colours.

use serde::{Deserialize, Serialize};

use crate::graph::BlendMode;

/// sRGB transfer: an 8-bit channel to linear light, 0..=1.
pub fn to_linear(c: u8) -> f32 {
    let c = f32::from(c) / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// Linear light back to an 8-bit sRGB channel, clamped.
pub fn from_linear(c: f32) -> u8 {
    let c = c.clamp(0.0, 1.0);
    let s = if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    };
    (s * 255.0).round() as u8
}

/// A colour in OKLab: `l` is perceived lightness 0..=1, `a` green–red and
/// `b` blue–yellow.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Oklab {
    pub l: f32,
    pub a: f32,
    pub b: f32,
}

/// OKLab in polar form: lightness 0..=1, chroma (0 is grey; sRGB reaches
/// about 0.32) and hue in degrees, 0..360.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Oklch {
    pub l: f32,
    pub c: f32,
    pub h: f32,
}

impl Oklab {
    pub fn from_linear_rgb([r, g, b]: [f32; 3]) -> Self {
        let l = 0.412_221_46 * r + 0.536_332_55 * g + 0.051_445_995 * b;
        let m = 0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b;
        let s = 0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b;
        let (l, m, s) = (l.cbrt(), m.cbrt(), s.cbrt());
        Oklab {
            l: 0.210_454_26 * l + 0.793_617_8 * m - 0.004_072_047 * s,
            a: 1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s,
            b: 0.025_904_037 * l + 0.782_771_77 * m - 0.808_675_77 * s,
        }
    }

    pub fn from_srgb([r, g, b]: [u8; 3]) -> Self {
        Self::from_linear_rgb([to_linear(r), to_linear(g), to_linear(b)])
    }

    /// Linear sRGB, not clamped: channels outside 0..=1 mean the colour is
    /// out of the sRGB gamut.
    pub fn to_linear_rgb(self) -> [f32; 3] {
        let l = self.l + 0.396_337_78 * self.a + 0.215_803_76 * self.b;
        let m = self.l - 0.105_561_346 * self.a - 0.063_854_17 * self.b;
        let s = self.l - 0.089_484_18 * self.a - 1.291_485_5 * self.b;
        let (l, m, s) = (l * l * l, m * m * m, s * s * s);
        [
            4.076_741_7 * l - 3.307_711_6 * m + 0.230_969_94 * s,
            -1.268_438 * l + 2.609_757_4 * m - 0.341_319_38 * s,
            -0.004_196_086_3 * l - 0.703_418_6 * m + 1.707_614_7 * s,
        ]
    }
}

impl From<Oklab> for Oklch {
    fn from(lab: Oklab) -> Self {
        let c = lab.a.hypot(lab.b);
        let h = lab.b.atan2(lab.a).to_degrees().rem_euclid(360.0);
        Oklch { l: lab.l, c, h }
    }
}

impl From<Oklch> for Oklab {
    fn from(lch: Oklch) -> Self {
        let (sin, cos) = lch.h.to_radians().sin_cos();
        Oklab {
            l: lch.l,
            a: lch.c * cos,
            b: lch.c * sin,
        }
    }
}

/// How far outside 0..=1 a linear channel may sit and still count as in
/// gamut: f32 rounding in the matrices, so relative to the colour's own
/// size. A fixed slack would be huge next to a dark colour's tiny linear
/// values and let its chroma run past the real edge.
const GAMUT_SLACK: f32 = 2e-6;

fn fits([r, g, b]: [f32; 3]) -> bool {
    let size = r.abs().max(g.abs()).max(b.abs());
    let below = -GAMUT_SLACK * size;
    [r, g, b]
        .iter()
        .all(|&c| c >= below && c <= 1.0 + GAMUT_SLACK)
}

impl Oklch {
    pub fn from_srgb(rgb: [u8; 3]) -> Self {
        Oklab::from_srgb(rgb).into()
    }

    /// Whether sRGB can show this colour exactly.
    pub fn in_gamut(self) -> bool {
        fits(Oklab::from(self).to_linear_rgb())
    }

    /// The most chroma sRGB can show at this lightness and hue.
    pub fn max_chroma(l: f32, h: f32) -> f32 {
        if l <= 0.0 || l >= 1.0 {
            return 0.0;
        }
        let (mut lo, mut hi) = (0.0_f32, 0.4_f32);
        for _ in 0..20 {
            let mid = (lo + hi) / 2.0;
            if (Oklch { l, c: mid, h }).in_gamut() {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        lo
    }

    /// Bring the colour into sRGB by lowering its chroma at the same
    /// lightness and hue, so it never shifts towards another hue the way
    /// clipping each channel does.
    pub fn to_gamut(self) -> Oklch {
        let l = self.l.clamp(0.0, 1.0);
        let c = self.c.max(0.0);
        let inside = Oklch { l, c, h: self.h };
        if inside.in_gamut() {
            return inside;
        }
        Oklch {
            l,
            c: c.min(Self::max_chroma(l, self.h)),
            h: self.h,
        }
    }

    /// 8-bit sRGB, mapped into the gamut first.
    pub fn to_srgb(self) -> [u8; 3] {
        let [r, g, b] = Oklab::from(self.to_gamut()).to_linear_rgb();
        [from_linear(r), from_linear(g), from_linear(b)]
    }
}

// How the opacities combine, everywhere paint is laid down:
//
// 1. Brush **flow** (per dab, with its pressure curve) builds coverage up
//    to at most brush **opacity**: how the paint goes down (the brush).
// 2. The swatch's **colour opacity** multiplies that coverage: how
//    see-through the paint itself is. It travels with the swatch to
//    every brush and to fills.
// 3. The drawing is then composited with the layer's **transparency** and
//    **blend mode** (`composite` below).
//
// render's `brush_opacity_and_colour_opacity_multiply` checks 1 and 2.

// Blending. The renderer, the picker's preview and inverse blending all go
// through these, so what the picker shows is what the render makes.

/// Composite a straight `source` colour with alpha `a_s` over a straight
/// `backdrop` with alpha `a_b` (W3C Compositing and Blending):
/// `co = cs(1-ab) + cb(1-as) + as*ab*B(cb, cs)`, `ao = as + ab - as*ab`.
/// Returns the **premultiplied** colour and the alpha.
#[inline]
pub fn composite(
    mode: BlendMode,
    backdrop: [f32; 3],
    a_b: f32,
    source: [f32; 3],
    a_s: f32,
) -> ([f32; 3], f32) {
    let mixed = mix(mode, backdrop, source);
    let a_o = a_s + a_b - a_s * a_b;
    let co = std::array::from_fn(|i| {
        source[i] * a_s * (1.0 - a_b) + backdrop[i] * a_b * (1.0 - a_s) + a_s * a_b * mixed[i]
    });
    (co, a_o)
}

/// What paint of colour `source` at `opacity` looks like on an opaque
/// `backdrop` with `mode`: the picker's preview. 8-bit sRGB in and out.
pub fn over_opaque(mode: BlendMode, backdrop: [u8; 3], source: [u8; 3], opacity: f32) -> [u8; 3] {
    let unit = |c: [u8; 3]| c.map(|v| f32::from(v) / 255.0);
    let (co, _) = composite(
        mode,
        unit(backdrop),
        1.0,
        unit(source),
        opacity.clamp(0.0, 1.0),
    );
    co.map(|v| (v * 255.0).round().clamp(0.0, 255.0) as u8)
}

/// The blend function B(backdrop, source) on straight (not premultiplied)
/// sRGB-encoded colours, 0..=1, from the W3C Compositing and Blending spec
/// (plus Add, Subtract, Divide and Linear Burn).
#[inline]
pub fn mix(mode: BlendMode, cb: [f32; 3], cs: [f32; 3]) -> [f32; 3] {
    let sep = |f: fn(f32, f32) -> f32| [f(cb[0], cs[0]), f(cb[1], cs[1]), f(cb[2], cs[2])];
    match mode {
        BlendMode::Normal => cs,
        BlendMode::Multiply => sep(|b, s| b * s),
        BlendMode::Screen => sep(screen),
        BlendMode::Overlay => sep(|b, s| hard_light(s, b)),
        BlendMode::Darken => sep(f32::min),
        BlendMode::Lighten => sep(f32::max),
        BlendMode::ColorDodge => sep(|b, s| {
            if b <= 0.0 {
                0.0
            } else if s >= 1.0 {
                1.0
            } else {
                (b / (1.0 - s)).min(1.0)
            }
        }),
        BlendMode::ColorBurn => sep(|b, s| {
            if b >= 1.0 {
                1.0
            } else if s <= 0.0 {
                0.0
            } else {
                1.0 - ((1.0 - b) / s).min(1.0)
            }
        }),
        BlendMode::LinearBurn => sep(|b, s| (b + s - 1.0).max(0.0)),
        BlendMode::Add => sep(|b, s| (b + s).min(1.0)),
        BlendMode::Subtract => sep(|b, s| (b - s).max(0.0)),
        BlendMode::Divide => sep(|b, s| {
            if s <= 0.0 {
                if b <= 0.0 {
                    0.0
                } else {
                    1.0
                }
            } else {
                (b / s).min(1.0)
            }
        }),
        BlendMode::Difference => sep(|b, s| (b - s).abs()),
        BlendMode::Exclusion => sep(|b, s| b + s - 2.0 * b * s),
        BlendMode::HardLight => sep(hard_light),
        BlendMode::SoftLight => sep(|b, s| {
            if s <= 0.5 {
                b - (1.0 - 2.0 * s) * b * (1.0 - b)
            } else {
                let d = if b <= 0.25 {
                    ((16.0 * b - 12.0) * b + 4.0) * b
                } else {
                    b.sqrt()
                };
                b + (2.0 * s - 1.0) * (d - b)
            }
        }),
        BlendMode::Hue => set_lum(set_sat(cs, sat(cb)), lum(cb)),
        BlendMode::Saturation => set_lum(set_sat(cb, sat(cs)), lum(cb)),
        BlendMode::Color => set_lum(cs, lum(cb)),
        BlendMode::Luminosity => set_lum(cb, lum(cs)),
    }
}

fn screen(b: f32, s: f32) -> f32 {
    b + s - b * s
}

fn hard_light(b: f32, s: f32) -> f32 {
    if s <= 0.5 {
        b * 2.0 * s
    } else {
        screen(b, 2.0 * s - 1.0)
    }
}

fn lum(c: [f32; 3]) -> f32 {
    0.3 * c[0] + 0.59 * c[1] + 0.11 * c[2]
}

fn clip_color(c: [f32; 3]) -> [f32; 3] {
    let l = lum(c);
    let n = c[0].min(c[1]).min(c[2]);
    let x = c[0].max(c[1]).max(c[2]);
    let mut c = c;
    if n < 0.0 && l - n > 1e-6 {
        c = c.map(|v| l + (v - l) * l / (l - n));
    }
    if x > 1.0 && x - l > 1e-6 {
        c = c.map(|v| l + (v - l) * (1.0 - l) / (x - l));
    }
    c.map(|v| v.clamp(0.0, 1.0))
}

fn set_lum(c: [f32; 3], l: f32) -> [f32; 3] {
    let d = l - lum(c);
    clip_color(c.map(|v| v + d))
}

fn sat(c: [f32; 3]) -> f32 {
    c[0].max(c[1]).max(c[2]) - c[0].min(c[1]).min(c[2])
}

fn set_sat(c: [f32; 3], s: f32) -> [f32; 3] {
    let (max, min) = (c[0].max(c[1]).max(c[2]), c[0].min(c[1]).min(c[2]));
    if max - min <= 1e-6 {
        return [0.0; 3];
    }
    c.map(|v| (v - min) * s / (max - min))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32, tolerance: f32) -> bool {
        (a - b).abs() <= tolerance
    }

    #[test]
    fn white_black_and_greys_have_no_chroma() {
        let white = Oklch::from_srgb([255, 255, 255]);
        assert!(close(white.l, 1.0, 1e-3) && white.c < 1e-3, "{white:?}");
        let black = Oklch::from_srgb([0, 0, 0]);
        assert!(close(black.l, 0.0, 1e-4), "{black:?}");
        for v in [1, 64, 128, 200] {
            let grey = Oklch::from_srgb([v, v, v]);
            assert!(grey.c < 1e-3, "{v}: {grey:?}");
        }
    }

    #[test]
    fn primaries_match_the_published_oklab_values() {
        // Ottosson's reference values for sRGB red, green and blue.
        let cases = [
            ([255, 0, 0], [0.627_955, 0.224_863, 0.125_846]),
            ([0, 255, 0], [0.866_440, -0.233_888, 0.179_498]),
            ([0, 0, 255], [0.452_014, -0.032_457, -0.311_528]),
        ];
        for (rgb, [l, a, b]) in cases {
            let lab = Oklab::from_srgb(rgb);
            assert!(
                close(lab.l, l, 1e-3) && close(lab.a, a, 1e-3) && close(lab.b, b, 1e-3),
                "{rgb:?}: {lab:?}"
            );
        }
    }

    #[test]
    fn every_srgb_colour_survives_the_round_trip() {
        for r in (0..=255).step_by(15) {
            for g in (0..=255).step_by(15) {
                for b in (0..=255).step_by(15) {
                    let rgb = [r as u8, g as u8, b as u8];
                    let lch = Oklch::from_srgb(rgb);
                    assert!(lch.in_gamut(), "{rgb:?}");
                    assert_eq!(lch.to_srgb(), rgb, "{lch:?}");
                }
            }
        }
    }

    #[test]
    fn out_of_gamut_colours_lose_chroma_not_hue() {
        // A very saturated cyan that sRGB can't show.
        let wanted = Oklch {
            l: 0.8,
            c: 0.3,
            h: 200.0,
        };
        assert!(!wanted.in_gamut());
        let mapped = wanted.to_gamut();
        assert!(mapped.in_gamut());
        assert!(mapped.c < wanted.c && mapped.c > 0.05, "{mapped:?}");
        assert_eq!((mapped.l, mapped.h), (wanted.l, wanted.h));
        // The 8-bit result reads back with the same hue.
        let back = Oklch::from_srgb(mapped.to_srgb());
        assert!(close(back.h, 200.0, 2.0), "{back:?}");
    }

    #[test]
    fn max_chroma_is_the_gamut_edge() {
        for h in [0.0, 60.0, 140.0, 260.0, 330.0] {
            let c = Oklch::max_chroma(0.6, h);
            assert!(Oklch { l: 0.6, c, h }.in_gamut());
            assert!(
                !Oklch {
                    l: 0.6,
                    c: c + 0.01,
                    h
                }
                .in_gamut(),
                "{h}"
            );
        }
        assert_eq!(Oklch::max_chroma(1.0, 30.0), 0.0);
    }

    #[test]
    fn the_gamut_edge_narrows_smoothly_towards_black() {
        // Below the cusp, darker never allows more chroma.
        for h in [30.0, 140.0, 264.0] {
            let mut last = f32::MAX;
            for i in 0..=40 {
                let l = 0.4 - i as f32 * 0.01;
                let c = Oklch::max_chroma(l, h);
                assert!(c <= last + 1e-4, "hue {h}, l {l}: {c} after {last}");
                last = c;
            }
        }
    }

    #[test]
    fn normal_paint_over_white_fades_with_opacity() {
        let red = [200, 40, 40];
        assert_eq!(over_opaque(BlendMode::Normal, [255; 3], red, 1.0), red);
        assert_eq!(over_opaque(BlendMode::Normal, [255; 3], red, 0.0), [255; 3]);
        assert_eq!(
            over_opaque(BlendMode::Normal, [255; 3], red, 0.5),
            [228, 148, 148]
        );
    }

    #[test]
    fn multiply_darkens_and_screen_lightens() {
        let backdrop = [180, 160, 140];
        let paint = [128, 128, 128];
        let m = over_opaque(BlendMode::Multiply, backdrop, paint, 1.0);
        let s = over_opaque(BlendMode::Screen, backdrop, paint, 1.0);
        for i in 0..3 {
            assert!(m[i] < backdrop[i] && s[i] > backdrop[i], "{m:?} {s:?}");
        }
        // Multiply by white changes nothing.
        assert_eq!(
            over_opaque(BlendMode::Multiply, backdrop, [255; 3], 1.0),
            backdrop
        );
    }
}
