//! File > Export, laid out like Harmony 27's export dialogs: output path,
//! export range (all frames, or from-to) and resolution. The export runs on
//! a background thread from a snapshot of the scene, with a progress bar
//! and Cancel (ENGINEERING E5.2, E5.3).

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;

use eframe::egui;
use symphony_render::{ExportJob, Format};
use symphony_ui::{theme, widgets};

use crate::session::Session;

enum Msg {
    Progress(u32, u32),
    Done(Result<PathBuf, String>),
}

struct Running {
    cancel: Arc<AtomicBool>,
    rx: Receiver<Msg>,
    done: u32,
    total: u32,
}

pub struct ExportDialog {
    format: Option<Format>,
    path: Option<PathBuf>,
    all_frames: bool,
    first: u32,
    last: u32,
    scale: f32,
    movie_fps: Option<f32>,
    running: Option<Running>,
    result: Option<Result<PathBuf, String>>,
}

impl Default for ExportDialog {
    fn default() -> Self {
        ExportDialog {
            format: None,
            path: None,
            all_frames: true,
            first: 1,
            last: 1,
            scale: 1.0,
            movie_fps: None,
            running: None,
            result: None,
        }
    }
}

fn title(f: Format) -> &'static str {
    match f {
        Format::AnimatedGif => "Export to Animated GIF",
        Format::PngSequence => "Export Image Sequence (PNG)",
        Format::Mp4Movie => "Export Movie (H.264 MP4)",
    }
}

/// What the dialog asks the app to do.
#[derive(Default)]
pub struct Request {
    /// Download Cisco's OpenH264 (the user clicked the button).
    pub download_codec: bool,
}

impl ExportDialog {
    pub fn open(&mut self, format: Format, s: &Session) {
        if self.running.is_some() {
            return;
        }
        self.format = Some(format);
        self.movie_fps = None;
        self.last = s.length();
        self.first = self.first.clamp(1, self.last);
        self.result = None;
        self.path = None;
    }

    pub fn busy(&self) -> bool {
        self.running.is_some()
    }

    /// Frames rendered so far, for the status bar (DESIGN D17.5).
    pub fn progress(&self) -> Option<(u32, u32)> {
        self.running.as_ref().map(|r| (r.done, r.total))
    }

    fn browse(&mut self, s: &Session) {
        let Some(f) = self.format else { return };
        let (ext, name) = match f {
            Format::AnimatedGif => ("gif", format!("{}.gif", s.name())),
            Format::PngSequence => ("png", format!("{}.png", s.name())),
            Format::Mp4Movie => ("mp4", format!("{}.mp4", s.name())),
        };
        if let Some(mut p) = rfd::FileDialog::new()
            .set_title(title(f))
            .set_file_name(name)
            .add_filter(ext, &[ext])
            .save_file()
        {
            if p.extension().is_none() {
                p.set_extension(ext);
            }
            self.path = Some(p);
        }
    }

    fn start(&mut self, s: &Session, ctx: &egui::Context, codec: Option<std::path::PathBuf>) {
        let (Some(format), Some(path)) = (self.format, self.path.clone()) else {
            return;
        };
        let (first, last) = if self.all_frames {
            (1, s.length())
        } else {
            (self.first, self.last.max(self.first))
        };
        let job = ExportJob {
            format,
            path: path.clone(),
            first,
            last,
            scale: self.scale,
            codec,
            movie_fps: self.movie_fps,
        };
        let scene = s.doc.clone();
        let cancel = Arc::new(AtomicBool::new(false));
        let (tx, rx) = channel();
        let (flag, ctx) = (cancel.clone(), ctx.clone());
        std::thread::spawn(move || {
            let report = |d, t| {
                let _ = tx.send(Msg::Progress(d, t));
                ctx.request_repaint();
            };
            let r = symphony_render::export(&scene, &job, &flag, &report).map(|_| path);
            let _ = tx.send(Msg::Done(r));
            ctx.request_repaint();
        });
        self.running = Some(Running {
            cancel,
            rx,
            done: 0,
            total: if format == Format::Mp4Movie {
                symphony_render::movie::output_frame_count(
                    last - first + 1,
                    s.doc.settings.fps,
                    self.movie_fps.unwrap_or(s.doc.settings.fps),
                )
            } else {
                last - first + 1
            },
        });
        self.result = None;
    }

