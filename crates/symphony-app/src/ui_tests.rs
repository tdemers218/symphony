//! UI tests: drive the real Camera view with simulated pen input, the way
//! an artist would, and check the scene that results. These catch bugs in
//! the wiring between input, tools and the document that unit tests of
//! each part can't see.

use eframe::egui::{self, Pos2, Vec2};
use egui_kittest::Harness;
use symphony_core::stroke::{ArtLayer, StrokeKind};
use symphony_pen::{PenSample, Phase, Tool as PenTool};

use crate::keymap::Profile;
use crate::session::{Session, Tool};
use crate::views::camera::{self, CanvasState};

pub struct Rig {
    pub s: Session,
    pub canvas: CanvasState,
    samples: Vec<PenSample>,
}

pub fn rig() -> Harness<'static, Rig> {
    let mut s = Session::new_scene();
    s.animate = false;
    let rig = Rig {
        s,
        canvas: CanvasState::default(),
        samples: Vec::new(),
    };
    let mut h = Harness::builder()
        .with_size(Vec2::new(1000.0, 700.0))
        .build_ui_state(
            |ui, r: &mut Rig| {
                let samples = std::mem::take(&mut r.samples);
                camera::ui(
                    ui,
                    &mut r.s,
                    &mut r.canvas,
                    &samples,
                    false,
                    Profile::Harmony,
                );
            },
            rig,
        );
    h.run_steps(2);
    h
}

fn sample(p: Pos2, phase: Phase, pressure: f32, tool: PenTool) -> PenSample {
    PenSample {
        x: p.x as f64,
        y: p.y as f64,
        pressure,
        tilt_x: 0.0,
        tilt_y: 0.0,
        rotation: 0.0,
        tangential: 0.0,
        phase,
        tool,
        buttons: 0,
        time: 0.0,
    }
}

/// Drag through scene points with the current tool (or the pen's eraser
/// end when `tool` is `PenTool::Eraser`), one frame per point.
pub fn drag(h: &mut Harness<'static, Rig>, scene: &[(f32, f32)], pressure: f32, tool: PenTool) {
    let n = scene.len();
    for (i, &(x, y)) in scene.iter().enumerate() {
        let p = h.state().canvas.to_screen(x, y);
        let phase = match i {
            0 => Phase::Down,
            _ if i + 1 == n => Phase::Up,
            _ => Phase::Move,
        };
        h.input_mut().events.push(egui::Event::PointerMoved(p));
        h.state_mut().samples.push(sample(p, phase, pressure, tool));
        h.step();
    }
    h.run_steps(2);
}

/// A straight line of `steps` points from a to b.
pub fn line(a: (f32, f32), b: (f32, f32), steps: usize) -> Vec<(f32, f32)> {
    (0..=steps)
        .map(|i| {
            let t = i as f32 / steps as f32;
            (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t)
        })
        .collect()
}

pub fn click(h: &mut Harness<'static, Rig>, x: f32, y: f32) {
    drag(h, &[(x, y), (x, y)], 1.0, PenTool::Pen);
}

/// The strokes of the drawing at frame 1 of layer `layer`.
pub fn strokes(h: &Harness<'static, Rig>, layer: usize) -> Vec<symphony_core::stroke::Stroke> {
    let s = &h.state().s;
    s.doc
        .exposed(layer, 1)
        .and_then(|d| s.doc.drawings.get(&d))
        .map(|d| d.strokes.clone())
        .unwrap_or_default()
}

pub fn fills(h: &Harness<'static, Rig>, layer: usize) -> Vec<symphony_core::stroke::Fill> {
    let s = &h.state().s;
    s.doc
        .exposed(layer, 1)
        .and_then(|d| s.doc.drawings.get(&d))
        .map(|d| d.fills.clone())
        .unwrap_or_default()
}

/// A closed square drawn with the brush, from (x0,y0) to (x1,y1).
pub fn draw_square(h: &mut Harness<'static, Rig>, x0: f32, y0: f32, x1: f32, y1: f32) {
    h.state_mut().s.tool = Tool::Brush;
    let mut pts = line((x0, y0), (x1, y0), 8);
    pts.extend(line((x1, y0), (x1, y1), 8));
    pts.extend(line((x1, y1), (x0, y1), 8));
    pts.extend(line((x0, y1), (x0, y0), 8));
    drag(h, &pts, 1.0, PenTool::Pen);
}

#[test]
fn drawing_a_stroke_adds_one_brush_stroke() {
    let mut h = rig();
    h.state_mut().s.tool = Tool::Brush;
    drag(
        &mut h,
        &line((-100.0, 0.0), (100.0, 0.0), 20),
        1.0,
        PenTool::Pen,
    );
    let st = strokes(&h, 0);
    assert_eq!(st.len(), 1);
    assert_eq!(st[0].kind, StrokeKind::Brush);
    assert_eq!(st[0].art, ArtLayer::LineArt);
    assert!(st[0].points.len() >= 2);
    // The line ends where the pen lifted (it used to stop short).
    let end = st[0].points.last().unwrap();
    assert!((end.x - 100.0).abs() < 0.5 && end.y.abs() < 0.5, "{end:?}");
}

#[test]
fn brush_size_is_fixed_to_the_drawing_by_default() {
    let mut h = rig();
    let size = h.state().s.brush.max_width;
    h.state_mut().canvas.set_zoom(2.0);
    h.run_steps(2);
    h.state_mut().s.tool = Tool::Brush;
    drag(
        &mut h,
        &line((-50.0, 0.0), (50.0, 0.0), 10),
        1.0,
        PenTool::Pen,
    );
    let st = strokes(&h, 0);
    // Width in drawing units equals the setting, whatever the zoom.
    assert!(
        (st[0].max_width - size).abs() < 0.01,
        "{} vs {size}",
        st[0].max_width
    );

    // With "Brush size follows zoom", sizes are screen points: the line's
    // width in the drawing is the size divided by the on-screen scale.
    h.state_mut().s.size_follows_zoom = true;
    let c = &h.state().canvas;
    let screen_per_scene = c.to_screen(100.0, 0.0).distance(c.to_screen(0.0, 0.0)) / 100.0;
    drag(
        &mut h,
        &line((-50.0, 40.0), (50.0, 40.0), 10),
        1.0,
        PenTool::Pen,
    );
    let want = size / screen_per_scene;
    let st = strokes(&h, 0);
    assert!(
        (st[1].max_width - want).abs() < 0.05,
        "{} vs {want}",
        st[1].max_width
    );
    // Zooming in further makes newly drawn lines finer in the drawing.
    h.state_mut().canvas.set_zoom(4.0);
    h.run_steps(2);
    drag(
        &mut h,
        &line((-30.0, 10.0), (30.0, 10.0), 10),
        1.0,
        PenTool::Pen,
    );
    let st = strokes(&h, 0);
    assert!(
        (st[2].max_width - want / 2.0).abs() < 0.05,
        "{} vs {}",
        st[2].max_width,
        want / 2.0
    );
}

#[test]
fn painting_inside_a_closed_shape_fills_it() {
    let mut h = rig();
    draw_square(&mut h, -100.0, -100.0, 100.0, 100.0);
    h.state_mut().s.tool = Tool::Paint;
    click(&mut h, 0.0, 0.0);
    let f = fills(&h, 0);
    let st = strokes(&h, 0);
    assert_eq!(
        f.len(),
        1,
        "one painted zone; notice {:?}; ends {:?} {:?}",
        h.state().s.notice,
        st.first().map(|s| s.points[0]),
        st.first().and_then(|s| s.points.last().copied())
    );
    assert!(f[0].contains(0.0, 0.0));
    assert!(!f[0].contains(150.0, 0.0));
}

/// Drag the eraser (the Eraser tool, full pressure) along a line.
fn erase_line(h: &mut Harness<'static, Rig>, a: (f32, f32), b: (f32, f32)) {
    h.state_mut().s.tool = Tool::Eraser;
    drag(h, &line(a, b, 6), 1.0, PenTool::Pen);
}

fn brush_line(h: &mut Harness<'static, Rig>, a: (f32, f32), b: (f32, f32)) {
    h.state_mut().s.tool = Tool::Brush;
    drag(h, &line(a, b, 20), 1.0, PenTool::Pen);
}

#[test]
fn erasing_across_a_brush_line_splits_it_with_the_eraser_shape() {
    let mut h = rig();
    brush_line(&mut h, (-200.0, 0.0), (200.0, 0.0));
    erase_line(&mut h, (0.0, -60.0), (0.0, 60.0));
    let st = strokes(&h, 0);
    assert_eq!(st.len(), 2, "cut in two");
    for piece in &st {
        assert!(!piece.shape.is_empty(), "brush pieces are outlines");
        assert!(
            !symphony_vector::hits(piece, 0.0, 0.0, 0.0),
            "nothing left under the eraser"
        );
    }
    // The eraser end of the pen erases too, whatever tool is active.
    h.state_mut().s.tool = Tool::Brush;
    drag(
        &mut h,
        &line((-150.0, -60.0), (-150.0, 60.0), 6),
        1.0,
        PenTool::Eraser,
    );
    assert_eq!(strokes(&h, 0).len(), 3, "the pen's eraser end cut again");
}

#[test]
fn erasing_paint_leaves_a_hole() {
    let mut h = rig();
    draw_square(&mut h, -150.0, -150.0, 150.0, 150.0);
    h.state_mut().s.tool = Tool::Paint;
    click(&mut h, 0.0, 0.0);
    assert!(fills(&h, 0)[0].contains(0.0, 0.0));
    erase_line(&mut h, (-10.0, 0.0), (10.0, 0.0));
    let f = fills(&h, 0);
    assert!(!f.is_empty(), "the rest of the paint stays");
    assert!(
        !f.iter().any(|f| f.contains(0.0, 0.0)),
        "a hole where the eraser went"
    );
    assert!(f.iter().any(|f| f.contains(100.0, 100.0)));
}

#[test]
fn erase_all_layers_reaches_through_every_visible_layer() {
    let mut h = rig();
    brush_line(&mut h, (-200.0, 0.0), (200.0, 0.0));
    h.state_mut().s.add_layer();
    let top = h.state().s.selected;
    brush_line(&mut h, (-200.0, 0.0), (200.0, 0.0));
    let bottom = if top == 0 { 1 } else { 0 };
    // Only the selected layer by default.
    erase_line(&mut h, (-100.0, -60.0), (-100.0, 60.0));
    assert_eq!(strokes(&h, top).len(), 2);
    assert_eq!(strokes(&h, bottom).len(), 1);
    // Every visible layer with the option on.
    h.state_mut().s.erase_all_layers = true;
    erase_line(&mut h, (100.0, -60.0), (100.0, 60.0));
    assert_eq!(strokes(&h, top).len(), 3);
    assert_eq!(strokes(&h, bottom).len(), 2);
}

#[test]
fn erase_all_frames_reaches_every_drawing_of_the_layer() {
    let mut h = rig();
    brush_line(&mut h, (-200.0, 0.0), (200.0, 0.0));
    h.state_mut().s.frame = 2;
    brush_line(&mut h, (-200.0, 0.0), (200.0, 0.0));
    let s = &h.state().s;
    let (d1, d2) = (s.doc.exposed(0, 1).unwrap(), s.doc.exposed(0, 2).unwrap());
    assert_ne!(d1, d2, "two drawings");
    let count = |h: &Harness<'static, Rig>, d| h.state().s.doc.drawings[&d].strokes.len();
    h.state_mut().s.erase_all_frames = true;
    erase_line(&mut h, (0.0, -60.0), (0.0, 60.0));
    assert_eq!(count(&h, d1), 2);
    assert_eq!(count(&h, d2), 2);
    // One drag is one undo, across all the drawings it touched.
    h.state_mut().s.undo();
    assert_eq!(count(&h, d1), 1);
    assert_eq!(count(&h, d2), 1);
}

