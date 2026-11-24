//! Stage texture for Camera. Pan and rotation only move the existing texture.

use eframe::egui::{self, Color32, Pos2};
use symphony_core::id::Id;
use symphony_core::math::Affine;
use symphony_render::{render_camera, StrokePreview};

use crate::session::{Session, Tool};

#[derive(Clone, PartialEq)]
struct RenderKey {
    session: String,
    version: u64,
    frame: u32,
    stage: [u32; 2],
    selection: Option<(Id, Vec<Id>, Affine)>,
    hidden_text: Option<(Id, Vec<Id>)>,
}

type Rendered = (RenderKey, Option<(u32, u32, Vec<u8>)>);

#[derive(Default)]
pub(super) struct Cache {
    key: Option<RenderKey>,
    texture: Option<egui::TextureHandle>,
    capture_wallpaper: bool,
    wallpaper_pixels: Option<crate::wallpaper::CameraPixels>,
    /// A render running in the background, and the key it's for. The
    /// full renderer can take a tenth of a second on a busy scene, so it
    /// never runs on the interface thread; the last image stays up until
    /// the new one lands, and only the newest change is rendered next.
    job: Option<(RenderKey, std::sync::mpsc::Receiver<Rendered>)>,
    failed: bool,
}

impl Cache {
    pub(super) fn capture_wallpaper(&mut self, capture: bool) {
        self.capture_wallpaper = capture;
        if !capture {
            self.wallpaper_pixels = None;
        }
    }

    pub(super) fn wallpaper_pixels(&self, s: &Session) -> Option<crate::wallpaper::CameraPixels> {
        let key = self.key.as_ref()?;
        (key.session == s.key && key.version == s.doc.version && key.frame == s.frame)
            .then(|| self.wallpaper_pixels.clone())
            .flatten()
    }

    pub(super) fn paint(
        &mut self,
        painter: &egui::Painter,
        s: &Session,
        corners: [Pos2; 4],
        transform: Option<Affine>,
        hidden_text: Option<(Id, Vec<Id>)>,
    ) -> bool {
        let selection = if s.tool == Tool::Select {
            s.selection
                .drawing
                .zip(transform)
                .map(|(drawing, transform)| (drawing, s.selection.strokes.clone(), transform))
        } else {
            None
        };
        let stage = [s.doc.settings.width.max(1), s.doc.settings.height.max(1)];
        let key = RenderKey {
            session: s.key.clone(),
            version: s.doc.version,
            frame: s.frame,
            stage,
            selection,
            hidden_text,
        };
        // Collect a finished render.
        if let Some((_, rx)) = &self.job {
            if let Ok((done, image)) = rx.try_recv() {
                self.job = None;
                match image {
                    Some((width, height, pixels)) => {
                        let texture = egui::ColorImage::from_rgba_premultiplied(
                            [width as usize, height as usize],
                            &pixels,
                        );
                        self.wallpaper_pixels =
                            self.capture_wallpaper
                                .then(|| crate::wallpaper::CameraPixels {
                                    size: [width, height],
                                    rgba: std::sync::Arc::new(pixels),
                                });
                        if let Some(handle) = &mut self.texture {
                            handle.set(texture, egui::TextureOptions::LINEAR);
                        } else {
                            self.texture = Some(painter.ctx().load_texture(
                                "camera-display",
                                texture,
                                egui::TextureOptions::LINEAR,
                            ));
                        }
                        self.failed = false;
                    }
                    None => {
                        self.texture = None;
                        self.wallpaper_pixels = None;
                        self.failed = true;
                    }
                }
                self.key = Some(done);
            }
        }
        // Start rendering the newest state if it isn't shown or on its way.
        let wanted =
            self.key.as_ref() != Some(&key) && self.job.as_ref().is_none_or(|(k, _)| *k != key);
        if wanted && self.job.is_none() {
            let scale = (1920.0 / stage[0].max(stage[1]) as f32).min(1.0);
            let [width, height] = stage.map(|size| (size as f32 * scale).round().max(1.0) as u32);
            let mut scene = s.doc.clone();
            let frame = s.frame;
            let job_key = key.clone();
            let (tx, rx) = std::sync::mpsc::channel();
            let ctx = painter.ctx().clone();
            std::thread::spawn(move || {
                if let Some((drawing, glyphs)) = &job_key.hidden_text {
                    if let Some(drawing) = scene.drawings.get_mut(drawing) {
                        drawing
                            .strokes
                            .retain(|stroke| !glyphs.contains(&stroke.id));
                    }
                }
                let preview = job_key
                    .selection
                    .as_ref()
                    .map(|(drawing, strokes, transform)| StrokePreview {
                        drawing: *drawing,
                        strokes,
                        transform: *transform,
                    });
                let image = render_camera(&scene, frame, width, height, preview)
                    .map(|image| (width, height, image.take()));
                let _ = tx.send((job_key, image));
                ctx.request_repaint();
            });
            self.job = Some((key, rx));
        } else if wanted {
            // Busy: check back once the running render lands.
            painter.ctx().request_repaint();
        }

        let Some(texture) = &self.texture else {
            // Nothing yet: still rendering is fine, only a failure isn't.
            return !self.failed;
        };
        let mut mesh = egui::Mesh::with_texture(texture.id());
        let uv = [
            Pos2::ZERO,
            Pos2::new(1.0, 0.0),
            Pos2::new(1.0, 1.0),
            Pos2::new(0.0, 1.0),
        ];
        mesh.vertices.extend(
            corners
                .into_iter()
                .zip(uv)
                .map(|(pos, uv)| egui::epaint::Vertex {
                    pos,
                    uv,
                    color: Color32::WHITE,
                }),
        );
        mesh.indices.extend([0, 1, 2, 0, 2, 3]);
        painter.add(egui::Shape::mesh(mesh));
        true
    }
}

