//! Wallpaper and glass masks share a small ladder of prefiltered textures.
use std::{
    path::{Path, PathBuf},
    sync::{
        mpsc::{self, Receiver, TryRecvError},
        Arc,
    },
    time::Duration,
};

use eframe::egui::{self, Color32, Pos2, Rect, Vec2};
use image::{imageops, ImageReader, RgbImage};
use serde::{Deserialize, Serialize};
use symphony_ui::theme;

use crate::session::{CanvasColours, Session};

const BLUR_LEVELS: [f32; 5] = [0.0, 3.0, 8.0, 15.0, 26.0];
const GLASS_BLUR: u8 = 32;
const CAMERA_REFRESH: f64 = 1.0 / 6.0;
const CAMERA_PREVIEW_EDGE: u32 = 384;
/// A new Camera frame crossfades over the last one in this long, seconds.
const CAMERA_FADE: f64 = 0.25;

#[derive(Clone, Default, PartialEq, Serialize, Deserialize)]
pub enum Choice {
    #[default]
    Black,
    Plain,
    Studio,
    Dusk,
    Custom(PathBuf),
    Camera,
}

impl Choice {
    fn label(&self) -> &'static str {
        match self {
            Self::Black => "Black",
            Self::Plain => "Plain",
            Self::Studio => "Studio",
            Self::Dusk => "Dusk",
            Self::Custom(_) => "Custom picture…",
            Self::Camera => "Current frame",
        }
    }
}

#[derive(Clone, Copy)]
pub enum ChromeSide {
    Top,
    Bottom,
}

#[derive(Clone)]
pub struct CameraPixels {
    pub size: [u32; 2],
    pub rgba: Arc<Vec<u8>>,
}

#[derive(Default)]
struct CameraRefresh {
    session: String,
    requested: Option<(u64, u32, [u8; 3])>,
    last_started: Option<f64>,
    source: Option<(u64, Arc<symphony_core::doc::Scene>)>,
}

pub struct Wallpaper {
    pub choice: Choice,
    pub blur: u8,
    pending: Option<Receiver<Result<Vec<RgbImage>, String>>>,
    textures: Vec<egui::TextureHandle>,
    /// The Camera frame being faded out, and when the fade began.
    previous: Vec<egui::TextureHandle>,
    fade_start: f64,
    now: f64,
    uploads: u64,
    backdrop: Rect,
    error: Option<String>,
    camera: CameraRefresh,
}

impl Wallpaper {
    pub fn new(ctx: &egui::Context, choice: Choice, blur: u8) -> Self {
        let mut wallpaper = Self {
            choice: Choice::Black,
            pending: None,
            blur: blur.min(100),
            textures: Vec::new(),
            previous: Vec::new(),
            fade_start: 0.0,
            now: 0.0,
            uploads: 0,
            backdrop: Rect::NOTHING,
            error: None,
            camera: CameraRefresh::default(),
        };
        wallpaper.select(ctx, choice);
        wallpaper
    }

    fn select(&mut self, ctx: &egui::Context, choice: Choice) {
        self.textures.clear();
        self.previous.clear();
        self.pending = None;
        self.error = None;
        self.camera = CameraRefresh::default();
        if let Choice::Custom(path) = &choice {
            let path = path.clone();
            let ctx = ctx.clone();
            let (tx, rx) = mpsc::channel();
            self.pending = Some(rx);
            std::thread::spawn(move || {
                let result = load(&path);
                let _ = tx.send(result);
                ctx.request_repaint();
            });
        }
        self.choice = choice;
    }

    pub fn uses_camera(&self) -> bool {
        self.choice == Choice::Camera
    }

    pub fn sync_camera_session(&mut self, key: &str) {
        if self.uses_camera() && self.camera.session != key {
            self.camera = CameraRefresh {
                session: key.to_owned(),
                ..CameraRefresh::default()
            };
            self.textures.clear();
            self.pending = None;
        }
    }