/// Renders an erased line to a PNG for a human look (not a pass/fail
/// check): `SYMPHONY_UI_SHOT=path cargo test -p symphony-app erased_line_look`.
#[test]
fn erased_line_look() {
    let Ok(path) = std::env::var("SYMPHONY_UI_SHOT") else {
        return;
    };
    let mut h = rig();
    h.state_mut().s.brush.max_width = 40.0;
    h.state_mut().s.brush.min_width = 40.0;
    brush_line(&mut h, (-300.0, 0.0), (300.0, 0.0));
    h.state_mut().s.eraser.max_width = 50.0;
    erase_line(&mut h, (-120.0, -80.0), (-120.0, 80.0));
    erase_line(&mut h, (60.0, -40.0), (140.0, -10.0));
    let s = &h.state().s;
    let img = symphony_render::render_frame(&s.doc, 1, 960, 540).unwrap();
    img.save(path).unwrap();
}

#[test]
fn resizing_the_view_keeps_the_drawing_scale() {
    let mut h = rig();
    let scale = |h: &Harness<'static, Rig>| {
        let c = &h.state().canvas;
        c.to_screen(100.0, 0.0).distance(c.to_screen(0.0, 0.0)) / 100.0
    };
    let fitted = scale(&h);
    assert!(
        fitted > 0.0 && fitted < 1.0,
        "the 1920-wide stage was fitted into 1000 px: {fitted}"
    );
    // Dragging a panel edge: the view shrinks, the drawing doesn't.
    for w in [900.0, 760.0, 640.0] {
        h.set_size(Vec2::new(w, 700.0));
        h.run_steps(2);
        assert!(
            (scale(&h) - fitted).abs() < 1e-4,
            "scale changed at width {w}"
        );
    }
    // 100% is actual size: one scene unit per screen point.
    h.state_mut().canvas.apply(crate::keymap::Action::ResetZoom);
    h.run_steps(2);
    assert!((scale(&h) - 1.0).abs() < 1e-4);
    // Reset View fits the stage to the (now smaller) view again.
    h.state_mut().canvas.apply(crate::keymap::Action::ResetView);
    h.run_steps(2);
    assert!(scale(&h) < fitted, "refitted to the narrower view");
}

/// Frame cost of a busy scene, still and while the view is being resized.
/// Not a pass/fail test: `cargo test --release -p symphony-app
/// frame_cost -- --ignored --nocapture`.
#[test]
#[ignore]
fn frame_cost() {
    use symphony_core::stroke::{Point, Stroke};
    let mut h = rig();
    // 600 pressure strokes of 60 points each, like a dense character page.
    {
        let s = &mut h.state_mut().s;
        let swatch = s.swatch;
        for i in 0..600 {
            let y = -400.0 + (i % 60) as f32 * 13.0;
            let x0 = -700.0 + (i / 60) as f32 * 140.0;
            let st = Stroke {
                id: s.doc.ids.fresh(),
                kind: StrokeKind::Brush,
                art: Default::default(),
                swatch,
                min_width: 1.0,
                max_width: 6.0,
                points: (0..60)
                    .map(|k| {
                        let t = k as f32 / 59.0;
                        Point {
                            x: x0 + t * 120.0,
                            y: y + (t * 9.0).sin() * 5.0,
                            pressure: (t * 3.1).sin(),
                            pen: Default::default(),
                        }
                    })
                    .collect(),
                tips: Default::default(),
                texture: None,
                shape: Vec::new(),
            };
            s.commit_stroke(0, 1, st);
        }
    }
    h.run_steps(3);
    let time = |h: &mut Harness<'static, Rig>, resize: bool| {
        let n = 30;
        let start = std::time::Instant::now();
        for i in 0..n {
            if resize {
                h.set_size(Vec2::new(1000.0 - (i % 15) as f32 * 8.0, 700.0));
            }
            h.step();
        }
        start.elapsed() / n
    };
    let still = time(&mut h, false);
    let resizing = time(&mut h, true);
    println!("frame_cost: still {still:?} per frame, resizing {resizing:?} per frame");
}

#[test]
fn drawn_lines_keep_few_points_and_follow_the_stroke_detail_setting() {
    // A gentle curve drawn with many pen samples.
    let curve: Vec<(f32, f32)> = (0..=120)
        .map(|i| {
            let t = i as f32 / 120.0;
            (-200.0 + t * 400.0, (t * std::f32::consts::PI).sin() * 80.0)
        })
        .collect();
    let mut h = rig();
    h.state_mut().s.tool = Tool::Brush;
    h.state_mut().s.brush.smoothing = 0.0;
    h.state_mut().s.stroke_detail = 1.0;
    drag(&mut h, &curve, 1.0, PenTool::Pen);
    let full = strokes(&h, 0)[0].points.len();
    h.state_mut().s.stroke_detail = 0.3;
    drag(&mut h, &curve, 1.0, PenTool::Pen);
    let light = strokes(&h, 0)[1].points.len();
    assert!(full > 100, "full detail keeps every sample: {full}");
    assert!(
        light * 4 < full,
        "lower detail keeps far fewer: {light} of {full}"
    );
}

/// Renders a lightly kept curve big, for a human look at its smoothness:
/// `SYMPHONY_UI_SHOT=path cargo test -p symphony-app curve_look`.
#[test]
fn curve_look() {
    let Ok(path) = std::env::var("SYMPHONY_UI_SHOT") else {
        return;
    };
    let mut h = rig();
    h.state_mut().s.tool = Tool::Brush;
    h.state_mut().s.brush.max_width = 30.0;
    h.state_mut().s.brush.min_width = 4.0;
    h.state_mut().s.stroke_detail = 0.3;
    let curve: Vec<(f32, f32)> = (0..=150)
        .map(|i| {
            let t = i as f32 / 150.0;
            (-600.0 + t * 1200.0, (t * 9.0).sin() * 250.0)
        })
        .collect();
    drag(&mut h, &curve, 1.0, PenTool::Pen);
    let s = &h.state().s;
    eprintln!(
        "kept {} of {} points",
        strokes(&h, 0)[0].points.len(),
        curve.len()
    );
    symphony_render::render_frame(&s.doc, 1, 1920, 1080)
        .unwrap()
        .save(path)
        .unwrap();
}

#[test]
fn erasing_a_textured_line_keeps_its_texture_and_cuts_its_shape() {
    use symphony_core::stroke::StrokeTexture;
    let mut h = rig();
    h.state_mut().s.brush.texture = Some(StrokeTexture {
        tip: None,
        grain: None,
        spacing: 0.1,
        opacity: 1.0,
        angle: 0.0,
        follow: false,
        grain_scale: 1.0,
        flow: 1.0,
        hardness: 1.0,
        opacity_by_pressure: [1.0; 4],
        ..Default::default()
    });
    h.state_mut().s.brush.min_width = 20.0;
    h.state_mut().s.brush.max_width = 20.0;
    brush_line(&mut h, (-200.0, 0.0), (200.0, 0.0));
    erase_line(&mut h, (0.0, -60.0), (0.0, 60.0));
    let st = strokes(&h, 0);
    assert_eq!(st.len(), 2, "cut in two");
    for piece in &st {
        assert!(piece.texture.is_some(), "still textured");
        assert!(!piece.shape.is_empty(), "clipped to an outline");
        // Nothing of it is stamped where the eraser went.
        let c = symphony_vector::stamp::rasterize(
            piece,
            piece.texture.as_ref().unwrap(),
            None,
            None,
            symphony_core::math::Affine::IDENTITY,
            [-1000, -1000, 1000, 1000],
        )
        .unwrap();
        let (cx, cy) = (0 - c.x, 0 - c.y);
        if cx >= 0 && cy >= 0 && cx < c.width as i32 && cy < c.height as i32 {
            assert_eq!(c.data[(cy * c.width as i32 + cx) as usize], 0);
        }
    }
}

#[test]
fn line_editor_pushes_lines_and_moves_shift_picked_points() {
    let mut h = rig();
    h.state_mut().s.tool = Tool::Pencil;
    h.state_mut().s.pencil.smoothing = 0.0;
    drag(
        &mut h,
        &line((-200.0, 0.0), (200.0, 0.0), 20),
        1.0,
        PenTool::Pen,
    );
    let before = strokes(&h, 0)[0].clone();

    // Push the middle of the line downwards.
    h.state_mut().s.tool = Tool::LineEditor;
    h.state_mut().s.line_edit_radius = 60.0;
    drag(&mut h, &line((0.0, 0.0), (0.0, 30.0), 6), 1.0, PenTool::Pen);
    let pushed = strokes(&h, 0)[0].clone();
    let y_near = |st: &symphony_core::stroke::Stroke, x: f32| {
        st.points
            .iter()
            .min_by(|a, b| (a.x - x).abs().total_cmp(&(b.x - x).abs()))
            .map(|p| p.y)
            .unwrap()
    };
    assert!(
        y_near(&pushed, 0.0) > 20.0,
        "the middle moved: {}",
        y_near(&pushed, 0.0)
    );
    assert!(y_near(&pushed, -200.0).abs() < 0.5, "the ends stayed");
    // One drag, one undo.
    h.state_mut().s.undo();
    assert_eq!(strokes(&h, 0)[0], before);

    // Shift-drag over the right end picks its points...
    h.input_mut()
        .events
        .push(egui::Event::ModifiersChanged(egui::Modifiers::SHIFT));
    drag(
        &mut h,
        &line((190.0, 0.0), (200.0, 0.0), 2),
        1.0,
        PenTool::Pen,
    );
    h.input_mut()
        .events
        .push(egui::Event::ModifiersChanged(egui::Modifiers::NONE));
    assert!(!h.state().s.line_points.is_empty(), "points picked");
    // ...then a plain drag moves only those.
    drag(&mut h, &line((0.0, 0.0), (0.0, 50.0), 4), 1.0, PenTool::Pen);
    let moved = strokes(&h, 0)[0].clone();
    assert!(y_near(&moved, 200.0) > 40.0, "the picked end moved");
    assert!(y_near(&moved, -200.0).abs() < 0.5, "the other end didn't");
}

/// Cost of Line Editor pushes on a busy page. Not pass/fail:
/// `cargo test --release -p symphony-app line_edit_cost -- --ignored --nocapture`.
#[test]
#[ignore]
fn line_edit_cost() {
    use symphony_core::stroke::{Point, Stroke};
    let mut s = crate::session::Session::new_scene();
    let swatch = s.swatch;
    for i in 0..600 {
        let y = -400.0 + (i % 60) as f32 * 13.0;
        let x0 = -700.0 + (i / 60) as f32 * 140.0;
        let st = Stroke {
            id: s.doc.ids.fresh(),
            kind: StrokeKind::Pencil,
            art: Default::default(),
            swatch,
            min_width: 2.0,
            max_width: 2.0,
            points: (0..60)
                .map(|k| Point {
                    x: x0 + k as f32 * 2.0,
                    y,
                    pressure: 1.0,
                    pen: Default::default(),
                })
                .collect(),
            tips: Default::default(),
            texture: None,
            shape: Vec::new(),
        };
        s.commit_stroke(0, 1, st);
    }
    let d = s.doc.exposed(0, 1).unwrap();
    let start = std::time::Instant::now();
    for k in 0..60 {
        s.line_push(d, [-650.0 + k as f32, -400.0], 30.0, [1.0, 0.5]);
    }
    s.line_edit_finish(d, 0.5);
    println!("line_edit_cost: {:?} per push step", start.elapsed() / 60);
}