/// Cut a stage-shaped opening in an opaque black surround, including after rotation.
pub(super) fn mask_outside_stage(
    painter: &egui::Painter,
    viewport: egui::Rect,
    corners: [Pos2; 4],
) {
    let center = corners[0].lerp(corners[2], 0.5);
    let half_side = corners[0]
        .distance(corners[1])
        .min(corners[1].distance(corners[2]))
        * 0.5;
    if half_side <= f32::EPSILON {
        painter.rect_filled(viewport, 0.0, Color32::BLACK);
        return;
    }
    let reach = viewport.center().distance(center) + viewport.size().length();
    let scale = (reach / half_side).max(1.0);
    let outside = corners.map(|corner| center + (corner - center) * scale);
    let mut border = egui::Mesh::default();
    for point in corners.into_iter().chain(outside) {
        border.colored_vertex(point, Color32::BLACK);
    }
    for a in 0..4 {
        let b = (a + 1) % 4;
        border.add_triangle(a, b, a + 4);
        border.add_triangle(a + 4, b, b + 4);
    }
    painter.add(egui::Shape::mesh(border));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keymap::Profile;
    use crate::views::camera::{self, CanvasState};
    use egui_kittest::Harness;
    use symphony_core::graph::{Gradient, NodeKind};

    #[test]
    fn camera_draws_the_display_texture_and_drawing_does_not() {
        let mut session = Session::new_scene();
        session.doc.settings.width = 100;
        session.doc.settings.height = 64;
        let gradient = session.doc.graph.add(
            "Gradient",
            NodeKind::Gradient(Gradient {
                start: [30, 50, 80, 255],
                end: [30, 50, 80, 255],
                angle: 0.0,
            }),
        );
        session.doc.graph.connect(gradient, session.doc.display, 0);

        let mut harness = Harness::builder()
            .with_size(egui::vec2(700.0, 480.0))
            .build_ui_state(
                |ui, (session, camera_state, drawing_state, drawing_only)| {
                    let state = if *drawing_only {
                        drawing_state
                    } else {
                        camera_state
                    };
                    camera::ui(ui, session, state, &[], *drawing_only, Profile::Harmony);
                },
                (
                    session,
                    CanvasState::default(),
                    CanvasState::default(),
                    false,
                ),
            );
        harness.state_mut().0.frame = 2;
        // The render runs in the background: wait for frame 2's to land.
        for _ in 0..500 {
            harness.run_steps(1);
            let landed = harness
                .state()
                .1
                .display
                .key
                .as_ref()
                .is_some_and(|k| k.frame == 2);
            if landed {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        let camera_state = &harness.state().1;
        let texture = camera_state.display.texture.as_ref().unwrap().id();
        assert_eq!(camera_state.display.key.as_ref().unwrap().frame, 2);
        let image = render_camera(&harness.state().0.doc, 2, 100, 64, None).unwrap();
        assert_eq!(&image.data()[..4], &[30, 50, 80, 255]);

        let draws_camera_texture = |output: &egui::FullOutput| {
            output.shapes.iter().any(|shape| {
                matches!(&shape.shape, egui::Shape::Mesh(mesh) if mesh.texture_id == texture)
            })
        };
        assert!(draws_camera_texture(harness.output()));
        let draws_stage_mask = |output: &egui::FullOutput| {
            output.shapes.iter().any(|shape| matches!(
                &shape.shape, egui::Shape::Mesh(mesh) if mesh.vertices.len() == 8
                    && mesh.indices.len() == 24 && mesh.vertices.iter().all(|v| v.color == Color32::BLACK)
            ))
        };
        assert!(!draws_stage_mask(harness.output()));
        harness.state_mut().0.doc.settings.bars.on = true;
        harness.run_steps(1);
        assert!(draws_stage_mask(harness.output()));

        harness.state_mut().0.frame = 3;
        harness.state_mut().1.render_effects = false;
        harness.run_steps(2);
        assert!(!draws_camera_texture(harness.output()));
        assert!(harness.state().1.display.job.is_none());
        assert_eq!(harness.state().1.display.key.as_ref().unwrap().frame, 2);

        harness.state_mut().3 = true;
        harness.run_steps(1);
        assert!(!draws_camera_texture(harness.output()));
        assert!(!draws_stage_mask(harness.output()));
        assert!(harness.state().2.display.texture.is_none());
    }
}
