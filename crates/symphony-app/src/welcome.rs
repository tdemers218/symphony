//! Scene setup shown before the editor is painted.
use std::{
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver, TryRecvError},
};

use eframe::egui::{self, Margin, Sense, Vec2};
use symphony_core::doc::{Scene, Settings};
use symphony_ui::{feedback, theme, widgets};

use crate::session::Session;

pub enum Action {
    Open,
    Recent(PathBuf),
    Cancel,
    Created(Box<Scene>, PathBuf),
}

type CreatedScene = Result<(Box<Scene>, PathBuf), String>;

pub struct Welcome {
    name: String,
    folder: Option<PathBuf>,
    settings: Settings,
    pending: Option<Receiver<CreatedScene>>,
    error: Option<String>,
    can_cancel: bool,
    mark: Option<egui::TextureHandle>,
}

impl Welcome {
    pub fn new(can_cancel: bool) -> Self {
        Self {
            name: "Untitled scene".into(),
            folder: dirs::document_dir(),
            settings: Settings::default(),
            pending: None,
            error: None,
            can_cancel,
            mark: None,
        }
    }

    pub fn creating(&self) -> bool {
        self.pending.is_some()
    }

    pub fn ui(
        &mut self,
        ui: &mut egui::Ui,
        recent: &[PathBuf],
        preparing: bool,
        opening: bool,
    ) -> Option<Action> {
        if let Some(rx) = &self.pending {
            match rx.try_recv() {
                Ok(Ok((scene, path))) => return Some(Action::Created(scene, path)),
                Ok(Err(error)) => {
                    self.pending = None;
                    self.error = Some(error);
                }
                Err(TryRecvError::Disconnected) => {
                    self.pending = None;
                    self.error = Some(
                        "Scene creation stopped. Check the project folder before trying again."
                            .into(),
                    );
                }
                Err(TryRecvError::Empty) => {}
            }
        }

        if cfg!(target_os = "macos") {
            let top = egui::Rect::from_min_size(
                ui.max_rect().min,
                Vec2::new(ui.available_width(), theme::TOP_BAR),
            );
            let drag = ui.interact(top, egui::Id::unique("welcome-titlebar"), Sense::drag());
            if drag.drag_started() {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
            }
        }
        let mark = self
            .mark
            .get_or_insert_with(|| crate::app_icon::texture(ui.ctx()))
            .id();
        let backdrop = ui.max_rect();
        let art_size = (backdrop.height() * 0.55).min(backdrop.width() * 0.36);
        let art_center = egui::pos2(
            backdrop.left() + backdrop.width() * 0.75,
            backdrop.center().y,
        );
        ui.painter().image(
            mark,
            egui::Rect::from_center_size(art_center, Vec2::splat(art_size)),
            egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
            egui::Color32::WHITE.gamma_multiply(0.72),
        );
        let busy = preparing || opening || self.pending.is_some();
        let mut action = None;
        egui::ScrollArea::vertical().show(ui, |ui| {
            let width = ui.available_width().min(900.0);
            let left = ((ui.available_width() - width) / 2.0).max(0.0);
            ui.horizontal(|ui| {
                ui.add_space(left);
                ui.vertical(|ui| {
                    ui.set_width(width);
                    ui.add_space(48.0);
                    ui.horizontal_centered(|ui| {
                        ui.spacing_mut().item_spacing.x = 1.0;
                        ui.vertical(|ui| {
                            // Align the bottom of the brush mark with the type baseline.
                            ui.add_space(10.0);
                            let image = egui::load::SizedTexture::new(mark, Vec2::new(31.0, 36.0));
                            ui.add(
                                egui::Image::new(image)
                                    .uv(egui::Rect::from_min_max(
                                        egui::pos2(0.16, 0.11),
                                        egui::pos2(0.84, 0.89),
                                    ))
                                    .alt_text("S"),
                            );
                        });
                        ui.label(egui::RichText::new("ymphony").font(theme::wordmark()));
                    });
                    ui.add_space(16.0);
                    ui.horizontal_top(|ui| {
                        let form_width = width * 0.53;
                        let card = egui::Frame::new()
                            .fill(theme::GLASS_PANEL)
                            .corner_radius(theme::GLASS_RADIUS)
                            .inner_margin(Margin::same(18))
                            .show(ui, |ui| {
                                ui.vertical(|ui| {
                                    ui.set_width((form_width - 36.0).max(0.0));
                                    ui.heading("New scene");
                                    ui.add_space(8.0);
                                    ui.add_enabled_ui(!opening && self.pending.is_none(), |ui| {
                                        self.form(ui);
                                    });
                                    ui.add_space(10.0);
                                    if let Some(error) = &self.error {
                                        ui.label(error);
                                    }
                                    ui.horizontal_wrapped(|ui| {
                                        if widgets::primary_button(
                                            ui,
                                            "Create scene",
                                            !busy && self.folder.is_some(),
                                        )
                                        .clicked()
                                        {
                                            self.create(ui.ctx());
                                        }
                                        if ui
                                            .add_enabled(!busy, egui::Button::new("Open project…"))
                                            .clicked()
                                        {
                                            action = Some(Action::Open);
                                        }
                                        if self.can_cancel
                                            && ui
                                                .add_enabled(!busy, egui::Button::new("Cancel"))
                                                .clicked()
                                        {
                                            action = Some(Action::Cancel);
                                        }
                                        if busy {
                                            feedback::spinner(ui, 14.0);
                                            ui.label(if opening {
                                                "Opening project"
                                            } else if preparing {
                                                "Checking for recovered scenes"
                                            } else {
                                                "Creating scene"
                                            });
                                        }
                                    });
                                    if !recent.is_empty() {
                                        ui.add_space(8.0);
                                        ui.label(
                                            egui::RichText::new("Recent projects")
                                                .color(theme::TEXT_SECONDARY),
                                        );
                                        ui.add_enabled_ui(!busy, |ui| {
                                            for path in recent {
                                                let name = symphony_io::project_name(path);
                                                let folder =
                                                    path.parent().and_then(Path::file_name);
                                                let text = folder.map_or_else(
                                                    || name.clone(),
                                                    |folder| {
                                                        format!(
                                                            "{name}   •   {}",
                                                            folder.to_string_lossy()
                                                        )
                                                    },
                                                );
                                                let row = egui::Button::new(text)
                                                    .min_size(Vec2::new(
                                                        ui.available_width(),
                                                        theme::ROW,
                                                    ))
                                                    .wrap_mode(egui::TextWrapMode::Truncate);
                                                if ui
                                                    .add(row)
                                                    .on_hover_text(path.display().to_string())
                                                    .clicked()
                                                {
                                                    action = Some(Action::Recent(path.clone()));
                                                }
                                            }
                                        });
                                    }
                                });
                            });
                        theme::glass_rim(ui.painter(), card.response.rect);
                    });
                    ui.add_space(20.0);
                });
            });
        });
        action
    }

