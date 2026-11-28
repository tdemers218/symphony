//! How long stamping takes for a long stroke:
//! `cargo run --release -p symphony-vector --example stamp_bench`

use symphony_core::id::Id;
use symphony_core::math::Affine;
use symphony_core::stroke::{Point, Stroke, StrokeKind, StrokeTexture, Texture};

fn main() {
    let tip = Texture {
        width: 256,
        height: 256,
        pixels: (0..256 * 256).map(|i| (i % 7 * 40) as u8).collect(),
    };
    let grain = Texture {
        width: 512,
        height: 512,
        pixels: (0..512 * 512).map(|i| ((i * 37) % 255) as u8).collect(),
    };
    for (len, width, spacing) in [
        (1000.0, 20.0, 0.1),
        (1000.0, 20.0, 0.03),
        (2000.0, 60.0, 0.03),
    ] {
        let points = (0..=400)
            .map(|i| {
                let t = i as f32 / 400.0;
                Point {
                    x: t * len,
                    y: (t * 20.0).sin() * 50.0 + 100.0,
                    pressure: 1.0,
                    width_offset: 0.0,
                    pen: Default::default(),
                }
            })
            .collect();
        let st = Stroke {
            id: Id(1),
            kind: StrokeKind::Brush,
            art: Default::default(),
            swatch: Id(0),
            min_width: width,
            max_width: width,
            points,
            tips: Default::default(),
            texture: None,
            shape: Vec::new(),
        };
        let tex = StrokeTexture {
            tip: Some(1),
            grain: None,
            spacing,
            opacity: 1.0,
            angle: 0.0,
            follow: true,
            grain_scale: 1.0,
            flow: 1.0,
            hardness: 1.0,
            opacity_by_pressure: [1.0; 4],
            ..Default::default()
        };
        for grained in [false, true] {
            let start = std::time::Instant::now();
            let n = 10;
            for _ in 0..n {
                stamp_once(&st, &tex, &tip, grained.then_some(&grain));
            }
            println!(
                "{len}px long, {width}px wide, spacing {spacing}{}: {:?} per stroke",
                if grained { ", subtract grain" } else { "" },
                start.elapsed() / n
            );
        }
    }
}

fn stamp_once(st: &Stroke, tex: &StrokeTexture, tip: &Texture, grain: Option<&Texture>) {
    let tex = StrokeTexture {
        grain_mode: symphony_core::stroke::GrainMode::Subtract,
        ..*tex
    };
    let c = symphony_vector::stamp::rasterize(
        st,
        &tex,
        Some(tip),
        grain,
        Affine::scale(2.0, 2.0),
        [-100_000, -100_000, 100_000, 100_000],
    );
    std::hint::black_box(c);
}