    pub fn update_camera(
        &mut self,
        ctx: &egui::Context,
        session: &Session,
        pixels: Option<CameraPixels>,
    ) {
        if !self.uses_camera() {
            return;
        }
        self.sync_camera_session(&session.key);
        let paper = CanvasColours::get(session.colours.stage);
        let paper = [paper.r(), paper.g(), paper.b()];
        let stamp = (session.doc.version, session.frame, paper);
        // Hold still during playback: re-rendering every frame costs time
        // and flickers behind the panels. It catches up when playback stops.
        if session.playing && !self.textures.is_empty() {
            return;
        }
        if self.pending.is_some() || self.camera.requested == Some(stamp) {
            return;
        }
        let now = ctx.input(|i| i.time);
        if let Some(last) = self.camera.last_started {
            let remaining = CAMERA_REFRESH - (now - last);
            if remaining > 0.0 {
                ctx.request_repaint_after(Duration::from_secs_f64(remaining));
                return;
            }
        }

        // Reuse the Camera view's full render. With the tab hidden, render
        // only a small preview and keep the same snapshot across playback frames.
        let image = match pixels {
            Some(pixels) => {
                self.camera.source = None;
                CameraSource::Rendered(pixels)
            }
            None => {
                let source = self
                    .camera
                    .source
                    .get_or_insert_with(|| (stamp.0, Arc::new(session.doc.clone())));
                if source.0 != stamp.0 {
                    *source = (stamp.0, Arc::new(session.doc.clone()));
                }
                CameraSource::Scene(source.1.clone(), session.frame)
            }
        };
        self.camera.last_started = Some(now);
        self.camera.requested = Some(stamp);
        let (tx, rx) = mpsc::channel();
        self.pending = Some(rx);
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = camera_levels(image, paper);
            let _ = tx.send(result);
            ctx.request_repaint();
        });
    }

    pub fn paint(&mut self, ui: &egui::Ui) {
        if let Some(rx) = &self.pending {
            match rx.try_recv() {
                Ok(Ok(images)) if self.uses_camera() && !self.textures.is_empty() => {
                    // A new Camera frame: keep the old one and crossfade.
                    self.uploads += 1;
                    let fresh = images
                        .into_iter()
                        .enumerate()
                        .map(|(level, image)| {
                            let size = [image.width() as usize, image.height() as usize];
                            ui.ctx().load_texture(
                                format!("workspace-wallpaper-{}-{level}", self.uploads),
                                egui::ColorImage::from_rgb(size, image.as_raw()),
                                egui::TextureOptions::LINEAR,
                            )
                        })
                        .collect();
                    self.previous = std::mem::replace(&mut self.textures, fresh);
                    self.fade_start = ui.input(|i| i.time);
                    self.pending = None;
                }
                Ok(Ok(images)) => {
                    for (level, image) in images.into_iter().enumerate() {
                        let size = [image.width() as usize, image.height() as usize];
                        let image = egui::ColorImage::from_rgb(size, image.as_raw());
                        if let Some(texture) = self.textures.get_mut(level) {
                            texture.set(image, egui::TextureOptions::LINEAR);
                        } else {
                            self.textures.push(ui.ctx().load_texture(
                                format!("workspace-wallpaper-{level}"),
                                image,
                                egui::TextureOptions::LINEAR,
                            ));
                        }
                    }
                    self.pending = None;
                }
                Ok(Err(error)) => {
                    self.error = Some(error);
                    self.pending = None;
                }
                Err(TryRecvError::Disconnected) => {
                    self.error = Some("The wallpaper couldn’t be loaded.".into());
                    self.pending = None;
                }
                Err(TryRecvError::Empty) => {}
            }
        }
        self.now = ui.input(|i| i.time);
        if !self.previous.is_empty() {
            if self.fade() >= 1.0 {
                self.previous.clear();
            } else {
                ui.ctx().request_repaint();
            }
        }
        let rect = ui.max_rect();
        self.backdrop = rect;
        let painter = ui.painter();
        let palette = match self.choice {
            Choice::Studio => Some([[46, 56, 59], [28, 36, 43], [21, 25, 31], [35, 40, 47]]),
            Choice::Dusk => Some([[60, 43, 64], [35, 48, 69], [28, 29, 44], [49, 37, 51]]),
            _ => None,
        };
        if let Some(palette) = palette {
            let mut mesh = egui::Mesh::default();
            for (pos, [r, g, b]) in [
                rect.left_top(),
                rect.right_top(),
                rect.right_bottom(),
                rect.left_bottom(),
            ]
            .into_iter()
            .zip(palette)
            {
                mesh.colored_vertex(pos, Color32::from_rgb(r, g, b));
            }
            mesh.add_triangle(0, 1, 2);
            mesh.add_triangle(0, 2, 3);
            painter.add(egui::Shape::mesh(mesh));
        } else {
            painter.rect_filled(
                rect,
                0.0,
                if self.choice == Choice::Black {
                    Color32::BLACK
                } else {
                    theme::BG_BASE
                },
            );
            if let Some(image) = self.sample(rect, 0.0, self.blur, Color32::WHITE) {
                painter.add(image);
                painter.rect_filled(rect, 0.0, Color32::from_black_alpha(65));
            }
        }
    }

    /// Sample the same wallpaper in window coordinates, clipped to rounded glass.
    pub fn glass(&self, rect: Rect) -> Option<egui::Shape> {
        self.glass_rounded(rect, theme::GLASS_RADIUS)
    }

    /// `glass` clipped to another radius (the welcome card).
    pub fn glass_rounded(&self, rect: Rect, radius: u8) -> Option<egui::Shape> {
        // Interface effects off (slower computers): plain tint, no blur.
        if !theme::effects() {
            return None;
        }
        self.sample(
            rect,
            f32::from(radius),
            self.blur.max(GLASS_BLUR),
            Color32::from_gray(204),
        )
    }

    /// Fade both the cached backdrop sample and the chrome tint into the workspace.
    pub fn chrome(&self, rect: Rect, side: ChromeSide) -> egui::Shape {
        let mut layers = Vec::new();
        if theme::effects() && !self.textures.is_empty() && self.backdrop.is_positive() {
            // The bars use the strongest cached blur, so their edge reads
            // soft; during a Camera crossfade, both frames.
            let fade = self.fade();
            let mut sets = vec![(&self.textures, fade)];
            if fade < 1.0 {
                sets.insert(0, (&self.previous, 1.0));
            }
            for (textures, alpha) in sets {
                let level = textures.len() - 1;
                layers.push(egui::Shape::mesh(chrome_mesh(
                    rect,
                    side,
                    Some((textures[level].id(), self.backdrop)),
                    Color32::from_gray(204).gamma_multiply(alpha),
                )));
            }
        }
        layers.push(egui::Shape::mesh(chrome_mesh(
            rect,
            side,
            None,
            theme::GLASS_CHROME,
        )));
        egui::Shape::Vec(layers)
    }

    /// How far the Camera crossfade has gone, 0..=1 (1 when not fading).
    fn fade(&self) -> f32 {
        if self.previous.is_empty() || !theme::effects() {
            return 1.0;
        }
        let t = ((self.now - self.fade_start) / CAMERA_FADE).clamp(0.0, 1.0) as f32;
        symphony_ui::motion::Motion::Select.ease(t)
    }

    fn sample(&self, rect: Rect, radius: f32, amount: u8, tint: Color32) -> Option<egui::Shape> {
        if !rect.is_positive() || !self.backdrop.is_positive() || self.textures.is_empty() {
            return None;
        }
        let current = self.sample_from(&self.textures, rect, radius, amount, tint);
        let fade = self.fade();
        if fade >= 1.0 {
            return Some(current);
        }
        // The old frame underneath, the new one fading in over it.
        let old = self.sample_from(&self.previous, rect, radius, amount, tint);
        let new = self.sample_from(
            &self.textures,
            rect,
            radius,
            amount,
            tint.gamma_multiply(fade),
        );
        Some(egui::Shape::Vec(vec![old, new]))
    }

    fn sample_from(
        &self,
        textures: &[egui::TextureHandle],
        rect: Rect,
        radius: f32,
        amount: u8,
        tint: Color32,
    ) -> egui::Shape {
        let (lower, upper, blend) = blur_mix(amount);
        let max = textures.len() - 1;
        let shape = |level: usize, tint| {
            egui::Shape::mesh(mask_mesh(
                textures[level.min(max)].id(),
                rect,
                self.backdrop,
                radius,
                tint,
            ))
        };
        let lower = shape(lower, tint);
        if blend == 0.0 {
            lower
        } else {
            egui::Shape::Vec(vec![lower, shape(upper, tint.gamma_multiply(blend))])
        }
    }

    pub fn preferences(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("Wallpaper");
            egui::ComboBox::from_id_salt("wallpaper")
                .selected_text(self.choice.label())
                .show_ui(ui, |ui| {
                    for choice in [
                        Choice::Black,
                        Choice::Plain,
                        Choice::Studio,
                        Choice::Dusk,
                        Choice::Camera,
                    ] {
                        if ui
                            .selectable_label(self.choice == choice, choice.label())
                            .clicked()
                        {
                            self.select(ui.ctx(), choice);
                        }
                    }
                    if ui.button("Choose picture…").clicked() {
                        ui.close();
                        if let Some(path) = rfd::FileDialog::new()
                            .add_filter("Wallpaper image", &["png", "jpg", "jpeg"])
                            .pick_file()
                        {
                            self.select(ui.ctx(), Choice::Custom(path));
                        }
                    }
                });
        });
        if matches!(self.choice, Choice::Custom(_) | Choice::Camera) {
            symphony_ui::widgets::precise_slider(
                ui,
                egui::Slider::new(&mut self.blur, 0..=100)
                    .text("Wallpaper blur")
                    .suffix("%"),
            );
        }
        if self.pending.is_some() && self.textures.is_empty() {
            ui.horizontal(|ui| {
                symphony_ui::feedback::spinner(ui, 14.0);
                ui.label("Preparing wallpaper");
                if ui.button("Cancel").clicked() {
                    self.select(ui.ctx(), Choice::Plain);
                }
            });
        }
        if let Some(error) = &self.error {
            ui.label(error);
        }
    }
}

