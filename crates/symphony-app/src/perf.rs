//! Performance testing in the real app (ENGINEERING: performance is
//! design). With environment variables:
//! - `SYMPHONY_PERF_SCENE=heavy`: start on a generated heavy scene (40
//!   layers, thousands of plain, textured and vector-brush strokes,
//!   animated pegs, a few effects, 120 frames).
//! - `SYMPHONY_PERF_VIEW=<tab title>`: bring that view to the front.
//! - `SYMPHONY_PERF_PLAY=1`: start playing.
//! - `SYMPHONY_PERF_SECONDS=<n>`: quit after n seconds and print a frame
//!   time summary (with `SYMPHONY_PERF_SPIN=1` to redraw every frame).
//! - `SYMPHONY_PERF_OPEN=<project>`: open a real project (measure on real
//!   art: synthetic scenes miss things).
//! - `SYMPHONY_PERF_NAV=zoom|pan`: keep navigating.
//!
//! The full list and the rules are in docs/ENGINEERING.md E6.

use symphony_core::stroke::{Point, Stroke, StrokeKind, StrokeTexture};

use crate::session::Session;

/// A scene heavy enough to show where frame time goes.
pub fn heavy_scene() -> Session {
    let mut s = Session::new_scene();
    s.set_length(120);
    for _ in 0..39 {
        s.add_layer();
    }
    let swatch = s.swatch;
    let layers = s.layer_count();
    for layer in 0..layers {
        for frame in [1u32, 13, 25, 37] {
            for k in 0..20 {
                let i = layer * 1000 + frame as usize * 20 + k;
                let textured = i.is_multiple_of(7);
                let vector_brush = i.is_multiple_of(11);
                let points: Vec<Point> = (0..60)
                    .map(|j| {
                        let t = j as f32 / 60.0;
                        Point {
                            x: -880.0 + (layer % 8) as f32 * 220.0 + t * 200.0,
                            y: -480.0
                                + (layer / 8) as f32 * 190.0
                                + k as f32 * 8.0
                                + (t * 9.0 + i as f32).sin() * 12.0,
                            pressure: 0.3 + 0.7 * (t * std::f32::consts::PI).sin(),
                            width_offset: 0.0,
                            pen: Default::default(),
                        }
                    })
                    .collect();
                let texture = (textured || vector_brush).then(|| StrokeTexture {
                    spacing: 0.15,
                    vector: vector_brush,
                    ..Default::default()
                });
                let st = Stroke {
                    id: s.doc.ids.fresh(),
                    kind: StrokeKind::Brush,
                    art: Default::default(),
                    swatch,
                    min_width: 1.0,
                    max_width: if textured { 9.0 } else { 4.0 },
                    points,
                    tips: Default::default(),
                    texture,
                    shape: Vec::new(),
                };
                s.commit_stroke(layer, frame, st);
            }
            // Hold each drawing for 12 frames.
            let held = s.doc.exposed(layer, frame);
            let column = s.doc.layers[layer].column;
            if let Some(col) = s.doc.xsheet.column_mut(column) {
                for f in frame + 1..frame + 12 {
                    col.set(f, held);
                }
            }
        }
    }
    // A few effects (they take the full renderer instead of the GPU path).
    if std::env::var("SYMPHONY_PERF_SCENE").is_ok_and(|v| v == "effects") {
        for layer in [0, 5, 10] {
            let node = s.doc.layers[layer].node;
            crate::views::node::insert_near(
                &mut s,
                node,
                symphony_core::graph::NodeKind::Effect(symphony_core::graph::Effect::Blur {
                    radius: 4.0,
                    mode: Default::default(),
                }),
            );
        }
    }
    // Animated pegs.
    for layer in 0..layers {
        let peg = s.doc.layers[layer].peg;
        s.select_graph_node(peg, false);
        for (frame, dx) in [(1u32, 0.0f32), (60, 40.0), (120, 0.0)] {
            s.frame = frame;
            s.adjust_peg(layer, |v| v.x = dx);
            s.end_gesture();
        }
    }
    s.frame = 1;
    s.history.clear();
    s
}

/// SYMPHONY_PERF_SECONDS: frame times so far, and whether to quit now.
pub struct Run {
    started: std::time::Instant,
    seconds: f32,
    /// CPU time per frame (ms), after warm-up.
    frames: Vec<f32>,
    /// Wall time between frames (ms): includes GPU and display waits, so
    /// it's what smoothness actually feels like.
    gaps: Vec<f32>,
    last: Option<std::time::Instant>,
    /// The slowest frame during warm-up (opening, first stamps).
    warmup_worst: f32,
}

impl Run {
    pub fn from_env() -> Option<Self> {
        let seconds = std::env::var("SYMPHONY_PERF_SECONDS").ok()?.parse().ok()?;
        Some(Run {
            started: std::time::Instant::now(),
            seconds,
            frames: Vec::new(),
            gaps: Vec::new(),
            last: None,
            warmup_worst: 0.0,
        })
    }

    /// Note a frame; when the time is up, print a summary and quit the
    /// process (a timed run never stops at "Save changes?").
    pub fn frame(&mut self, spent: std::time::Duration) {
        let now = std::time::Instant::now();
        let ms = spent.as_secs_f32() * 1000.0;
        let warm = self.started.elapsed().as_secs_f32() > 1.5;
        if warm {
            self.frames.push(ms);
            if let Some(last) = self.last {
                self.gaps
                    .push(now.duration_since(last).as_secs_f32() * 1000.0);
            }
        } else {
            self.warmup_worst = self.warmup_worst.max(ms);
        }
        self.last = Some(now);
        if self.started.elapsed().as_secs_f32() < self.seconds + 1.5 {
            return;
        }
        let stats = |v: &[f32]| {
            let mut f = v.to_vec();
            f.sort_by(f32::total_cmp);
            let at = |q: f32| {
                f.get(((f.len() as f32 - 1.0) * q) as usize)
                    .copied()
                    .unwrap_or(0.0)
            };
            (
                at(0.5),
                at(0.95),
                at(0.99),
                f.last().copied().unwrap_or(0.0),
            )
        };
        let (m, p95, p99, w) = stats(&self.frames);
        let (gm, gp95, gp99, gw) = stats(&self.gaps);
        let janks = self.gaps.iter().filter(|g| **g > 16.7).count();
        println!(
            "perf run: {} frames in {:.1} s | CPU median {m:.2} p95 {p95:.2} p99 {p99:.2} worst {w:.2} ms | \
             between frames median {gm:.2} p95 {gp95:.2} p99 {gp99:.2} worst {gw:.2} ms, {janks} over 16.7 ms | \
             warm-up worst {:.1} ms",
            self.frames.len(),
            self.seconds,
            self.warmup_worst
        );
        std::process::exit(0);
    }
}
