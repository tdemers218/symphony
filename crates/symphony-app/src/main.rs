//! Symphony desktop app. Window anatomy follows docs/DESIGN.md §3: one top
//! bar (the title bar on macOS), the Tools toolbar card, the dock of view
//! cards, and the status bar. Shortcuts come only from the keymap
//! (ENGINEERING E4.3). Saving and autosave run off the main thread (E5.2).

mod brush_import;
mod codec;
mod keymap;
mod presets;
mod session;
mod shot;
#[cfg(test)]
mod ui_tests;
mod update;
mod views;
mod wallpaper;
mod welcome;
mod workspace;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;

use eframe::egui;
use egui_dock::{DockArea, DockState};
use serde::{Deserialize, Serialize};
use symphony_core::doc::Scene;
use symphony_io::Recovery;
use symphony_pen::{DeviceInfo, PenInput, PenSample};
use symphony_ui::feedback;
use symphony_ui::icons::{self, Icon};
use symphony_ui::theme;

use keymap::{Action, Profile};
use session::{Session, Tool};
use views::{camera, export, function, node, panels, timing};
use workspace::{View, Workspace};

/// Autosave interval, seconds (E7.5).
const AUTOSAVE_EVERY: f64 = 120.0;

#[derive(Serialize, Deserialize)]
struct Saved {
    #[serde(default)]
    recent_projects: Vec<PathBuf>,
    #[serde(default)]
    wallpaper: wallpaper::Choice,
    #[serde(default = "yes")]
    interface_animations: bool,
    workspace: Workspace,
    layouts: Vec<(Workspace, DockState<View>)>,
    #[serde(default)]
    profile: Profile,
    /// DESIGN D16.2: on by default.
    #[serde(default = "yes")]
    use_no_ai: bool,
    #[serde(default)]
    pressure_gamma: Option<f32>,
    #[serde(default)]
    brush_size_cursor: bool,
    /// Harmony's Support Overlay and Underlay Arts preference.
    #[serde(default)]
    overlay_underlay: bool,
    #[serde(default)]
    size_follows_zoom: bool,
    /// Use Cisco's OpenH264 for movies (Cisco requires the user can turn it off).
    #[serde(default = "yes")]
    use_openh264: bool,
    #[serde(default)]
    presets: Option<presets::Presets>,
}

fn yes() -> bool {
    true
}

/// Something waiting on "Save changes?".
#[derive(Clone, Copy, PartialEq, Eq)]
enum Pending {
    ImportHarmony,
    Update,
    Quit,
    New,
    Open,
}

enum CodecMsg {
    Progress(u64, Option<u64>),
    Done(Result<PathBuf, String>),
}

/// Downloading Cisco's codec in the background (D17.5).
struct CodecDownload {
    rx: Receiver<CodecMsg>,
    done: u64,
    total: Option<u64>,
}

/// A project being read in the background (DESIGN D17.1).
struct Opening {
    harmony: bool,
    path: PathBuf,
    started: f64,
    rx: Receiver<Result<Scene, String>>,
}

/// Confirmations clear after this many seconds (D17.6).
const NOTICE_SECONDS: f64 = 4.0;

/// Background save results: (version saved, path, result).
type SaveResult = (u64, PathBuf, Result<(), String>);

type RecoveredScene = (String, Option<PathBuf>, Scene);

struct SymphonyApp {
    welcome: Option<welcome::Welcome>,
    recent_projects: Vec<PathBuf>,
    recovery_scan: Option<Receiver<Vec<RecoveredScene>>>,
    wallpaper: wallpaper::Wallpaper,
    interface_animations: bool,
    s: Session,
    /// Brush and Pencil presets, kept in preferences like Harmony's.
    presets: presets::Presets,
    workspace: Workspace,
    layouts: Vec<(Workspace, DockState<View>)>,
    profile: Profile,
    use_no_ai: bool,
    pen: Option<PenInput>,
    camera: camera::CanvasState,
    drawing: camera::CanvasState,
    function: function::FunctionState,
    node_view: node::NodeViewState,
    timing: timing::TimingState,
    export: export::ExportDialog,
    maximised: Option<View>,
    show_preferences: bool,
    show_icon_sheet: bool,
    last_time: Option<f64>,
    frame_clock: f64,
    shot: Option<shot::Shot>,
    recovery: Option<Recovery>,
    recovered: Vec<RecoveredScene>,
    pending: Option<Pending>,
    allow_close: bool,
    error: Option<String>,
    save_tx: Sender<SaveResult>,
    save_rx: Receiver<SaveResult>,
    autosave_at: f64,
    autosaved_version: u64,
    autosaving: Arc<AtomicBool>,
    title: String,
    saving_jobs: u32,
    use_openh264: bool,
    codec_download: Option<CodecDownload>,
    /// Brush files dropped on the window, being read in the background.
    brush_import: Option<brush_import::BrushImport>,
    show_licences: bool,
    opening: Option<Opening>,
    notice_seen: Option<(String, f64)>,
}

fn tool_icon(t: Tool) -> Icon {
    match t {
        Tool::Select => Icon::Select,
        Tool::Transform => Icon::Transform,
        Tool::Brush => Icon::Brush,
        Tool::Pencil => Icon::Pencil,
        Tool::Eraser => Icon::Eraser,
        Tool::Paint => Icon::Paint,
        Tool::CloseGap => Icon::CloseGap,
        Tool::Dropper => Icon::Dropper,
        Tool::Hand => Icon::Hand,
        Tool::Zoom => Icon::Zoom,
    }
}

fn tool_action(t: Tool) -> Action {
    match t {
        Tool::Select => Action::ToolSelect,
        Tool::Transform => Action::ToolTransform,
        Tool::Brush => Action::ToolBrush,
        Tool::Pencil => Action::ToolPencil,
        Tool::Eraser => Action::ToolEraser,
        Tool::Paint => Action::ToolPaint,
        Tool::CloseGap => Action::ToolCloseGap,
        Tool::Dropper => Action::ToolDropper,
        Tool::Hand => Action::ToolHand,
        Tool::Zoom => Action::ToolZoom,
    }
}

fn recovery_dir() -> Option<PathBuf> {
    dirs::data_dir().map(|d| d.join("Symphony").join("recovery"))
}