/// How far the bars' soft edge reaches into the workspace, in points.
const CHROME_FEATHER: f32 = 22.0;

fn chrome_mesh(
    rect: Rect,
    side: ChromeSide,
    texture: Option<(egui::TextureId, Rect)>,
    fill: Color32,
) -> egui::Mesh {
    let mut mesh = texture.map_or_else(egui::Mesh::default, |(id, _)| egui::Mesh::with_texture(id));
    let inside = match side {
        ChromeSide::Top => rect.top(),
        ChromeSide::Bottom => rect.bottom(),
    };
    let edge = match side {
        ChromeSide::Top => rect.bottom(),
        ChromeSide::Bottom => rect.top(),
    };
    let direction = (edge - inside).signum();
    // A long, soft edge (Tristan: "more blurry"): it starts fading well
    // inside the bar and dies out CHROME_FEATHER points into the workspace,
    // with a smoothstep so neither end shows a line.
    let transition = (rect.height() * 0.45).min(12.0);
    let start = edge - direction * transition;
    let end = edge + direction * CHROME_FEATHER;
    const STEPS: usize = 10;
    let rows = [(inside, 1.0f32)].into_iter().chain((0..=STEPS).map(|i| {
        let s = i as f32 / STEPS as f32;
        let smooth = s * s * (3.0 - 2.0 * s);
        (egui::lerp(start..=end, s), 1.0 - smooth)
    }));
    for (row, (y, alpha)) in rows.enumerate() {
        for x in [rect.left(), rect.right()] {
            let pos = Pos2::new(x, y);
            let uv = texture.map_or(egui::epaint::WHITE_UV, |(_, backdrop)| {
                let offset = pos - backdrop.min;
                Pos2::new(offset.x / backdrop.width(), offset.y / backdrop.height())
            });
            mesh.vertices.push(egui::epaint::Vertex {
                pos,
                uv,
                color: fill.gamma_multiply(alpha),
            });
        }
        if row > 0 {
            let previous = (row as u32 - 1) * 2;
            mesh.add_triangle(previous, previous + 1, previous + 2);
            mesh.add_triangle(previous + 1, previous + 3, previous + 2);
        }
    }
    mesh
}