/// The toolbar offers what fits the work: a peg gets Transform, and
/// coming back to drawings returns to the last drawing tool.
#[test]
fn tools_follow_what_you_work_on() {
    use crate::session::{Tool, ToolContext};
    let mut s = crate::session::Session::new_scene();
    s.tool = Tool::Brush;
    s.fit_tool_to_context();
    assert_eq!(s.tool_context(), ToolContext::Drawing);
    assert!(
        !Tool::Transform.fits(ToolContext::Drawing),
        "Select transforms selections"
    );
    assert!(
        !Tool::Pencil.fits(ToolContext::Drawing),
        "the Brush makes vector lines"
    );
    let peg = s.doc.layers[0].peg;
    s.select_graph_node(peg, false);
    s.fit_tool_to_context();
    assert_eq!(s.tool_context(), ToolContext::Peg);
    assert_eq!(s.tool, Tool::Transform);
    assert!(
        !Tool::Brush.fits(ToolContext::Peg),
        "nothing to draw on a peg"
    );
    let node = s.doc.layers[0].node;
    s.select_graph_node(node, false);
    s.fit_tool_to_context();
    assert_eq!(s.tool, Tool::Brush, "back to the drawing tool");
}

/// Searching a blend mode adds a Blend Modes node straight onto the
/// selected layer: wired in, visible in the render, one undo removes it.
#[test]
fn search_puts_a_blend_mode_on_the_selected_layer() {
    use symphony_core::graph::{BlendMode, Effect, NodeKind};
    use symphony_core::stroke::{Point, Stroke};
    let mut s = Session::new_scene();
    // The same thick line on two layers, so the top one has something
    // behind it to blend with.
    s.add_layer();
    let swatch = s.swatch;
    for l in 0..2 {
        let st = Stroke {
            id: s.doc.ids.fresh(),
            kind: StrokeKind::Pencil,
            art: Default::default(),
            swatch,
            min_width: 40.0,
            max_width: 40.0,
            points: [(-300.0, 0.0), (300.0, 0.0)]
                .iter()
                .map(|&(x, y)| Point {
                    x,
                    y,
                    pressure: 1.0,
                    pen: Default::default(),
                })
                .collect(),
            tips: Default::default(),
            texture: None,
            shape: Vec::new(),
        };
        s.commit_stroke(l, s.frame, st);
    }
    s.selected = 0;
    let layer = s.doc.layers[0];
    let links = s.doc.graph.links().to_vec();
    let before = symphony_render::render_camera(&s.doc, s.frame, 160, 90, None).unwrap();
    let mut view = crate::views::node::NodeViewState::default();
    crate::views::node::add_from_search(
        &mut s,
        &mut view,
        NodeKind::Effect(Effect::Blend {
            mode: BlendMode::Difference,
            amount: 1.0,
        }),
    );
    let blend = s
        .doc
        .graph
        .nodes()
        .find(|(_, n)| matches!(n.kind, NodeKind::Effect(Effect::Blend { .. })))
        .map(|(id, _)| id)
        .expect("the node was added");
    assert_eq!(
        s.doc.graph.input(blend, 0),
        Some(layer.node),
        "fed by the layer"
    );
    assert!(
        s.doc.graph.links().iter().any(|l| l.from == blend),
        "and feeds where the layer went"
    );
    let after = symphony_render::render_camera(&s.doc, s.frame, 160, 90, None).unwrap();
    assert!(before.data() != after.data(), "the render changes");
    s.undo();
    assert_eq!(s.doc.graph.links(), &links[..], "one undo rewires back");
    assert!(s.doc.graph.node(blend).is_none());
}

/// With `SYMPHONY_PERF_GATE=1` (scripts/perf.sh), fail a benchmark that
/// took longer than `budget_ms`. Budgets are about twice today's time on
/// an M1 under load, so only a real slowdown trips them.
fn perf_gate(what: &str, took: std::time::Duration, budget_ms: f32) {
    if std::env::var_os("SYMPHONY_PERF_GATE").is_none() {
        return;
    }
    let ms = took.as_secs_f32() * 1000.0;
    assert!(
        ms <= budget_ms,
        "{what} took {ms:.2} ms, over its {budget_ms} ms budget (ENGINEERING E6)"
    );
}

/// Cost of the eraser on a busy page of brush strokes and fills. Not
/// pass/fail: `cargo test --release -p symphony-app erase_cost -- --ignored --nocapture`.
#[test]
#[ignore]
fn erase_cost() {
    use symphony_core::stroke::{Fill, Point, Stroke};
    let mut s = crate::session::Session::new_scene();
    let swatch = s.swatch;
    for i in 0..400 {
        let y = -400.0 + (i % 40) as f32 * 20.0;
        let x0 = -700.0 + (i / 40) as f32 * 140.0;
        let st = Stroke {
            id: s.doc.ids.fresh(),
            kind: StrokeKind::Brush,
            art: Default::default(),
            swatch,
            min_width: 3.0,
            max_width: 8.0,
            points: (0..60)
                .map(|k| Point {
                    x: x0 + k as f32 * 2.0,
                    y: y + (k as f32 * 0.3).sin() * 4.0,
                    pressure: 0.7,
                    pen: Default::default(),
                })
                .collect(),
            tips: Default::default(),
            texture: None,
            shape: Vec::new(),
        };
        s.commit_stroke(0, 1, st);
    }
    let d = s.doc.exposed(0, 1).unwrap();
    for i in 0..200 {
        let (x, y) = (
            -700.0 + (i % 20) as f32 * 70.0,
            -400.0 + (i / 20) as f32 * 80.0,
        );
        let fill = Fill {
            linked: None,
            id: s.doc.ids.fresh(),
            swatch,
            art: Default::default(),
            contours: vec![(0..24)
                .map(|k| {
                    let a = k as f32 / 24.0 * std::f32::consts::TAU;
                    [x + a.cos() * 30.0, y + a.sin() * 30.0]
                })
                .collect()],
        };
        s.doc.drawings.get_mut(&d).unwrap().fills.push(fill);
    }
    let start = std::time::Instant::now();
    for k in 0..60 {
        let x = -650.0 + k as f32 * 4.0;
        s.erase(d, &[([x, -390.0], 10.0), ([x + 4.0, -390.0], 10.0)]);
    }
    let per_move = start.elapsed() / 60;
    println!("erase_cost: {per_move:?} per eraser move");
    perf_gate("eraser move on plain strokes", per_move, 2.0);
    let start = std::time::Instant::now();
    for _ in 0..5 {
        std::hint::black_box(symphony_render::render_camera(&s.doc, 1, 1920, 1080, None));
    }
    println!(
        "render_camera 1920x1080: {:?} per render",
        start.elapsed() / 5
    );
}

