//! Editor backdrop. Decode and blur once, then stretch the cached texture.
use std::{
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver, TryRecvError},
};

use eframe::egui::{self, Color32};
use image::{imageops, ImageReader, Rgb, RgbImage};
use serde::{Deserialize, Serialize};
use symphony_ui::theme;

#[derive(Clone, Default, PartialEq, Serialize, Deserialize)]
pub enum Choice {
    #[default]
    Black,
    Plain,
    Studio,
    Dusk,
    Custom(PathBuf),
}

impl Choice {
    fn label(&self) -> &'static str {
        match self {
            Self::Black => "Black",
            Self::Plain => "Plain",
            Self::Studio => "Studio",
            Self::Dusk => "Dusk",
            Self::Custom(_) => "Custom picture…",
        }
    }
}

pub struct Wallpaper {
    pub choice: Choice,
    pending: Option<Receiver<Result<RgbImage, String>>>,
    texture: Option<egui::TextureHandle>,
    error: Option<String>,
}

impl Wallpaper {
    pub fn new(ctx: &egui::Context, choice: Choice) -> Self {
        let mut wallpaper = Self {
            choice: Choice::Black,
            pending: None,
            texture: None,
            error: None,
        };
        wallpaper.select(ctx, choice);
        wallpaper
    }

    fn select(&mut self, ctx: &egui::Context, choice: Choice) {
        self.texture = None;
        self.pending = None;
        self.error = None;
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

    pub fn paint(&mut self, ui: &egui::Ui) {
        if let Some(rx) = &self.pending {
            match rx.try_recv() {
                Ok(Ok(image)) => {
                    let size = [image.width() as usize, image.height() as usize];
                    self.texture = Some(ui.ctx().load_texture(
                        "workspace-wallpaper",
                        egui::ColorImage::from_rgb(size, image.as_raw()),
                        egui::TextureOptions::LINEAR,
                    ));
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
        let rect = ui.max_rect();
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
            if let Some(image) = &self.texture {
                painter.image(
                    image.id(),
                    rect,
                    egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                    Color32::WHITE,
                );
            }
        }
    }

    pub fn preferences(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("Wallpaper");
            egui::ComboBox::from_id_salt("wallpaper")
                .selected_text(self.choice.label())
                .show_ui(ui, |ui| {
                    for choice in [Choice::Black, Choice::Plain, Choice::Studio, Choice::Dusk] {
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
        if self.pending.is_some() {
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

fn load(path: &Path) -> Result<RgbImage, String> {
    let result = (|| -> image::ImageResult<_> {
        let mut reader = ImageReader::open(path)?.with_guessed_format()?;
        let mut limits = image::Limits::default();
        limits.max_image_width = Some(8192);
        limits.max_image_height = Some(8192);
        limits.max_alloc = Some(128 * 1024 * 1024);
        reader.limits(limits);
        Ok(reader.decode()?.thumbnail(640, 640).into_rgb8())
    })();
    let image = result.map_err(|e| format!("Couldn’t open the wallpaper ({e})."))?;
    let mut blurred = imageops::blur(&image, 18.0);
    for pixel in blurred.pixels_mut() {
        let [r, g, b] = pixel.0.map(u16::from);
        let gray = (54 * r + 183 * g + 19 * b) / 256;
        *pixel = Rgb([r, g, b].map(|channel| ((channel + 2 * gray) * 74 / 765) as u8));
    }
    Ok(blurred)
}