fn blur_mix(amount: u8) -> (usize, usize, f32) {
    let max = BLUR_LEVELS.len() - 1;
    let level = f32::from(amount.min(100)) * max as f32 / 100.0;
    let lower = level.floor() as usize;
    (lower, (lower + 1).min(max), level.fract())
}

fn mask_mesh(
    id: egui::TextureId,
    rect: Rect,
    backdrop: Rect,
    radius: f32,
    tint: Color32,
) -> egui::Mesh {
    let radius = radius.min(rect.width() / 2.0).min(rect.height() / 2.0);
    let uv = |point: Pos2| {
        let local = point - backdrop.min;
        Pos2::new(local.x / backdrop.width(), local.y / backdrop.height())
    };
    let mut mesh = egui::Mesh::with_texture(id);
    let mut vertex = |pos| {
        mesh.vertices.push(egui::epaint::Vertex {
            pos,
            uv: uv(pos),
            color: tint,
        })
    };
    vertex(rect.center());
    for (index, corner) in [
        rect.right_top(),
        rect.right_bottom(),
        rect.left_bottom(),
        rect.left_top(),
    ]
    .into_iter()
    .enumerate()
    {
        if radius == 0.0 {
            vertex(corner);
            continue;
        }
        let offset = corner - rect.center();
        let arc = corner - Vec2::new(offset.x.signum() * radius, offset.y.signum() * radius);
        for segment in 0..=6 {
            let angle = (index as f32 - 1.0 + segment as f32 / 6.0) * std::f32::consts::FRAC_PI_2;
            vertex(arc + Vec2::angled(angle) * radius);
        }
    }
    let last = mesh.vertices.len() as u32 - 1;
    for index in 1..=last {
        mesh.add_triangle(0, index, if index == last { 1 } else { index + 1 });
    }
    mesh
}