impl SymphonyApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        theme::apply(&cc.egui_ctx);
        // Dark window frame on every OS (DESIGN D12.3).
        cc.egui_ctx
            .send_viewport_cmd(egui::ViewportCommand::SetTheme(egui::SystemTheme::Dark));
        let pen = match PenInput::attach(cc) {
            Ok(p) => Some(p),
            Err(e) => {
                eprintln!("Pen input: {e}. Falling back to mouse.");
                None
            }
        };
        let saved: Option<Saved> = cc
            .storage
            .and_then(|s| eframe::get_value(s, eframe::APP_KEY));
        let mut wallpaper_choice = saved
            .as_ref()
            .map(|s| s.wallpaper.clone())
            .unwrap_or_default();
        if std::env::var_os("SYMPHONY_SCREENSHOT").is_some() {
            wallpaper_choice = match std::env::var("SYMPHONY_SHOT_WALLPAPER").as_deref() {
                Ok("Plain") => wallpaper::Choice::Plain,
                Ok("Studio") => wallpaper::Choice::Studio,
                Ok("Dusk") => wallpaper::Choice::Dusk,
                Ok("Custom") => std::env::var_os("SYMPHONY_SHOT_WALLPAPER_FILE")
                    .map_or(wallpaper::Choice::Plain, |path| {
                        wallpaper::Choice::Custom(path.into())
                    }),
                _ => wallpaper::Choice::Black,
            };
        }
        let recent_projects = saved
            .as_ref()
            .map(|s| s.recent_projects.clone())
            .unwrap_or_default();
        let interface_animations = saved.as_ref().is_none_or(|s| s.interface_animations);
        theme::set_motion(&cc.egui_ctx, interface_animations);
        let brush_size_cursor = saved.as_ref().is_some_and(|s| s.brush_size_cursor);
        let overlay_underlay = saved.as_ref().is_some_and(|s| s.overlay_underlay);
        let size_follows_zoom = saved.as_ref().is_some_and(|s| s.size_follows_zoom);
        let use_openh264 = saved.as_ref().is_none_or(|s| s.use_openh264);
        let presets = saved
            .as_ref()
            .and_then(|s| s.presets.clone())
            .unwrap_or_default();
        let (mut workspace, mut layouts, profile, use_no_ai, gamma) = match saved {
            Some(s) => (
                s.workspace,
                s.layouts,
                s.profile,
                s.use_no_ai,
                s.pressure_gamma,
            ),
            None => (Workspace::Default, Vec::new(), Profile::Harmony, true, None),
        };
        for ws in Workspace::ALL {
            if !layouts.iter().any(|(w, _)| *w == ws) {
                layouts.push((ws, ws.preset()));
            }
        }
        let shot = shot::Shot::from_env();
        let mut s = if shot.is_some() || std::env::var_os("SYMPHONY_DEMO_ART").is_some() {
            Session::demo()
        } else {
            Session::new_scene()
        };
        if let Some(g) = gamma {
            s.pressure_gamma = g;
        }
        s.brush_size_cursor = brush_size_cursor;
        s.overlay_underlay = overlay_underlay;
        s.size_follows_zoom = size_follows_zoom;
        // Screenshots use fresh presets, so design reviews are repeatable.
        if shot.is_some() {
            layouts = Workspace::ALL.iter().map(|w| (*w, w.preset())).collect();
            if let Ok(name) = std::env::var("SYMPHONY_WORKSPACE") {
                if let Some(w) = Workspace::ALL
                    .into_iter()
                    .find(|w| format!("{w:?}") == name)
                {
                    workspace = w;
                }
            }
            // SYMPHONY_SHOT_PAINT: the Paint tool with a protected colour,
            // to review the paint options and the protection marker.
            // SYMPHONY_SHOT_VIEW=Function: bring that tab to the front.
            if std::env::var("SYMPHONY_SHOT_VIEW").as_deref() == Ok("Function") {
                for (_, dock) in layouts.iter_mut() {
                    if let Some(found) = dock.find_tab(&View::Function) {
                        let _ = dock.set_active_tab(found);
                    }
                }
            }
            // SYMPHONY_SHOT_TOOL=Brush|Pencil: review that tool's properties.
            match std::env::var("SYMPHONY_SHOT_TOOL").as_deref() {
                Ok("Brush") => s.tool = Tool::Brush,
                Ok("Pencil") => s.tool = Tool::Pencil,
                Ok("Eraser") => s.tool = Tool::Eraser,
                _ => {}
            }
            if std::env::var_os("SYMPHONY_SHOT_PAINT").is_some() {
                s.tool = Tool::Paint;
                if let Some(id) = s.doc.palette.swatches.get(1).map(|sw| sw.id) {
                    s.toggle_protect_swatch(id);
                }
            }
        }
        let recovery = recovery_dir().map(Recovery::new);
        let welcome = (shot.is_none() || std::env::var_os("SYMPHONY_SHOT_WELCOME").is_some())
            .then(|| welcome::Welcome::new(false));
        let recovery_scan = recovery_dir().filter(|_| shot.is_none()).map(|dir| {
            let (tx, rx) = channel();
            let ctx = cc.egui_ctx.clone();
            std::thread::spawn(move || {
                let _ = tx.send(Recovery::new(dir).pending());
                ctx.request_repaint();
            });
            rx
        });
        let (save_tx, save_rx) = channel();
        let version = s.doc.version;
        SymphonyApp {
            welcome,
            recent_projects,
            recovery_scan,
            wallpaper: wallpaper::Wallpaper::new(&cc.egui_ctx, wallpaper_choice),
            interface_animations,
            s,
            workspace,
            layouts,
            profile,
            use_no_ai,
            pen,
            camera: Default::default(),
            drawing: Default::default(),
            function: Default::default(),
            node_view: Default::default(),
            timing: Default::default(),
            export: Default::default(),
            maximised: None,
            show_preferences: std::env::var_os("SYMPHONY_SHOT_PREFS").is_some(),
            show_icon_sheet: std::env::var_os("SYMPHONY_SHOT_ICONS").is_some(),
            last_time: None,
            frame_clock: 0.0,
            shot,
            recovery,
            recovered: Vec::new(),
            pending: None,
            allow_close: false,
            error: None,
            save_tx,
            save_rx,
            autosave_at: 0.0,
            autosaved_version: version,
            autosaving: Arc::new(AtomicBool::new(false)),
            title: String::new(),
            saving_jobs: 0,
            use_openh264,
            presets,
            codec_download: None,
            brush_import: None,
            show_licences: false,
            opening: None,
            notice_seen: None,
        }
    }

    fn layout_mut(&mut self) -> &mut DockState<View> {
        let ws = self.workspace;
        &mut self
            .layouts
            .iter_mut()
            .find(|(w, _)| *w == ws)
            .expect("every workspace has a layout")
            .1
    }

    fn show_view(&mut self, view: View) {
        let layout = self.layout_mut();
        match layout.find_tab(&view) {
            Some(path) => {
                let _ = layout.set_active_tab(path);
            }
            None => layout.push_to_focused_leaf(view),
        }
    }

    // ---- Files -------------------------------------------------------------

    /// Run `p` now, or ask to save unsaved work first. Losing work can't be
    /// undone, so this is one of the few confirmations (D8.4).
    fn guard(&mut self, ctx: &egui::Context, p: Pending) {
        if self.s.dirty() {
            self.pending = Some(p);
        } else {
            self.proceed(ctx, p);
        }
    }

    fn proceed(&mut self, ctx: &egui::Context, p: Pending) {
        match p {
            Pending::Update => match update::start() {
                Ok(_) => self.proceed(ctx, Pending::Quit),
                Err(e) => self.error = Some(format!("Couldn’t start the updater ({e}).")),
            },
            Pending::Quit => {
                self.allow_close = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            Pending::New => self.welcome = Some(welcome::Welcome::new(true)),
            Pending::Open => self.open_dialog(false, ctx),
            Pending::ImportHarmony => self.open_dialog(true, ctx),
        }
    }

    fn replace_session(&mut self, next: Session) {
        if let Some(r) = &self.recovery {
            r.clear(&self.s.key);
        }
        let (gamma, cursor) = (self.s.pressure_gamma, self.s.brush_size_cursor);
        let layers = self.s.overlay_underlay;
        let follows = self.s.size_follows_zoom;
        let (brush, pencil, eraser) = (self.s.brush, self.s.pencil, self.s.eraser);
        let eraser_tip = self.s.eraser_tip;
        self.s = next;
        self.s.pressure_gamma = gamma;
        self.s.brush_size_cursor = cursor;
        self.s.overlay_underlay = layers;
        self.s.size_follows_zoom = follows;
        // Tool settings belong to the artist, not the scene.
        self.s.brush = brush;
        self.s.pencil = pencil;
        self.s.eraser = eraser;
        self.s.eraser_tip = eraser_tip;
        self.autosaved_version = self.s.doc.version;
        self.camera.reset_view();
        self.drawing.reset_view();
    }

    fn open_dialog(&mut self, harmony: bool, ctx: &egui::Context) {
        let dialog = rfd::FileDialog::new();
        let path = if harmony {
            dialog
                .set_title("Import Harmony scene structure")
                .add_filter("Harmony scene", &["xstage"])
                .pick_file()
        } else {
            dialog
                .set_title("Open a Symphony project (.symphony folder)")
                .pick_folder()
        };
        let Some(path) = path else { return };
        self.open_path(path, harmony, ctx);
    }

    fn open_path(&mut self, path: PathBuf, harmony: bool, ctx: &egui::Context) {
        // Read in the background; a loading dialog shows meanwhile (D17.1).
        let (tx, rx) = channel();
        let p = path.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = if harmony {
                symphony_io::harmony::import(&p)
            } else {
                symphony_io::open(&p).map_err(|e| e.0)
            };
            let _ = tx.send(result);
            ctx.request_repaint();
        });
        self.opening = Some(Opening {
            harmony,
            path,
            started: self.last_time.unwrap_or(0.0),
            rx,
        });
    }

    fn remember_project(&mut self, path: &std::path::Path) {
        self.recent_projects.retain(|project| project != path);
        self.recent_projects.insert(0, path.to_owned());
        self.recent_projects.truncate(5);
    }

    fn poll_startup(&mut self) {
        let Some(rx) = &self.recovery_scan else {
            return;
        };
        match rx.try_recv() {
            Ok(scenes) => {
                self.recovered = scenes;
                self.recovery_scan = None;
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => self.recovery_scan = None,
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
        }
    }

    fn poll_open(&mut self) {
        let Some(o) = &self.opening else { return };
        match o.rx.try_recv() {
            Ok(Ok(doc)) => {
                let harmony = o.harmony;
                let path = (!harmony).then(|| o.path.clone());
                self.opening = None;
                if let Some(path) = &path {
                    self.remember_project(path);
                }
                self.welcome = None;
                self.replace_session(Session::from_doc(doc, path));
                if harmony {
                    self.s.doc.version += 1;
                    self.show_view(View::NodeLibrary);
                    self.s.notice =
                        Some("Imported scene structure. See Node Library for coverage.".into());
                    return;
                }
                self.s.notice = Some(format!("Opened {}", self.s.name()));
            }
            Ok(Err(e)) => {
                self.opening = None;
                self.error = Some(e);
            }
            Err(_) => {}
        }
    }

    fn ask_save_path(&self) -> Option<PathBuf> {
        rfd::FileDialog::new()
            .set_title("Save scene")
            .set_file_name(format!("{}.symphony", self.s.name()))
            .save_file()
            .map(|p| symphony_io::with_extension(&p))
    }

    fn save(&mut self, save_as: bool) {
        let path = match (&self.s.path, save_as) {
            (Some(p), false) => p.clone(),
            _ => match self.ask_save_path() {
                Some(p) => p,
                None => return,
            },
        };
        // Snapshot, then write in the background (E5.3, E6).
        let (doc, version, tx) = (self.s.doc.clone(), self.s.doc.version, self.save_tx.clone());
        std::thread::spawn(move || {
            let r = symphony_io::save(&doc, &path).map_err(|e| e.0);
            let _ = tx.send((version, path, r));
        });
        self.saving_jobs += 1;
    }

    /// Save synchronously. Only used right before quitting or replacing the scene.
    fn save_now(&mut self) -> bool {
        let Some(path) = self.s.path.clone().or_else(|| self.ask_save_path()) else {
            return false;
        };
        match symphony_io::save(&self.s.doc, &path) {
            Ok(()) => {
                self.s.saved_version = self.s.doc.version;
                self.remember_project(&path);
                self.s.path = Some(path);
                if let Some(r) = &self.recovery {
                    r.clear(&self.s.key);
                }
                true
            }
            Err(e) => {
                self.error = Some(e.0);
                false
            }
        }
    }

    fn poll_saves(&mut self) {
        while let Ok((version, path, result)) = self.save_rx.try_recv() {
            self.saving_jobs = self.saving_jobs.saturating_sub(1);
            match result {
                Ok(()) => {
                    self.s.saved_version = version;
                    self.remember_project(&path);
                    self.s.path = Some(path);
                    self.s.notice = Some(format!("Saved {}", self.s.name()));
                    if !self.s.dirty() {
                        if let Some(r) = &self.recovery {
                            r.clear(&self.s.key);
                        }
                    }
                }
                Err(e) => self.error = Some(e),
            }
        }
    }

    /// The codec to hand to movie export: downloaded and turned on.
    fn codec(&self) -> Option<PathBuf> {
        self.use_openh264.then(codec::installed).flatten()
    }

    /// Fetch Cisco's OpenH264 in the background, at the user's request.
    fn start_codec_download(&mut self, ctx: &egui::Context) {
        if self.codec_download.is_some() {
            return;
        }
        let (tx, rx) = channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let t2 = tx.clone();
            let c2 = ctx.clone();
            let r = codec::download(&move |d, t| {
                let _ = t2.send(CodecMsg::Progress(d, t));
                c2.request_repaint();
            });
            let _ = tx.send(CodecMsg::Done(r));
            ctx.request_repaint();
        });
        self.use_openh264 = true;
        self.codec_download = Some(CodecDownload {
            rx,
            done: 0,
            total: None,
        });
    }

    fn poll_codec(&mut self) {
        let Some(d) = self.codec_download.as_mut() else {
            return;
        };
        while let Ok(m) = d.rx.try_recv() {
            match m {
                CodecMsg::Progress(done, total) => (d.done, d.total) = (done, total),
                CodecMsg::Done(r) => {
                    match r {
                        Ok(_) => self.s.notice = Some("OpenH264 is ready for movie export".into()),
                        Err(e) => self.error = Some(e),
                    }
                    self.codec_download = None;
                    return;
                }
            }
        }
    }

    /// Preferences > Video: Cisco's codec, with the notice and controls its
    /// licence requires (docs/CODECS.md).
    fn video_preferences(&mut self, ui: &mut egui::Ui) {
        ui.separator();
        ui.heading("Video");
        ui.label(egui::RichText::new(codec::NOTICE).color(theme::TEXT_PRIMARY));
        symphony_ui::widgets::checkbox(ui, &mut self.use_openh264, "Use OpenH264 for movie export");
        let dense = |ui: &mut egui::Ui, t: String| {
            ui.label(
                egui::RichText::new(t)
                    .font(theme::dense())
                    .color(theme::TEXT_SECONDARY),
            );
        };
        if let Some(d) = &self.codec_download {
            ui.horizontal(|ui| {
                feedback::spinner(ui, 14.0);
                ui.label("Downloading from Cisco");
            });
            feedback::progress_bar(
                ui,
                d.total.map(|t| d.done as f32 / t.max(1) as f32),
                ui.available_width(),
            );
        } else if let Some(p) = codec::installed() {
            dense(
                ui,
                format!(
                    "Version {} is installed at {}.",
                    codec::VERSION,
                    p.display()
                ),
            );
            if ui.button("Remove OpenH264").clicked() {
                codec::remove();
            }
        } else if codec::file_name().is_some() {
            dense(
                ui,
                "Not downloaded. Movie export downloads it from Cisco when you ask.".into(),
            );
            if ui.button("Download OpenH264 from Cisco").clicked() {
                let ctx = ui.ctx().clone();
                self.start_codec_download(&ctx);
            }
        } else {
            dense(
                ui,
                "Cisco doesn't publish OpenH264 for this kind of computer.".into(),
            );
        }
        dense(ui, "Cisco's licence covers personal and other unpaid use. Commercial work may need its own H.264 licence.".into());
        if ui.link("Show licence").clicked() {
            self.show_licences = true;
        }
    }

    /// Confirmations show briefly, then clear (D17.6).
    fn expire_notice(&mut self, now: f64) {
        match (&self.s.notice, &self.notice_seen) {
            (Some(n), Some((seen, at))) if n == seen => {
                if now - at > NOTICE_SECONDS {
                    self.s.notice = None;
                    self.notice_seen = None;
                }
            }
            (Some(n), _) => self.notice_seen = Some((n.clone(), now)),
            (None, _) => self.notice_seen = None,
        }
    }

    fn autosave(&mut self, now: f64) {
        if now - self.autosave_at < AUTOSAVE_EVERY {
            return;
        }
        self.autosave_at = now;
        let v = self.s.doc.version;
        if !self.s.dirty() || v == self.autosaved_version || self.autosaving.load(Ordering::SeqCst)
        {
            return;
        }
        let Some(dir) = recovery_dir() else { return };
        self.autosaved_version = v;
        self.autosaving.store(true, Ordering::SeqCst);
        let (doc, key, origin, flag) = (
            self.s.doc.clone(),
            self.s.key.clone(),
            self.s.path.clone(),
            self.autosaving.clone(),
        );
        std::thread::spawn(move || {
            if let Err(e) = Recovery::new(dir).write(&key, &doc, origin.as_deref()) {
                eprintln!("Autosave failed: {e}");
            }
            flag.store(false, Ordering::SeqCst);
        });
    }

    // ---- Actions -----------------------------------------------------------

    fn tick_playback(&mut self, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        let dt = self.last_time.map_or(0.0, |t| now - t);
        self.last_time = Some(now);
        if !self.s.playing {
            self.frame_clock = 0.0;
            return;
        }
        self.frame_clock += dt;
        let step = 1.0 / self.s.doc.settings.fps.max(1.0) as f64;
        while self.frame_clock >= step {
            self.frame_clock -= step;
            self.s.frame = if self.s.frame >= self.s.length() {
                1
            } else {
                self.s.frame + 1
            };
        }
        ctx.request_repaint();
    }

    fn run(&mut self, ctx: &egui::Context, action: Action) {
        match action {
            Action::NewScene => self.guard(ctx, Pending::New),
            Action::Open => self.guard(ctx, Pending::Open),
            Action::Save => self.save(false),
            Action::SaveAs => self.save(true),
            Action::ZoomIn
            | Action::ZoomOut
            | Action::RotateCw
            | Action::RotateCcw
            | Action::ResetZoom
            | Action::ResetRotation
            | Action::ResetPan
            | Action::ResetView => {
                self.camera.apply(action);
                self.drawing.apply(action);
            }
            Action::MaximiseView => {
                self.maximised = match self.maximised {
                    Some(_) => None,
                    None => self.layout_mut().find_active_focused().map(|(_, v)| *v),
                };
            }
            other => {
                let s = &mut self.s;
                match other {
                    Action::Undo => s.undo(),
                    Action::SelectAll => {
                        s.tool = Tool::Select;
                        s.select_all();
                    }
                    Action::DeselectAll => s.deselect(),
                    Action::Delete => s.delete_selection(),
                    Action::Cut => s.cut_selection(),
                    Action::Copy => s.copy_selection(),
                    Action::Paste => {
                        s.tool = Tool::Select;
                        s.paste();
                    }
                    Action::FlipHorizontal | Action::FlipVertical => {
                        let h = other == Action::FlipHorizontal;
                        if s.tool == Tool::Transform {
                            let layer = s.selected;
                            s.adjust_peg(layer, |v| {
                                if h {
                                    v.scale_x = -v.scale_x
                                } else {
                                    v.scale_y = -v.scale_y
                                }
                            });
                            s.end_gesture();
                        } else {
                            s.flip_selection(h);
                        }
                    }
                    Action::Redo => s.redo(),
                    Action::PrevFrame => s.frame = s.frame.saturating_sub(1).max(1),
                    Action::NextFrame => s.frame = (s.frame + 1).min(s.length()),
                    Action::FirstFrame => s.frame = 1,
                    Action::LastFrame => s.frame = s.length(),
                    Action::PlayPause => s.playing = !s.playing,
                    Action::IncreaseExposure => s.change_exposure(1),
                    Action::DecreaseExposure => s.change_exposure(-1),
                    Action::CreateEmptyDrawing => s.new_drawing(),
                    Action::AddLayer => s.add_layer(),
                    Action::ToggleOnionSkin => s.onion.on = !s.onion.on,
                    Action::ShowStrokes => s.show_strokes = !s.show_strokes,
                    Action::RespectProtectedColours => s.toggle_respect_protected(),
                    Action::SwitchLineColourArt => s.switch_line_colour_art(),
                    Action::SwitchOverlayUnderlay => s.switch_overlay_underlay(),
                    Action::PreviewArtLayers => s.art_preview = !s.art_preview,
                    Action::SubstituteNext => s.substitute(1),
                    Action::SubstitutePrevious => s.substitute(-1),
                    Action::GoToNextDrawing => s.go_to_drawing(true),
                    Action::GoToPreviousDrawing => s.go_to_drawing(false),
                    Action::CreateColourArt => {
                        let (layer, frame) = (s.selected, s.frame);
                        s.create_colour_art(layer, frame);
                    }
                    Action::ToolPaint
                    | Action::ToolPaintUnpainted
                    | Action::ToolRepaint
                    | Action::ToolUnpaint => {
                        use session::PaintMode;
                        s.tool = Tool::Paint;
                        s.paint_mode = match other {
                            Action::ToolPaintUnpainted => PaintMode::PaintUnpainted,
                            Action::ToolRepaint => PaintMode::Repaint,
                            Action::ToolUnpaint => PaintMode::Unpaint,
                            _ => PaintMode::Paint,
                        };
                    }
                    Action::OnionMorePrevious => s.onion_step(true, 1),
                    Action::OnionLessPrevious => s.onion_step(true, -1),
                    Action::OnionMoreNext => s.onion_step(false, 1),
                    Action::OnionLessNext => s.onion_step(false, -1),
                    Action::OnionPrevious1 => s.onion_set(true, 1),
                    Action::OnionPrevious2 => s.onion_set(true, 2),
                    Action::OnionPrevious3 => s.onion_set(true, 3),
                    Action::OnionNoPrevious => s.onion_set(true, 0),
                    Action::OnionNext1 => s.onion_set(false, 1),
                    Action::OnionNext2 => s.onion_set(false, 2),
                    Action::OnionNext3 => s.onion_set(false, 3),
                    Action::OnionNoNext => s.onion_set(false, 0),
                    _ => {
                        if let Some(t) = Tool::ALL.into_iter().find(|t| tool_action(*t) == other) {
                            if t.planned_for().is_none() {
                                s.tool = t;
                            }
                        }
                    }
                }
            }
        }
    }

    fn shortcuts(&mut self, ctx: &egui::Context) {
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        for a in Action::ALL {
            if keymap::pressed(ctx, self.profile, a) {
                self.run(ctx, a);
            }
        }
    }

    // ---- Chrome ------------------------------------------------------------

    /// Menu item that shows its shortcut (D8.7).
    fn menu_action(&mut self, ui: &mut egui::Ui, action: Action) {
        self.menu_item(ui, action, action.name().to_owned());
    }

    fn menu_item(&mut self, ui: &mut egui::Ui, action: Action, label: String) {
        let btn = egui::Button::new(label).shortcut_text(keymap::label(self.profile, action));
        if ui.add(btn).clicked() {
            let ctx = ui.ctx().clone();
            ui.close();
            self.run(&ctx, action);
        }
    }

    /// A menu item for an on/off setting, ticked while it's on.
    fn menu_toggle(&mut self, ui: &mut egui::Ui, action: Action, on: bool) {
        let btn = egui::Button::new(action.name())
            .selected(on)
            .shortcut_text(keymap::label(self.profile, action));
        if ui.add(btn).clicked() {
            let ctx = ui.ctx().clone();
            ui.close();
            self.run(&ctx, action);
        }
    }

    fn menus(&mut self, ui: &mut egui::Ui) {
        ui.menu_button("File", |ui| {
            self.menu_item(ui, Action::NewScene, "New scene".into());
            self.menu_item(ui, Action::Open, "Open...".into());
            if ui
                .button("Import Harmony scene structure…")
                .on_hover_text(
                    "Import scene settings, drawing layers and exposures from an .xstage file",
                )
                .clicked()
            {
                ui.close();
                self.guard(ui.ctx(), Pending::ImportHarmony);
            }
            ui.separator();
            self.menu_action(ui, Action::Save);
            self.menu_item(ui, Action::SaveAs, "Save as...".into());
            ui.separator();
            ui.menu_button("Export", |ui| {
                if ui.button("Animated GIF...").clicked() {
                    self.export
                        .open(symphony_render::Format::AnimatedGif, &self.s);
                    ui.close();
                }
                if ui.button("Image Sequence (PNG)...").clicked() {
                    self.export
                        .open(symphony_render::Format::PngSequence, &self.s);
                    ui.close();
                }
                if ui.button("Movie (H.264 MP4)...").clicked() {
                    self.export.open(symphony_render::Format::Mp4Movie, &self.s);
                    ui.close();
                }
            });
            ui.separator();
            if ui.button("Quit Symphony").clicked() {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            }
        });
        ui.menu_button("Edit", |ui| {
            let undo = self
                .s
                .history
                .undo_name()
                .map_or("Undo".into(), |n| format!("Undo {n}"));
            let redo = self
                .s
                .history
                .redo_name()
                .map_or("Redo".into(), |n| format!("Redo {n}"));
            self.menu_item(ui, Action::Undo, undo);
            self.menu_item(ui, Action::Redo, redo);
            ui.separator();
            self.menu_action(ui, Action::Cut);
            self.menu_action(ui, Action::Copy);
            self.menu_action(ui, Action::Paste);
            self.menu_action(ui, Action::Delete);
            ui.separator();
            self.menu_action(ui, Action::SelectAll);
            self.menu_action(ui, Action::DeselectAll);
            ui.separator();
            self.menu_action(ui, Action::FlipHorizontal);
            self.menu_action(ui, Action::FlipVertical);
            ui.separator();
            if ui.button("Preferences").clicked() {
                self.show_preferences = true;
                ui.close();
            }
        });
        ui.menu_button("Drawing", |ui| {
            self.menu_action(ui, Action::CreateColourArt);
            ui.separator();
            self.menu_action(ui, Action::SwitchLineColourArt);
            if self.s.overlay_underlay {
                self.menu_action(ui, Action::SwitchOverlayUnderlay);
            }
            let on = self.s.art_preview;
            self.menu_toggle(ui, Action::PreviewArtLayers, on);
            ui.separator();
            ui.menu_button("Colour Protection", |ui| {
                let on = self.s.respect_protected;
                self.menu_toggle(ui, Action::RespectProtectedColours, on);
            });
        });
        ui.menu_button("Timing", |ui| {
            self.menu_action(ui, Action::IncreaseExposure);
            self.menu_action(ui, Action::DecreaseExposure);
            ui.separator();
            self.menu_action(ui, Action::SubstitutePrevious);
            self.menu_action(ui, Action::SubstituteNext);
            self.menu_action(ui, Action::GoToPreviousDrawing);
            self.menu_action(ui, Action::GoToNextDrawing);
            ui.separator();
            self.menu_action(ui, Action::CreateEmptyDrawing);
            self.menu_action(ui, Action::AddLayer);
        });
        ui.menu_button("Play", |ui| {
            self.menu_action(ui, Action::PlayPause);
            self.menu_action(ui, Action::PrevFrame);
            self.menu_action(ui, Action::NextFrame);
            self.menu_action(ui, Action::FirstFrame);
            self.menu_action(ui, Action::LastFrame);
        });
        ui.menu_button("View", |ui| {
            self.menu_action(ui, Action::ToggleOnionSkin);
            let on = self.s.show_strokes;
            self.menu_toggle(ui, Action::ShowStrokes, on);
            ui.separator();
            self.menu_action(ui, Action::ZoomIn);
            self.menu_action(ui, Action::ZoomOut);
            self.menu_action(ui, Action::RotateCw);
            self.menu_action(ui, Action::RotateCcw);
            ui.separator();
            self.menu_action(ui, Action::ResetZoom);
            self.menu_action(ui, Action::ResetRotation);
            self.menu_action(ui, Action::ResetPan);
            self.menu_action(ui, Action::ResetView);
        });
        ui.menu_button("Windows", |ui| {
            for v in View::ALL {
                if ui.button(v.title()).clicked() {
                    self.show_view(v);
                    ui.close();
                }
            }
            ui.separator();
            self.menu_action(ui, Action::MaximiseView);
        });
        ui.menu_button("Help", |ui| {
            if ui.button("Licences").clicked() {
                self.show_licences = true;
                ui.close();
            }
            if ui.button("Icon sheet").clicked() {
                self.show_icon_sheet = true;
                ui.close();
            }
        });
        ui.separator();
        if ui
            .button("Update")
            .on_hover_text("Update Symphony")
            .clicked()
        {
            self.guard(ui.ctx(), Pending::Update);
        }
    }

    /// D3.1: menus left, scene name centred, workspace right. On macOS this
    /// bar is the title bar, so its empty space drags the window.
    fn top_bar(&mut self, ui: &mut egui::Ui) {
        let rect = ui.max_rect();
        let drag = ui.interact(
            rect,
            egui::Id::unique("symphony-top-bar-drag"),
            egui::Sense::click_and_drag(),
        );
        if drag.drag_started() {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
        }
        if drag.double_clicked() {
            let max = ui.ctx().input(|i| i.viewport().maximized.unwrap_or(false));
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::Maximized(!max));
        }
        let mut title = self.s.name();
        if self.s.dirty() {
            title.push_str("  (edited)");
        }
        ui.horizontal_centered(|ui| {
            // Room for the macOS window buttons (D12.2).
            if cfg!(target_os = "macos") {
                ui.add_space(72.0);
            }
            egui::MenuBar::new().ui(ui, |ui| {
                self.menus(ui);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let mut ws = self.workspace;
                    egui::ComboBox::from_id_salt("workspace")
                        .width(170.0)
                        .selected_text(ws.title())
                        .show_ui(ui, |ui| {
                            for w in Workspace::ALL {
                                ui.selectable_value(&mut ws, w, w.title());
                            }
                            ui.separator();
                            if ui.button("Reset this workspace").clicked() {
                                *self.layout_mut() = self.workspace.preset();
                                self.maximised = None;
                            }
                        });
                    if ws != self.workspace {
                        self.workspace = ws;
                        self.maximised = None;
                    }
                    ui.colored_label(theme::TEXT_SECONDARY, "Workspace");
                    let space = ui
                        .available_rect_before_wrap()
                        .shrink2(egui::vec2(6.0, 0.0));
                    if space.is_positive() {
                        let galley =
                            ui.painter()
                                .layout_no_wrap(title, theme::body(), theme::TEXT_PRIMARY);
                        let width = galley.size().x;
                        let x = (rect.center().x - width * 0.5)
                            .clamp(space.left(), (space.right() - width).max(space.left()));
                        let y = rect.center().y - galley.size().y * 0.5;
                        ui.painter().with_clip_rect(space).galley(
                            egui::pos2(x, y),
                            galley,
                            theme::TEXT_PRIMARY,
                        );
                    }
                });
            });
        });
    }

    /// The Tools toolbar card, in groups (D6.2).
    fn tools_strip(&mut self, ui: &mut egui::Ui) {
        const GROUPS: [&[Tool]; 4] = [
            &[Tool::Select, Tool::Transform],
            &[Tool::Brush, Tool::Pencil, Tool::Eraser],
            &[Tool::Paint, Tool::CloseGap, Tool::Dropper],
            &[Tool::Hand, Tool::Zoom],
        ];
        let frame = egui::Frame::new()
            .fill(theme::GLASS_PANEL)
            .corner_radius(theme::GLASS_RADIUS)
            .inner_margin(egui::Margin::same(theme::GLASS_TOOL_INSET))
            .show(ui, |ui| {
                ui.set_min_height(ui.available_height());
                ui.spacing_mut().item_spacing.y = 2.0;
                for (g, group) in GROUPS.iter().enumerate() {
                    if g > 0 {
                        ui.add_space(theme::TOOL_GROUP_GAP - 2.0);
                    }
                    for &t in group.iter() {
                        let mut tip = keymap::tooltip(self.profile, tool_action(t));
                        if let Some(m) = t.planned_for() {
                            tip.push_str(&format!("\nComing in {m}"));
                        }
                        let enabled = t.planned_for().is_none();
                        if icons::tool_button(ui, tool_icon(t), self.s.tool == t, enabled, &tip)
                            .clicked()
                        {
                            self.s.tool = t;
                        }
                    }
                }
            });
        theme::glass_rim(ui.painter(), frame.response.rect);
    }

    fn status_bar(&mut self, ui: &mut egui::Ui, device: Option<&DeviceInfo>) {
        let dense = |ui: &mut egui::Ui, text: String, colour: egui::Color32| {
            ui.label(egui::RichText::new(text).font(theme::dense()).color(colour));
        };
        ui.horizontal_centered(|ui| {
            ui.spacing_mut().item_spacing.x = 16.0;
            let s = &self.s;
            dense(
                ui,
                format!("Frame {} of {}", s.frame, s.length()),
                theme::TEXT_SECONDARY,
            );
            let drawing = s
                .drawing_name(s.selected, s.frame)
                .map_or("empty cell".into(), |d| format!("drawing {d}"));
            dense(
                ui,
                format!("{}, {drawing}", s.layer_name(s.selected)),
                theme::TEXT_SECONDARY,
            );
            dense(
                ui,
                format!(
                    "Tool {}, {}",
                    tool_action(s.tool).name(),
                    session::art_name(s.art)
                ),
                theme::TEXT_SECONDARY,
            );
            if s.animate {
                // D8.5: a visible mode, in its token colour (amber = time).
                dense(ui, "Animate mode on".into(), theme::ACCENT_TIME);
            }
            if s.onion.on {
                dense(ui, "Onion skin on".into(), theme::TEXT_PRIMARY);
            }
            let notice = s.notice.clone();
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // Which build this is, so testers know what they're running.
                dense(
                    ui,
                    format!(
                        "v{} ({})",
                        env!("CARGO_PKG_VERSION"),
                        env!("SYMPHONY_BUILD")
                    ),
                    theme::TEXT_DISABLED,
                );
                dense(
                    ui,
                    format!("{} profile", self.profile.title()),
                    theme::TEXT_SECONDARY,
                );
                let (pen, dot) = match device {
                    Some(d) if d.in_proximity => ("Pen in range", theme::STATE_OK),
                    Some(_) => ("Pen ready", theme::TEXT_SECONDARY),
                    None => ("Mouse only", theme::TEXT_DISABLED),
                };
                dense(ui, pen.into(), theme::TEXT_SECONDARY);
                let (r, _) = ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
                ui.painter().circle_filled(r.center(), 3.5, dot);

                // Activity area (D17.5): background work stays visible.
                let mut jobs: Vec<(String, Option<f32>)> = Vec::new();
                if let Some((done, total)) = self.export.progress() {
                    jobs.push((
                        format!("Exporting frame {done} of {total}"),
                        Some(done as f32 / total.max(1) as f32),
                    ));
                }
                if let Some(d) = &self.codec_download {
                    jobs.push((
                        "Downloading OpenH264".into(),
                        d.total.map(|t| d.done as f32 / t.max(1) as f32),
                    ));
                }
                if let Some(b) = &self.brush_import {
                    jobs.push((
                        if b.files == 1 {
                            "Importing brushes".into()
                        } else {
                            format!("Importing brushes from {} files", b.files)
                        },
                        None,
                    ));
                }
                if self.saving_jobs > 0 {
                    jobs.push(("Saving".into(), None));
                }
                if self.autosaving.load(Ordering::SeqCst) {
                    jobs.push(("Autosaving".into(), None));
                }
                for (label, frac) in jobs {
                    ui.add_space(8.0);
                    if let Some(f) = frac {
                        feedback::progress_bar(ui, Some(f), 64.0);
                    }
                    dense(ui, label, theme::TEXT_PRIMARY);
                    feedback::spinner(ui, 12.0);
                }
                // The last notice gets whatever room is left, cut short
                // with an ellipsis (full text on hover), never overlapping.
                if let Some(n) = notice {
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(&n)
                                    .font(theme::dense())
                                    .color(theme::TEXT_PRIMARY),
                            )
                            .truncate(),
                        )
                        .on_hover_text(n);
                    });
                }
            });
        });
    }

    /// Screenshot mode: draw sample strokes with the first imported
    /// brushes, to review textured strokes on the canvas.
    fn shot_strokes(&mut self) {
        use symphony_core::stroke::{Point, Stroke, StrokeKind};
        let imported: Vec<_> = self
            .presets
            .brush
            .iter()
            .filter(|p| p.source.is_some())
            .take(6)
            .cloned()
            .collect();
        for (row, p) in imported.iter().enumerate() {
            let b = p.settings;
            if let Some(tex) = b.texture {
                for k in [tex.tip, tex.grain].into_iter().flatten() {
                    brush_import::ensure_texture(&mut self.s.doc, k);
                }
            }
            let y = -170.0 + row as f32 * 60.0;
            let points = (0..=60)
                .map(|i| {
                    let t = i as f32 / 60.0;
                    Point {
                        x: -420.0 + t * 380.0,
                        y: y + (t * std::f32::consts::TAU).sin() * 14.0,
                        pressure: (t * std::f32::consts::PI).sin(),
                    }
                })
                .collect();
            let st = Stroke {
                id: self.s.doc.ids.fresh(),
                kind: StrokeKind::Brush,
                art: Default::default(),
                swatch: self.s.swatch,
                min_width: b.min_width.min(40.0),
                max_width: b.max_width.min(40.0),
                points,
                tips: Default::default(),
                texture: b.texture,
                shape: Vec::new(),
            };
            let layer = self.s.doc.layers.len().saturating_sub(1);
            self.s.commit_stroke(layer, 1, st);
        }
    }

    /// Brush and preset files dropped anywhere on the window import as
    /// Brush presets; while files hover, say what dropping will do.
    fn file_drops(&mut self, ctx: &egui::Context) {
        if let Some(job) = &self.brush_import {
            if let Some(done) = job.poll() {
                self.brush_import = None;
                let n = done.presets.len();
                for p in done.presets {
                    let name = self
                        .presets
                        .unique_name(presets::PresetTool::Brush, &p.name);
                    self.presets.brush.push(presets::Preset { name, ..p });
                }
                // One line: what arrived, then anything that didn't.
                let mut text = match done.messages.as_slice() {
                    [] if n == 0 => "No brushes were imported.".to_owned(),
                    [] => format!("Imported {n} brushes."),
                    [only] => format!("Imported {only}."),
                    [first, rest @ ..] => format!("Imported {first}. {}", rest.join(". ")),
                };
                if n == 0 {
                    text = text.replacen("Imported ", "", 1);
                }
                for m in &done.messages {
                    eprintln!("Brush import: {m}");
                }
                if n > 0 {
                    self.s.tool = Tool::Brush;
                }
                if self.shot.is_some() && n > 0 {
                    self.shot_strokes();
                }
                self.s.notice = Some(text);
            }
        }
        let (hovered, dropped) =
            ctx.input(|i| (i.raw.hovered_files.len(), i.raw.dropped_files.clone()));
        if hovered > 0 {
            egui::Area::new(egui::Id::unique("symphony-drop-hint"))
                .order(egui::Order::Foreground)
                .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
                .interactable(false)
                .show(ctx, |ui| {
                    egui::Frame::new()
                        .fill(theme::BG_RAISED)
                        .stroke(egui::Stroke::new(1.0, theme::ACCENT_SELECT))
                        .corner_radius(theme::RADIUS_FLOATING)
                        .inner_margin(egui::Margin::same(16))
                        .show(ui, |ui| {
                            ui.label(
                                egui::RichText::new("Drop to import brushes")
                                    .font(egui::FontId::proportional(theme::SIZE_HEADING))
                                    .color(theme::TEXT_PRIMARY),
                            );
                            ui.label(
                                egui::RichText::new(
                                    "Krita, MyPaint, Procreate, Clip Studio and GIMP brushes, or Symphony presets",
                                )
                                .font(theme::dense())
                                .color(theme::TEXT_SECONDARY),
                            );
                        });
                });
        }
        let paths: Vec<std::path::PathBuf> = dropped
            .into_iter()
            .map(|f| f.path().to_path_buf())
            .filter(|p| !p.as_os_str().is_empty())
            .collect();
        if paths.is_empty() {
            return;
        }
        let (brushes, rest): (Vec<_>, Vec<_>) = paths
            .into_iter()
            .partition(|p| symphony_brushes::is_brush_file(p));
        let mut notes = Vec::new();
        for p in rest {
            let is_preset = p
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case(presets::EXTENSION));
            let name = p
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            notes.push(if is_preset {
                match self.presets.import(&p) {
                    Ok(n) => format!("Imported {n} presets from {name}."),
                    Err(e) => format!("{name}: {e}"),
                }
            } else {
                format!("{name} isn't a brush file Symphony can import.")
            });
        }
        if !notes.is_empty() {
            self.s.notice = Some(notes.join(" "));
        }
        if !brushes.is_empty() {
            if self.brush_import.is_some() {
                self.s.notice =
                    Some("Still importing the last brushes. Drop these again in a moment.".into());
            } else {
                self.brush_import = Some(brush_import::BrushImport::start(brushes, ctx));
            }
        }
    }

    fn dialogs(&mut self, ctx: &egui::Context) {
        if let Some(o) = &self.opening {
            let name = symphony_io::project_name(&o.path);
            let elapsed = self.last_time.unwrap_or(0.0) - o.started;
            let mut cancel = false;
            egui::Modal::new(egui::Id::unique("symphony-opening")).show(ctx, |ui| {
                ui.set_width(320.0);
                ui.horizontal(|ui| {
                    feedback::spinner(ui, 16.0);
                    ui.label(format!("Opening {name}"));
                });
                ui.add_space(8.0);
                feedback::progress_bar(ui, None, ui.available_width());
                if let Some(e) = feedback::elapsed_label(elapsed) {
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(e)
                                .font(theme::dense())
                                .color(theme::TEXT_SECONDARY),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            cancel = ui.button("Cancel").clicked();
                        });
                    });
                }
            });
            if cancel {
                // The read finishes in the background and is ignored.
                self.opening = None;
            }
        }

        if let Some(p) = self.pending {
            let name = self.s.name();
            let mut choice = None;
            egui::Modal::new(egui::Id::unique("symphony-unsaved")).show(ctx, |ui| {
                ui.heading(format!("Save changes to {name}?"));
                ui.label("If you don't save, the changes since your last save are lost.");
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Save").clicked() {
                        choice = Some(0);
                    }
                    if ui.button("Don't save").clicked() {
                        choice = Some(1);
                    }
                    if ui.button("Cancel").clicked() {
                        choice = Some(2);
                    }
                });
            });
            match choice {
                Some(0) => {
                    self.pending = None;
                    if self.save_now() {
                        self.proceed(ctx, p);
                    }
                }
                Some(1) => {
                    self.pending = None;
                    // Scene setup can be cancelled. Keep the previous scene
                    // dirty and recoverable until the replacement succeeds.
                    if p != Pending::New {
                        if let Some(r) = &self.recovery {
                            r.clear(&self.s.key);
                        }
                        self.s.saved_version = self.s.doc.version;
                    }
                    self.proceed(ctx, p);
                }
                Some(_) => self.pending = None,
                None => {}
            }
        }

        if !self.recovered.is_empty() {
            let n = self.recovered.len();
            let mut choice = None;
            egui::Modal::new(egui::Id::unique("symphony-recover")).show(ctx, |ui| {
                ui.heading("Recover unsaved work?");
                let what = if n == 1 { "a scene".to_owned() } else { format!("{n} scenes") };
                ui.label(format!("Symphony didn't close properly last time. It autosaved {what} you hadn't saved."));
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Recover").clicked() {
                        choice = Some(true);
                    }
                    if ui.button("Discard").clicked() {
                        choice = Some(false);
                    }
                });
            });
            if let Some(recover) = choice {
                if recover {
                    let (key, origin, doc) = self.recovered.remove(0);
                    let mut next = Session::from_doc(doc, origin);
                    next.key = key;
                    // Recovered work isn't saved yet, so it shows as edited.
                    next.saved_version = next.doc.version.wrapping_sub(1);
                    self.welcome = None;
                    self.s = next;
                    self.autosaved_version = self.s.doc.version;
                }
                if let Some(r) = &self.recovery {
                    for (k, _, _) in self.recovered.drain(..) {
                        r.clear(&k);
                    }
                }
            }
        }

        if let Some(e) = self.error.clone() {
            egui::Modal::new(egui::Id::unique("symphony-error")).show(ctx, |ui| {
                ui.heading("That didn't work");
                ui.label(e);
                ui.add_space(8.0);
                if ui.button("OK").clicked() {
                    self.error = None;
                }
            });
        }
    }
}

