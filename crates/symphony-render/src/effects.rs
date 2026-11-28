//! Raster effects operate on premultiplied RGBA so transparent edges stay clean.

use symphony_core::colour;
use symphony_core::graph::{BlendMode, BlurMode, Effect, Gradient};
use tiny_skia::Pixmap;

pub fn apply(image: &mut Pixmap, effect: Effect, scale: f32) {
    match effect {
        Effect::Blur { radius, mode } => blur(image, radius, mode, scale),
        Effect::Grain { amount, seed } => {
            let amount = amount.clamp(0.0, 1.0);
            for (index, pixel) in image
                .data_mut()
                .as_chunks_mut::<4>()
                .0
                .iter_mut()
                .enumerate()
            {
                // Stable pixel-space noise. Modulate colour without changing coverage.
                let mut hash = (index as u32 ^ seed).wrapping_add(0x9e3779b9);
                hash = (hash ^ (hash >> 16)).wrapping_mul(0x85ebca6b);
                hash = (hash ^ (hash >> 13)).wrapping_mul(0xc2b2ae35);
                let noise = ((hash ^ (hash >> 16)) >> 24) as f32 / 127.5 - 1.0;
                let alpha = pixel[3] as f32;
                for channel in &mut pixel[..3] {
                    *channel = (*channel as f32 + noise * amount * alpha)
                        .round()
                        .clamp(0.0, alpha) as u8;
                }
            }
        }
        Effect::Opacity { amount } => {
            let amount = amount.clamp(0.0, 1.0);
            for value in image.data_mut() {
                *value = (*value as f32 * amount).round() as u8;
            }
        }
        // Blending happens when the layer lands (`blend`).
        Effect::Blend { .. } | Effect::Mask { .. } => {}
    }
}

fn blur(image: &mut Pixmap, radius: f32, mode: BlurMode, scale: f32) {
    if !radius.is_finite() || radius <= 0.0 {
        return;
    }
    let pixels = (radius * scale).clamp(0.0, 256.0);
    match mode {
        BlurMode::Box | BlurMode::Gaussian => {
            let box_radius = if mode == BlurMode::Gaussian {
                // Three box kernels approximate a Gaussian. Their
                // combined variance is r*(r+1), with sigma = pixels/2.
                ((1.0 + pixels * pixels).sqrt() - 1.0) / 2.0
            } else {
                pixels
            }
            .round() as usize;
            if box_radius == 0 {
                return;
            }
            let (w, h) = (image.width() as usize, image.height() as usize);
            let mut scratch = vec![0; image.data().len()];
            for _ in 0..if mode == BlurMode::Gaussian { 3 } else { 1 } {
                box_pass(image.data(), &mut scratch, w, h, 1, w, box_radius);
                box_pass(&scratch, image.data_mut(), h, w, w, 1, box_radius);
            }
        }
        BlurMode::Motion { angle } if angle.is_finite() && pixels >= 0.5 => {
            let (sin, cos) = angle.to_radians().sin_cos();
            let steps = ((pixels.ceil() as usize) | 1).clamp(3, 25);
            let transforms: Vec<_> = (0..steps)
                .map(|i| {
                    let d = pixels * (i as f32 / (steps - 1) as f32 - 0.5);
                    [1.0, 0.0, d * cos, d * sin]
                })
                .collect();
            sample_blur(image, &transforms);
        }
        BlurMode::RadialSpin | BlurMode::RadialZoom => {
            let (cx, cy) = (
                (image.width() as f32 - 1.0) * 0.5,
                (image.height() as f32 - 1.0) * 0.5,
            );
            let transforms: Vec<_> = (0..17)
                .map(|i| {
                    let t = i as f32 / 16.0;
                    let (a, b) = if mode == BlurMode::RadialSpin {
                        let (sin, cos) = ((t - 0.5) * radius.min(180.0).to_radians()).sin_cos();
                        (cos, sin)
                    } else {
                        (1.0 / (1.0 + t * radius.min(200.0) / 100.0), 0.0)
                    };
                    [a, b, cx - a * cx + b * cy, cy - b * cx - a * cy]
                })
                .collect();
            sample_blur(image, &transforms);
        }
        BlurMode::Motion { .. } => {}
    }
}