enum CameraSource {
    Rendered(CameraPixels),
    Scene(Arc<symphony_core::doc::Scene>, u32),
}

fn preview_size([width, height]: [u32; 2]) -> [u32; 2] {
    let scale = (CAMERA_PREVIEW_EDGE as f32 / width.max(height).max(1) as f32).min(1.0);
    [width, height].map(|side| (side as f32 * scale).round().max(1.0) as u32)
}

fn camera_levels(source: CameraSource, paper: [u8; 3]) -> Result<Vec<RgbImage>, String> {
    let image = match source {
        CameraSource::Rendered(pixels) => {
            let [width, height] = preview_size(pixels.size);
            let [source_width, source_height] = pixels.size;
            let source = image::ImageBuffer::<image::Rgba<u8>, &[u8]>::from_raw(
                source_width,
                source_height,
                pixels.rgba.as_slice(),
            )
            .ok_or("The Camera wallpaper couldn’t read the frame.")?;
            imageops::resize(&source, width, height, imageops::FilterType::Triangle)
        }
        CameraSource::Scene(scene, frame) => {
            let [width, height] = preview_size([scene.settings.width, scene.settings.height]);
            let pixmap = symphony_render::render_camera(&scene, frame, width, height, None)
                .ok_or("The Camera wallpaper couldn’t render this frame.")?;
            image::RgbaImage::from_raw(width, height, pixmap.take())
                .ok_or("The Camera wallpaper couldn’t read the frame.")?
        }
    };
    let rgb = RgbImage::from_fn(image.width(), image.height(), |x, y| {
        let pixel = image.get_pixel(x, y).0;
        let paper_amount = 255 - u16::from(pixel[3]);
        image::Rgb(std::array::from_fn(|channel| {
            (u16::from(pixel[channel]) + (u16::from(paper[channel]) * paper_amount + 127) / 255)
                .min(255) as u8
        }))
    });
    let blurred: Vec<_> = BLUR_LEVELS[1..]
        .iter()
        .map(|&sigma| imageops::blur(&rgb, sigma * 0.5))
        .collect();
    Ok(std::iter::once(rgb).chain(blurred).collect())
}