struct Viewer<'a> {
    workspace: Workspace,
    s: &'a mut Session,
    camera: &'a mut camera::CanvasState,
    drawing: &'a mut camera::CanvasState,
    node_view: &'a mut node::NodeViewState,
    function: &'a mut function::FunctionState,
    timing: &'a mut timing::TimingState,
    samples: &'a [PenSample],
    device: Option<&'a DeviceInfo>,
    profile: Profile,
    presets: &'a mut presets::Presets,
}

impl Viewer<'_> {
    fn view(&mut self, ui: &mut egui::Ui, v: View) {
        // Fade the incoming content in place. Pen coordinates and hit targets stay fixed.
        let duration = ui.style().animation_time as f64;
        if duration > 0.0 && !matches!(v, View::Camera | View::Drawing) {
            let key = egui::Id::unique(("view-appearance", self.workspace, v));
            let frame = ui.ctx().cumulative_frame_nr();
            let now = ui.input(|i| i.time);
            let start = ui.data_mut(|data| {
                let (last, since) = data.get_temp::<(u64, f64)>(key).unwrap_or((frame, now));
                let start = if frame > last.saturating_add(1) {
                    now
                } else {
                    since
                };
                data.insert_temp(key, (frame, start));
                start
            });
            let progress = ((now - start) / duration).clamp(0.0, 1.0) as f32;
            if progress < 1.0 {
                ui.multiply_opacity(1.0 - 0.5 * (1.0 - progress).powi(3));
                ui.ctx().request_repaint();
            }
        }
        match v {
            View::Camera => camera::ui(ui, self.s, self.camera, self.samples, false, self.profile),
            View::Drawing => camera::ui(ui, self.s, self.drawing, self.samples, true, self.profile),
            View::Timeline => timing::timeline(ui, self.s, self.timing, self.profile),
            View::Xsheet => timing::xsheet(ui, self.s, self.timing, self.profile),
            View::Node => node::ui(ui, self.s, self.node_view),
            View::ToolProperties => panels::tool_properties(ui, self.s, self.presets, self.device),
            View::Colour => panels::colour(ui, self.s),
            View::LayerProperties => panels::layer_properties(ui, self.s),
            View::Function => function::ui(ui, self.s, self.function),
            View::NodeLibrary => node::library(ui, self.s, self.node_view),
            View::Library => panels::planned(ui, "Library (templates and symbols)", "M6"),
        }
    }
}