/// Sample affine paths in premultiplied RGBA. Pixels beyond the image
/// contribute transparent black, just as they do in a box pass.
fn sample_blur(image: &mut Pixmap, transforms: &[[f32; 4]]) {
    let (w, h) = (image.width() as usize, image.height() as usize);
    let source = image.data().to_vec();
    let source = source.as_chunks::<4>().0;
    let count = transforms.len() as f32;
    for (index, dest) in image
        .data_mut()
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .enumerate()
    {
        let (x, y) = ((index % w) as f32, (index / w) as f32);
        let mut sum = [0.0; 4];
        for &[a, b, u, v] in transforms {
            let (sx, sy) = (a * x - b * y + u, b * x + a * y + v);
            if sx <= -1.0 || sy <= -1.0 || sx >= w as f32 || sy >= h as f32 {
                continue;
            }
            let (ix, iy) = (sx.floor() as isize, sy.floor() as isize);
            let (fx, fy) = (sx - ix as f32, sy - iy as f32);
            for (py, wy) in [(iy, 1.0 - fy), (iy + 1, fy)] {
                if !(0..h as isize).contains(&py) {
                    continue;
                }
                for (px, wx) in [(ix, 1.0 - fx), (ix + 1, fx)] {
                    if (0..w as isize).contains(&px) {
                        for (acc, value) in
                            sum.iter_mut().zip(source[py as usize * w + px as usize])
                        {
                            *acc += value as f32 * wx * wy;
                        }
                    }
                }
            }
        }
        *dest = sum.map(|value| (value / count).round() as u8);
    }
}

/// Composite `src` onto `dst` with `mode` at `amount` opacity, both
/// premultiplied RGBA of the same size. W3C Compositing and Blending:
/// `co = cs(1-ab) + cb(1-as) + as*ab*B(cb, cs)`, `ao = as + ab - as*ab`.
pub fn blend(dst: &mut Pixmap, src: &Pixmap, mode: BlendMode, amount: f32) {
    let amount = if amount.is_finite() {
        amount.clamp(0.0, 1.0)
    } else {
        1.0
    };
    if amount <= 0.0 || dst.width() != src.width() || dst.height() != src.height() {
        return;
    }
    let (d, _) = dst.data_mut().as_chunks_mut::<4>();
    let (s, _) = src.data().as_chunks::<4>();
    for (b, sp) in d.iter_mut().zip(s) {
        let a_s = sp[3] as f32 / 255.0 * amount;
        if a_s <= 0.0 {
            continue;
        }
        let a_b = b[3] as f32 / 255.0;
        // Straight (unpremultiplied) colours.
        let unp = |p: &[u8; 4], a: f32| -> [f32; 3] {
            if a <= 0.0 {
                [0.0; 3]
            } else {
                let k = 1.0 / (p[3] as f32 / 255.0).max(1e-6) / 255.0;
                [p[0] as f32 * k, p[1] as f32 * k, p[2] as f32 * k].map(|v| v.clamp(0.0, 1.0))
            }
        };
        let cs = unp(sp, a_s);
        let cb = unp(b, a_b);
        let (co, a_o) = colour::composite(mode, cb, a_b, cs, a_s);
        for i in 0..3 {
            b[i] = (co[i] * 255.0).round().clamp(0.0, 255.0) as u8;
        }
        b[3] = (a_o * 255.0).round().clamp(0.0, 255.0) as u8;
    }
}

fn box_pass(
    src: &[u8],
    dst: &mut [u8],
    length: usize,
    lines: usize,
    stride: usize,
    line_stride: usize,
    radius: usize,
) {
    let samples = (radius * 2 + 1) as u32;
    for line in 0..lines {
        let at = |offset| (line * line_stride + offset * stride) * 4;
        let mut sum = [0u32; 4];
        for i in 0..=radius.min(length - 1) {
            for (total, &value) in sum.iter_mut().zip(&src[at(i)..at(i) + 4]) {
                *total += value as u32;
            }
        }
        for i in 0..length {
            for (channel, &total) in sum.iter().enumerate() {
                dst[at(i) + channel] = ((total + samples / 2) / samples) as u8;
            }
            if let Some(left) = i.checked_sub(radius) {
                for (total, &value) in sum.iter_mut().zip(&src[at(left)..at(left) + 4]) {
                    *total -= value as u32;
                }
            }
            let right = i + radius + 1;
            if right < length {
                for (total, &value) in sum.iter_mut().zip(&src[at(right)..at(right) + 4]) {
                    *total += value as u32;
                }
            }
        }
    }
}

pub fn gradient(image: &mut Pixmap, gradient: Gradient) {
    let (sin, cos) = gradient.angle.to_radians().sin_cos();
    let width = image.width() as usize;
    let height = image.height() as usize;
    let span = cos.abs() * width as f32 + sin.abs() * height as f32;
    for (index, pixel) in image
        .data_mut()
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .enumerate()
    {
        let x = (index % width) as f32 + 0.5 - width as f32 / 2.0;
        let y = (index / width) as f32 + 0.5 - height as f32 / 2.0;
        let t = (0.5 + (x * cos + y * sin) / span).clamp(0.0, 1.0);
        let mix = |c: usize| gradient.start[c] as f32 * (1.0 - t) + gradient.end[c] as f32 * t;
        let alpha = mix(3) / 255.0;
        for (c, value) in pixel[..3].iter_mut().enumerate() {
            *value = (mix(c) * alpha + *value as f32 * (1.0 - alpha)).round() as u8;
        }
        pixel[3] = (mix(3) + pixel[3] as f32 * (1.0 - alpha)).round() as u8;
    }
}