fn load(path: &Path) -> Result<Vec<RgbImage>, String> {
    let result = (|| -> image::ImageResult<_> {
        let mut reader = ImageReader::open(path)?.with_guessed_format()?;
        let mut limits = image::Limits::default();
        limits.max_image_width = Some(8192);
        limits.max_image_height = Some(8192);
        limits.max_alloc = Some(128 * 1024 * 1024);
        reader.limits(limits);
        reader.decode()
    })();
    let image = result.map_err(|e| format!("Couldn’t open the wallpaper ({e})."))?;
    let sharp = image.thumbnail(1600, 1600).into_rgb8();
    let filtered = image
        .resize(768, 768, imageops::FilterType::Triangle)
        .into_rgb8();
    let levels = std::iter::once(sharp)
        .chain(
            BLUR_LEVELS[1..]
                .iter()
                .map(|&sigma| imageops::blur(&filtered, sigma)),
        )
        .collect();
    Ok(levels)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn camera_preview_reuses_premultiplied_pixels_and_keeps_paper_colour() {
        let pixels = CameraPixels {
            size: [2, 1],
            rgba: Arc::new(vec![0, 0, 0, 0, 64, 0, 0, 128]),
        };
        let levels = camera_levels(CameraSource::Rendered(pixels), [200, 100, 50]).unwrap();
        assert_eq!(levels.len(), BLUR_LEVELS.len());
        assert_eq!(levels[0][(0, 0)].0, [200, 100, 50]);
        assert_eq!(levels[0][(1, 0)].0, [164, 50, 25]);
        assert_eq!(preview_size([1920, 1080]), [384, 216]);
        assert_eq!(preview_size([1080, 1920]), [216, 384]);
    }

    #[test]
    fn chrome_feathers_toward_the_workspace_on_both_edges() {
        let rect = Rect::from_min_max(Pos2::new(0.0, 40.0), Pos2::new(400.0, 64.0));
        let top = chrome_mesh(rect, ChromeSide::Top, None, Color32::WHITE);
        let bottom = chrome_mesh(rect, ChromeSide::Bottom, None, Color32::WHITE);
        assert_eq!(top.vertices[0].pos.y, rect.top());
        assert_eq!(bottom.vertices[0].pos.y, rect.bottom());
        assert_eq!(
            top.vertices.last().unwrap().pos.y,
            rect.bottom() + CHROME_FEATHER
        );
        assert_eq!(
            bottom.vertices.last().unwrap().pos.y,
            rect.top() - CHROME_FEATHER
        );
        for mesh in [top, bottom] {
            assert_eq!(mesh.vertices[0].color.a(), 255);
            assert_eq!(mesh.vertices.last().unwrap().color.a(), 0);
            assert!(mesh
                .vertices
                .windows(2)
                .all(|pair| pair[0].color.a() >= pair[1].color.a()));
        }
    }

    #[test]
    fn blur_slider_blends_cached_levels_and_preserves_zero() {
        assert_eq!(blur_mix(0), (0, 1, 0.0));
        assert_eq!(blur_mix(25), (1, 2, 0.0));
        assert_eq!(blur_mix(100), (4, 4, 0.0));
        let (lower, upper, blend) = blur_mix(62);
        assert_eq!((lower, upper), (2, 3));
        assert!((blend - 0.48).abs() < 0.0001);
    }

    #[test]
    fn moving_glass_resamples_wallpaper_in_screen_coordinates() {
        let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 400.0));
        let pane = Rect::from_min_size(Pos2::new(100.0, 120.0), Vec2::new(200.0, 120.0));
        let id = egui::TextureId::Managed(1);
        let mask = mask_mesh(id, pane, screen, 12.0, Color32::WHITE);
        let moved = mask_mesh(
            id,
            pane.translate(Vec2::X * 80.0),
            screen,
            12.0,
            Color32::WHITE,
        );
        assert_eq!(mask.vertices[0].pos, pane.center());
        assert_eq!(moved.texture_id, mask.texture_id);
        assert!((moved.vertices[0].uv.x - mask.vertices[0].uv.x - 0.1).abs() < 0.0001);
        assert!(mask
            .vertices
            .iter()
            .all(|v| v.pos.distance(pane.right_top()) > 0.0));
    }
}