    /// `codec` is Cisco's OpenH264 when it's downloaded and turned on.
    /// `downloading` is true while it's being fetched.
    pub fn ui(
        &mut self,
        ctx: &egui::Context,
        s: &Session,
        codec: Option<std::path::PathBuf>,
        downloading: bool,
    ) -> Request {
        let mut request = Request::default();
        if let Some(r) = self.running.as_mut() {
            while let Ok(m) = r.rx.try_recv() {
                match m {
                    Msg::Progress(d, t) => (r.done, r.total) = (d, t),
                    Msg::Done(res) => {
                        self.result = Some(res);
                        self.running = None;
                        break;
                    }
                }
            }
        }
        let Some(format) = self.format else {
            return request;
        };
        let mut open = true;
        egui::Window::new(title(format))
            .open(&mut open)
            .resizable(false)
            .collapsible(false)
            .show(ctx, |ui| {
                ui.set_width(380.0);
                let busy = self.running.is_some();
                ui.add_enabled_ui(!busy, |ui| {
                    ui.label("Output");
                    ui.horizontal(|ui| {
                        let shown = self
                            .path
                            .as_ref()
                            .map_or("Not chosen".to_owned(), |p| p.display().to_string());
                        ui.label(
                            egui::RichText::new(shown)
                                .font(theme::dense())
                                .color(theme::TEXT_SECONDARY),
                        );
                        if ui.button("Browse...").clicked() {
                            self.browse(s);
                        }
                    });
                    if format == Format::PngSequence {
                        ui.label(
                            egui::RichText::new(
                                "Frame numbers are added: name_0001.png, name_0002.png...",
                            )
                            .font(theme::dense())
                            .color(theme::TEXT_SECONDARY),
                        );
                    }
                    ui.add_space(6.0);
                    ui.label("Export range");
                    widgets::radio_value(
                        ui,
                        &mut self.all_frames,
                        true,
                        &format!("All (1 to {})", s.length()),
                    );
                    ui.horizontal(|ui| {
                        widgets::radio_value(ui, &mut self.all_frames, false, "Frames");
                        let len = s.length();
                        ui.add(egui::DragValue::new(&mut self.first).range(1..=len));
                        ui.label("to");
                        ui.add(egui::DragValue::new(&mut self.last).range(1..=len));
                    });
                    ui.add_space(6.0);
                    if format == Format::Mp4Movie {
                        ui.horizontal(|ui| {
                            ui.label("Movie frame rate");
                            egui::ComboBox::from_id_salt("movie-rate").selected_text(
                                self.movie_fps.map_or_else(|| format!("Scene rate ({} fps)", s.doc.settings.fps),
                                    |rate| format!("{rate} fps"))
                            ).show_ui(ui, |ui| {
                                ui.selectable_value(&mut self.movie_fps, None, "Scene rate");
                                for rate in widgets::FRAME_RATES {
                                    ui.selectable_value(&mut self.movie_fps, Some(rate), format!("{rate:.0} fps"));
                                }
                            });
                        });
                    }
                    ui.label("Resolution");
                    let (w, h) = (s.doc.settings.width, s.doc.settings.height);
                    egui::ComboBox::from_id_salt("export-scale")
                        .selected_text(scale_label(self.scale, w, h))
                        .show_ui(ui, |ui| {
                            for k in [1.0, 0.5, 0.25] {
                                ui.selectable_value(&mut self.scale, k, scale_label(k, w, h));
                            }
                        });
                });
                ui.add_space(10.0);
                if let Some(r) = &self.running {
                    let frac = if r.total > 0 {
                        r.done as f32 / r.total as f32
                    } else {
                        0.0
                    };
                    ui.add(
                        egui::ProgressBar::new(frac)
                            .text(format!("Rendering frame {} of {}", r.done, r.total)),
                    );
                    if ui.button("Cancel").clicked() {
                        r.cancel.store(true, Ordering::SeqCst);
                    }
                } else {
                    match &self.result {
                        Some(Ok(p)) => {
                            ui.label(format!("Exported to {}", p.display()));
                        }
                        Some(Err(e)) => {
                            ui.label(e.as_str());
                        }
                        None => {}
                    }
                    let needs_codec = format == Format::Mp4Movie && codec.is_none();
                    if needs_codec {
                        ui.label(
                            egui::RichText::new(
                                "Movies use Cisco's OpenH264 codec. Symphony downloads it from Cisco when you ask, \
                                 and you can turn it off or remove it in Preferences > Video.",
                            )
                            .font(theme::dense())
                            .color(theme::TEXT_SECONDARY),
                        );
                        ui.label(egui::RichText::new(crate::codec::NOTICE).font(theme::dense()).color(theme::TEXT_SECONDARY));
                        if downloading {
                            ui.horizontal(|ui| {
                                symphony_ui::feedback::spinner(ui, 14.0);
                                ui.label("Downloading OpenH264 from Cisco");
                            });
                        } else if ui.button("Download OpenH264 from Cisco").clicked() {
                            request.download_codec = true;
                        }
                    }
                    let ready = self.path.is_some() && !needs_codec;
                    if symphony_ui::widgets::primary_button(ui, "Export", ready)
                        .on_disabled_hover_text(if needs_codec { "Download the codec first" } else { "Choose where to save first" })
                        .clicked()
                    {
                        self.start(s, ctx, codec.clone());
                    }
                }
            });
        if !open && self.running.is_none() {
            self.format = None;
        }
        request
    }
}

fn scale_label(k: f32, w: u32, h: u32) -> String {
    let (ow, oh) = ((w as f32 * k).round() as u32, (h as f32 * k).round() as u32);
    if k == 1.0 {
        format!("Same as scene ({ow} x {oh})")
    } else {
        format!("{:.0}% ({ow} x {oh})", k * 100.0)
    }
}