impl egui_dock::TabViewer for Viewer<'_> {
    type Tab = View;

    fn id(&mut self, tab: &mut View) -> egui::Id {
        egui::Id::unique(("symphony-view", *tab))
    }

    fn title(&mut self, tab: &mut View) -> egui::WidgetText {
        tab.title().into()
    }

    fn ui(&mut self, ui: &mut egui::Ui, tab: &mut View) {
        self.view(ui, *tab);
    }

    fn scroll_bars(&self, tab: &View) -> [bool; 2] {
        // Canvas-like views manage their own space.
        match tab {
            View::Camera | View::Drawing | View::Node | View::Timeline | View::Xsheet => {
                [false, false]
            }
            _ => [false, true],
        }
    }
}

impl eframe::App for SymphonyApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if let Some(s) = self.shot.as_mut() {
            s.tick(&ctx);
        }
        // Design review only: show the shared loading screen (DESIGN D17.7).
        // Screenshot mode can import brushes as if dropped (SYMPHONY_SHOT_DROP).
        if let (Some(_), Some(p)) = (&self.shot, std::env::var_os("SYMPHONY_SHOT_DROP")) {
            if self.brush_import.is_none() && !self.presets.brush.iter().any(|p| p.source.is_some())
            {
                self.brush_import = Some(brush_import::BrushImport::finished(vec![p.into()]));
            }
        }
        if self.shot.is_some() && std::env::var_os("SYMPHONY_SHOT_LOADING").is_some() {
            feedback::loading_screen(
                ui,
                "Building the latest Symphony",
                Some("Compiling symphony-core (148 of about 246)"),
                Some(0.6),
                12.4,
            );
            return;
        }

        if ctx.input(|i| i.viewport().close_requested())
            && self
                .welcome
                .as_ref()
                .is_some_and(welcome::Welcome::creating)
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
        }

        // Closing with unsaved work asks first.
        if ctx.input(|i| i.viewport().close_requested()) && self.s.dirty() && !self.allow_close {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.pending = Some(Pending::Quit);
        }

        self.poll_startup();
        if self.welcome.is_some() {
            self.last_time = Some(ctx.input(|i| i.time));
            self.poll_saves();
            self.poll_open();
            if let Some(welcome) = self.welcome.as_mut() {
                self.wallpaper.paint(ui);
                match welcome.ui(
                    ui,
                    &self.recent_projects,
                    self.recovery_scan.is_some(),
                    self.opening.is_some(),
                ) {
                    Some(welcome::Action::Open) => self.open_dialog(false, &ctx),
                    Some(welcome::Action::Recent(path)) => self.open_path(path, false, &ctx),
                    Some(welcome::Action::Cancel) => self.welcome = None,
                    Some(welcome::Action::Created(scene, path)) => {
                        self.remember_project(&path);
                        self.replace_session(Session::from_doc(*scene, Some(path)));
                        self.welcome = None;
                    }
                    None => {}
                }
                self.dialogs(&ctx);
                return;
            }
        }

        let samples = match &self.pen {
            Some(p) => p.drain(),
            None => camera::pointer_samples(&ctx),
        };
        let device = self.pen.as_ref().map(|p| p.device());
        let now = ctx.input(|i| i.time);
        self.poll_saves();
        self.poll_open();
        self.poll_codec();
        self.expire_notice(now);
        self.autosave(now);
        self.tick_playback(&ctx);
        if self.pending.is_none() && self.recovered.is_empty() && self.error.is_none() {
            self.shortcuts(&ctx);
        }

        let title = if self.s.dirty() {
            format!("{} (edited) - Symphony", self.s.name())
        } else {
            format!("{} - Symphony", self.s.name())
        };
        if title != self.title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.title = title;
        }

        self.wallpaper.paint(ui);
        let base = egui::Frame::new().fill(theme::GLASS_CHROME);
        egui::Panel::top("top-bar")
            .show_separator_line(false)
            .frame(base.inner_margin(egui::Margin::symmetric(10, 0)))
            .exact_size(theme::TOP_BAR)
            .show(ui, |ui| self.top_bar(ui));
        egui::Panel::bottom("status")
            .show_separator_line(false)
            .frame(base.inner_margin(egui::Margin::symmetric(12, 0)))
            .exact_size(theme::STATUS_BAR)
            .show(ui, |ui| self.status_bar(ui, device.as_ref()));
        let g = theme::GUTTER as i8;
        egui::Panel::left("tools")
            .show_separator_line(false)
            .frame(egui::Frame::new().inner_margin(egui::Margin {
                left: g,
                right: 0,
                top: g,
                bottom: g,
            }))
            .exact_size(theme::TOOLS_STRIP + theme::GUTTER)
            .resizable(false)
            .show(ui, |ui| self.tools_strip(ui));

        let mut open = self.show_preferences;
        egui::Window::new("Preferences")
            .open(&mut open)
            .resizable(false)
            .collapsible(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(&ctx, |ui| {
                egui::ScrollArea::vertical()
                    .max_height(560.0)
                    .show(ui, |ui| {
                        ui.heading("Interface");
                        self.wallpaper.preferences(ui);
                        if symphony_ui::widgets::checkbox(
                            ui,
                            &mut self.interface_animations,
                            "Animate interface transitions",
                        )
                        .changed()
                        {
                            theme::set_motion(&ctx, self.interface_animations);
                        }
                        ui.separator();
                        panels::preferences(
                            ui,
                            &mut self.profile,
                            &mut self.use_no_ai,
                            &mut self.s.brush_size_cursor,
                            &mut self.s.overlay_underlay,
                            &mut self.s.size_follows_zoom,
                        );
                        self.video_preferences(ui);
                    })
            });
        self.show_preferences = open;
        let mut open = self.show_icon_sheet;
        egui::Window::new("Icon sheet")
            .open(&mut open)
            .collapsible(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(&ctx, panels::icon_sheet);
        self.show_icon_sheet = open;

        let ws = self.workspace;
        let maximised = self.maximised;
        let layout = &mut self
            .layouts
            .iter_mut()
            .find(|(w, _)| *w == ws)
            .expect("layout")
            .1;
        let mut viewer = Viewer {
            workspace: ws,
            s: &mut self.s,
            camera: &mut self.camera,
            drawing: &mut self.drawing,
            node_view: &mut self.node_view,
            function: &mut self.function,
            timing: &mut self.timing,
            samples: &samples,
            device: device.as_ref(),
            profile: self.profile,
            presets: &mut self.presets,
        };
        let central = egui::Frame::new();
        egui::CentralPanel::default()
            .frame(central)
            .show(ui, |ui| match maximised {
                Some(v) => {
                    let card = egui::Frame::new()
                        .fill(theme::GLASS_PANEL)
                        .corner_radius(theme::GLASS_RADIUS)
                        .outer_margin(egui::Margin::same(theme::GUTTER as i8))
                        .inner_margin(egui::Margin::same(4))
                        .show(ui, |ui| viewer.view(ui, v));
                    theme::glass_rim(ui.painter(), card.response.rect.shrink(theme::GUTTER));
                }

                None => {
                    // Reserve the fill below the dock's widgets. Leaf bounds are
                    // final only after the splitters have processed this frame.
                    let painter = ui.painter().clone();
                    let background = painter.add(egui::Shape::Noop);
                    DockArea::new(layout)
                        .style(theme::dock_style(ui.style()))
                        .show_add_buttons(false)
                        .show_close_buttons(false)
                        .show_leaf_close_all_buttons(false)
                        .show_leaf_collapse_buttons(false)
                        .show_inside(ui, &mut viewer);
                    let mut fills = Vec::new();
                    for leaf in layout
                        .main_surface()
                        .iter()
                        .filter_map(|node| node.get_leaf())
                    {
                        let rect = leaf.rect();
                        if rect.is_positive() && !leaf.tabs.is_empty() {
                            fills.push(egui::Shape::rect_filled(
                                rect,
                                theme::GLASS_RADIUS,
                                theme::GLASS_PANEL,
                            ));
                            theme::glass_rim(&painter, rect);
                        }
                    }
                    painter.set(background, egui::Shape::Vec(fills));
                }
            });

        let codec_path = self.codec();
        let downloading = self.codec_download.is_some();
        if self
            .export
            .ui(&ctx, &self.s, codec_path, downloading)
            .download_codec
        {
            self.start_codec_download(&ctx);
        }
        let mut open = self.show_licences;
        egui::Window::new("Licences")
            .open(&mut open)
            .collapsible(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .default_size([560.0, 480.0])
            .show(&ctx, |ui| {
                ui.label("Symphony is free software under the GNU General Public License, version 3 or later.");
                ui.separator();
                ui.heading("OpenH264");
                ui.label(codec::NOTICE);
                egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                    ui.label(egui::RichText::new(codec::LICENSE).font(theme::dense()));
                });
            });
        self.show_licences = open;
        self.file_drops(&ctx);
        self.dialogs(&ctx);

        let pending = self.pen.as_ref().is_some_and(|p| p.has_pending());
        if pending || self.camera.busy() || self.drawing.busy() {
            ctx.request_repaint();
        }
        if self.export.busy() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
        // Keep autosave ticking even when idle.
        ctx.request_repaint_after(std::time::Duration::from_secs(5));
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        // Screenshot runs use fresh layouts and test data; they must never
        // overwrite the person's real preferences.
        if self.shot.is_some() {
            return;
        }
        let saved = Saved {
            recent_projects: self.recent_projects.clone(),
            wallpaper: self.wallpaper.choice.clone(),
            interface_animations: self.interface_animations,
            workspace: self.workspace,
            layouts: self.layouts.clone(),
            profile: self.profile,
            use_no_ai: self.use_no_ai,
            pressure_gamma: Some(self.s.pressure_gamma),
            brush_size_cursor: self.s.brush_size_cursor,
            overlay_underlay: self.s.overlay_underlay,
            size_follows_zoom: self.s.size_follows_zoom,
            use_openh264: self.use_openh264,
            presets: Some(self.presets.clone()),
        };
        eframe::set_value(storage, eframe::APP_KEY, &saved);
    }

    fn on_exit(&mut self) {
        // A clean exit with nothing unsaved needs no recovery file (E7.5).
        if !self.s.dirty() {
            if let Some(r) = &self.recovery {
                r.clear(&self.s.key);
            }
        }
    }
}