    fn form(&mut self, ui: &mut egui::Ui) {
        egui::Grid::new("scene-setup")
            .spacing(Vec2::new(12.0, 8.0))
            .show(ui, |ui| {
                ui.label("Name");
                ui.add(egui::TextEdit::singleline(&mut self.name).desired_width(290.0));
                ui.end_row();
                ui.label("Save location");
                ui.horizontal(|ui| {
                    if ui.button("Choose folder…").clicked() {
                        let mut dialog =
                            rfd::FileDialog::new().set_title("Choose where to save the scene");
                        if let Some(folder) = &self.folder {
                            dialog = dialog.set_directory(folder);
                        }
                        if let Some(folder) = dialog.pick_folder() {
                            self.folder = Some(folder);
                        }
                    }
                    if let Some(folder) = &self.folder {
                        let name = folder
                            .file_name()
                            .unwrap_or(folder.as_os_str())
                            .to_string_lossy();
                        ui.label(name).on_hover_text(folder.display().to_string());
                    }
                });
                ui.end_row();
                ui.label("Camera size");
                let size = (self.settings.width, self.settings.height);
                let presets = [
                    ("HD", 1920, 1080),
                    ("HD portrait", 1080, 1920),
                    ("Square", 1080, 1080),
                    ("4K UHD", 3840, 2160),
                ];
                let label = presets
                    .iter()
                    .find(|(_, w, h)| (*w, *h) == size)
                    .map_or("Custom", |p| p.0);
                egui::ComboBox::from_id_salt("resolution")
                    .selected_text(label)
                    .show_ui(ui, |ui| {
                        for (label, w, h) in presets {
                            if ui
                                .selectable_label(size == (w, h), format!("{label}  {w} × {h}"))
                                .clicked()
                            {
                                self.settings.width = w;
                                self.settings.height = h;
                            }
                        }
                    });
                ui.end_row();
                ui.label("Dimensions");
                ui.horizontal(|ui| {
                    ui.add(
                        egui::DragValue::new(&mut self.settings.width)
                            .range(1..=8192)
                            .prefix("W "),
                    );
                    ui.add(
                        egui::DragValue::new(&mut self.settings.height)
                            .range(1..=8192)
                            .prefix("H "),
                    );
                });
                ui.end_row();
                ui.label("Timing");
                ui.horizontal(|ui| {
                    widgets::frame_rate(ui, "scene-fps", &mut self.settings.fps);
                    ui.add(
                        egui::DragValue::new(&mut self.settings.length)
                            .range(1..=100_000)
                            .suffix(" frames"),
                    );
                });
                ui.end_row();
            });
    }

    fn create(&mut self, ctx: &egui::Context) {
        let name = self.name.trim();
        if name.is_empty()
            || name.starts_with('.')
            || name.ends_with('.')
            || name
                .chars()
                .any(|ch| ch.is_control() || "/\\:*?\"<>|".contains(ch))
        {
            self.error = Some("Enter a scene name without slashes or filename punctuation.".into());
            return;
        }
        let Some(folder) = &self.folder else { return };
        let path = symphony_io::with_extension(&folder.join(name));
        let settings = self.settings.clone();
        let (tx, rx) = mpsc::channel();
        self.pending = Some(rx);
        self.error = None;
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = save_new(&path, settings).map(|scene| (Box::new(scene), path));
            let _ = tx.send(result);
            ctx.request_repaint();
        });
    }
}

fn save_new(path: &Path, settings: Settings) -> Result<Scene, String> {
    std::fs::create_dir(path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::AlreadyExists {
            "A project or file already has that name. Choose another scene name.".into()
        } else {
            format!("Couldn’t create the project folder ({e}).")
        }
    })?;
    let mut scene = Session::new_scene().doc;
    scene.settings = settings;
    symphony_io::save(&scene, path).map_err(|e| e.0)?;
    Ok(scene)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_a_project_with_chosen_settings_and_preserves_existing_files() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("Walk.symphony");
        let settings = Settings {
            width: 1080,
            height: 1920,
            fps: 12.0,
            length: 96,
        };
        save_new(&path, settings.clone()).unwrap();

        let saved = symphony_io::open(&path).unwrap();
        assert_eq!(saved.settings, settings);
        assert_eq!(saved.layers.len(), 1);
        assert!(save_new(&path, Settings::default()).is_err());
        assert_eq!(symphony_io::open(&path).unwrap().settings, settings);
    }
}