/// Frame cost of the Camera view on a busy scene at 2560x1440: idle,
/// panning, zooming and drawing. Not pass/fail:
/// `cargo test --release -p symphony-app busy_frame_cost -- --ignored --nocapture`.
#[test]
#[ignore]
fn busy_frame_cost() {
    use symphony_core::stroke::{Fill, Point, Stroke, StrokeTexture};
    let mut h = rig();
    h.set_size(Vec2::new(2560.0, 1440.0));
    {
        let s = &mut h.state_mut().s;
        let swatch = s.swatch;
        let line = |s: &mut crate::session::Session, i: usize, textured: bool| Stroke {
            id: s.doc.ids.fresh(),
            kind: StrokeKind::Brush,
            art: Default::default(),
            swatch,
            min_width: 2.0,
            max_width: if textured { 14.0 } else { 5.0 },
            points: (0..80)
                .map(|k| Point {
                    x: -900.0 + (i % 30) as f32 * 60.0 + k as f32 * 2.5,
                    y: -500.0 + (i / 30) as f32 * 45.0 + (k as f32 * 0.3).sin() * 10.0,
                    pressure: 0.4 + 0.6 * (k as f32 * 0.1).sin().abs(),
                    pen: Default::default(),
                })
                .collect(),
            tips: Default::default(),
            texture: textured.then(StrokeTexture::default),
            shape: Vec::new(),
        };
        for i in 0..640 {
            let st = line(s, i, i % 16 == 0);
            s.commit_stroke(0, 1, st);
        }
        let d = s.doc.exposed(0, 1).unwrap();
        for i in 0..100 {
            let (x, y) = (
                -900.0 + (i % 10) as f32 * 180.0,
                -500.0 + (i / 10) as f32 * 100.0,
            );
            let fill = Fill {
                linked: None,
                id: s.doc.ids.fresh(),
                swatch,
                art: Default::default(),
                contours: vec![(0..32)
                    .map(|k| {
                        let a = k as f32 / 32.0 * std::f32::consts::TAU;
                        [x + a.cos() * 40.0, y + a.sin() * 30.0]
                    })
                    .collect()],
            };
            s.doc.drawings.get_mut(&d).unwrap().fills.push(fill);
        }
        s.doc.version += 1;
    }
    // Let background stamping settle.
    for _ in 0..40 {
        h.step();
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let time = |h: &mut Harness<'static, Rig>,
                n: u32,
                f: &mut dyn FnMut(&mut Harness<'static, Rig>, u32)| {
        let start = std::time::Instant::now();
        for i in 0..n {
            f(h, i);
            h.step();
        }
        start.elapsed() / n
    };
    let idle = time(&mut h, 30, &mut |_, _| {});
    let zoom = time(&mut h, 30, &mut |h, _| {
        h.input_mut().events.push(egui::Event::Zoom(1.06));
    });
    let tool = h.state().s.tool;
    h.state_mut().s.tool = Tool::Pencil;
    let pts = line((-200.0, 0.0), (200.0, 60.0), 60);
    let start = std::time::Instant::now();
    drag(&mut h, &pts, 0.8, PenTool::Pen);
    let draw = start.elapsed() / 63;
    h.state_mut().s.tool = tool;
    println!("busy_frame_cost idle {idle:?}, zoom {zoom:?}, drawing {draw:?} per frame");
}

/// Mirror view flips only what you see: drawing still lands under the pen,
/// at the right scene coordinates.
#[test]
fn mirrored_view_draws_where_the_pen_is() {
    let mut h = rig();
    h.state_mut()
        .canvas
        .apply(crate::keymap::Action::MirrorView);
    h.run_steps(2);
    let (left, right) = (
        h.state().canvas.to_screen(-100.0, 0.0),
        h.state().canvas.to_screen(60.0, 0.0),
    );
    assert!(left.x > right.x, "scene left shows on screen right");
    h.state_mut().s.tool = Tool::Pencil;
    drag(
        &mut h,
        &line((-100.0, 0.0), (60.0, 0.0), 12),
        0.8,
        PenTool::Pen,
    );
    let st = &strokes(&h, 0)[0];
    let (first, last) = (st.points[0], *st.points.last().unwrap());
    assert!(
        (first.x + 100.0).abs() < 2.0 && (last.x - 60.0).abs() < 2.0,
        "{first:?} {last:?}"
    );
}

/// Each gesture is one undo step, however many frames it lasts, so a long
/// session doesn't run through the undo history.
#[test]
fn each_gesture_is_one_undo_step() {
    let mut h = rig();
    let steps = |h: &Harness<'static, Rig>| h.state().s.history.undo_len();
    h.state_mut().s.tool = Tool::Pencil;
    let before = steps(&h);
    drag(
        &mut h,
        &line((-150.0, 0.0), (150.0, 30.0), 40),
        0.8,
        PenTool::Pen,
    );
    assert_eq!(steps(&h), before + 1, "a stroke");
    h.state_mut().s.tool = Tool::Eraser;
    let before = steps(&h);
    drag(
        &mut h,
        &line((-100.0, -20.0), (100.0, 40.0), 40),
        0.8,
        PenTool::Pen,
    );
    assert_eq!(steps(&h), before + 1, "an eraser drag");
    h.state_mut().s.tool = Tool::LineEditor;
    let before = steps(&h);
    drag(
        &mut h,
        &line((-120.0, 5.0), (-60.0, 25.0), 30),
        0.8,
        PenTool::Pen,
    );
    assert!(steps(&h) <= before + 1, "a Line Editor drag");
}

/// Importing images end to end: a picture becomes its own layer held for
/// the scene and shows in the render; numbered pictures become one layer,
/// a frame each; one undo takes an import away.
#[test]
fn imported_images_become_layers_that_render() {
    let dir = std::env::temp_dir().join(format!("symphony-import-{}", std::process::id()));
    let cache = dir.join("cache");
    std::fs::create_dir_all(&dir).unwrap();
    let save = |name: &str, rgb: [u8; 3]| {
        let p = dir.join(name);
        image::RgbImage::from_pixel(64, 36, image::Rgb(rgb))
            .save(&p)
            .unwrap();
        p
    };
    let bg = save("bg.png", [220, 30, 30]);
    let seq: Vec<_> = (1..=3)
        .map(|i| save(&format!("walk_{i:03}.png"), [30, 30, 220]))
        .collect();

    let mut s = crate::session::Session::new_scene();
    s.doc.media_dirs = vec![cache.clone()];
    let layers = s.layer_count();
    let steps = s.history.undo_len();

    let out = crate::import::stage_images_into(&[bg], &cache);
    assert!(out.problems.is_empty(), "{:?}", out.problems);
    assert_eq!(s.import_images(out.groups), 1);
    assert_eq!(s.layer_count(), layers + 1);
    assert_eq!(s.history.undo_len(), steps + 1, "one undo step");
    let i = (0..s.layer_count())
        .find(|&i| s.layer_name(i) == "bg")
        .expect("named after the file");
    assert_eq!(
        i,
        s.layer_count() - 1,
        "a full-frame picture is a background: at the bottom"
    );
    let len = s.length();
    let d = s.doc.exposed(i, len).expect("held to the end of the scene");
    assert!(s.doc.drawings[&d].image.is_some());
    // It renders: the middle of the frame is the picture's red.
    let pix = symphony_render::render_camera(&s.doc, 1, 192, 108, None).unwrap();
    let mid = pix.pixel(96, 54).unwrap();
    assert!(mid.red() > 150 && mid.blue() < 100, "{mid:?}");

    // A numbered sequence: one layer, one frame each, from the current frame.
    s.frame = 5;
    let out = crate::import::stage_images_into(&seq, &cache);
    assert_eq!(s.import_images(out.groups), 3);
    let w = (0..s.layer_count())
        .find(|&i| s.layer_name(i) == "walk")
        .expect("one layer");
    let frames: Vec<_> = (5..=7).map(|f| s.doc.exposed(w, f).unwrap()).collect();
    assert!(frames[0] != frames[1] && frames[1] != frames[2]);

    s.undo();
    assert!((0..s.layer_count()).all(|i| s.layer_name(i) != "walk"));
    std::fs::remove_dir_all(&dir).ok();
}

/// Importing sound end to end: a clip at the current frame, the scene
/// lengthened to fit, the waveform ready, one undo removes it.
#[test]
fn imported_sound_lands_on_the_timeline() {
    let dir = std::env::temp_dir().join(format!("symphony-sound-{}", std::process::id()));
    let cache = dir.join("cache");
    std::fs::create_dir_all(&dir).unwrap();
    let wav = dir.join("line.wav");
    crate::audio::write_tone(&wav, 3.0);
    let mut s = crate::session::Session::new_scene();
    s.doc.media_dirs = vec![cache.clone()];
    s.doc.settings.fps = 24.0;
    let len = s.length();
    let out = crate::import::stage_sounds_into(&[wav], &cache);
    assert!(out.problems.is_empty(), "{:?}", out.problems);
    assert_eq!(s.import_sounds(out.sounds), 1);
    assert_eq!(s.doc.sounds.len(), 1);
    assert_eq!(s.doc.sounds[0].start, 1);
    assert_eq!(s.doc.sounds[0].name, "line");
    assert!(s.length() >= 72, "3 s at 24 fps fits: {}", s.length());
    assert!(s.doc.media_path(s.doc.sounds[0].media).is_some());
    s.undo();
    assert!(s.doc.sounds.is_empty());
    assert_eq!(s.length(), len);
    std::fs::remove_dir_all(&dir).ok();
}

/// Video import round trip: a 30 fps MP4 (made by Symphony's exporter)
/// becomes one layer resampled to a 24 fps scene, with frames that look
/// like the movie. Needs the OpenH264 codec; skipped without it.
#[test]
fn imported_video_becomes_a_frame_sequence() {
    let Some(codec) = crate::codec::installed() else {
        eprintln!("OpenH264 not installed: skipping");
        return;
    };
    let dir = std::env::temp_dir().join(format!("symphony-video-{}", std::process::id()));
    let cache = dir.join("cache");
    std::fs::create_dir_all(&dir).unwrap();
    // A 1 s, 30 fps movie of a red picture.
    let red = dir.join("red.png");
    image::RgbImage::from_pixel(64, 36, image::Rgb([220, 30, 30]))
        .save(&red)
        .unwrap();
    let mut src = crate::session::Session::new_scene();
    src.doc.media_dirs = vec![cache.clone()];
    src.doc.settings.fps = 30.0;
    let out = crate::import::stage_images_into(&[red], &cache);
    src.import_images(out.groups);
    let movie = dir.join("clip.mp4");
    let key = symphony_render::checkpoint::fingerprint(&src.doc, 320, 180);
    let ck = symphony_render::checkpoint::Checkpoint::folder(&movie, &key, 320, 180).unwrap();
    symphony_render::movie::export_mp4(
        &src.doc,
        &codec,
        &movie,
        &ck,
        1,
        30,
        320,
        180,
        None,
        &std::sync::atomic::AtomicBool::new(false),
        &|_, _| {},
    )
    .unwrap();
    ck.finish();

    let got = crate::video::import_into(&movie, &codec, 24.0, [1920, 1080], &cache);
    assert!(got.problems.is_empty(), "{:?}", got.problems);
    let g = got.frames.expect("frames");
    assert!(
        (23..=25).contains(&g.frames.len()),
        "1 s at 24 fps: {}",
        g.frames.len()
    );
    assert_eq!(g.name, "clip");
    let first = image::open(cache.join(&g.frames[0].file))
        .unwrap()
        .to_rgb8();
    let p = first.get_pixel(first.width() / 2, first.height() / 2);
    assert!(p[0] > 160 && p[1] < 90 && p[2] < 90, "looks red: {p:?}");

    let mut s = crate::session::Session::new_scene();
    s.doc.media_dirs = vec![cache.clone()];
    s.doc.settings.fps = 24.0;
    let before = s.layer_count();
    s.import_images(vec![g]);
    assert_eq!(s.layer_count(), before + 1);
    std::fs::remove_dir_all(&dir).ok();
}

/// Erasing heavy textured art: scribbling back and forth over the same
/// spot must stay fast, and erased shapes mustn't keep growing.
#[test]
#[ignore = "benchmark: cargo test --release textured_erase_cost -- --ignored --nocapture"]
fn textured_erase_cost() {
    use symphony_core::stroke::{Point, Stroke, StrokeTexture};
    let mut s = crate::session::Session::new_scene();
    let swatch = s.swatch;
    for i in 0..300 {
        let y = -300.0 + (i % 30) as f32 * 20.0;
        let x0 = -500.0 + (i / 30) as f32 * 100.0;
        let st = Stroke {
            id: s.doc.ids.fresh(),
            kind: StrokeKind::Brush,
            art: Default::default(),
            swatch,
            min_width: 10.0,
            max_width: 30.0,
            points: (0..120)
                .map(|k| Point {
                    x: x0 + k as f32 * 2.5,
                    y: y + (k as f32 * 0.2).sin() * 15.0,
                    pressure: 0.5 + (k as f32 * 0.1).sin() * 0.4,
                    pen: Default::default(),
                })
                .collect(),
            tips: Default::default(),
            texture: Some(StrokeTexture::default()),
            shape: Vec::new(),
        };
        s.commit_stroke(0, 1, st);
    }
    let d = s.doc.exposed(0, 1).unwrap();
    let mut worst = std::time::Duration::ZERO;
    let start = std::time::Instant::now();
    let moves = 400;
    for k in 0..moves {
        // Back and forth across a 300-unit patch, drifting down.
        let t = (k % 40) as f32 / 40.0;
        let x = -300.0 + if (k / 40) % 2 == 0 { t } else { 1.0 - t } * 300.0;
        let y = -250.0 + k as f32 * 0.8;
        let one = std::time::Instant::now();
        s.erase(d, &[([x, y], 25.0), ([x + 8.0, y + 1.0], 25.0)]);
        worst = worst.max(one.elapsed());
    }
    let dr = &s.doc.drawings[&d];
    let verts: usize = dr
        .strokes
        .iter()
        .map(|st| st.shape.iter().map(Vec::len).sum::<usize>())
        .sum();
    let most: usize = dr
        .strokes
        .iter()
        .map(|st| st.shape.iter().map(Vec::len).sum::<usize>())
        .max()
        .unwrap_or(0);
    let per_move = start.elapsed() / moves;
    println!(
        "textured_erase_cost: {per_move:?} per move, worst {worst:?}; {} strokes, {verts} shape points (most in one: {most})",
        dr.strokes.len()
    );
    perf_gate("eraser move on textured strokes", per_move, 2.0);
    perf_gate("worst eraser move on textured strokes", worst, 12.0);
    let start = std::time::Instant::now();
    for _ in 0..3 {
        std::hint::black_box(symphony_render::render_camera(&s.doc, 1, 1920, 1080, None));
    }
    println!("render_camera after erasing: {:?}", start.elapsed() / 3);
}

#[test]
#[ignore = "benchmark"]
fn outline_cost() {
    use symphony_core::stroke::{Point, Stroke, StrokeTexture};
    let mut s = crate::session::Session::new_scene();
    let st = Stroke {
        id: s.doc.ids.fresh(),
        kind: StrokeKind::Brush,
        art: Default::default(),
        swatch: s.swatch,
        min_width: 10.0,
        max_width: 30.0,
        points: (0..120)
            .map(|k| Point {
                x: k as f32 * 2.5,
                y: (k as f32 * 0.2).sin() * 15.0,
                pressure: 0.5 + (k as f32 * 0.1).sin() * 0.4,
                pen: Default::default(),
            })
            .collect(),
        tips: Default::default(),
        texture: Some(StrokeTexture::default()),
        shape: Vec::new(),
    };
    let t = std::time::Instant::now();
    let o = symphony_vector::shape::outline(&st);
    let pts: usize = o.iter().flatten().map(Vec::len).sum();
    println!("outline: {:?}, {pts} points", t.elapsed());
    let eraser =
        symphony_vector::shape::eraser_shape(&[([100.0, 0.0], 25.0), ([108.0, 1.0], 25.0)]);
    let t = std::time::Instant::now();
    let _ = symphony_vector::shape::erase(&o[0], &eraser);
    println!("one cut: {:?}", t.elapsed());
}

/// TGA (no file signature) and SVG (vector) import as pictures.
#[test]
fn tga_and_svg_import() {
    let dir = std::env::temp_dir().join(format!("symphony-tga-svg-{}", std::process::id()));
    let cache = dir.join("cache");
    std::fs::create_dir_all(&dir).unwrap();
    let tga = dir.join("bg.tga");
    image::RgbImage::from_pixel(32, 18, image::Rgb([10, 200, 30]))
        .save(&tga)
        .unwrap();
    let svg = dir.join("logo.svg");
    std::fs::write(
        &svg,
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="50"><rect width="100" height="50" fill="#e03020"/></svg>"##,
    )
    .unwrap();
    let out = crate::import::stage_images_into(&[tga, svg], &cache);
    assert!(out.problems.is_empty(), "{:?}", out.problems);
    // The TGA is a picture; the SVG is a vector drawing (editable).
    let frames: Vec<_> = out.groups.iter().flat_map(|g| g.frames.iter()).collect();
    assert_eq!(frames.len(), 1);
    assert_eq!(frames[0].name, "bg");
    assert_eq!(out.vectors.len(), 1);
    assert_eq!(out.vectors[0].name, "logo");
    let art = &out.vectors[0].frames[0];
    assert_eq!(art.size, [100.0, 50.0]);
    assert_eq!(art.pieces.len(), 1);
    assert_eq!(art.pieces[0].rgba, [224, 48, 32, 255]);
    std::fs::remove_dir_all(&dir).ok();
}

/// The Line tool: a straight line whatever path the pen takes; Shift
/// snaps its angle to 22.5° steps; Alt bends it through the pen.
#[test]
fn the_line_tool_draws_straight_snapped_and_bent_lines() {
    use crate::session::Tool;
    // A fresh layer: its peg is untouched, so drawing space is scene space.
    let mut h = rig();
    h.state_mut().s.add_layer();
    let layer = h.state().s.selected;
    let last = move |h: &Harness<'static, Rig>| strokes(h, layer).last().cloned().expect("a line");
    h.state_mut().s.tool = Tool::Line;
    let before = strokes(&h, layer).len();
    // A wobbly path: the line goes straight from start to end.
    drag(
        &mut h,
        &[(-100.0, 0.0), (-20.0, 40.0), (40.0, -30.0), (100.0, 0.0)],
        1.0,
        PenTool::Pen,
    );
    assert_eq!(strokes(&h, layer).len(), before + 1);
    let st = last(&h);
    assert!(st.points.iter().all(|p| p.y.abs() < 0.5), "straight");
    let (a, b) = (st.points[0], *st.points.last().unwrap());
    assert!((a.x + 100.0).abs() < 1.0 && (b.x - 100.0).abs() < 1.0);
    // Shift: 100 across and 30 down (about 17°) snaps to 22.5°.
    h.input_mut()
        .events
        .push(egui::Event::ModifiersChanged(egui::Modifiers::SHIFT));
    drag(
        &mut h,
        &[(0.0, 0.0), (100.0, 30.0), (100.0, 30.0)],
        1.0,
        PenTool::Pen,
    );
    h.input_mut()
        .events
        .push(egui::Event::ModifiersChanged(egui::Modifiers::NONE));
    let st = last(&h);
    let (a, b) = (st.points[0], *st.points.last().unwrap());
    let angle = (b.y - a.y).atan2(b.x - a.x).to_degrees();
    assert!((angle - 22.5).abs() < 0.5, "snapped: {angle}");
    // Alt: after setting the end, the pen bends the line through itself.
    let to = |h: &Harness<'static, Rig>, x, y| h.state().canvas.to_screen(x, y);
    let steps: [((f32, f32), Phase, bool); 4] = [
        ((-100.0, 100.0), Phase::Down, false),
        ((100.0, 100.0), Phase::Move, false),
        ((0.0, 40.0), Phase::Move, true),
        ((0.0, 40.0), Phase::Up, true),
    ];
    for ((x, y), phase, alt) in steps {
        let p = to(&h, x, y);
        h.input_mut()
            .events
            .push(egui::Event::ModifiersChanged(if alt {
                egui::Modifiers::ALT
            } else {
                egui::Modifiers::NONE
            }));
        h.input_mut().events.push(egui::Event::PointerMoved(p));
        h.state_mut()
            .samples
            .push(sample(p, phase, 1.0, PenTool::Pen));
        h.step();
    }
    h.input_mut()
        .events
        .push(egui::Event::ModifiersChanged(egui::Modifiers::NONE));
    h.run_steps(2);
    let st = last(&h);
    let mid = st.points[st.points.len() / 2];
    assert!(
        mid.x.abs() < 3.0 && (mid.y - 40.0).abs() < 3.0,
        "bent through the pen: {mid:?}"
    );
    let (a, b) = (st.points[0], *st.points.last().unwrap());
    assert!(
        (a.y - 100.0).abs() < 1.0 && (b.y - 100.0).abs() < 1.0,
        "ends stay put"
    );
}

/// Brush kinds: Bitmap lists textured brushes; Vector lists clean ones plus
/// a vector version of every bitmap brush (same name, no stamps).
#[test]
fn bitmap_brushes_also_come_as_vector_versions() {
    use crate::presets::Preset;
    use crate::views::panels::brush_list;
    let s = crate::session::Session::new_scene();
    let clean = Preset {
        name: "Ink".into(),
        settings: s.brush,
        source: None,
        tip: None,
        grain: None,
    };
    let mut stamped = clean.clone();
    stamped.name = "Grainy".into();
    stamped.settings.texture = Some(symphony_core::stroke::StrokeTexture::default());
    let all = vec![clean, stamped];
    let names = |l: Vec<Preset>| l.into_iter().map(|p| p.name).collect::<Vec<_>>();
    assert_eq!(names(brush_list(&all, true)), ["Grainy"]);
    let vector = brush_list(&all, false);
    assert_eq!(names(vector.clone()), ["Ink", "Grainy"]);
    assert!(
        vector
            .iter()
            .all(|p| !crate::views::panels::is_bitmap(&p.settings)),
        "vector versions are vector brushes"
    );
    let grainy = vector.iter().find(|p| p.name == "Grainy").unwrap();
    assert!(
        grainy.settings.texture.is_some_and(|t| t.vector),
        "it keeps its tip, stamped as shapes"
    );
}

/// The pen's tilt and rotation are kept on the line's points (so brushes
/// that follow them still do after editing) and survive saving; lines
/// from a mouse store nothing extra.
#[test]
fn strokes_keep_how_the_pen_was_held() {
    let mut h = rig();
    h.state_mut().s.add_layer();
    let layer = h.state().s.selected;
    h.state_mut().s.tool = crate::session::Tool::Brush;
    let pts = line((-80.0, 0.0), (80.0, 0.0), 8);
    let n = pts.len();
    for (i, &(x, y)) in pts.iter().enumerate() {
        let p = h.state().canvas.to_screen(x, y);
        let phase = match i {
            0 => Phase::Down,
            _ if i + 1 == n => Phase::Up,
            _ => Phase::Move,
        };
        let mut sm = sample(p, phase, 0.8, PenTool::Pen);
        sm.tilt_x = 0.0;
        sm.tilt_y = 35.0;
        sm.rotation = 90.0;
        h.input_mut().events.push(egui::Event::PointerMoved(p));
        h.state_mut().samples.push(sm);
        h.step();
    }
    h.run_steps(2);
    let st = strokes(&h, layer).last().cloned().expect("a stroke");
    assert!(
        st.points.iter().all(|p| (p.pen.tilt - 35.0).abs() < 0.5),
        "tilt kept"
    );
    assert!(
        st.points.iter().all(|p| (p.pen.azimuth - 90.0).abs() < 0.5),
        "lean direction kept"
    );
    assert!(
        st.points
            .iter()
            .all(|p| (p.pen.rotation - 90.0).abs() < 0.5),
        "barrel kept"
    );
    let json = serde_json::to_string(&st).unwrap();
    let back: symphony_core::stroke::Stroke = serde_json::from_str(&json).unwrap();
    assert_eq!(back.points, st.points, "survives saving");
    // A mouse line saves points without pen data at all.
    let mut flat = st.clone();
    for p in &mut flat.points {
        p.pen = Default::default();
    }
    assert!(!serde_json::to_string(&flat).unwrap().contains("\"pen\""));
}

/// A vector brush (a bitmap brush's tip stamped as shapes): drawn in the
/// Camera view it renders solid where the line is, and the eraser cuts
/// its merged outline into pieces.
#[test]
fn vector_brushes_draw_solid_shapes_and_erase_cleanly() {
    let mut h = rig();
    h.state_mut().s.add_layer();
    let layer = h.state().s.selected;
    {
        let s = &mut h.state_mut().s;
        s.tool = crate::session::Tool::Brush;
        s.brush.min_width = 16.0;
        s.brush.max_width = 16.0;
        s.brush.texture = Some(symphony_core::stroke::StrokeTexture {
            spacing: 0.2,
            vector: true,
            ..Default::default()
        });
    }
    drag(
        &mut h,
        &line((-200.0, 0.0), (200.0, 0.0), 20),
        1.0,
        PenTool::Pen,
    );
    let st = strokes(&h, layer).last().cloned().expect("a stroke");
    assert!(st.texture.is_some_and(|t| t.vector));
    let s = &h.state().s;
    let (w, ht) = (s.doc.settings.width, s.doc.settings.height);
    let img = symphony_render::render_camera(&s.doc, 1, w, ht, None).unwrap();
    // Scene origin is the frame centre; the line runs along y = 0.
    let px = |x: f32, y: f32| {
        let (cx, cy) = ((w as f32 / 2.0 + x) as u32, (ht as f32 / 2.0 + y) as u32);
        img.pixel(cx, cy).unwrap().alpha()
    };
    assert!(px(0.0, 0.0) > 200, "solid on the line");
    assert!(px(0.0, 30.0) < 10, "nothing well off it");
    // Erase across the middle: two pieces, each an outline.
    let d = h.state().s.doc.exposed(layer, 1).unwrap();
    let eraser: Vec<symphony_vector::cut::Disc> = (0..10)
        .map(|i| ([0.0, -20.0 + i as f32 * 4.0], 10.0))
        .collect();
    h.state_mut().s.erase(d, &eraser);
    let pieces = strokes(&h, layer);
    let cut: Vec<_> = pieces
        .iter()
        .filter(|p| p.texture.is_some_and(|t| t.vector))
        .collect();
    assert_eq!(cut.len(), 2, "split in two");
    assert!(
        cut.iter().all(|p| !p.shape.is_empty()),
        "each piece is its outline"
    );
}

/// Real-project stamping cost: `SYMPHONY_PERF_OPEN=<project> cargo test
/// --release -p symphony-app real_scene_stamp_cost -- --ignored --nocapture`.
/// Times stamping every textured stroke at a typical screen scale, one
/// stroke at a time on one thread (the old way) vs in parallel bands.
#[test]
#[ignore = "needs a real project"]
fn real_scene_stamp_cost() {
    let Some(path) = std::env::var_os("SYMPHONY_PERF_OPEN") else {
        return;
    };
    let scene = symphony_io::open(std::path::Path::new(&path)).unwrap();
    let m = symphony_core::math::Affine::translate(1200.0, 700.0)
        .compose(symphony_core::math::Affine::scale(0.9, 0.9));
    let clip = [0, 0, 2600, 1500];
    let strokes: Vec<_> = scene
        .drawings
        .values()
        .flat_map(|d| d.strokes.iter())
        .filter(|s| s.texture.is_some_and(|t| !t.vector))
        .collect();
    let tex = |k: Option<u64>| k.and_then(|k| scene.textures.get(&k));
    let start = std::time::Instant::now();
    for st in &strokes {
        let t = st.texture.unwrap();
        let mut s = symphony_vector::stamp::Stamper::new(t, m, clip);
        let curved = symphony_core::stroke::Stroke {
            points: symphony_vector::curve::spline(&st.points, 0.75 / 0.9),
            ..(*st).clone()
        };
        s.extend(&curved, tex(t.tip), tex(t.grain));
        std::hint::black_box(s.coverage());
    }
    let one = start.elapsed();
    let start = std::time::Instant::now();
    for st in &strokes {
        let t = st.texture.unwrap();
        std::hint::black_box(symphony_vector::stamp::rasterize(
            st,
            &t,
            tex(t.tip),
            tex(t.grain),
            m,
            clip,
        ));
    }
    let bands = start.elapsed();
    let start = std::time::Instant::now();
    let reps: u32 = std::env::var("SYMPHONY_PERF_REPS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1);
    for _ in 0..reps {
        std::hint::black_box(symphony_render::render_camera(&scene, 1, 1920, 1080, None));
    }
    println!(
        "{} textured strokes: one thread {:?}, parallel bands {:?}; full camera render 1080p {:?}",
        strokes.len(),
        one,
        bands,
        start.elapsed() / reps
    );
}

/// Export speed on a real project: frames one at a time (the old way) vs
/// a PNG sequence and a GIF through the batched export. `SYMPHONY_PERF_OPEN=
/// <project> cargo test --release -p symphony-app real_scene_export_cost --
/// --ignored --nocapture`.
#[test]
#[ignore = "needs a real project"]
fn real_scene_export_cost() {
    let Some(path) = std::env::var_os("SYMPHONY_PERF_OPEN") else {
        return;
    };
    let scene = symphony_io::open(std::path::Path::new(&path)).unwrap();
    let last = scene.settings.length.clamp(1, 24);
    let (w, h) = (scene.settings.width, scene.settings.height);
    let start = std::time::Instant::now();
    for f in 1..=last {
        std::hint::black_box(symphony_render::render_frame(&scene, f, w, h));
    }
    let one = start.elapsed();
    let dir = std::env::temp_dir().join(format!("symphony-export-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut times = Vec::new();
    for (format, name) in [
        (symphony_render::Format::PngSequence, "f.png"),
        (symphony_render::Format::AnimatedGif, "a.gif"),
    ] {
        let job = symphony_render::ExportJob {
            format,
            path: dir.join(name),
            first: 1,
            last,
            scale: 1.0,
            codec: None,
            movie_fps: None,
        };
        let start = std::time::Instant::now();
        symphony_render::export(
            &scene,
            &job,
            &std::sync::atomic::AtomicBool::new(false),
            &|_, _| {},
        )
        .unwrap();
        times.push(start.elapsed());
    }
    let _ = std::fs::remove_dir_all(&dir);
    let start = std::time::Instant::now();
    let key = symphony_render::checkpoint::fingerprint(&scene, w, h);
    let hashing = start.elapsed();
    println!(
        "{last} frames at {w}x{h}: rendering one at a time {one:?}; PNG export {:?}, GIF export {:?}; \
         scene fingerprint {hashing:?} ({} chars)",
        times[0],
        times[1],
        key.len()
    );
}

/// A real crash mid-export: run this, kill it (`kill -9`) part way, and
/// run it again; the second run reports the frames it reused.
/// `SYMPHONY_PERF_OPEN=<project> SYMPHONY_EXPORT_TO=<dir> cargo test
/// --release -p symphony-app export_after_a_crash -- --ignored --nocapture`.
#[test]
#[ignore = "needs a real project and a kill"]
fn export_after_a_crash() {
    let (Some(path), Some(to)) = (
        std::env::var_os("SYMPHONY_PERF_OPEN"),
        std::env::var_os("SYMPHONY_EXPORT_TO"),
    ) else {
        return;
    };
    let scene = symphony_io::open(std::path::Path::new(&path)).unwrap();
    let format = match std::env::var("SYMPHONY_EXPORT_FORMAT").as_deref() {
        Ok("gif") => symphony_render::Format::AnimatedGif,
        _ => symphony_render::Format::PngSequence,
    };
    let name = if format == symphony_render::Format::AnimatedGif {
        "shot.gif"
    } else {
        "shot.png"
    };
    let job = symphony_render::ExportJob {
        format,
        path: std::path::Path::new(&to).join(name),
        first: 1,
        last: 120,
        scale: 1.0,
        codec: None,
        movie_fps: None,
    };
    let done = symphony_render::export(
        &scene,
        &job,
        &std::sync::atomic::AtomicBool::new(false),
        &|d, t| eprintln!("frame {d} of {t}"),
    )
    .unwrap();
    println!("export finished, {} frames reused", done.reused);
}

/// SVG files and vectorized pictures come in as drawings the tools work
/// on: lines on Line Art and colour zones on Colour Art, in palette
/// colours. The eraser cuts both, the Paint tool repaints a zone, and one
/// undo takes the whole import back.
#[test]
fn imported_svg_and_traced_pictures_are_editable() {
    use crate::import::{stage_into, ImportOptions};
    use crate::vector_import::Vectorize;
    use symphony_core::stroke::ArtLayer;
    let dir = std::env::temp_dir().join(format!("symphony-vec-import-{}", std::process::id()));
    let cache = dir.join("cache");
    std::fs::create_dir_all(&dir).unwrap();
    let svg = dir.join("badge.svg");
    std::fs::write(
        &svg,
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="160" height="90">
            <rect x="20" y="15" width="120" height="60" fill="#3366cc"/>
            <path d="M 20 45 L 140 45" stroke="#111111" stroke-width="6"/>
        </svg>"##,
    )
    .unwrap();
    // Line art on paper, as a scan would be.
    let png = dir.join("sketch.png");
    let mut img = image::RgbaImage::from_pixel(160, 90, image::Rgba([250, 248, 240, 255]));
    for x in 10..150 {
        for y in 60..64 {
            img.put_pixel(x, y, image::Rgba([15, 15, 15, 255]));
        }
    }
    img.save(&png).unwrap();

    let mut s = crate::session::Session::new_scene();
    let swatches = s.doc.palette.swatches.len();
    let opts = ImportOptions {
        as_vectors: true,
        vectorize: Vectorize::LineArt { threshold: 0.5 },
        fps: 24.0,
    };
    let out = stage_into(&[svg, png], &cache, opts);
    assert!(out.problems.is_empty(), "{:?}", out.problems);
    assert!(out.groups.is_empty(), "nothing came in as a locked picture");
    assert_eq!(out.vectors.len(), 2);
    let layers = s.layer_count();
    assert_eq!(s.import_vectors(out.vectors), 2);
    assert_eq!(s.layer_count(), layers + 2);
    assert!(
        s.doc.palette.swatches.len() > swatches,
        "imported colours joined the palette"
    );

    // The SVG's drawing: one zone, one line, placed in the scene.
    let svg_layer = (0..s.layer_count())
        .find(|&l| {
            s.doc
                .exposed(l, 1)
                .and_then(|d| s.doc.drawings.get(&d))
                .is_some_and(|d| !d.fills.is_empty())
        })
        .expect("the SVG's layer");
    let d = s.doc.exposed(svg_layer, 1).unwrap();
    let dr = &s.doc.drawings[&d];
    assert_eq!(dr.fills.len(), 1);
    assert_eq!(dr.fills[0].art, ArtLayer::ColourArt);
    assert_eq!(dr.strokes.len(), 1);
    assert_eq!(dr.strokes[0].art, ArtLayer::LineArt);
    assert!(dr.image.is_none());
    let zone_area = |s: &crate::session::Session| -> f32 {
        s.doc.drawings[&d]
            .fills
            .iter()
            .map(|f| symphony_vector::shape::shape_area(&f.contours).abs())
            .sum()
    };
    let before = zone_area(&s);
    // The eraser cuts the line and the zone at the middle.
    s.art = ArtLayer::LineArt;
    s.erase(d, &[([0.0, 0.0], 40.0)]);
    s.end_gesture();
    let dr = &s.doc.drawings[&d];
    assert!(dr.strokes.len() >= 2, "the line was cut in two");
    s.art = ArtLayer::ColourArt;
    s.erase(d, &[([0.0, 0.0], 40.0)]);
    s.end_gesture();
    assert!(
        zone_area(&s) < before - 3000.0,
        "the zone lost the eraser's shape"
    );
    // The Paint tool repaints the zone.
    let red = s.doc.palette.swatches[0].id;
    s.swatch = red;
    let corner = s.doc.drawings[&d].fills[0].contours[0][0];
    let inside = [corner[0] * 0.9, corner[1] * 0.9];
    s.paint_at(svg_layer, 1, inside[0], inside[1], 1.0);
    assert!(s.doc.drawings[&d].fills.iter().any(|f| f.swatch == red));

    // The traced sketch: lines only, no paper.
    let sketch = (0..s.layer_count())
        .find(|&l| {
            l != svg_layer
                && s.doc.exposed(l, 1).is_some_and(|d| {
                    s.doc
                        .drawings
                        .get(&d)
                        .is_some_and(|d| !d.strokes.is_empty())
                })
        })
        .expect("the sketch's layer");
    let sd = &s.doc.drawings[&s.doc.exposed(sketch, 1).unwrap()];
    assert!(sd.fills.is_empty());
    assert!(sd.strokes.iter().all(|st| st.art == ArtLayer::LineArt));

    // Undo the edits: the imported zone renders in its colour; one more
    // undo takes the whole import back.
    while s.history.undo_name() != Some("Import as vector drawing") {
        assert!(
            s.history.undo_len() > 0,
            "the import step is in the history"
        );
        s.undo();
    }
    let img = symphony_render::render_frame(&s.doc, 1, 320, 180).unwrap();
    assert!(
        img.pixels().any(|p| p.0[2] > 150 && p.0[0] < 100),
        "the blue zone shows"
    );
    s.undo();
    assert_eq!(s.layer_count(), layers, "the import is one undo step");
    let _ = std::fs::remove_dir_all(&dir);
}

/// See how a picture traces: `SYMPHONY_TRACE_IN=<image> SYMPHONY_TRACE_OUT=
/// <png> [SYMPHONY_TRACE_COLOURS=8] cargo test --release -p symphony-app
/// trace_a_picture -- --ignored --nocapture`. Prints the time and pieces.
#[test]
#[ignore = "needs a picture"]
fn trace_a_picture() {
    use crate::vector_import::Vectorize;
    let (Some(input), Some(out)) = (
        std::env::var_os("SYMPHONY_TRACE_IN"),
        std::env::var_os("SYMPHONY_TRACE_OUT"),
    ) else {
        return;
    };
    let how = match std::env::var("SYMPHONY_TRACE_COLOURS")
        .ok()
        .and_then(|c| c.parse().ok())
    {
        Some(colours) => Vectorize::Colour { colours },
        None => Vectorize::LineArt { threshold: 0.5 },
    };
    let opts = crate::import::ImportOptions {
        as_vectors: true,
        vectorize: how,
        fps: 24.0,
    };
    let dir = std::env::temp_dir().join("symphony-trace-cache");
    let mut input = std::path::PathBuf::from(input);
    // An SVG is drawn into a picture first (a stand-in for a scan).
    if input.extension().is_some_and(|e| e == "svg") {
        let tree = resvg::usvg::Tree::from_data(
            &std::fs::read(&input).unwrap(),
            &resvg::usvg::Options::default(),
        )
        .unwrap();
        let (w, h) = (tree.size().width() as u32, tree.size().height() as u32);
        let mut pix = resvg::tiny_skia::Pixmap::new(w, h).unwrap();
        pix.fill(resvg::tiny_skia::Color::WHITE);
        resvg::render(&tree, Default::default(), &mut pix.as_mut());
        input = dir.join("drawn.png");
        std::fs::create_dir_all(&dir).unwrap();
        pix.save_png(&input).unwrap();
    }
    let start = std::time::Instant::now();
    let got = crate::import::stage_into(&[input], &dir, opts);
    let traced = start.elapsed();
    assert!(got.problems.is_empty(), "{:?}", got.problems);
    let pieces: usize = got
        .vectors
        .iter()
        .flat_map(|g| &g.frames)
        .map(|a| a.pieces.len())
        .sum();
    let mut s = crate::session::Session::new_scene();
    for l in (0..s.layer_count()).rev() {
        s.delete_layer(l);
    }
    s.import_vectors(got.vectors);
    let img = symphony_render::render_frame(&s.doc, 1, 1920, 1080).unwrap();
    img.save(out).unwrap();
    println!("traced in {traced:?}: {pieces} pieces");
}

/// The Select tool works on colour zones too (imported or painted):
/// clicking one selects it, dragging moves it, Delete removes it, and
/// select all includes zones on the current art layer.
#[test]
fn colour_zones_can_be_selected_moved_and_deleted() {
    use symphony_core::stroke::{ArtLayer, Fill};
    let mut h = rig();
    h.state_mut().s.add_layer();
    let layer = h.state().s.selected;
    // A square zone on Colour Art, through the importer's path.
    {
        let s = &mut h.state_mut().s;
        let col = s.doc.layers[layer].column;
        let d = s.doc.make_drawing(col);
        let did = d.id;
        s.doc.drawings.insert(did, d);
        s.doc.xsheet.column_mut(col).unwrap().set(1, Some(did));
        let swatch = s.doc.palette.swatches[1].id;
        let fill = Fill {
            linked: None,
            id: s.doc.ids.fresh(),
            swatch,
            art: ArtLayer::ColourArt,
            contours: vec![vec![
                [-40.0, -40.0],
                [40.0, -40.0],
                [40.0, 40.0],
                [-40.0, 40.0],
            ]],
        };
        s.doc.drawings.get_mut(&did).unwrap().fills.push(fill);
        s.art = ArtLayer::ColourArt;
        s.tool = Tool::Select;
        s.frame = 1;
    }
    h.run_steps(2);
    let centre = |h: &Harness<'static, Rig>| {
        let f = &fills(h, layer)[0];
        let c = &f.contours[0];
        (
            c.iter().map(|p| p[0]).sum::<f32>() / c.len() as f32,
            c.iter().map(|p| p[1]).sum::<f32>() / c.len() as f32,
        )
    };
    let start = centre(&h);
    // Press inside the zone and drag it 60 units right (the Select tool
    // follows the pointer's button).
    let path = line((0.0, 0.0), (60.0, 0.0), 8);
    for (i, &(x, y)) in path.iter().enumerate() {
        let pos = h.state().canvas.to_screen(x, y);
        h.input_mut().events.push(egui::Event::PointerMoved(pos));
        let press = match i {
            0 => Some(true),
            _ if i + 1 == path.len() => Some(false),
            _ => None,
        };
        if let Some(pressed) = press {
            h.input_mut().events.push(egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            });
        }
        h.step();
    }
    h.run_steps(2);
    assert_eq!(
        h.state().s.selection.strokes.len(),
        1,
        "the zone is selected"
    );
    let moved = centre(&h);
    assert!(
        (moved.0 - start.0 - 60.0).abs() < 3.0 && (moved.1 - start.1).abs() < 3.0,
        "{start:?} -> {moved:?}"
    );
    // Select all picks it again; Delete removes it.
    h.state_mut().s.deselect();
    h.state_mut().s.select_all();
    assert_eq!(h.state().s.selection.strokes.len(), 1);
    h.state_mut().s.delete_selection();
    assert!(fills(&h, layer).is_empty());
    h.state_mut().s.undo();
    assert_eq!(fills(&h, layer).len(), 1, "undo brings it back");
}

/// Layered files keep their stacking in the scene (the top layer is drawn
/// over the bottom one, even when both fill the frame), and an animated
/// GIF becomes one layer whose drawings are held as long as its frames.
#[test]
fn layered_files_stack_and_animations_keep_their_timing() {
    use crate::import::{stage_into, ImportOptions};
    let dir = std::env::temp_dir().join(format!("symphony-layered-{}", std::process::id()));
    let cache = dir.join("cache");
    std::fs::create_dir_all(&dir).unwrap();
    let psd = dir.join("rg.psd");
    std::fs::write(
        &psd,
        include_bytes!("../tests/fixtures/two-layers-red-green-1x1.psd"),
    )
    .unwrap();
    let gif = dir.join("blink.gif");
    {
        use image::codecs::gif::GifEncoder;
        use image::{Delay, Frame};
        let mut enc = GifEncoder::new(std::fs::File::create(&gif).unwrap());
        for (c, ms) in [(0u8, 250u32), (255, 125)] {
            let img = image::RgbaImage::from_pixel(16, 9, image::Rgba([c, c, c, 255]));
            enc.encode_frame(Frame::from_parts(
                img,
                0,
                0,
                Delay::from_numer_denom_ms(ms, 1),
            ))
            .unwrap();
        }
    }
    let mut s = crate::session::Session::new_scene();
    for l in (0..s.layer_count()).rev() {
        s.delete_layer(l);
    }
    let opts = ImportOptions {
        fps: 24.0,
        ..Default::default()
    };
    let out = stage_into(&[psd], &cache, opts);
    assert!(out.problems.is_empty(), "{:?}", out.problems);
    s.doc.media_dirs = vec![cache.clone()];
    assert_eq!(s.import_images(out.groups), 2);
    // Red is the PSD's top layer: it covers the green one.
    let img = symphony_render::render_frame(&s.doc, 1, 64, 36).unwrap();
    assert_eq!(img.get_pixel(32, 18).0, [255, 0, 0, 255]);

    let out = stage_into(&[gif], &cache, opts);
    assert_eq!(out.groups.len(), 1);
    // 250 ms then 125 ms at 24 fps: 6 frames then 3.
    assert_eq!(out.groups[0].frames.len(), 9);
    assert_eq!(out.groups[0].frames[0].key, out.groups[0].frames[5].key);
    assert_ne!(out.groups[0].frames[5].key, out.groups[0].frames[6].key);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Less common picture formats import too, kept as PNG in the project.
#[test]
fn more_picture_formats_import() {
    let dir = std::env::temp_dir().join(format!("symphony-formats-{}", std::process::id()));
    let cache = dir.join("cache");
    std::fs::create_dir_all(&dir).unwrap();
    let img = image::RgbaImage::from_pixel(12, 6, image::Rgba([40, 90, 200, 255]));
    let mut paths = Vec::new();
    for ext in ["qoi", "exr", "ico", "ppm", "ff", "hdr"] {
        let p = dir.join(format!("swatch-{ext}.{ext}"));
        let ok = match ext {
            "ppm" => image::DynamicImage::ImageRgba8(img.clone())
                .to_rgb8()
                .save(&p),
            "hdr" => image::DynamicImage::ImageRgba8(img.clone())
                .to_rgb32f()
                .save(&p),
            "exr" => image::DynamicImage::ImageRgba8(img.clone())
                .to_rgba32f()
                .save(&p),
            "ff" => image::DynamicImage::ImageRgba8(img.clone())
                .to_rgba16()
                .save(&p),
            _ => img.save(&p),
        };
        ok.unwrap_or_else(|e| panic!("{ext}: {e}"));
        paths.push(p);
    }
    let out = crate::import::stage_into(&paths, &cache, Default::default());
    assert!(out.problems.is_empty(), "{:?}", out.problems);
    let frames: Vec<_> = out.groups.iter().flat_map(|g| &g.frames).collect();
    assert_eq!(frames.len(), 6);
    for f in frames {
        assert!(f.file.ends_with(".png"), "{}", f.file);
        assert_eq!(f.size, [12, 6]);
        let px = image::open(cache.join(&f.file)).unwrap().to_rgba8();
        let p = px.get_pixel(3, 3).0;
        assert!(p[2] > 150 && p[0] < 80, "{}: {p:?}", f.name);
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// A Mesh Warp bends the drawing under it in renders: a straight line
/// with the grid's middle point pulled down comes out curved, its middle
/// lowered and its ends where they were; the drawing itself is unchanged.
#[test]
fn mesh_warp_bends_the_drawing_in_renders() {
    use symphony_core::graph::NodeKind;
    use symphony_core::stroke::{Point, Stroke};
    let mut s = crate::session::Session::new_scene();
    for l in (1..s.layer_count()).rev() {
        s.delete_layer(l);
    }
    let layer = 0;
    let p = |x: f32| Point {
        x,
        y: 0.0,
        pressure: 1.0,
        pen: Default::default(),
    };
    // Clear the layer's own drawing, then one straight line across.
    let col = s.doc.layers[layer].column;
    let d = s.doc.make_drawing(col);
    let did = d.id;
    s.doc.drawings.insert(did, d);
    s.doc.xsheet.column_mut(col).unwrap().set(1, Some(did));
    let st = Stroke {
        id: s.doc.ids.fresh(),
        kind: StrokeKind::Brush,
        art: Default::default(),
        swatch: s.doc.palette.swatches[0].id,
        min_width: 12.0,
        max_width: 12.0,
        points: vec![p(-600.0), p(600.0)],
        tips: Default::default(),
        texture: None,
        shape: Vec::new(),
    };
    s.doc.drawings.get_mut(&did).unwrap().strokes.push(st);
    s.frame = 1;
    let dark_rows = |s: &crate::session::Session, x: u32| -> Vec<u32> {
        let img = symphony_render::render_frame(&s.doc, 1, 1920, 1080).unwrap();
        (0..1080)
            .filter(|&y| {
                let p = img.get_pixel(x, y).0;
                (p[0] as u32 + p[1] as u32 + p[2] as u32) < 300
            })
            .collect()
    };
    let flat = dark_rows(&s, 960);
    assert!(!flat.is_empty(), "the line shows");
    let node = s.doc.layers[layer].node;
    crate::views::node::insert_near(&mut s, node, crate::views::node::mesh_warp(1920, 1080));
    let warp = s.doc.graph.input(node, 0).unwrap();
    assert_eq!(dark_rows(&s, 960), flat, "a flat grid changes nothing");
    // Pull the middle point (row 2, column 2 of the 4 x 4 grid) down.
    let mut before = s.doc.graph.node(warp).unwrap().clone();
    if let NodeKind::MeshWarp(w) = &mut before.kind {
        w.points[2 * 5 + 2][1] = symphony_core::anim::Value::Constant(150.0);
    }
    *s.doc.graph.node_mut(warp).unwrap() = before;
    let bent = dark_rows(&s, 960);
    let shift = bent[0] as i64 - flat[0] as i64;
    assert!(
        (shift - 150).abs() <= 3,
        "the middle follows the point: {shift}"
    );
    // Near the end (x = 400 + 960), the line hardly moves.
    let end = dark_rows(&s, 960 + 590);
    assert!(end
        .first()
        .is_some_and(|y| (*y as i64 - flat[0] as i64).abs() < 40));
    // The drawing itself is as drawn.
    assert_eq!(s.doc.drawings[&did].strokes[0].points.len(), 2);
}

/// The Text tool: click, type, ⌘↩: the letters land in the drawing as
/// vector shapes in the current colour, on the current art layer, near
/// where you clicked, as one undo step. Esc cancels.
#[test]
fn the_text_tool_types_vector_letters() {
    let mut h = rig();
    h.state_mut().s.add_layer();
    let layer = h.state().s.selected;
    h.state_mut().s.tool = Tool::Text;
    h.run_steps(2);
    let before = strokes(&h, layer).len();
    let at = h.state().canvas.to_screen(-200.0, -50.0);
    let press = |h: &mut Harness<'static, Rig>, pressed: bool| {
        h.input_mut().events.push(egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
    };
    h.input_mut().events.push(egui::Event::PointerMoved(at));
    press(&mut h, true);
    h.step();
    press(&mut h, false);
    h.run_steps(3);
    h.input_mut().events.push(egui::Event::Text("Hi".into()));
    h.run_steps(2);
    h.input_mut().events.push(egui::Event::Key {
        key: egui::Key::Enter,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::COMMAND,
    });
    h.run_steps(3);
    let after = strokes(&h, layer);
    if after.len() == before {
        // No fonts on this machine (headless CI): nothing to type with.
        assert!(crate::vector_import::text_shapes("Hi", &Default::default()).is_empty());
        return;
    }
    let new: Vec<_> = after[before..].to_vec();
    assert!(new.len() >= 2, "H and i (with its dot): {}", new.len());
    let s = &h.state().s;
    assert!(new
        .iter()
        .all(|st| st.swatch == s.swatch && st.art == s.art && !st.shape.is_empty()));
    // Near the click (the drawing's own space here is scene space).
    let (lo, _) = symphony_vector::bounds_of(&new).unwrap();
    assert!(
        (lo[0] + 200.0).abs() < 30.0 && (lo[1] + 50.0).abs() < 60.0,
        "{lo:?}"
    );
    h.state_mut().s.undo();
    assert_eq!(strokes(&h, layer).len(), before, "one undo step");
}

/// Mesh Warp in Points mode, puppet style: points placed on an "arm"
/// (a thick line), a mesh built over the artwork, the end point lifted:
/// the arm swings up in renders, its base stays, and a separate shape no
/// point is on (a "ball" below) doesn't move.
#[test]
fn points_warp_bends_only_the_connected_artwork() {
    use symphony_core::graph::NodeKind;
    use symphony_core::stroke::{Point, Stroke};
    use symphony_core::warp::WarpKind;
    let mut s = crate::session::Session::new_scene();
    for l in (1..s.layer_count()).rev() {
        s.delete_layer(l);
    }
    let col = s.doc.layers[0].column;
    let d = s.doc.make_drawing(col);
    let did = d.id;
    s.doc.drawings.insert(did, d);
    s.doc.xsheet.column_mut(col).unwrap().set(1, Some(did));
    let line = |s: &mut crate::session::Session, a: (f32, f32), b: (f32, f32), w: f32| {
        let p = |(x, y): (f32, f32)| Point {
            x,
            y,
            pressure: 1.0,
            pen: Default::default(),
        };
        let st = Stroke {
            id: s.doc.ids.fresh(),
            kind: StrokeKind::Brush,
            art: Default::default(),
            swatch: s.doc.palette.swatches[0].id,
            min_width: w,
            max_width: w,
            points: vec![p(a), p(b)],
            tips: Default::default(),
            texture: None,
            shape: Vec::new(),
        };
        s.doc.drawings.get_mut(&did).unwrap().strokes.push(st);
    };
    line(&mut s, (-400.0, 0.0), (400.0, 0.0), 40.0); // the arm
    line(&mut s, (0.0, 300.0), (10.0, 300.0), 60.0); // the ball
    s.frame = 1;
    let node = s.doc.layers[0].node;
    crate::views::node::insert_near(&mut s, node, crate::views::node::mesh_warp(1920, 1080));
    let warp = s.doc.graph.input(node, 0).unwrap();
    s.select_graph_node(warp, false);
    // Points mode: the shoulder held by two points, the hand at the end.
    if let NodeKind::MeshWarp(w) = &mut s.doc.graph.node_mut(warp).unwrap().kind {
        w.kind = WarpKind::Points;
        w.pins = vec![[-400.0, -10.0], [-400.0, 10.0], [400.0, 0.0]];
        w.pin_moves = vec![
            [
                symphony_core::anim::Value::Constant(0.0),
                symphony_core::anim::Value::Constant(0.0),
            ];
            3
        ];
    }
    let ink_at = |s: &crate::session::Session, x: u32, y: u32| {
        let img = symphony_render::render_frame(&s.doc, 1, 1920, 1080).unwrap();
        let p = img.get_pixel(x, y).0;
        (p[0] as u32 + p[1] as u32 + p[2] as u32) < 300
    };
    // Scene (x, y) is pixel (960 + x, 540 + y) here.
    assert!(ink_at(&s, 960 + 350, 540), "the arm's end, before");
    crate::views::warp_edit::build_mesh(&mut s, warp).unwrap();
    assert!(ink_at(&s, 960 + 350, 540), "building changes nothing yet");
    // Lift the hand 300 up.
    if let NodeKind::MeshWarp(w) = &mut s.doc.graph.node_mut(warp).unwrap().kind {
        w.pin_moves[2][1] = symphony_core::anim::Value::Constant(-300.0);
    }
    let img = symphony_render::render_frame(&s.doc, 1, 1920, 1080).unwrap();
    let ink = |x: u32, y: u32| {
        let p = img.get_pixel(x, y).0;
        (p[0] as u32 + p[1] as u32 + p[2] as u32) < 300
    };
    assert!(!ink(960 + 390, 540), "the end left its old place");
    assert!(
        (180..260).any(|y| ink(960 + 360, y)) || (200..300).any(|y| ink(960 + 300, y)),
        "the end went up"
    );
    assert!(ink(960 - 380, 540), "the shoulder stayed");
    assert!(ink(965, 840), "the ball (no point on it) didn't move");
}

/// Deleting from the Node view and Timeline: an added peg or Mesh Warp
/// goes and its chain closes up (the drawing stays under the layer's
/// peg); a Drawing node takes its layer; the Camera and a layer's own peg
/// refuse; one undo brings a whole delete back. Cells and sounds delete
/// too.
#[test]
fn nodes_cells_and_sounds_can_be_deleted_and_undone() {
    use crate::views::node::{delete_nodes, insert_near, mesh_warp};
    use symphony_core::graph::NodeKind;
    let mut s = crate::session::Session::new_scene();
    let layer = s.doc.layers[0];
    let before_links = s.doc.graph.links().to_vec();
    insert_near(&mut s, layer.node, mesh_warp(100, 100));
    insert_near(&mut s, layer.node, NodeKind::Peg(Default::default()));
    let peg = s.doc.graph.input(layer.node, 0).unwrap();
    let warp = s.doc.graph.input(peg, 0).unwrap();
    assert!(matches!(
        s.doc.graph.node(warp).unwrap().kind,
        NodeKind::MeshWarp(_)
    ));
    let version = s.history.undo_len();
    assert_eq!(delete_nodes(&mut s, &[peg, warp]), 2);
    assert!(s.doc.graph.node(peg).is_none() && s.doc.graph.node(warp).is_none());
    assert_eq!(
        s.doc.graph.input(layer.node, 0),
        Some(layer.peg),
        "the chain closed up"
    );
    let mut now = s.doc.graph.links().to_vec();
    let mut was = before_links.clone();
    now.sort_by_key(|l| (l.from, l.to, l.port));
    was.sort_by_key(|l| (l.from, l.to, l.port));
    assert_eq!(now, was);
    assert_eq!(s.history.undo_len(), version + 1, "one undo step");
    s.undo();
    assert!(s.doc.graph.node(peg).is_some() && s.doc.graph.node(warp).is_some());
    assert_eq!(s.doc.graph.input(layer.node, 0), Some(peg));
    // The Camera and a layer's own peg stay.
    let display = s.doc.display;
    assert_eq!(delete_nodes(&mut s, &[display, layer.peg]), 0);
    // A Drawing node takes its layer.
    // The only layer can't go; with another one it can.
    assert_eq!(delete_nodes(&mut s, &[layer.node]), 0);
    s.add_layer();
    let layers = s.layer_count();
    assert_eq!(delete_nodes(&mut s, &[layer.node]), 1);
    assert_eq!(s.layer_count(), layers - 1);
    s.undo();
    assert_eq!(s.layer_count(), layers);
    // Cells: clear keeps the frame, remove pulls the rest back.
    s.selected = s
        .doc
        .layers
        .iter()
        .position(|l| l.node == layer.node)
        .unwrap();
    let col = s.doc.layers[s.selected].column;
    let row = s.selected;
    s.frame = 1;
    let st = symphony_core::stroke::Stroke {
        id: s.doc.ids.fresh(),
        kind: StrokeKind::Brush,
        art: Default::default(),
        swatch: s.swatch,
        min_width: 2.0,
        max_width: 2.0,
        points: vec![
            symphony_core::stroke::Point {
                x: 0.0,
                y: 0.0,
                pressure: 1.0,
                pen: Default::default(),
            };
            2
        ],
        tips: Default::default(),
        texture: None,
        shape: Vec::new(),
    };
    s.commit_stroke(row, 1, st);
    s.change_exposure(1);
    s.change_exposure(1);
    let len = s.doc.xsheet.column(col).unwrap().len();
    assert!(len >= 3, "{len}");
    s.clear_cell();
    assert!(s.doc.exposed(row, 1).is_none());
    assert_eq!(s.doc.xsheet.column(col).unwrap().len(), len);
    s.undo();
    s.remove_frame();
    assert_eq!(s.doc.xsheet.column(col).unwrap().len(), len - 1);
    s.undo();
    assert_eq!(s.doc.xsheet.column(col).unwrap().len(), len);
}
