//! What zooming a busy drawing costs per frame, tessellating every line:
//! `cargo run --release -p symphony-vector --example zoom_bench`
use symphony_core::id::Id;
use symphony_core::math::Affine;
use symphony_core::stroke::{Point, Stroke, StrokeKind};

fn main() {
    // 600 wavy lines of 60 points, like a dense sketch.
    let strokes: Vec<Stroke> = (0..600)
        .map(|i| Stroke {
            id: Id(i),
            kind: StrokeKind::Pencil,
            art: Default::default(),
            swatch: Id(0),
            min_width: 1.0,
            max_width: 4.0,
            points: (0..60)
                .map(|k| Point {
                    x: -900.0 + (i % 30) as f32 * 60.0 + k as f32 * 3.0,
                    y: -500.0 + (i / 30) as f32 * 50.0 + (k as f32 * 0.4).sin() * 12.0,
                    pressure: 0.5 + 0.5 * (k as f32 * 0.2).sin(),
                    width_offset: 0.0,
                    pen: Default::default(),
                })
                .collect(),
            tips: Default::default(),
            texture: None,
            shape: Vec::new(),
        })
        .collect();
    for zoom in [1.0f32, 4.0, 16.0] {
        let m = Affine::scale(zoom, zoom);
        let start = std::time::Instant::now();
        let mut tris = 0;
        for st in &strokes {
            tris += symphony_vector::tessellate(st, m, 0.6).indices.len() / 3;
        }
        println!(
            "zoom {zoom}x: {:?} per frame, {tris} triangles",
            start.elapsed()
        );
    }
}