/// On macOS the top bar becomes the title bar: content runs under it and
/// the window buttons float at its left (D3.1, D12.2).
fn viewport() -> egui::ViewportBuilder {
    // Design review can force a window size: SYMPHONY_SHOT_SIZE=1280x800.
    let size = std::env::var("SYMPHONY_SHOT_SIZE")
        .ok()
        .and_then(|v| {
            v.split_once('x')
                .and_then(|(w, h)| Some([w.parse().ok()?, h.parse().ok()?]))
        })
        .unwrap_or([1440.0, 900.0]);
    let v = egui::ViewportBuilder::default()
        .with_title("Symphony")
        .with_inner_size(size)
        .with_min_inner_size([960.0, 600.0]);
    if cfg!(target_os = "macos") {
        v.with_fullsize_content_view(true)
            .with_titlebar_shown(false)
            .with_title_shown(false)
    } else {
        v
    }
}

fn main() -> eframe::Result {
    let _version_lock = std::env::current_exe()
        .and_then(|exe| symphony_launcher::editor_lock(&exe))
        .map_err(|e| eframe::Error::AppCreation(Box::new(e)))?;
    if std::env::args_os().nth(1).as_deref() == Some(std::ffi::OsStr::new("--update")) {
        return symphony_launcher::run_release_manager(true);
    }
    let options = eframe::NativeOptions {
        viewport: viewport(),
        // A forced review size must win over the remembered window size.
        persist_window: std::env::var_os("SYMPHONY_SHOT_SIZE").is_none(),
        // Screenshot runs keep their state in a throwaway folder, so a
        // design review never reads or changes the real preferences.
        persistence_path: std::env::var_os("SYMPHONY_SCREENSHOT")
            .map(|_| std::env::temp_dir().join("symphony-shot-state")),
        ..Default::default()
    };
    eframe::run_native(
        "Symphony",
        options,
        Box::new(|cc| Ok(Box::new(SymphonyApp::new(cc)))),
    )
}