pub fn mask(image: &mut Pixmap, matte: &Pixmap, inverted: bool) {
    for (pixel, mask) in image
        .data_mut()
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .zip(matte.data().as_chunks::<4>().0)
    {
        let alpha = if inverted { 255 - mask[3] } else { mask[3] };
        for channel in pixel {
            *channel = ((u16::from(*channel) * u16::from(alpha) + 127) / 255) as u8;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blur_modes_spread_ink_in_the_expected_direction() {
        let mut source = Pixmap::new(49, 49).unwrap();
        source.data_mut()[(24 * 49 + 12) * 4..][..4].copy_from_slice(&[255; 4]);
        let alpha = |image: &Pixmap, x: usize, y: usize| image.data()[(y * 49 + x) * 4 + 3];
        for mode in BlurMode::ALL {
            let mut blurred = source.clone();
            let radius = match mode {
                BlurMode::RadialSpin | BlurMode::RadialZoom => 60.0,
                _ => 8.0,
            };
            apply(&mut blurred, Effect::Blur { radius, mode }, 1.0);
            assert_ne!(blurred, source, "{}", mode.title());
            match mode {
                BlurMode::Motion { .. } => {
                    assert!(alpha(&blurred, 14, 24) > 0);
                    assert_eq!(alpha(&blurred, 12, 26), 0);
                }
                BlurMode::RadialSpin => assert!(alpha(&blurred, 12, 27) > 0),
                BlurMode::RadialZoom => assert!(alpha(&blurred, 9, 24) > 0),
                _ => {
                    assert!(alpha(&blurred, 14, 24) > 0);
                    assert_eq!(alpha(&blurred, 14, 24), alpha(&blurred, 12, 26));
                }
            }
        }
    }

    /// One opaque pixel of `rgb` (0..=255).
    fn px(rgb: [u8; 3], a: u8) -> Pixmap {
        let mut p = Pixmap::new(1, 1).unwrap();
        let k = a as f32 / 255.0;
        p.data_mut().copy_from_slice(&[
            (rgb[0] as f32 * k).round() as u8,
            (rgb[1] as f32 * k).round() as u8,
            (rgb[2] as f32 * k).round() as u8,
            a,
        ]);
        p
    }

    fn blended(back: [u8; 3], front: [u8; 3], mode: BlendMode) -> [u8; 4] {
        let mut b = px(back, 255);
        blend(&mut b, &px(front, 255), mode, 1.0);
        b.data().try_into().unwrap()
    }

    #[test]
    fn modes_match_their_formulas() {
        let (b, s) = ([200, 100, 50], [100, 100, 100]);
        assert_eq!(blended(b, s, BlendMode::Normal), [100, 100, 100, 255]);
        // 200*100/255 = 78.4
        assert_eq!(blended(b, s, BlendMode::Multiply)[0], 78);
        assert_eq!(blended(b, s, BlendMode::Subtract), [100, 0, 0, 255]);
        assert_eq!(blended(b, s, BlendMode::Add), [255, 200, 150, 255]);
        // Divide: 200/100 clips to 255; 100/100 = 255; 50/100 = 127.5.
        let d = blended(b, s, BlendMode::Divide);
        assert_eq!(&d[..2], &[255, 255]);
        assert!((127..=128).contains(&d[2]));
        assert_eq!(blended(b, s, BlendMode::Difference), [100, 0, 50, 255]);
        assert_eq!(blended(b, s, BlendMode::Darken), [100, 100, 50, 255]);
        assert_eq!(blended(b, s, BlendMode::Lighten), [200, 100, 100, 255]);
        // Screen of white is white; multiply by white changes nothing.
        assert_eq!(
            blended(b, [255; 3], BlendMode::Screen),
            [255, 255, 255, 255]
        );
        assert_eq!(
            blended(b, [255; 3], BlendMode::Multiply),
            [200, 100, 50, 255]
        );
        // Luminosity of grey on a colour keeps no hue difference: grey.
        let l = blended([255, 0, 0], [128, 128, 128], BlendMode::Saturation);
        assert!(l[0] == l[1] && l[1] == l[2], "{l:?}");
    }

    #[test]
    fn transparency_and_opacity_behave() {
        // Over nothing, any mode just shows the source.
        let mut empty = Pixmap::new(1, 1).unwrap();
        blend(&mut empty, &px([10, 20, 30], 255), BlendMode::Subtract, 1.0);
        assert_eq!(empty.data(), &[10, 20, 30, 255]);
        // Opacity 0 changes nothing; half opacity lands halfway.
        let mut b = px([200, 200, 200], 255);
        blend(&mut b, &px([100, 100, 100], 255), BlendMode::Subtract, 0.0);
        assert_eq!(b.data(), &[200, 200, 200, 255]);
        blend(&mut b, &px([100, 100, 100], 255), BlendMode::Subtract, 0.5);
        assert_eq!(b.data(), &[150, 150, 150, 255]);
    }
}
