//! Scene setup shown before the editor is painted.
use std::{
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver, TryRecvError},
};

use eframe::egui::{self, Margin, Sense, Vec2};
use symphony_core::doc::{Scene, Settings};
use symphony_ui::motion::{self, Motion};
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
    /// Length shown in seconds instead of frames.
    seconds: bool,
    /// When this screen first drew, for its fade-in.
    born: Option<f64>,
}

/// Scene presets: name, width, height, frame rate, length in seconds.
const PRESETS: [(&str, u32, u32, f32, f32); 6] = [
    ("HD 1080p", 1920, 1080, 24.0, 10.0),
    ("4K UHD", 3840, 2160, 24.0, 10.0),
    ("720p", 1280, 720, 24.0, 10.0),
    ("Vertical", 1080, 1920, 30.0, 15.0),
    ("Square", 1080, 1080, 30.0, 10.0),
    ("Storyboard", 1920, 1080, 12.0, 30.0),
];

/// The column the logo, title and panel share.
const COLUMN: f32 = 560.0;

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
            seconds: false,
            born: None,
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
        glass: &dyn Fn(egui::Rect) -> Option<egui::Shape>,
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
        let busy = preparing || opening || self.pending.is_some();
        let mut action = None;

        // Logo, then title, then panel fade in, a beat apart (Appear).
        let ctx = ui.ctx().clone();
        let now = ctx.input(|i| i.time);
        let born = *self.born.get_or_insert(now);
        let step = |delay: f64| -> f32 {
            if !motion::enabled(&ctx) {
                return 1.0;
            }
            let t = ((now - born - delay) / Motion::Appear.seconds()).clamp(0.0, 1.0) as f32;
            if t < 1.0 {
                ctx.request_repaint();
            }
            Motion::Appear.ease(t)
        };
        let (logo_in, title_in, panel_in) = (step(0.0), step(0.08), step(0.16));

        // One centred column on the 8-point grid: mark, title, panel. The
        // whole column is centred vertically using last frame's height.
        let area = ui.max_rect();
        let height_id = egui::Id::unique("welcome-height");
        let last_h: f32 = ctx.data(|d| d.get_temp(height_id)).unwrap_or(560.0);
        let top =
            (area.top() + ((area.height() - last_h) / 2.0).max(theme::TOP_BAR + 16.0)).round();
        let column = egui::Rect::from_min_size(
            egui::pos2((area.center().x - COLUMN / 2.0).round(), top),
            Vec2::new(COLUMN.min(area.width() - 32.0), area.bottom() - top),
        );
        let mut col = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(column)
                .layout(egui::Layout::top_down(egui::Align::Center)),
        );
        // The mark.
        col.scope(|ui| {
            ui.multiply_opacity(logo_in);
            ui.add(
                egui::Image::new(egui::load::SizedTexture::new(mark, Vec2::splat(88.0)))
                    .alt_text("Symphony logo"),
            );
        });
        col.add_space(8.0);
        // The title, centred under the mark.
        col.scope(|ui| {
            ui.multiply_opacity(title_in);
            ui.label(egui::RichText::new("Symphony").font(theme::wordmark()));
        });
        col.add_space(24.0);

        // The panel: glass over the blurred wallpaper.
        let painter = col.painter().clone();
        let backdrop = painter.add(egui::Shape::Noop);
        let card = col
            .scope(|ui| {
                ui.multiply_opacity(panel_in);
                egui::Frame::new()
                    .fill(theme::GLASS_PANEL)
                    .corner_radius(theme::HERO_RADIUS)
                    .inner_margin(Margin::same(theme::HERO_PAD))
                    .show(ui, |ui| {
                        ui.set_width(column.width() - 48.0);
                        ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                            ui.heading("New scene");
                            ui.add_space(8.0);
                            ui.add_enabled_ui(!opening && self.pending.is_none(), |ui| {
                                self.presets(ui);
                                ui.add_space(16.0);
                                self.form(ui);
                            });
                            ui.add_space(16.0);
                            if let Some(error) = &self.error {
                                ui.label(error);
                                ui.add_space(8.0);
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
                                    && ui.add_enabled(!busy, egui::Button::new("Cancel")).clicked()
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
                                ui.add_space(16.0);
                                ui.label(
                                    egui::RichText::new("Recent projects")
                                        .color(theme::TEXT_SECONDARY),
                                );
                                ui.add_enabled_ui(!busy, |ui| {
                                    for path in recent.iter().take(5) {
                                        let name = symphony_io::project_name(path);
                                        let folder = path.parent().and_then(Path::file_name);
                                        let text = folder.map_or_else(
                                            || name.clone(),
                                            |folder| {
                                                format!("{name}   •   {}", folder.to_string_lossy())
                                            },
                                        );
                                        let row = egui::Button::new(text)
                                            .min_size(Vec2::new(ui.available_width(), theme::ROW))
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
                    })
                    .response
                    .rect
            })
            .inner;
        if panel_in > 0.0 {
            let mut under = Vec::new();
            if let Some(sample) = glass(card) {
                under.push(sample);
            }
            painter.set(backdrop, egui::Shape::Vec(under));
            theme::glass_rim_rounded(&painter, card, theme::HERO_RADIUS);
        }
        let used = card.bottom() - top;
        if (used - last_h).abs() > 0.5 {
            ctx.data_mut(|d| d.insert_temp(height_id, used));
            ctx.request_repaint();
        }
        action
    }

    /// One-click scene setups: size, frame rate and length together.
    fn presets(&mut self, ui: &mut egui::Ui) {
        ui.label(egui::RichText::new("Presets").color(theme::TEXT_SECONDARY));
        ui.horizontal_wrapped(|ui| {
            for (name, w, h, fps, secs) in PRESETS {
                let s = &self.settings;
                let length = (secs * fps).round() as u32;
                let on = (s.width, s.height, s.length) == (w, h, length) && s.fps == fps;
                if ui
                    .selectable_label(on, name)
                    .on_hover_text(format!("{w} × {h}, {fps:.0} fps, {secs:.0} seconds"))
                    .clicked()
                {
                    self.settings.width = w;
                    self.settings.height = h;
                    self.settings.fps = fps;
                    self.settings.length = length;
                }
            }
        });
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
                    frame_preview(ui, self.settings.width, self.settings.height);
                });
                ui.end_row();
                ui.label("Timing");
                ui.horizontal(|ui| {
                    widgets::frame_rate(ui, "scene-fps", &mut self.settings.fps);
                    let fps = self.settings.fps.max(1.0);
                    if self.seconds {
                        let mut secs = self.settings.length as f32 / fps;
                        if ui
                            .add(
                                egui::DragValue::new(&mut secs)
                                    .range(0.1..=10_000.0)
                                    .speed(0.1)
                                    .max_decimals(2)
                                    .suffix(" s"),
                            )
                            .changed()
                        {
                            self.settings.length = ((secs * fps).round() as u32).max(1);
                        }
                    } else {
                        ui.add(
                            egui::DragValue::new(&mut self.settings.length)
                                .range(1..=100_000)
                                .suffix(" frames"),
                        );
                    }
                    ui.selectable_value(&mut self.seconds, false, "Frames");
                    ui.selectable_value(&mut self.seconds, true, "Seconds");
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

/// A small picture of the frame's shape, with its aspect ratio.
fn frame_preview(ui: &mut egui::Ui, w: u32, h: u32) {
    let (w, h) = (w.max(1) as f32, h.max(1) as f32);
    let box_size = 28.0;
    let size = if w >= h {
        Vec2::new(box_size, (box_size * h / w).max(3.0))
    } else {
        Vec2::new((box_size * w / h).max(3.0), box_size)
    };
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(box_size), Sense::hover());
    let frame = egui::Rect::from_center_size(rect.center(), size);
    let p = ui.painter();
    p.rect_filled(frame, 2.0, theme::CANVAS_STAGE.gamma_multiply(0.85));
    p.rect_stroke(
        frame,
        2.0,
        egui::Stroke::new(1.0, theme::LINE_STRONG),
        egui::StrokeKind::Inside,
    );
    let ratio = aspect_name(w as u32, h as u32);
    response.on_hover_text(format!("Frame shape {ratio}"));
    ui.label(
        egui::RichText::new(ratio)
            .font(theme::dense())
            .color(theme::TEXT_SECONDARY),
    );
}

/// "16:9", "9:16", "1:1", or the reduced ratio.
fn aspect_name(w: u32, h: u32) -> String {
    fn gcd(a: u32, b: u32) -> u32 {
        if b == 0 {
            a
        } else {
            gcd(b, a % b)
        }
    }
    let g = gcd(w, h).max(1);
    let (a, b) = (w / g, h / g);
    if a <= 64 && b <= 64 {
        format!("{a}:{b}")
    } else {
        format!("{:.2}:1", w as f32 / h as f32)
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
    fn aspect_ratios_read_naturally() {
        assert_eq!(aspect_name(1920, 1080), "16:9");
        assert_eq!(aspect_name(1080, 1920), "9:16");
        assert_eq!(aspect_name(1080, 1080), "1:1");
        assert_eq!(
            aspect_name(2048, 1080),
            "256:135".replace("256:135", "1.90:1")
        );
    }

    #[test]
    fn creates_a_project_with_chosen_settings_and_preserves_existing_files() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("Walk.symphony");
        let settings = Settings {
            width: 1080,
            height: 1920,
            fps: 12.0,
            length: 96,
            ..Default::default()
        };
        save_new(&path, settings.clone()).unwrap();

        let saved = symphony_io::open(&path).unwrap();
        assert_eq!(saved.settings, settings);
        assert_eq!(saved.layers.len(), 1);
        assert!(save_new(&path, Settings::default()).is_err());
        assert_eq!(symphony_io::open(&path).unwrap().settings, settings);
    }
}
