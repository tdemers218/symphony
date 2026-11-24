//! Symphony desktop app. Window anatomy follows docs/DESIGN.md §3: one top
//! bar (the title bar on macOS), the Tools toolbar card, the dock of view
//! cards, and the status bar. Shortcuts come only from the keymap
//! (ENGINEERING E4.3). Saving and autosave run off the main thread (E5.2).

mod app_icon;
mod audio;
mod backdrop;
mod brush_import;
mod codec;
mod crash;
mod import;
mod keymap;
mod layered;
mod perf;
mod presets;
mod save_dialog;
mod search;
mod session;
mod shot;
#[cfg(test)]
mod ui_tests;
mod update;
mod vector_import;
mod video;
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

/// Pages of the Preferences window.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum PrefsPage {
    Interface,
    Drawing,
    Canvas,
    Workflow,
    Video,
    Ai,
}

impl PrefsPage {
    const ALL: [PrefsPage; 6] = [
        PrefsPage::Interface,
        PrefsPage::Drawing,
        PrefsPage::Canvas,
        PrefsPage::Workflow,
        PrefsPage::Video,
        PrefsPage::Ai,
    ];

    fn title(self) -> &'static str {
        match self {
            PrefsPage::Interface => "Interface",
            PrefsPage::Drawing => "Drawing",
            PrefsPage::Canvas => "Canvas",
            PrefsPage::Workflow => "Workflow and shortcuts",
            PrefsPage::Video => "Video",
            // The page promises no generative AI; "AI" alone read like a feature.
            PrefsPage::Ai => "AI-free",
        }
    }
}

/// Autosave interval, seconds (E7.5).
const AUTOSAVE_EVERY: f64 = 120.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
enum ToolDock {
    #[default]
    Left,
    Right,
    Top,
    Bottom,
}

impl ToolDock {
    const ALL: [Self; 4] = [Self::Left, Self::Right, Self::Top, Self::Bottom];

    fn label(self) -> &'static str {
        match self {
            Self::Left => "Left",
            Self::Right => "Right",
            Self::Top => "Top",
            Self::Bottom => "Bottom",
        }
    }

    fn horizontal(self) -> bool {
        matches!(self, Self::Top | Self::Bottom)
    }

    fn preview(self, workspace: egui::Rect) -> egui::Rect {
        let mut rect = workspace.shrink(theme::GUTTER);
        match self {
            Self::Left => rect.max.x = rect.min.x + theme::TOOLS_STRIP,
            Self::Right => rect.min.x = rect.max.x - theme::TOOLS_STRIP,
            Self::Top => rect.max.y = rect.min.y + theme::TOOLS_STRIP,
            Self::Bottom => rect.min.y = rect.max.y - theme::TOOLS_STRIP,
        }
        rect.intersect(workspace)
    }

    fn nearest(rect: egui::Rect, pos: egui::Pos2) -> Self {
        let centre = rect.center();
        let local = pos - centre;
        let x = local.x / rect.width().max(1.0);
        let y = local.y / rect.height().max(1.0);
        if x.abs() > y.abs() {
            if x < 0.0 {
                Self::Left
            } else {
                Self::Right
            }
        } else if y < 0.0 {
            Self::Top
        } else {
            Self::Bottom
        }
    }
}

#[derive(Serialize, Deserialize)]
struct Saved {
    /// How pictures were last imported (Picture or Vector drawing).
    #[serde(default)]
    import_options: Option<import::ImportOptions>,
    #[serde(default)]
    tool_dock: ToolDock,
    #[serde(default)]
    recent_projects: Vec<PathBuf>,
    #[serde(default)]
    wallpaper: wallpaper::Choice,
    #[serde(default)]
    wallpaper_blur: u8,
    #[serde(default = "yes")]
    interface_animations: bool,
    /// Blur, glass and shadows (off on slower computers).
    #[serde(default = "yes")]
    interface_effects: bool,
    workspace: Workspace,
    layouts: Vec<(Workspace, DockState<View>)>,
    #[serde(default)]
    profile: Profile,
    /// Shortcuts remapped from search, per profile.
    #[serde(default)]
    remaps: Vec<keymap::Remap>,
    /// DESIGN D16.2: on by default.
    #[serde(default = "yes")]
    use_no_ai: bool,
    #[serde(default)]
    pressure_gamma: Option<f32>,
    /// Brush size cursor, on by default (Tristan: the size preview is
    /// the cursor). A new key, so earlier "off" defaults don't carry over.
    #[serde(default = "yes", rename = "brush_size_cursor_on")]
    brush_size_cursor: bool,
    /// Harmony's Support Overlay and Underlay Arts preference.
    #[serde(default)]
    overlay_underlay: bool,
    #[serde(default)]
    size_follows_zoom: bool,
    #[serde(default)]
    stroke_detail: Option<f32>,
    #[serde(default)]
    colours: Option<session::CanvasColours>,
    /// Use Cisco's OpenH264 for movies (Cisco requires the user can turn it off).
    #[serde(default = "yes")]
    use_openh264: bool,
    #[serde(default)]
    presets: Option<presets::Presets>,
    /// How often each search result was chosen, to rank favourites first.
    #[serde(default)]
    search_uses: std::collections::HashMap<String, u32>,
}

fn preferences_window(ctx: &egui::Context, id: egui::Id) -> egui::Window<'static> {
    egui::Window::new("Preferences")
        .id(id)
        .resizable(false)
        .collapsible(false)
        .movable(true)
        .pivot(egui::Align2::CENTER_CENTER)
        .default_pos(ctx.content_rect().center())
        .frame(
            egui::Frame::window(&ctx.global_style())
                // The glass (live blur and tint) is painted underneath.
                .fill(egui::Color32::TRANSPARENT)
                .stroke(egui::Stroke::NONE)
                .corner_radius(theme::GLASS_RADIUS),
        )
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
    /// SYMPHONY_PERF_SECONDS: a timed performance run.
    perf_run: Option<perf::Run>,
    /// SYMPHONY_PERF_OPEN: a project to open on the first frame.
    perf_open: Option<PathBuf>,
    tool_dock: ToolDock,
    tool_options_open: bool,
    save_dialog: Option<(save_dialog::SaveDialog, Option<Pending>)>,
    inbetween_count: u32,
    welcome: Option<welcome::Welcome>,
    /// The status notice on show and when it appeared, so notices fade in,
    /// hold a moment and fade out instead of staying forever.
    notice_shown: Option<(String, f64)>,
    /// Autosave was running last frame (to say "Autosaved" when it ends).
    was_autosaving: bool,
    /// Last frame's tool and size, and the hint shown beside the cursor
    /// when either changes over the canvas: text and when it appeared.
    last_tool_state: (Tool, f32),
    cursor_hint: Option<(String, f64)>,
    recent_projects: Vec<PathBuf>,
    recovery_scan: Option<Receiver<Vec<RecoveredScene>>>,
    wallpaper: wallpaper::Wallpaper,
    interface_animations: bool,
    /// Blur, glass and shadows; off also turns animations off.
    interface_effects: bool,
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
    /// The search box (Tab), when open.
    palette: Option<views::palette::PaletteState>,
    /// The view under the mouse last frame, for search ranking.
    hovered_view: Option<View>,
    /// Screenshot mode opened the search box already.
    shot_search_opened: bool,
    search_uses: std::collections::HashMap<String, u32>,
    node_view: node::NodeViewState,
    timing: timing::TimingState,
    export: export::ExportDialog,
    maximised: Option<View>,
    show_preferences: bool,
    prefs_page: PrefsPage,
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
    /// Images being read and copied in the background.
    image_import: Option<import::ImageImport>,
    /// Pictures waiting for the Import Images question.
    image_import_asking: Option<Vec<PathBuf>>,
    /// How pictures were last imported (remembered).
    import_options: import::ImportOptions,
    sound_import: Option<import::SoundImport>,
    video_import: Option<video::VideoImport>,
    /// The scene's sound, in step with the playhead.
    player: audio::Player,
    /// Screenshot runs import their file once.
    shot_imported: bool,
    show_licences: bool,
    /// A crash report from last time, offered once (path, text).
    crash_report: Option<(PathBuf, String)>,
    opening: Option<Opening>,
    notice_seen: Option<(String, f64)>,
}

/// The bar between two views, while you point at it: a slim rounded pill
/// along the gap, replacing the dock's own hard-edged bar. Blue while
/// dragging.
fn resize_grip(ui: &egui::Ui, leaves: &[egui::Rect]) {
    use symphony_ui::motion;
    // What's being dragged: which kinds of gap (vertical, horizontal), and
    // where the grip was. While dragging, the grip stays on the nearest gap
    // of that kind even when a fast pointer runs ahead of the divider, so it
    // never blinks off mid-drag.
    #[derive(Clone, Copy, Default)]
    struct Grip {
        held: Option<(bool, bool)>,
        shapes: [Option<egui::Rect>; 2],
    }
    let id = egui::Id::unique("symphony-resize-grip-state");
    let mut grip: Grip = ui.data(|d| d.get_temp(id)).unwrap_or_default();
    let (down, pressed) = ui.input(|i| (i.pointer.primary_down(), i.pointer.primary_pressed()));
    if !down {
        grip.held = None;
    }
    let pointer = ui.ctx().pointer_hover_pos();
    let covered = pointer.is_some_and(|p| {
        ui.ctx()
            .layer_id_at(p)
            .is_some_and(|layer| layer != ui.layer_id())
    });
    let gap = theme::GUTTER * 2.0 + 2.0;
    // While held, look much further along the drag axis.
    let reach = if grip.held.is_some() { 80.0 } else { 3.0 };
    let mut vertical: Option<(f32, egui::Rect)> = None;
    let mut horizontal: Option<(f32, egui::Rect)> = None;
    if let (Some(p), false) = (pointer, covered) {
        for a in leaves {
            for b in leaves {
                let dx = b.left() - a.right();
                let (top, bottom) = (a.top().max(b.top()), a.bottom().min(b.bottom()));
                if dx > 0.0 && dx <= gap && bottom > top {
                    let r = egui::Rect::from_x_y_ranges(a.right()..=b.left(), top..=bottom);
                    let d = (p.x - r.center().x).abs();
                    if r.expand2(egui::vec2(reach, gap)).contains(p)
                        && vertical.is_none_or(|(best, _)| d < best)
                    {
                        vertical = Some((d, r));
                    }
                }
                let dy = b.top() - a.bottom();
                let (left, right) = (a.left().max(b.left()), a.right().min(b.right()));
                if dy > 0.0 && dy <= gap && right > left {
                    let r = egui::Rect::from_x_y_ranges(left..=right, a.bottom()..=b.top());
                    let d = (p.y - r.center().y).abs();
                    if r.expand2(egui::vec2(gap, reach)).contains(p)
                        && horizontal.is_none_or(|(best, _)| d < best)
                    {
                        horizontal = Some((d, r));
                    }
                }
            }
        }
    }
    let (mut vertical, mut horizontal) = (vertical.map(|v| v.1), horizontal.map(|h| h.1));
    if let Some((v, h)) = grip.held {
        // Only the kinds being dragged.
        vertical = vertical.filter(|_| v);
        horizontal = horizontal.filter(|_| h);
    } else if let Some(p) = pointer {
        // Not dragging: a single bar shows only right at the gap.
        let near = |r: &egui::Rect| r.expand(3.0).contains(p);
        if vertical.is_some() != horizontal.is_some() {
            vertical = vertical.filter(near);
            horizontal = horizontal.filter(near);
        }
    }
    if pressed && grip.held.is_none() && (vertical.is_some() || horizontal.is_some()) {
        grip.held = Some((vertical.is_some(), horizontal.is_some()));
    }
    let shapes: [Option<egui::Rect>; 2] = match (vertical, horizontal) {
        // A crossing: a small cross, since dragging here moves both ways.
        (Some(v), Some(h)) => {
            let c = egui::pos2(v.center().x, h.center().y);
            [
                Some(egui::Rect::from_center_size(c, egui::vec2(3.0, 26.0))),
                Some(egui::Rect::from_center_size(c, egui::vec2(26.0, 3.0))),
            ]
        }
        (Some(v), None) => [
            Some(egui::Rect::from_center_size(
                v.center(),
                egui::vec2(3.0, (v.height() - 24.0).max(12.0)),
            )),
            None,
        ],
        (None, Some(h)) => [
            Some(egui::Rect::from_center_size(
                h.center(),
                egui::vec2((h.width() - 24.0).max(12.0), 3.0),
            )),
            None,
        ],
        (None, None) => [None, None],
    };
    let showing = shapes[0].is_some();
    if showing {
        grip.shapes = shapes;
    }
    ui.data_mut(|d| d.insert_temp(id, grip));
    // Fade in and out rather than popping (the last place fades out).
    let shown = motion::presence(ui.ctx(), id.with("shown"), showing);
    if shown <= 0.0 {
        return;
    }
    let colour = if grip.held.is_some() {
        theme::ACCENT_SELECT
    } else {
        theme::TEXT_SECONDARY.gamma_multiply(0.7)
    }
    .gamma_multiply(shown);
    for r in grip.shapes.into_iter().flatten() {
        ui.painter().rect_filled(r, 1.5, colour);
    }
}

/// When a view lands somewhere new (docked, undocked into a window, or
/// merged into another view's tabs), a glass outline travels from where it
/// was to where it landed with the shared Settle motion, so you see where
/// it went.
fn settle_moved_views(ctx: &egui::Context, layout: &egui_dock::DockState<View>) {
    use symphony_ui::motion::{self, Motion};
    #[derive(Clone)]
    struct Ghost {
        view: View,
        from: egui::Rect,
        start: f64,
    }
    let seen_id = egui::Id::unique("symphony-dock-seen");
    let ghost_id = egui::Id::unique("symphony-dock-ghosts");
    let now = ctx.input(|i| i.time);
    // Each view: its leaf's rect, and who it shares the leaf with.
    let mut current: Vec<(View, egui::Rect, Vec<View>)> = Vec::new();
    for (_, leaf) in layout.iter_leaves() {
        let rect = leaf.rect();
        if !rect.is_positive() || !rect.is_finite() {
            continue;
        }
        let mut group = leaf.tabs.clone();
        group.sort_by_key(|v| format!("{v:?}"));
        for &v in &leaf.tabs {
            current.push((v, rect, group.clone()));
        }
    }
    let seen: Vec<(View, egui::Rect, Vec<View>)> =
        ctx.data(|d| d.get_temp(seen_id)).unwrap_or_default();
    let mut ghosts: Vec<Ghost> = ctx.data(|d| d.get_temp(ghost_id)).unwrap_or_default();
    if motion::enabled(ctx) {
        for (v, rect, group) in &current {
            let moved = seen
                .iter()
                .find(|(s, ..)| s == v)
                .filter(|(_, old, old_group)| {
                    old_group != group
                        && (old.min - rect.min).length() + (old.max - rect.max).length() > 4.0
                });
            if let Some((_, old, _)) = moved {
                ghosts.retain(|g| g.view != *v);
                ghosts.push(Ghost {
                    view: *v,
                    from: *old,
                    start: now,
                });
            }
        }
    }
    // Views that vanished for a frame (a drop resolves over two frames)
    // keep their last place.
    let mut keep = current.clone();
    for s in &seen {
        if !keep.iter().any(|(v, ..)| *v == s.0) && layout.find_tab(&s.0).is_some() {
            keep.push(s.clone());
        }
    }
    ctx.data_mut(|d| d.insert_temp(seen_id, keep));
    let length = Motion::Settle.seconds();
    ghosts.retain(|g| now - g.start < length);
    if !ghosts.is_empty() {
        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::unique("symphony-dock-ghost-layer"),
        ));
        for g in &ghosts {
            let Some((_, to, _)) = current.iter().find(|(v, ..)| *v == g.view) else {
                continue;
            };
            let t = ((now - g.start) / length) as f32;
            let p = Motion::Settle.ease(t);
            let rect = egui::Rect::from_min_max(
                g.from.min + (to.min - g.from.min) * p,
                g.from.max + (to.max - g.from.max) * p,
            );
            let fade = 1.0 - t * t;
            painter.rect(
                rect,
                theme::GLASS_RADIUS,
                theme::ACCENT_SELECT.gamma_multiply(0.07 * fade),
                egui::Stroke::new(1.5, theme::ACCENT_SELECT.gamma_multiply(0.6 * fade)),
                egui::StrokeKind::Inside,
            );
        }
        ctx.request_repaint();
    }
    ctx.data_mut(|d| d.insert_temp(ghost_id, ghosts));
}

fn tool_icon(t: Tool) -> Icon {
    match t {
        Tool::Select => Icon::Select,
        Tool::Transform => Icon::Transform,
        Tool::Brush => Icon::Brush,
        Tool::Line => Icon::Line,
        Tool::Text => Icon::Text,
        Tool::Pencil => Icon::Pencil,
        Tool::Eraser => Icon::Eraser,
        Tool::Paint => Icon::Paint,
        Tool::CloseGap => Icon::CloseGap,
        Tool::Dropper => Icon::Dropper,
        Tool::LineEditor => Icon::LineEdit,
        Tool::Hand => Icon::Hand,
        Tool::Zoom => Icon::Zoom,
    }
}

fn tool_action(t: Tool) -> Action {
    match t {
        Tool::Select => Action::ToolSelect,
        Tool::Transform => Action::ToolTransform,
        Tool::Brush => Action::ToolBrush,
        Tool::Line => Action::ToolLine,
        Tool::Text => Action::ToolText,
        Tool::Pencil => Action::ToolPencil,
        Tool::Eraser => Action::ToolEraser,
        Tool::Paint => Action::ToolPaint,
        Tool::CloseGap => Action::ToolCloseGap,
        Tool::Dropper => Action::ToolDropper,
        Tool::LineEditor => Action::ToolLineEditor,
        Tool::Hand => Action::ToolHand,
        Tool::Zoom => Action::ToolZoom,
    }
}

fn save_project(scene: &Scene, path: &std::path::Path, create: bool) -> Result<(), String> {
    if create {
        std::fs::create_dir(path)
            .map_err(|e| format!("Couldn’t create the project folder ({e})."))?;
    }
    symphony_io::save(scene, path).map_err(|e| e.0)
}

fn recovery_dir() -> Option<PathBuf> {
    dirs::data_dir().map(|d| d.join("Symphony").join("recovery"))
}

impl SymphonyApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        #[cfg(target_os = "macos")]
        app_icon::set_dock_icon();
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
        let wallpaper_blur = saved.as_ref().map_or(0, |saved| saved.wallpaper_blur);
        let mut wallpaper_choice = saved
            .as_ref()
            .map(|s| s.wallpaper.clone())
            .unwrap_or_default();
        if std::env::var_os("SYMPHONY_SCREENSHOT").is_some() {
            wallpaper_choice = match std::env::var("SYMPHONY_SHOT_WALLPAPER").as_deref() {
                Ok("Plain") => wallpaper::Choice::Plain,
                Ok("Studio") => wallpaper::Choice::Studio,
                Ok("Dusk") => wallpaper::Choice::Dusk,
                Ok("Camera") => wallpaper::Choice::Camera,
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
        let interface_effects = saved.as_ref().is_none_or(|s| s.interface_effects);
        theme::set_effects(&cc.egui_ctx, interface_effects);
        theme::set_motion(&cc.egui_ctx, interface_animations && interface_effects);
        let brush_size_cursor = saved.as_ref().is_none_or(|s| s.brush_size_cursor);
        let overlay_underlay = saved.as_ref().is_some_and(|s| s.overlay_underlay);
        let size_follows_zoom = saved.as_ref().is_some_and(|s| s.size_follows_zoom);
        let stroke_detail = saved.as_ref().and_then(|s| s.stroke_detail);
        let colours = saved.as_ref().and_then(|s| s.colours);
        let use_openh264 = saved.as_ref().is_none_or(|s| s.use_openh264);
        let presets = saved
            .as_ref()
            .and_then(|s| s.presets.clone())
            .unwrap_or_default();
        let search_uses = saved
            .as_ref()
            .map(|s| s.search_uses.clone())
            .unwrap_or_default();
        let tool_dock = saved
            .as_ref()
            .map_or(ToolDock::Left, |saved| saved.tool_dock);
        let import_options = saved
            .as_ref()
            .and_then(|s| s.import_options)
            .unwrap_or_default();
        keymap::set_remaps(saved.as_ref().map(|s| s.remaps.clone()).unwrap_or_default());
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
        // Screenshot runs can pick a profile: SYMPHONY_SHOT_PROFILE=Symphony.
        let profile = match std::env::var("SYMPHONY_SHOT_PROFILE").as_deref() {
            Ok("Symphony") => Profile::Symphony,
            Ok("Harmony") => Profile::Harmony,
            _ => profile,
        };
        for ws in Workspace::ALL {
            if !layouts.iter().any(|(w, _)| *w == ws) {
                layouts.push((ws, ws.preset()));
            }
        }
        let shot = shot::Shot::from_env();
        // Screenshot runs never show a crash prompt.
        let crash_offer = shot.is_none().then(crash::pending).flatten();
        let shot_recover = shot.is_some() && std::env::var_os("SYMPHONY_SHOT_RECOVER").is_some();
        let demo = shot.is_some() && std::env::var_os("SYMPHONY_SHOT_EMPTY").is_none();
        let perf_scene = std::env::var_os("SYMPHONY_PERF_SCENE").is_some();
        let mut s = if perf_scene {
            perf::heavy_scene()
        } else if demo || std::env::var_os("SYMPHONY_DEMO_ART").is_some() {
            Session::demo()
        } else {
            Session::new_scene()
        };
        if std::env::var_os("SYMPHONY_PERF_PLAY").is_some() {
            s.playing = true;
        }
        if let Some(g) = gamma {
            s.pressure_gamma = g;
        }
        s.brush_size_cursor = brush_size_cursor;
        s.overlay_underlay = overlay_underlay;
        s.size_follows_zoom = size_follows_zoom;
        if let Some(d) = stroke_detail {
            s.stroke_detail = d.clamp(0.1, 1.0);
        }
        if let Some(c) = colours {
            s.colours = c;
        }
        // SYMPHONY_PERF_VIEW: bring a view to the front for measuring it.
        if let Ok(name) = std::env::var("SYMPHONY_PERF_VIEW") {
            if let Some(view) = View::ALL.into_iter().find(|view| view.title() == name) {
                for (_, dock) in layouts.iter_mut() {
                    if let Some(found) = dock.find_tab(&view) {
                        let _ = dock.set_active_tab(found);
                    }
                }
            }
        }
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
            // Bring a named tab to the front for design review.
            if let Ok(name) = std::env::var("SYMPHONY_SHOT_VIEW") {
                if let Some(view) = View::ALL.into_iter().find(|view| view.title() == name) {
                    for (_, dock) in layouts.iter_mut() {
                        if let Some(found) = dock.find_tab(&view) {
                            let _ = dock.set_active_tab(found);
                        }
                    }
                }
            }
            // SYMPHONY_SHOT_BARS=<aspect>,<radius>: black bars, for review.
            if let Ok(v) = std::env::var("SYMPHONY_SHOT_BARS") {
                let mut it = v.split(',').map(|x| x.trim().parse::<f32>().unwrap_or(0.0));
                s.doc.settings.bars = symphony_core::doc::Bars {
                    on: true,
                    aspect: it.next().unwrap_or(0.0),
                    radius: it.next().unwrap_or(0.0),
                };
            }
            // SYMPHONY_SHOT_RANGE: a playback range (frames 10 to 30).
            if std::env::var_os("SYMPHONY_SHOT_RANGE").is_some() {
                s.play_range = Some((10, 30));
            }
            // SYMPHONY_SHOT_LIGHT: onion skin and light table on, for review.
            if std::env::var_os("SYMPHONY_SHOT_LIGHT").is_some() {
                s.onion.on = true;
                s.light_table = true;
            }
            // SYMPHONY_SHOT_TOOL=Brush|Pencil: review that tool's properties.
            match std::env::var("SYMPHONY_SHOT_TOOL").as_deref() {
                Ok("Brush") => s.tool = Tool::Brush,
                Ok("Pencil") => s.tool = Tool::Pencil,
                Ok("Eraser") => s.tool = Tool::Eraser,
                Ok("Text") => s.tool = Tool::Text,
                _ => {}
            }
            // SYMPHONY_SHOT_TEXT=<words>: type them with the Text tool.
            if let Ok(words) = std::env::var("SYMPHONY_SHOT_TEXT") {
                s.text_style.size = 140.0;
                s.text_style.bold = true;
                let (layer, frame) = (s.selected, s.frame);
                s.add_text(layer, frame, [-520.0, -380.0], &words);
            }
            // SYMPHONY_SHOT_WARP: a Mesh Warp on the selected layer with
            // its middle pulled, selected for editing.
            if std::env::var_os("SYMPHONY_SHOT_WARP").is_some() {
                if let Some(node) = s.doc.layers.get(s.selected).map(|l| l.node) {
                    let (w, h) = (s.doc.settings.width, s.doc.settings.height);
                    views::node::insert_near(&mut s, node, views::node::mesh_warp(w, h));
                    let points = std::env::var("SYMPHONY_SHOT_WARP").is_ok_and(|v| v == "points");
                    // The artwork's box, for placing points on it.
                    let bounds = s
                        .doc
                        .exposed(s.selected, s.frame)
                        .and_then(|d| s.doc.drawings.get(&d))
                        .and_then(|d| symphony_vector::bounds_of(&d.strokes));
                    if let Some(id) = s.doc.graph.input(node, 0) {
                        if let Some(n) = s.doc.graph.node_mut(id) {
                            if let symphony_core::graph::NodeKind::MeshWarp(m) = &mut n.kind {
                                if let (true, Some((lo, hi))) = (points, bounds) {
                                    let cx = (lo[0] + hi[0]) / 2.0;
                                    m.kind = symphony_core::warp::WarpKind::Points;
                                    m.lines = symphony_core::warp::WarpLines::Shape;
                                    m.pins = vec![[cx, hi[1] - 4.0], [cx, lo[1] + 4.0]];
                                    m.pin_moves = vec![
                                        [
                                            symphony_core::anim::Value::Constant(0.0),
                                            symphony_core::anim::Value::Constant(0.0),
                                        ];
                                        2
                                    ];
                                } else {
                                    m.points[12] = [
                                        symphony_core::anim::Value::Constant(90.0),
                                        symphony_core::anim::Value::Constant(-120.0),
                                    ];
                                }
                            }
                        }
                        s.select_graph_node(id, false);
                        s.fit_tool_to_context();
                        if points && views::warp_edit::build_mesh(&mut s, id).is_ok() {
                            if let Some(n) = s.doc.graph.node_mut(id) {
                                if let symphony_core::graph::NodeKind::MeshWarp(m) = &mut n.kind {
                                    m.pin_moves[1] = [
                                        symphony_core::anim::Value::Constant(140.0),
                                        symphony_core::anim::Value::Constant(30.0),
                                    ];
                                }
                            }
                        }
                    }
                }
            }
            if std::env::var_os("SYMPHONY_SHOT_PAINT").is_some() {
                s.tool = Tool::Paint;
                if let Some(id) = s.doc.palette.swatches.get(1).map(|sw| sw.id) {
                    s.toggle_protect_swatch(id);
                }
            }
        }
        let recovery = recovery_dir().map(Recovery::new);
        // SYMPHONY_PERF_EDITOR: straight into the editor, for measuring it.
        let welcome = ((shot.is_none() && std::env::var_os("SYMPHONY_PERF_EDITOR").is_none())
            || std::env::var_os("SYMPHONY_SHOT_WELCOME").is_some())
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
            perf_run: perf::Run::from_env(),
            perf_open: std::env::var_os("SYMPHONY_PERF_OPEN").map(PathBuf::from),
            tool_dock,
            tool_options_open: false,
            save_dialog: std::env::var_os("SYMPHONY_SHOT_SAVE").map(|_| {
                (
                    save_dialog::SaveDialog::new("Untitled scene".into(), None),
                    None,
                )
            }),
            inbetween_count: 1,
            welcome,
            recent_projects,
            recovery_scan,
            wallpaper: wallpaper::Wallpaper::new(&cc.egui_ctx, wallpaper_choice, wallpaper_blur),
            interface_animations,
            interface_effects,
            s,
            workspace,
            layouts,
            profile,
            use_no_ai,
            pen,
            camera: {
                // SYMPHONY_SHOT_MIRROR: the Camera view mirrored, for review.
                let mut c = camera::CanvasState::default();
                c.mirrored = std::env::var_os("SYMPHONY_SHOT_MIRROR").is_some();
                c
            },
            drawing: Default::default(),
            function: Default::default(),
            palette: None,
            hovered_view: None,
            shot_search_opened: false,
            search_uses,
            node_view: Default::default(),
            timing: Default::default(),
            export: Default::default(),
            maximised: None,
            show_preferences: std::env::var_os("SYMPHONY_SHOT_PREFS").is_some(),
            // SYMPHONY_SHOT_PREFS=Canvas opens that page in screenshot runs.
            prefs_page: std::env::var("SYMPHONY_SHOT_PREFS")
                .ok()
                .and_then(|v| PrefsPage::ALL.into_iter().find(|p| format!("{p:?}") == v))
                .unwrap_or(PrefsPage::Interface),
            show_icon_sheet: std::env::var_os("SYMPHONY_SHOT_ICONS").is_some(),
            last_time: None,
            frame_clock: 0.0,
            notice_shown: None,
            was_autosaving: false,
            last_tool_state: (Tool::Brush, 0.0),
            cursor_hint: None,
            shot,
            recovery,
            // SYMPHONY_SHOT_RECOVER: show the recovery dialog, for review.
            recovered: if shot_recover {
                vec![(String::new(), None, Scene::new(Default::default()))]
            } else {
                Vec::new()
            },
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
            image_import: None,
            image_import_asking: None,
            import_options,
            sound_import: None,
            video_import: None,
            player: Default::default(),
            shot_imported: false,
            show_licences: false,
            crash_report: crash_offer,
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
        let detail = self.s.stroke_detail;
        let colours = self.s.colours;
        let (brush, pencil, eraser) = (self.s.brush, self.s.pencil, self.s.eraser);
        let eraser_tip = self.s.eraser_tip;
        self.s = next;
        self.s.pressure_gamma = gamma;
        self.s.brush_size_cursor = cursor;
        self.s.overlay_underlay = layers;
        self.s.size_follows_zoom = follows;
        self.s.stroke_detail = detail;
        self.s.colours = colours;
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

    fn show_save_dialog(&mut self, next: Option<Pending>) {
        self.save_dialog = Some((
            save_dialog::SaveDialog::new(self.s.name(), self.s.path.as_deref()),
            next,
        ));
    }

    fn save(&mut self, save_as: bool) {
        if let Some(path) = self.s.path.clone().filter(|_| !save_as) {
            self.save_to(path, false);
        } else {
            self.show_save_dialog(None);
        }
    }

    fn save_to(&mut self, path: PathBuf, create: bool) {
        // Snapshot, then write in the background (E5.3, E6).
        let (doc, version, tx) = (self.s.doc.clone(), self.s.doc.version, self.save_tx.clone());
        std::thread::spawn(move || {
            let r = save_project(&doc, &path, create);
            let _ = tx.send((version, path, r));
        });
        self.saving_jobs += 1;
    }

    /// Save synchronously. Only used right before quitting or replacing the scene.
    fn save_now(&mut self, path: PathBuf, create: bool) -> bool {
        match save_project(&self.s.doc, &path, create) {
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
                self.error = Some(e);
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
        let len = self.s.length().max(1);
        while self.frame_clock >= step {
            self.frame_clock -= step;
            match next_play_frame(self.s.frame, self.s.play_range, len, self.s.loop_playback) {
                Some(f) => self.s.frame = f,
                None => {
                    self.s.playing = false;
                    self.frame_clock = 0.0;
                    break;
                }
            }
        }
        ctx.request_repaint();
    }

    fn run(&mut self, ctx: &egui::Context, action: Action) {
        match action {
            Action::NewScene => self.guard(ctx, Pending::New),
            Action::Open => self.guard(ctx, Pending::Open),
            Action::Save => self.save(false),
            Action::SaveAs => self.save(true),
            Action::Import => self.import_dialog(ctx),
            Action::ZoomIn
            | Action::ZoomOut
            | Action::RotateCw
            | Action::RotateCcw
            | Action::ResetZoom
            | Action::ResetRotation
            | Action::ResetPan
            | Action::ResetView
            | Action::MirrorView => {
                let pointer = ctx.pointer_hover_pos();
                self.camera.apply_at(action, pointer);
                self.drawing.apply_at(action, pointer);
            }
            Action::Search => {
                // Tab keeps moving keyboard focus between controls when one
                // has it; otherwise it opens search next to the mouse.
                if ctx.memory(|mem| mem.focused().is_none()) && self.palette.is_none() {
                    let at = ctx
                        .pointer_hover_pos()
                        .unwrap_or_else(|| ctx.content_rect().center());
                    let here = self.hovered_view.or_else(|| {
                        self.layout_mut()
                            .find_active_focused()
                            .map(|(_, view)| *view)
                    });
                    self.palette = Some(views::palette::PaletteState::new(at, here));
                }
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
                    Action::SetPlayStart => {
                        let end = s.play_range.map_or(s.length(), |r| r.1);
                        s.play_range = Some((s.frame, end.max(s.frame)));
                        s.note(format!("Playback starts at frame {}", s.frame));
                    }
                    Action::SetPlayEnd => {
                        let start = s.play_range.map_or(1, |r| r.0);
                        s.play_range = Some((start.min(s.frame), s.frame));
                        s.note(format!("Playback ends at frame {}", s.frame));
                    }
                    Action::ClearPlayRange => {
                        s.play_range = None;
                        s.note("Playing the whole scene");
                    }
                    Action::ToggleLoop => {
                        s.loop_playback = !s.loop_playback;
                        s.note(if s.loop_playback {
                            "Loop on"
                        } else {
                            "Loop off"
                        });
                    }
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
        if self.palette.is_none() && !ctx.text_edit_focused() {
            let here = self.hovered_view.or_else(|| {
                self.layout_mut()
                    .find_active_focused()
                    .map(|(_, view)| *view)
            });
            if here == Some(View::Timeline)
                && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::K))
            {
                views::timing::TimelineAction::SetKey.run(&mut self.s);
            }
        }
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        if self.palette.is_none() && self.s.tool == Tool::Select {
            self.s.validate_selection();
            if !self.s.selection.strokes.is_empty() {
                if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::F)) {
                    self.s.fill_selection();
                }
                if ctx.input_mut(|i| {
                    i.consume_key(egui::Modifiers::NONE, egui::Key::Backspace)
                        || i.consume_key(egui::Modifiers::NONE, egui::Key::Delete)
                }) {
                    self.s.delete_selection();
                }
            }
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
            self.menu_item(ui, Action::Import, "Import…".into());
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
            ui.horizontal(|ui| {
                ui.label("Scene frame rate");
                let mut fps = self.s.doc.settings.fps;
                symphony_ui::widgets::frame_rate(ui, "timing-menu-fps", &mut fps);
                self.s.set_frame_rate(fps);
            });
            ui.menu_button("Insert in-betweens", |ui| {
                let remaining = 100_000u32.saturating_sub(self.s.length());
                ui.add_enabled(
                    remaining > 0,
                    egui::DragValue::new(&mut self.inbetween_count)
                        .range(1..=remaining.max(1))
                        .suffix(" frames"),
                );
                if symphony_ui::widgets::primary_button(ui, "Insert after playhead", remaining > 0)
                    .clicked()
                {
                    self.s
                        .insert_inbetweens(self.inbetween_count.min(remaining));
                    ui.close();
                }
            });
            ui.separator();
            views::timing::TimelineAction::SetKey.menu(ui, &mut self.s);
            views::timing::TimelineAction::Duplicate.menu(ui, &mut self.s);
            views::timing::TimelineAction::InsertNew.menu(ui, &mut self.s);
            views::timing::TimelineAction::Extend.menu(ui, &mut self.s);
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
            ui.separator();
            self.menu_action(ui, Action::SetPlayStart);
            self.menu_action(ui, Action::SetPlayEnd);
            self.menu_action(ui, Action::ClearPlayRange);
            self.menu_action(ui, Action::ToggleLoop);
            self.menu_action(ui, Action::PrevFrame);
            self.menu_action(ui, Action::NextFrame);
            self.menu_action(ui, Action::FirstFrame);
            self.menu_action(ui, Action::LastFrame);
        });
        ui.menu_button("View", |ui| {
            self.menu_action(ui, Action::ToggleOnionSkin);
            let on = self.s.show_strokes;
            self.menu_toggle(ui, Action::ShowStrokes, on);
            ui.checkbox(&mut self.camera.render_effects, "Render Camera Effects");
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
            self.menu_action(ui, Action::MirrorView);
            ui.separator();
            if ui.button("Arrange Nodes").clicked() {
                views::node::arrange(&mut self.s, &mut self.node_view);
                ui.close();
            }
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
            // A developer tool, only in debug builds.
            if cfg!(debug_assertions) && ui.button("Icon sheet").clicked() {
                self.show_icon_sheet = true;
                ui.close();
            }
            ui.separator();
            if ui
                .button("Update Symphony…")
                .on_hover_text("Check for a newer Symphony and install it")
                .clicked()
            {
                ui.close();
                self.guard(ui.ctx(), Pending::Update);
            }
            ui.separator();
            ui.label(format!(
                "v{} ({})",
                env!("CARGO_PKG_VERSION"),
                env!("SYMPHONY_BUILD")
            ));
        });
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

    fn tools_panel(&mut self, ui: &mut egui::Ui) {
        let dock = self.tool_dock;
        let workspace = ui.available_rect_before_wrap();
        let panel = match dock {
            ToolDock::Left => egui::Panel::left("tools-left"),
            ToolDock::Right => egui::Panel::right("tools-right"),
            ToolDock::Top => egui::Panel::top("tools-top"),
            ToolDock::Bottom => egui::Panel::bottom("tools-bottom"),
        };
        let gap = theme::GUTTER as i8;
        let margins = match dock {
            ToolDock::Left => egui::Margin {
                left: gap,
                right: 0,
                top: gap,
                bottom: gap,
            },
            ToolDock::Right => egui::Margin {
                left: 0,
                right: gap,
                top: gap,
                bottom: gap,
            },
            ToolDock::Top => egui::Margin {
                left: gap,
                right: gap,
                top: gap,
                bottom: 0,
            },
            ToolDock::Bottom => egui::Margin {
                left: gap,
                right: gap,
                top: 0,
                bottom: gap,
            },
        };
        panel
            .show_separator_line(false)
            .frame(egui::Frame::new().inner_margin(margins))
            .exact_size(theme::TOOLS_STRIP + theme::GUTTER)
            .resizable(false)
            .show(ui, |ui| self.tools_strip(ui, dock.horizontal(), workspace));
    }

    /// A grip moves the toolbar between window edges. The context menu
    /// offers the same docking choices for keyboard and pen users.
    fn tools_strip(&mut self, ui: &mut egui::Ui, horizontal: bool, workspace: egui::Rect) {
        let frame = egui::Frame::new()
            .fill(theme::GLASS_PANEL)
            .corner_radius(theme::GLASS_RADIUS)
            .inner_margin(egui::Margin::same(theme::GLASS_TOOL_INSET))
            .show(ui, |ui| {
                let layout = if horizontal {
                    ui.set_min_width(ui.available_width());
                    egui::Layout::left_to_right(egui::Align::Center)
                } else {
                    ui.set_min_height(ui.available_height());
                    egui::Layout::top_down(egui::Align::Center)
                };
                ui.with_layout(layout, |ui| {
                    let size = if horizontal {
                        egui::vec2(16.0, 28.0)
                    } else {
                        egui::vec2(26.0, 15.0)
                    };
                    let (rect, grip) = ui.allocate_exact_size(size, egui::Sense::click_and_drag());
                    let grip = grip.on_hover_cursor(egui::CursorIcon::Grab);
                    for primary in [-3.0, 3.0] {
                        for cross in [-5.0, 0.0, 5.0] {
                            let offset = if horizontal {
                                egui::vec2(primary, cross)
                            } else {
                                egui::vec2(cross, primary)
                            };
                            ui.painter().circle_filled(
                                rect.center() + offset,
                                1.1,
                                theme::TEXT_SECONDARY,
                            );
                        }
                    }
                    let grip = grip.on_hover_text(
                        "Drag Tools to a window edge, or right-click to choose a side",
                    );
                    if let Some(pos) = grip
                        .interact_pointer_pos()
                        .or_else(|| ui.ctx().pointer_latest_pos())
                        .filter(|_| grip.dragged() || grip.drag_stopped())
                    {
                        let side = ToolDock::nearest(workspace, pos);
                        if grip.drag_stopped() {
                            self.tool_dock = side;
                            ui.ctx().request_repaint();
                        } else {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
                            let preview = side.preview(workspace);
                            let painter = ui.ctx().layer_painter(egui::LayerId::new(
                                egui::Order::Foreground,
                                egui::Id::unique("tools-dock-preview"),
                            ));
                            painter.rect(
                                preview,
                                theme::GLASS_RADIUS,
                                theme::ACCENT_SELECT.gamma_multiply(0.18),
                                egui::Stroke::new(1.0, theme::ACCENT_SELECT),
                                egui::StrokeKind::Inside,
                            );
                            painter.text(
                                preview.center(),
                                egui::Align2::CENTER_CENTER,
                                "Tools",
                                theme::dense(),
                                theme::TEXT_PRIMARY,
                            );
                        }
                    }
                    grip.context_menu(|ui| {
                        for side in ToolDock::ALL {
                            if ui
                                .selectable_value(&mut self.tool_dock, side, side.label())
                                .clicked()
                            {
                                ui.close();
                            }
                        }
                    });
                    let scroll = if horizontal {
                        egui::ScrollArea::horizontal()
                    } else {
                        egui::ScrollArea::vertical()
                    };
                    scroll
                        .id_salt("tools-buttons")
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            ui.with_layout(layout, |ui| self.tool_buttons(ui));
                        });
                });
            });
        theme::glass_rim(ui.painter(), frame.response.rect);
    }

    /// The current tool and its size (0 for tools without one).
    fn tool_state(&self) -> (Tool, f32) {
        let size = match self.s.tool {
            Tool::Brush => self.s.brush.max_width,
            Tool::Pencil => self.s.pencil.max_width,
            Tool::Eraser => self.s.eraser.max_width,
            _ => 0.0,
        };
        (self.s.tool, size)
    }

    /// Switching tool or changing its size while over the canvas shows the
    /// tool's name or the new size beside the cursor for a moment: your
    /// eyes are on the drawing, not the toolbar.
    fn cursor_feedback(&mut self, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        let state = self.tool_state();
        let (old_tool, old_size) = self.last_tool_state;
        self.last_tool_state = state;
        // Nothing to compare with on the first frames.
        let over_canvas =
            now > 0.5 && matches!(self.hovered_view, Some(View::Camera | View::Drawing));
        if over_canvas && state.0 != old_tool {
            let name = keymap::Action::ALL
                .into_iter()
                .find(|a| tool_action(state.0) == *a)
                .map_or("Tool", |a| a.name());
            let name = if self.s.pen_modes && matches!(state.0, Tool::Brush | Tool::Pencil) {
                format!("Pen: {name}")
            } else {
                name.to_owned()
            };
            self.cursor_hint = Some((name, now));
        } else if over_canvas
            && state.0 == old_tool
            && state.1 > 0.0
            && (state.1 - old_size).abs() > 0.01
        {
            self.cursor_hint = Some((format!("Size {:.1}", state.1), now));
        }
        let Some((text, since)) = self.cursor_hint.as_ref() else {
            return;
        };
        const HOLD: f64 = 0.8;
        let remaining = HOLD - (now - since);
        let shown = remaining > 0.0;
        let opacity = symphony_ui::motion::presence(
            ctx,
            egui::Id::unique("symphony-cursor-hint").with("presence"),
            shown,
        );
        if shown {
            // The shared motion helper requests frames during a fade.
            // The steady part only needs a wake-up when the hint expires.
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(remaining));
        } else if opacity <= 0.0 {
            self.cursor_hint = None;
            return;
        }
        let Some(p) = ctx.pointer_hover_pos() else {
            return;
        };
        egui::Area::new(egui::Id::unique("symphony-cursor-hint"))
            .order(egui::Order::Tooltip)
            .fixed_pos(p + egui::vec2(18.0, 14.0))
            .interactable(false)
            .fade_in(false)
            .show(ctx, |ui| {
                ui.multiply_opacity(opacity);
                egui::Frame::new()
                    .fill(theme::GLASS_FLOATING)
                    .corner_radius(theme::RADIUS_CONTROL)
                    .inner_margin(egui::Margin::symmetric(8, 3))
                    .show(ui, |ui| {
                        ui.label(
                            egui::RichText::new(text)
                                .font(theme::dense())
                                .color(theme::TEXT_PRIMARY),
                        );
                    });
            });
    }

    fn tool_buttons(&mut self, ui: &mut egui::Ui) {
        const TOOLS: [Tool; 12] = [
            Tool::Select,
            Tool::Transform,
            Tool::Brush,
            Tool::Line,
            Tool::Text,
            Tool::Eraser,
            Tool::LineEditor,
            Tool::Paint,
            Tool::CloseGap,
            Tool::Dropper,
            Tool::Hand,
            Tool::Zoom,
        ];
        let context = self.s.tool_context();
        ui.spacing_mut().item_spacing = egui::Vec2::splat(2.0);
        for tool in TOOLS.into_iter().filter(|tool| tool.fits(context)) {
            let mut tip = keymap::tooltip(self.profile, tool_action(tool));
            // Select shows that it also transforms what it selected.
            let icon = match tool {
                Tool::Select if !self.s.selection.strokes.is_empty() => {
                    tip.push_str("\nDrag, scale or rotate the selection");
                    Icon::Transform
                }
                // Both tips make vector strokes. The icon shows whether
                // the current brush uses a texture.
                Tool::Brush if !crate::views::panels::is_bitmap(&self.s.brush) => Icon::Pen,
                Tool::Brush => Icon::Brush,
                Tool::Transform => {
                    tip = format!("Transform the peg\n{tip}");
                    Icon::Transform
                }
                _ => tool_icon(tool),
            };
            if let Some(milestone) = tool.planned_for() {
                tip.push_str(&format!("\nComing in {milestone}"));
            }
            let response = icons::tool_button(
                ui,
                icon,
                self.s.tool == tool,
                tool.planned_for().is_none(),
                &tip,
            );
            if response.clicked() {
                self.s.tool = tool;
            }
            if response.double_clicked() {
                self.s.tool = tool;
                self.open_tool_properties();
            }
        }
    }

    /// Show Tool Properties: its docked tab if there is one, or the
    /// floating window.
    fn open_tool_properties(&mut self) {
        if self.layout_mut().find_tab(&View::ToolProperties).is_some() {
            self.maximised = None;
            self.show_view(View::ToolProperties);
            self.tool_options_open = false;
        } else {
            self.tool_options_open = true;
        }
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
                format!(
                    "{} × {} · {} fps",
                    s.doc.settings.width, s.doc.settings.height, s.doc.settings.fps
                ),
                theme::TEXT_SECONDARY,
            );
            if s.doc.column(s.selected).is_some_and(|column| column.locked) {
                dense(ui, "Layer locked".into(), theme::TEXT_PRIMARY);
            }
            if s.tool == Tool::Select && !s.selection.strokes.is_empty() {
                dense(
                    ui,
                    format!("{} selected", s.selection.strokes.len()),
                    theme::TEXT_PRIMARY,
                );
            }
            if s.animate {
                dense(ui, "Auto-key".into(), theme::ACCENT_TIME);
            }
            let notice = s.notice.clone();
            // Notices fade in (Appear), hold, then fade out (Dismiss) and
            // clear, so the bar only ever shows something new.
            const HOLD: f64 = 3.5;
            let now = ui.input(|i| i.time);
            let notice = match (&notice, &self.notice_shown) {
                (Some(n), Some((shown, _))) if n == shown => notice,
                (Some(n), _) => {
                    self.notice_shown = Some((n.clone(), now));
                    notice
                }
                (None, _) => {
                    self.notice_shown = None;
                    None
                }
            };
            let notice_opacity = self.notice_shown.as_ref().map_or(1.0, |(_, since)| {
                use symphony_ui::motion::Motion;
                let age = now - since;
                let fade_in = Motion::Appear.ease((age / Motion::Appear.seconds()) as f32);
                let out = ((age - HOLD) / (Motion::Dismiss.seconds() * 3.0)).clamp(0.0, 1.0) as f32;
                if age < HOLD + Motion::Dismiss.seconds() * 3.0 {
                    ui.ctx()
                        .request_repaint_after(std::time::Duration::from_millis(30));
                }
                (fade_in * (1.0 - Motion::Dismiss.ease(out))).clamp(0.0, 1.0)
            });
            let expired = self
                .notice_shown
                .as_ref()
                .is_some_and(|(_, since)| now - since > HOLD + 0.4);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if device.is_some_and(|pen| pen.in_proximity) {
                    dense(ui, "Pen in range".into(), theme::STATE_OK);
                }

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
                if let Some(v) = &self.video_import {
                    jobs.push((format!("Importing {}", v.name), None));
                }
                if let Some(i) = &self.sound_import {
                    jobs.push((
                        if i.files == 1 {
                            "Importing sound".into()
                        } else {
                            format!("Importing {} sounds", i.files)
                        },
                        None,
                    ));
                }
                if let Some(i) = &self.image_import {
                    jobs.push((
                        if i.files == 1 {
                            "Importing an image".into()
                        } else {
                            format!("Importing {} images", i.files)
                        },
                        None,
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
                if let Some(n) = notice.filter(|_| !expired) {
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        ui.multiply_opacity(notice_opacity);
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
                        pen: Default::default(),
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
    /// Everything search can find, built fresh when it opens or the query
    /// changes (it's small).
    fn search_entries(&self) -> Vec<search::Entry> {
        use search::{Command, Entry, Kind, Setting};
        use symphony_core::stroke::ArtLayer;
        let canvas = [View::Camera, View::Drawing];
        let mut out: Vec<Entry> = Vec::new();
        let node_views = vec![View::Node, View::NodeLibrary];
        out.push(
            Entry::new(
                "Arrange Nodes",
                Kind::Node,
                Command::ArrangeNodes,
                "node.arrange",
            )
            .words("auto arrange layout tidy graph")
            .ctx(&node_views),
        );
        if let Some(id) = views::node::selected_node(&self.s) {
            if let Some(label) = views::node::smart_joint_label(&self.s, id) {
                out.push(
                    Entry::new(
                        label,
                        Kind::Node,
                        Command::ToggleSmartJoint(id),
                        "node.smart-joint",
                    )
                    .words("inverse kinematics ik bone rig elbow knee")
                    .ctx(&node_views),
                );
            }
            if let Some(label) = views::node::drawing_conversion_label(&self.s, id) {
                out.push(
                    Entry::new(
                        label,
                        Kind::Node,
                        Command::ConvertDrawing(id),
                        "node.convert-drawing",
                    )
                    .words("convert change drawing mask matte regular artwork")
                    .ctx(&node_views),
                );
            }
        }
        for kind in views::node::addable_nodes() {
            let name = views::node::kind_label(&kind);
            use symphony_core::graph::{Effect, NodeKind};
            let aliases = match kind {
                NodeKind::Skeleton(_) => "node create start rig skeleton puppet",
                NodeKind::Bone(_) => "node create extend rig bone limb joint",
                NodeKind::Effect(Effect::Blur { .. }) =>
                    "node create insert effect box gaussian motion directional radial spin zoom blur",
                _ => "node create insert effect",
            };
            let mut entry = Entry::new(
                &format!("Add {name}"),
                Kind::Node,
                Command::AddNode(kind),
                &format!("node.add.{name}"),
            )
            .words(aliases);
            entry.context.clone_from(&node_views);
            out.push(entry);
        }
        // Every blend mode is its own search result ("subtract", "glow",
        // "shadow"...), adding a Blend Modes node already set to it.
        for mode in symphony_core::graph::BlendMode::ALL {
            let kind =
                symphony_core::graph::NodeKind::Effect(symphony_core::graph::Effect::Blend {
                    mode,
                    amount: 1.0,
                });
            let mut entry = Entry::new(
                &format!("Blend mode: {}", mode.title()),
                Kind::Node,
                Command::AddNode(kind),
                &format!("node.add.blend.{}", mode.title()),
            )
            .words(&format!(
                "blend modes layer mode composite {}",
                mode.words()
            ));
            entry.context.clone_from(&node_views);
            out.push(entry);
        }
        let mut drawing = Entry::new(
            "Add Drawing node",
            Kind::Node,
            Command::AddDrawingNode,
            "node.add.drawing",
        )
        .words("create insert drawing layer");
        drawing.context = node_views;
        out.push(drawing);
        for a in Action::ALL {
            if a == Action::Search {
                continue;
            }
            let id = a.id();
            let (kind, ctx): (Kind, Vec<View>) = match id.split('.').next().unwrap_or("") {
                "tool" => (
                    Kind::Tool,
                    vec![View::Camera, View::Drawing, View::ToolProperties],
                ),
                "timing" | "frame" | "playback" | "layer" => {
                    (Kind::Action, vec![View::Timeline, View::Xsheet])
                }
                "view" | "drawing" | "onion" => (Kind::Action, canvas.to_vec()),
                _ => (Kind::Action, Vec::new()),
            };
            let mut e = Entry::new(a.name(), kind, Command::Action(a), id)
                .words(&id.replace(['.', '_'], " "))
                .ctx(&ctx);
            e.shortcut = keymap::label(self.profile, a);
            out.push(e);
        }
        let timing_views = [View::Timeline, View::Xsheet];
        for action in views::timing::TimelineAction::ALL {
            let mut entry = Entry::new(
                action.label(),
                Kind::Action,
                Command::Timeline(action),
                &format!("timeline.{action:?}"),
            )
            .words(action.keywords())
            .ctx(&timing_views);
            if action == views::timing::TimelineAction::SetKey {
                entry.shortcut = "K in Timeline".into();
            }
            out.push(entry);
        }
        for (id, name) in self.s.element_drawings(self.s.selected) {
            out.push(
                Entry::new(
                    &format!("Reuse Drawing {name}"),
                    Kind::Action,
                    Command::ReuseDrawing(id),
                    &format!("timeline.reuse.{}", id.0),
                )
                .words("insert existing drawing exposure frame")
                .ctx(&timing_views),
            );
        }
        for fps in [12.0, 24.0, 30.0, 48.0] {
            out.push(
                Entry::new(
                    &format!("Set Scene to {fps} FPS"),
                    Kind::Setting,
                    Command::SetFrameRate(fps),
                    &format!("timing.fps.{fps}"),
                )
                .words("frame rate playback speed")
                .ctx(&timing_views),
            );
        }
        for (title, command, key, words, views) in [
            (
                "Open Node Properties",
                Command::ShowNodeProperties,
                "node.properties",
                "inspect layer settings",
                &[View::Node, View::LayerProperties][..],
            ),
            (
                "Toggle Camera Effects",
                Command::ToggleCameraEffects,
                "camera.effects",
                "render camera preview draft bypass effects",
                &[View::Camera][..],
            ),
            (
                "Render Node Preview",
                Command::RenderNodePreview,
                "node.preview",
                "display camera effects",
                &[View::Node, View::Camera][..],
            ),
            (
                "Reset Node View",
                Command::ResetNodeView,
                "node.reset",
                "reset pan zoom",
                &[View::Node][..],
            ),
            (
                "Add Colour Swatch",
                Command::AddSwatch,
                "colour.add",
                "new color palette",
                &[View::Colour][..],
            ),
            (
                "Toggle Auto-key",
                Command::ToggleAnimate,
                "timing.animate",
                "auto key animation animate mode",
                &[View::LayerProperties, View::Timeline][..],
            ),
            (
                "Reset This Workspace",
                Command::ResetWorkspace,
                "workspace.reset",
                "restore panels default docking",
                &[][..],
            ),
            (
                "Update Symphony",
                Command::UpdateApp,
                "app.update",
                "rebuild latest version",
                &[][..],
            ),
        ] {
            out.push(
                Entry::new(title, Kind::Action, command, key)
                    .words(words)
                    .ctx(views),
            );
        }
        // Views: bring a tab to the front.
        for v in View::ALL {
            out.push(
                Entry::new(
                    &format!("{} view", v.title()),
                    Kind::View,
                    Command::ShowView(v),
                    &format!("view.show.{v:?}"),
                )
                .words(if v == View::Multiplane {
                    "window panel tab parallax depth side staging"
                } else {
                    "window panel tab"
                }),
            );
        }
        // Export, by format.
        for (title, f, w) in [
            (
                "Export movie (MP4)",
                symphony_render::Format::Mp4Movie,
                "video h264 mp4 film",
            ),
            (
                "Export GIF",
                symphony_render::Format::AnimatedGif,
                "animated loop",
            ),
            (
                "Export image sequence (PNG)",
                symphony_render::Format::PngSequence,
                "frames pictures still",
            ),
        ] {
            out.push(
                Entry::new(
                    title,
                    Kind::Action,
                    Command::OpenExport(f),
                    &format!("export.{f:?}"),
                )
                .words(w),
            );
        }
        // Settings.
        let settings: [(&str, Setting, &str); 10] = [
            (
                "Brush size cursor",
                Setting::BrushSizeCursor,
                "circle outline cursor",
            ),
            (
                "Brush size follows zoom",
                Setting::SizeFollowsZoom,
                "screen relative",
            ),
            (
                "Support Overlay and Underlay Arts",
                Setting::OverlayUnderlay,
                "art layers advanced",
            ),
            (
                "Interface animations",
                Setting::InterfaceAnimations,
                "motion reduce",
            ),
            (
                "Interface effects (blur, glass, shadows)",
                Setting::InterfaceEffects,
                "slow computer performance fast lighter low end blur glass shadow",
            ),
            ("Use no AI", Setting::UseNoAi, "ai machine learning assist"),
            (
                "Harmony shortcut profile",
                Setting::ProfileHarmony,
                "keys keyboard toon boom",
            ),
            (
                "Symphony shortcut profile",
                Setting::ProfileSymphony,
                "keys keyboard",
            ),
            (
                "Stroke detail (points kept per line)",
                Setting::StrokeDetail,
                "smooth points curve optimise jaggy wobbly",
            ),
            (
                "Stage colour",
                Setting::StageColour,
                "grey gray background canvas paper",
            ),
        ];
        for (title, s, w) in settings {
            out.push(
                Entry::new(
                    title,
                    Kind::Setting,
                    Command::Setting(s),
                    &format!("setting.{s:?}"),
                )
                .words(w),
            );
        }
        out.push(
            Entry::new(
                "Preferences",
                Kind::Setting,
                Command::OpenPreferences,
                "prefs",
            )
            .words("settings options"),
        );
        // Tool size and layer visibility.
        out.push(
            Entry::new(
                "Bigger tool size",
                Kind::Action,
                Command::ToolSizeBy(1.25),
                "size.up",
            )
            .words("increase thicker larger brush pencil eraser")
            .ctx(&[View::Camera, View::Drawing, View::ToolProperties]),
        );
        out.push(
            Entry::new(
                "Smaller tool size",
                Kind::Action,
                Command::ToolSizeBy(0.8),
                "size.down",
            )
            .words("decrease thinner finer brush pencil eraser")
            .ctx(&[View::Camera, View::Drawing, View::ToolProperties]),
        );
        out.push(
            Entry::new(
                "Hide or show the selected layer",
                Kind::Action,
                Command::ToggleLayerVisible,
                "layer.visible",
            )
            .words("visibility eye")
            .ctx(&[View::Timeline, View::Xsheet]),
        );
        // Art layers.
        let mut arts = vec![
            (ArtLayer::LineArt, "Line Art"),
            (ArtLayer::ColourArt, "Colour Art"),
        ];
        if self.s.overlay_underlay {
            arts.push((ArtLayer::Overlay, "Overlay Art"));
            arts.push((ArtLayer::Underlay, "Underlay Art"));
        }
        for (art, name) in arts {
            out.push(
                Entry::new(
                    &format!("Draw on {name}"),
                    Kind::Action,
                    Command::SetArt(art),
                    &format!("art.{art:?}"),
                )
                .words("art layer")
                .ctx(&canvas),
            );
        }
        // The scene's layers, colours and the user's presets.
        for i in 0..self.s.layer_count() {
            let name = self.s.layer_name(i);
            out.push(
                Entry::new(
                    &format!("Layer {name}"),
                    Kind::Layer,
                    Command::SelectLayer(i),
                    &format!("layer.{name}"),
                )
                .ctx(&[View::Timeline, View::Xsheet, View::Node]),
            );
        }
        for sw in &self.s.doc.palette.swatches {
            out.push(
                Entry::new(
                    &format!("Colour {}", sw.name),
                    Kind::Colour,
                    Command::SelectSwatch(sw.id),
                    &format!("colour.{}", sw.name),
                )
                .words("swatch palette")
                .ctx(&[View::Colour]),
            );
        }
        for (pencil, list) in [(false, &self.presets.brush), (true, &self.presets.pencil)] {
            for p in list {
                let tool = if pencil { "Pencil" } else { "Brush" };
                out.push(
                    Entry::new(
                        &format!("{tool} preset {}", p.name),
                        Kind::Preset,
                        Command::UsePreset {
                            pencil,
                            name: p.name.clone(),
                        },
                        &format!("preset.{tool}.{}", p.name),
                    )
                    .words(p.source.as_deref().unwrap_or(""))
                    .ctx(&[View::ToolProperties]),
                );
            }
        }
        out
    }

    /// Show the search box when open, and run what is chosen.
    fn search_box(&mut self, ctx: &egui::Context) {
        let Some(mut st) = self.palette.take() else {
            return;
        };
        let entries = self.search_entries();
        let uses = self.search_uses.clone();
        match views::palette::ui(
            ctx,
            &mut st,
            &entries,
            &|k| uses.get(k).copied().unwrap_or(0),
            self.profile,
        ) {
            views::palette::Outcome::Open | views::palette::Outcome::Closing => {
                self.palette = Some(st)
            }
            views::palette::Outcome::Closed => {}
            views::palette::Outcome::Chosen(cmd, key) => {
                *self.search_uses.entry(key).or_insert(0) += 1;
                self.run_command(ctx, cmd);
                // Fade out after running it.
                st.closing = true;
                self.palette = Some(st);
            }
        }
    }

    fn run_command(&mut self, ctx: &egui::Context, cmd: search::Command) {
        use search::{Command, Setting};
        match cmd {
            Command::Action(a) => self.run(ctx, a),
            Command::ShowView(v) => {
                self.maximised = None;
                self.show_view(v);
            }
            Command::OpenPreferences => self.show_preferences = true,
            Command::OpenExport(f) => self.export.open(f, &self.s),
            Command::Setting(s) => {
                let flip = |b: &mut bool, name: &str, notice: &mut Option<String>| {
                    *b = !*b;
                    *notice = Some(format!("{name}: {}", if *b { "on" } else { "off" }));
                };
                let notice = &mut self.s.notice;
                match s {
                    Setting::BrushSizeCursor => {
                        flip(&mut self.s.brush_size_cursor, "Brush size cursor", notice)
                    }
                    Setting::SizeFollowsZoom => flip(
                        &mut self.s.size_follows_zoom,
                        "Brush size follows zoom",
                        notice,
                    ),
                    Setting::OverlayUnderlay => flip(
                        &mut self.s.overlay_underlay,
                        "Overlay and Underlay Arts",
                        notice,
                    ),
                    Setting::InterfaceEffects => {
                        self.interface_effects = !self.interface_effects;
                        theme::set_effects(ctx, self.interface_effects);
                        theme::set_motion(ctx, self.interface_animations && self.interface_effects);
                    }
                    Setting::InterfaceAnimations => {
                        self.interface_animations = !self.interface_animations;
                        theme::set_motion(ctx, self.interface_animations && self.interface_effects);
                    }
                    Setting::UseNoAi => self.use_no_ai = !self.use_no_ai,
                    Setting::ProfileHarmony => self.profile = Profile::Harmony,
                    Setting::ProfileSymphony => self.profile = Profile::Symphony,
                    Setting::StrokeDetail => {
                        self.prefs_page = PrefsPage::Drawing;
                        self.show_preferences = true;
                    }
                    Setting::StageColour => {
                        self.prefs_page = PrefsPage::Canvas;
                        self.show_preferences = true;
                    }
                }
            }
            Command::GoToFrame(f) => self.s.frame = f.clamp(1, self.s.length()),
            Command::SetZoom(z) => {
                self.camera.set_zoom(z);
                self.drawing.set_zoom(z);
            }
            Command::SetToolSize(n) => self.set_tool_size(|_| n),
            Command::ToolSizeBy(k) => self.set_tool_size(|w| w * k),
            Command::Timeline(action) => action.run(&mut self.s),
            Command::InsertInbetweens(count) => self
                .s
                .insert_inbetweens(count.min(100_000u32.saturating_sub(self.s.length()))),
            Command::ReuseDrawing(id) => self
                .s
                .insert_exposure(crate::session::InsertExposure::Reuse(id)),
            Command::SetFrameRate(fps) => self.s.set_frame_rate(fps),
            Command::AddSwatch => self.s.add_swatch(),
            Command::ToggleAnimate => self.s.animate = !self.s.animate,
            Command::ShowNodeProperties => {
                views::node::show_selected_properties(&self.s, &mut self.node_view)
            }
            Command::ToggleCameraEffects => {
                self.camera.render_effects = !self.camera.render_effects
            }
            Command::RenderNodePreview => views::node::show_render_preview(&mut self.node_view),
            Command::ResetNodeView => views::node::reset_view(&mut self.node_view),
            Command::ResetWorkspace => {
                *self.layout_mut() = self.workspace.preset();
                self.maximised = None;
            }
            Command::UpdateApp => self.guard(ctx, Pending::Update),
            Command::SetLength(n) => self.s.set_length(n),
            Command::ToggleLayerVisible => {
                let i = self.s.selected;
                self.s.toggle_visible(i);
            }
            Command::SelectLayer(i) => self.s.selected = i,
            Command::SelectSwatch(id) => self.s.swatch = id,
            Command::UsePreset { pencil, name } => {
                let list = if pencil {
                    &self.presets.pencil
                } else {
                    &self.presets.brush
                };
                if let Some(p) = list.iter().find(|p| p.name == name).cloned() {
                    if pencil {
                        self.s.pencil = p.settings;
                        self.s.tool = Tool::Pencil;
                    } else {
                        self.s.brush = p.settings;
                        self.s.tool = Tool::Brush;
                    }
                    let tool = if pencil {
                        presets::PresetTool::Pencil
                    } else {
                        presets::PresetTool::Brush
                    };
                    self.presets.set_current(tool, Some(name));
                }
            }
            Command::SetArt(a) => self.s.set_art(a),
            Command::ArrangeNodes => views::node::arrange(&mut self.s, &mut self.node_view),
            Command::ConvertDrawing(id) => views::node::convert_drawing(&mut self.s, id),
            Command::ToggleSmartJoint(id) => views::node::toggle_smart_joint(&mut self.s, id),
            Command::AddDrawingNode => {
                self.s.add_layer();
                if let Some(layer) = self.s.doc.layers.get(self.s.selected) {
                    self.s.select_graph_node(layer.node, true);
                }
            }
            Command::AddNode(kind) => {
                views::node::add_from_search(&mut self.s, &mut self.node_view, kind)
            }
        }
    }

    /// Change the current drawing tool's maximum size (minimum follows).
    fn set_tool_size(&mut self, f: impl Fn(f32) -> f32) {
        let b = match self.s.tool {
            Tool::Pencil => &mut self.s.pencil,
            Tool::Eraser => &mut self.s.eraser,
            _ => &mut self.s.brush,
        };
        let old = b.max_width.max(0.01);
        let new = f(old).clamp(0.5, panels::MAX_TOOL_SIZE);
        b.min_width = (b.min_width * new / old).min(new);
        b.max_width = new;
        self.s.notice = Some(format!("Tool size {new:.1}"));
    }

    /// File > Import…: pick any files Symphony can bring in.
    fn import_dialog(&mut self, ctx: &egui::Context) {
        let files = rfd::FileDialog::new()
            .set_title("Import")
            .add_filter("Everything Symphony can import", &import::extensions())
            .pick_files();
        if let Some(paths) = files {
            self.import_paths(ctx, paths);
        }
    }

    /// Bring in any files, from Import… or dropped on the window: each goes
    /// to the importer for its kind.
    fn import_paths(&mut self, ctx: &egui::Context, paths: Vec<PathBuf>) {
        use import::Kind;
        let mut by_kind: std::collections::BTreeMap<u8, Vec<PathBuf>> = Default::default();
        let mut notes = Vec::new();
        for p in paths {
            let slot = match import::classify(&p) {
                Kind::Image => 0,
                Kind::Brush => 1,
                Kind::Preset => 2,
                Kind::Project => 3,
                Kind::HarmonyScene => 4,
                Kind::Sound => 5,
                Kind::Video => 6,
                Kind::Unknown => 7,
            };
            by_kind.entry(slot).or_default().push(p);
        }
        let name = |p: &PathBuf| {
            p.file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default()
        };
        for (slot, paths) in by_kind {
            match slot {
                0 => {
                    let only_svg = paths
                        .iter()
                        .all(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("svg")));
                    if self.image_import.is_some() || self.image_import_asking.is_some() {
                        notes.push(
                            "Still importing the last images. Try these again in a moment.".into(),
                        );
                    } else if self.shot.is_some()
                        && std::env::var_os("SYMPHONY_SHOT_IMPORT_ASK").is_none()
                    {
                        self.image_import = Some(import::ImageImport::finished(
                            paths,
                            import::ImportOptions {
                                fps: self.s.doc.settings.fps,
                                ..Default::default()
                            },
                        ));
                    } else if only_svg {
                        // Nothing to ask: SVG is always a vector drawing.
                        let opts = import::ImportOptions {
                            fps: self.s.doc.settings.fps,
                            ..self.import_options
                        };
                        self.image_import = Some(import::ImageImport::start(paths, opts, ctx));
                    } else {
                        // SYMPHONY_SHOT_IMPORT_ASK=colour shows the question
                        // in a screenshot run, set to Vector drawing.
                        if let Ok(how) = std::env::var("SYMPHONY_SHOT_IMPORT_ASK") {
                            self.import_options.as_vectors = true;
                            if how == "colour" {
                                self.import_options.vectorize =
                                    vector_import::Vectorize::Colour { colours: 8 };
                            }
                        }
                        self.image_import_asking = Some(paths);
                    }
                }
                1 => {
                    if self.brush_import.is_some() {
                        notes.push(
                            "Still importing the last brushes. Try these again in a moment.".into(),
                        );
                    } else {
                        self.brush_import = Some(brush_import::BrushImport::start(paths, ctx));
                    }
                }
                2 => {
                    for p in paths {
                        notes.push(match self.presets.import(&p) {
                            Ok(n) => format!("Imported {n} presets from {}.", name(&p)),
                            Err(e) => format!("{}: {e}", name(&p)),
                        });
                    }
                }
                3 | 4 => {
                    // Opening replaces the scene: never over unsaved work.
                    if let Some(p) = paths.into_iter().next() {
                        if self.s.dirty() {
                            notes.push(format!(
                                "Save or close this scene first, then import {} again.",
                                name(&p)
                            ));
                        } else {
                            self.open_path(p, slot == 4, ctx);
                        }
                    }
                }
                5 => {
                    if self.sound_import.is_some() {
                        notes.push(
                            "Still importing the last sound. Try this again in a moment.".into(),
                        );
                    } else {
                        self.sound_import = Some(import::SoundImport::start(paths, ctx));
                    }
                }
                6 => {
                    // Videos decode with Cisco's OpenH264, which the user
                    // turns on themselves (Symphony never ships it).
                    let Some(codec) = codec::installed() else {
                        notes.push(
                            "Video import needs Cisco's free OpenH264 codec. Download it once in Preferences > Video, then import again."
                                .into(),
                        );
                        continue;
                    };
                    if self.video_import.is_some() {
                        notes.push(
                            "Still importing the last video. Try this again in a moment.".into(),
                        );
                        continue;
                    }
                    let mut paths = paths.into_iter();
                    if let Some(p) = paths.next() {
                        let st = &self.s.doc.settings;
                        self.video_import = Some(video::VideoImport::start(
                            p,
                            codec,
                            st.fps,
                            [st.width, st.height],
                            ctx,
                        ));
                    }
                    for p in paths {
                        notes.push(format!(
                            "{}: one video at a time. Import it again after this one.",
                            name(&p)
                        ));
                    }
                }
                _ => {
                    for p in paths {
                        notes.push(format!("{} isn't something Symphony can import.", name(&p)));
                    }
                }
            }
        }
        if !notes.is_empty() {
            self.s.notice = Some(notes.join(" "));
        }
    }

    fn file_drops(&mut self, ctx: &egui::Context) {
        // A video finished decoding: its frames become a layer from the
        // current frame, its sound a clip at the same frame.
        if let Some(job) = &self.video_import {
            if let Some(done) = job.poll() {
                let shown = job.name.clone();
                self.video_import = None;
                let frames = done.frames.as_ref().map_or(0, |g| g.frames.len());
                if let Some(g) = done.frames {
                    self.s.import_images(vec![g]);
                }
                let sound = done.sound.is_some();
                self.s.import_sounds(done.sound.into_iter().collect());
                let mut text = if frames > 0 {
                    format!(
                        "Imported {shown}: {frames} frames{} at frame {}.",
                        if sound { " and its sound" } else { "" },
                        self.s.frame
                    )
                } else {
                    String::new()
                };
                if !done.problems.is_empty() {
                    text.push(' ');
                    text.push_str(&done.problems.join(". "));
                }
                if !text.is_empty() {
                    self.s.notice = Some(text.trim().to_owned());
                }
            }
        }
        // Sounds finished decoding: put them on the timeline.
        if let Some(job) = &self.sound_import {
            if let Some(done) = job.poll() {
                self.sound_import = None;
                let names: Vec<String> = done.sounds.iter().map(|s| s.name.clone()).collect();
                let placed = self.s.import_sounds(done.sounds);
                let mut text = match placed {
                    0 => String::new(),
                    1 => format!("Imported {} at frame {}.", names[0], self.s.frame),
                    n => format!("Imported {n} sounds at frame {}.", self.s.frame),
                };
                if !done.problems.is_empty() {
                    text.push(' ');
                    text.push_str(&done.problems.join(". "));
                }
                if !text.is_empty() {
                    self.s.notice = Some(text.trim().to_owned());
                }
            }
        }
        // Pictures waiting on the Import Images question.
        if let Some(paths) = &self.image_import_asking {
            match import::options_ui(ctx, paths, &mut self.import_options) {
                Some(true) => {
                    let paths = self.image_import_asking.take().unwrap_or_default();
                    let opts = import::ImportOptions {
                        fps: self.s.doc.settings.fps,
                        ..self.import_options
                    };
                    self.image_import = Some(import::ImageImport::start(paths, opts, ctx));
                }
                Some(false) => self.image_import_asking = None,
                None => {}
            }
        }
        // Images finished reading in the background: place them.
        if let Some(job) = &self.image_import {
            if let Some(done) = job.poll() {
                self.image_import = None;
                let placed =
                    self.s.import_images(done.groups) + self.s.import_vectors(done.vectors);
                let mut text = match placed {
                    0 => String::new(),
                    1 => "Imported an image.".to_owned(),
                    n => format!("Imported {n} images."),
                };
                if !done.problems.is_empty() {
                    text.push(' ');
                    text.push_str(&done.problems.join(". "));
                }
                if !text.is_empty() {
                    self.s.notice = Some(text.trim().to_owned());
                }
            }
        }
        if let Some(job) = &self.brush_import {
            if let Some(done) = job.poll() {
                self.brush_import = None;
                let n = done.presets.len();
                // Importing a brush again (same name, same app) updates it
                // in place: improved importers reach old presets without
                // leaving numbered copies.
                let mut updated = 0;
                for p in done.presets {
                    if let Some(old) =
                        self.presets.brush.iter_mut().find(|o| {
                            o.name == p.name && o.source.is_some() && o.source == p.source
                        })
                    {
                        *old = p;
                        updated += 1;
                        continue;
                    }
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
                if updated > 0 {
                    text.push_str(&format!(
                        " {updated} already here {} updated.",
                        if updated == 1 { "was" } else { "were" }
                    ));
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
        // While files hover: a glass panel that fades and grows in, a soft
        // blue glow around the window, and both fade out again.
        let shown = symphony_ui::motion::presence(
            ctx,
            egui::Id::unique("symphony-drop-hint-shown"),
            hovered > 0,
        );
        if shown > 0.0 {
            let screen = ctx.content_rect();
            ctx.layer_painter(egui::LayerId::new(
                egui::Order::Foreground,
                egui::Id::unique("symphony-drop-glow"),
            ))
            .rect_stroke(
                screen.shrink(4.0),
                theme::GLASS_RADIUS,
                egui::Stroke::new(2.0, theme::ACCENT_SELECT.gamma_multiply(0.6 * shown)),
                egui::StrokeKind::Inside,
            );
            let lift = symphony_ui::motion::look(shown).drop;
            egui::Area::new(egui::Id::unique("symphony-drop-hint"))
                .order(egui::Order::Foreground)
                .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, lift))
                .interactable(false)
                .fade_in(false)
                .show(ctx, |ui| {
                    ui.multiply_opacity(shown);
                    let slot = ui.painter().add(egui::Shape::Noop);
                    let panel = egui::Frame::new()
                        .stroke(egui::Stroke::new(1.0, theme::ACCENT_SELECT.gamma_multiply(0.55)))
                        .corner_radius(theme::GLASS_RADIUS)
                        .shadow(ui.visuals().popup_shadow)
                        .inner_margin(egui::Margin::same(18))
                        .show(ui, |ui| {
                            ui.label(
                                egui::RichText::new("Drop to import")
                                    .font(egui::FontId::proportional(theme::SIZE_HEADING))
                                    .color(theme::TEXT_PRIMARY),
                            );
                            ui.label(
                                egui::RichText::new(
                                    "Images and image sequences, sound, video, brushes from other apps, Symphony presets and projects, Harmony scenes",
                                )
                                .font(theme::dense())
                                .color(theme::TEXT_SECONDARY),
                            );
                        });
                    ui.painter().set(
                        slot,
                        egui::Shape::Vec(backdrop::glass(
                            ui.ctx(),
                            panel.response.rect,
                            f32::from(theme::GLASS_RADIUS),
                        )),
                    );
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
        self.import_paths(ctx, paths);
    }

    fn dialogs(&mut self, ctx: &egui::Context) {
        if let Some(o) = &self.opening {
            let name = symphony_io::project_name(&o.path);
            let elapsed = self.last_time.unwrap_or(0.0) - o.started;
            let mut cancel = false;
            theme::glass_modal(ctx, "symphony-opening").show(ctx, |ui| {
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
            theme::glass_modal(ctx, "symphony-unsaved").show(ctx, |ui| {
                ui.heading(format!("Save changes to {name}?"));
                ui.label("If you don't save, the changes since your last save are lost.");
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if symphony_ui::widgets::primary_button(ui, "Save", true).clicked() {
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
                    if let Some(path) = self.s.path.clone() {
                        if self.save_now(path, false) {
                            self.proceed(ctx, p);
                        }
                    } else {
                        self.show_save_dialog(Some(p));
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

        if let Some((dialog, next)) = self.save_dialog.as_mut() {
            let action = dialog.show(ctx, self.s.path.as_deref());
            let next = *next;
            match action {
                Some(save_dialog::Action::Save { path, create }) => {
                    self.save_dialog = None;
                    if let Some(next) = next {
                        if self.save_now(path, create) {
                            self.proceed(ctx, next);
                        }
                    } else {
                        self.save_to(path, create);
                    }
                }
                Some(save_dialog::Action::Cancel) => self.save_dialog = None,
                None => {}
            }
        }

        if !self.recovered.is_empty() {
            let n = self.recovered.len();
            let mut choice = None;
            theme::glass_modal(ctx, "symphony-recover").show(ctx, |ui| {
                ui.set_max_width(400.0);
                ui.heading("Recover unsaved work?");
                ui.add_space(6.0);
                let what = if n == 1 { "a scene".to_owned() } else { format!("{n} scenes") };
                ui.label(format!("Symphony didn't close properly last time. It autosaved {what} you hadn't saved."));
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    if symphony_ui::widgets::primary_button(ui, "Recover", true).clicked() {
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
            theme::glass_modal(ctx, "symphony-error").show(ctx, |ui| {
                ui.set_max_width(400.0);
                ui.heading("That didn't work");
                ui.add_space(6.0);
                ui.label(e);
                ui.add_space(12.0);
                if ui.button("OK").clicked() {
                    self.error = None;
                }
            });
        }
    }
}

fn add_tab_menu(
    ui: &mut egui::Ui,
    section: egui_dock::NodePath,
    locations: &[(egui_dock::NodePath, View)],
) -> Option<View> {
    let mut chosen = None;
    ui.menu_button("Add Tab", |ui| {
        for view in View::ALL {
            let here = locations.contains(&(section, view));
            let elsewhere = locations.iter().any(|(_, existing)| *existing == view);
            let option = ui.add_enabled(!here, egui::Button::new(view.title()));
            if elsewhere && !here {
                option
                    .clone()
                    .on_hover_text("Move this tab here from its current section");
            }
            if option.clicked() {
                chosen = Some(view);
                ui.close();
            }
        }
    });
    if chosen.is_some() {
        ui.close();
    }
    chosen
}

fn add_tab_to(layout: &mut DockState<View>, section: egui_dock::NodePath, tab: View) {
    if let Ok(leaf) = layout.leaf(section) {
        if leaf.tabs.contains(&tab) {
            return;
        }
        let end = egui_dock::TabIndex(leaf.tabs.len());
        if let Some(from) = layout.find_tab(&tab) {
            layout.move_tab(from, (section, egui_dock::TabInsert::Insert(end)));
        } else if let Ok(leaf) = layout.leaf_mut(section) {
            leaf.append_tab(tab);
            layout.set_focused_node_and_surface(section);
        }
    }
}

fn tab_strip_menu(
    ui: &mut egui::Ui,
    layout: &DockState<View>,
    workspace: Workspace,
    locations: &[(egui_dock::NodePath, View)],
) -> Option<(egui_dock::NodePath, View)> {
    let ctx = ui.ctx();
    let popup = egui::Id::unique(("add-tab-at-strip", workspace));
    if !egui::Popup::is_any_open(ctx) && ctx.input(|i| i.pointer.secondary_clicked()) {
        if let Some(pos) = ctx.pointer_interact_pos() {
            let section = layout.iter_leaves().find_map(|(section, leaf)| {
                let rect = leaf.rect();
                let bottom = if leaf.viewport.is_positive() {
                    leaf.viewport.top().min(rect.top() + theme::TAB_ROW)
                } else {
                    (rect.top() + theme::TAB_ROW).min(rect.bottom())
                };
                let strip = egui::Rect::from_min_max(rect.min, egui::pos2(rect.right(), bottom));
                (!leaf.tab_bar_hidden && strip.contains(pos)).then_some(section)
            });
            if let Some(section) = section {
                ctx.data_mut(|data| data.insert_temp(popup, (section, pos)));
                egui::Popup::open_id(ctx, popup);
            }
        }
    }
    let (section, pos) =
        ctx.data(|data| data.get_temp::<(egui_dock::NodePath, egui::Pos2)>(popup))?;
    egui::Popup::new(popup, ctx.clone(), pos, ui.layer_id())
        .kind(egui::PopupKind::Menu)
        .open_memory(None)
        .show(|ui| add_tab_menu(ui, section, locations))
        .and_then(|response| response.inner.map(|view| (section, view)))
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
    hovered: &'a mut Option<View>,
    /// For tab animations: the context, and each leaf's open tab.
    ctx: egui::Context,
    active: Vec<View>,
    tab_locations: Vec<(egui_dock::NodePath, View)>,
    tab_request: Option<(egui_dock::NodePath, View)>,
}

impl Viewer<'_> {
    fn view(&mut self, ui: &mut egui::Ui, v: View) {
        if ui.ui_contains_pointer() {
            *self.hovered = Some(v);
        }
        // A view coming into sight (tab switch, workspace change) appears with
        // the shared Appear motion, fading in place: pen coordinates and hit
        // targets never move.
        if !matches!(v, View::Camera | View::Drawing) {
            let key = egui::Id::unique(("view-appearance", self.workspace, v));
            let frame = ui.ctx().cumulative_frame_nr();
            // Not drawn last frame: it's just appearing.
            let last = ui.data(|d| d.get_temp::<u64>(key));
            if last.is_none_or(|l| frame > l.saturating_add(1)) {
                symphony_ui::motion::start(ui.ctx(), key.with("p"), 0.5);
            }
            ui.data_mut(|d| d.insert_temp(key, frame));
            let p = symphony_ui::motion::value(
                ui.ctx(),
                key.with("p"),
                1.0,
                symphony_ui::motion::Motion::Appear,
            );
            if p < 1.0 {
                ui.multiply_opacity(p);
            }
        }
        let started = std::time::Instant::now();
        self.view_body(ui, v);
        perf_span(v.title(), started);
    }

    fn view_body(&mut self, ui: &mut egui::Ui, v: View) {
        match v {
            View::Camera => camera::ui(ui, self.s, self.camera, self.samples, false, self.profile),
            View::Drawing => camera::ui(ui, self.s, self.drawing, self.samples, true, self.profile),
            View::Timeline => timing::timeline(ui, self.s, self.timing, self.profile),
            View::Xsheet => timing::xsheet(ui, self.s, self.timing, self.profile),
            View::Node => node::ui(ui, self.s, self.node_view),
            View::Multiplane => views::multiplane::ui(ui, self.s, self.node_view),
            View::ToolProperties => panels::tool_properties(ui, self.s, self.presets, self.device),
            View::Colour => panels::colour(ui, self.s),
            View::LayerProperties => node::inspector(ui, self.s, self.node_view),
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
        // Switching tabs fades the new view in (Appear), rather than it
        // popping in: a view that wasn't drawn last frame starts from clear.
        let ctx = ui.ctx().clone();
        let pass = ctx.cumulative_pass_nr();
        let seen_id = egui::Id::unique(("tab-last-drawn", *tab));
        let fade_id = egui::Id::unique(("tab-fade", *tab));
        let last: Option<u64> = ctx.data(|d| d.get_temp(seen_id));
        ctx.data_mut(|d| d.insert_temp(seen_id, pass));
        if last.is_some_and(|l| l + 1 < pass) {
            symphony_ui::motion::start(&ctx, fade_id, 0.0);
        }
        let opacity =
            symphony_ui::motion::value(&ctx, fade_id, 1.0, symphony_ui::motion::Motion::Appear);
        if opacity < 1.0 {
            ui.multiply_opacity(opacity);
        }
        self.view(ui, *tab);
    }

    fn context_menu(&mut self, ui: &mut egui::Ui, _tab: &mut View, path: egui_dock::NodePath) {
        if let Some(view) = add_tab_menu(ui, path, &self.tab_locations) {
            self.tab_request = Some((path, view));
        }
    }

    /// Tabs ease between their open and closed look instead of switching at
    /// once, and have no keyboard-focus outline (the open tab's own fill
    /// already marks it).
    fn tab_style_override(
        &self,
        tab: &View,
        global: &egui_dock::TabStyle,
    ) -> Option<egui_dock::TabStyle> {
        let open = self.active.contains(tab);
        let t = symphony_ui::motion::value(
            &self.ctx,
            egui::Id::unique(("symphony-tab-open", *tab)),
            if open { 1.0 } else { 0.0 },
            symphony_ui::motion::Motion::Select,
        );
        let mix = |a: egui::Color32, b: egui::Color32| symphony_ui::motion::mix(a, b, t);
        let bg = mix(global.inactive.bg_fill, global.active.bg_fill);
        let text = mix(global.inactive.text_color, global.active.text_color);
        let mut s = global.clone();
        for st in [
            &mut s.active,
            &mut s.inactive,
            &mut s.focused,
            &mut s.active_with_kb_focus,
            &mut s.inactive_with_kb_focus,
            &mut s.focused_with_kb_focus,
        ] {
            st.bg_fill = bg;
            st.text_color = text;
            st.outline_color = egui::Color32::TRANSPARENT;
        }
        Some(s)
    }

    fn scroll_bars(&self, tab: &View) -> [bool; 2] {
        // Canvas-like views manage their own space.
        match tab {
            View::Camera
            | View::Drawing
            | View::Node
            | View::Timeline
            | View::Xsheet
            | View::Multiplane => [false, false],
            _ => [false, true],
        }
    }
}

/// The frame after `frame` during playback: inside the playback range
/// (or the whole scene), back to its start at the end when looping, or
/// None to stop there.
fn next_play_frame(frame: u32, range: Option<(u32, u32)>, len: u32, looping: bool) -> Option<u32> {
    let len = len.max(1);
    let (a, b) = range.map_or((1, len), |(a, b)| (a.clamp(1, len), b.clamp(1, len)));
    let (first, last) = (a.min(b), a.max(b));
    if frame < first || frame > last {
        Some(first)
    } else if frame == last {
        looping.then_some(first)
    } else {
        Some(frame + 1)
    }
}

#[cfg(test)]
#[test]
fn playback_stays_in_its_range() {
    // Whole scene, looping.
    assert_eq!(next_play_frame(47, None, 48, true), Some(48));
    assert_eq!(next_play_frame(48, None, 48, true), Some(1));
    // A range: loops inside it, and jumps into it from outside.
    assert_eq!(next_play_frame(20, Some((10, 20)), 48, true), Some(10));
    assert_eq!(next_play_frame(3, Some((10, 20)), 48, true), Some(10));
    // Loop off: stops at the end.
    assert_eq!(next_play_frame(20, Some((10, 20)), 48, false), None);
    // A range past the end is held to the scene.
    assert_eq!(next_play_frame(48, Some((40, 90)), 48, true), Some(40));
}

/// SYMPHONY_PERF=1: time between painting a frame and starting the next.
fn perf_gap(gap: std::time::Duration) {
    use std::sync::Mutex;
    /// Since, frames, total ms, worst ms, frames over 10 ms.
    type Gaps = (std::time::Instant, u32, f32, f32, u32);
    static ACC: Mutex<Option<Gaps>> = Mutex::new(None);
    let mut g = ACC.lock().unwrap();
    let a = g.get_or_insert((std::time::Instant::now(), 0, 0.0, 0.0, 0));
    let ms = gap.as_secs_f32() * 1000.0;
    a.1 += 1;
    a.2 += ms;
    a.3 = a.3.max(ms);
    if ms > 10.0 {
        a.4 += 1;
    }
    if a.0.elapsed().as_secs_f32() >= 1.0 {
        eprintln!(
            "after paint, before next frame: avg {:.2} ms, worst {:.2} ms, {} over 10 ms",
            a.2 / a.1.max(1) as f32,
            a.3,
            a.4
        );
        *g = None;
    }
}

/// SYMPHONY_PERF=1: add time spent in a named part of the frame.
fn perf_span(name: &'static str, start: std::time::Instant) {
    if std::env::var_os("SYMPHONY_PERF").is_none() {
        return;
    }
    let spent = start.elapsed();
    let mut g = PERF_SPANS.lock().unwrap();
    match g.iter_mut().find(|(n, _)| *n == name) {
        Some((_, t)) => *t += spent,
        None => g.push((name, spent)),
    }
}

static PERF_SPANS: std::sync::Mutex<Vec<(&'static str, std::time::Duration)>> =
    std::sync::Mutex::new(Vec::new());

/// SYMPHONY_PERF=1: once a second, print how many frames ran and the
/// slowest one's time, to find work that runs when nothing changes.
fn perf_log(spent: std::time::Duration) {
    use std::sync::Mutex;
    static STATE: Mutex<
        Option<(
            std::time::Instant,
            u32,
            std::time::Duration,
            std::time::Duration,
        )>,
    > = Mutex::new(None);
    if std::env::var_os("SYMPHONY_PERF").is_none() {
        return;
    }
    let mut g = STATE.lock().unwrap();
    let now = std::time::Instant::now();
    let (start, frames, total, worst) =
        g.get_or_insert((now, 0, Default::default(), Default::default()));
    *frames += 1;
    *total += spent;
    *worst = (*worst).max(spent);
    if now.duration_since(*start).as_secs_f32() >= 1.0 {
        let mut spans = PERF_SPANS.lock().unwrap();
        spans.sort_by_key(|s| std::cmp::Reverse(s.1));
        let parts: Vec<String> = spans
            .iter()
            .map(|(n, t)| format!("{n} {:?}", *t / *frames))
            .collect();
        spans.clear();
        eprintln!(
            "perf: {frames} frames/s, avg {:?}, worst {:?} | {}",
            *total / *frames,
            *worst,
            parts.join(", ")
        );
        *g = None;
    }
}

impl eframe::App for SymphonyApp {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        if std::env::var_os("SYMPHONY_PERF").is_some() {
            if let Some(gap) = eframe::egui_wgpu::perf_since_paint_end() {
                perf_gap(gap);
            }
        }
        let start = std::time::Instant::now();
        self.frame_ui(ui, frame);
        perf_log(start.elapsed());
        eframe::egui_wgpu::perf_ui_end();
        if let Some(run) = self.perf_run.as_mut() {
            run.frame(start.elapsed());
        }
        // SYMPHONY_PERF_SPIN: repaint every frame, for profiling.
        if std::env::var_os("SYMPHONY_PERF_SPIN").is_some() {
            ui.ctx().request_repaint();
        }
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        // Screenshot runs use fresh layouts and test data; they must never
        // overwrite the person's real preferences.
        if self.shot.is_some() {
            return;
        }
        let saved = Saved {
            remaps: keymap::remaps(),
            tool_dock: self.tool_dock,
            recent_projects: self.recent_projects.clone(),
            wallpaper: self.wallpaper.choice.clone(),
            wallpaper_blur: self.wallpaper.blur,
            interface_animations: self.interface_animations,
            interface_effects: self.interface_effects,
            workspace: self.workspace,
            layouts: self.layouts.clone(),
            profile: self.profile,
            use_no_ai: self.use_no_ai,
            pressure_gamma: Some(self.s.pressure_gamma),
            brush_size_cursor: self.s.brush_size_cursor,
            overlay_underlay: self.s.overlay_underlay,
            size_follows_zoom: self.s.size_follows_zoom,
            stroke_detail: Some(self.s.stroke_detail),
            colours: Some(self.s.colours),
            use_openh264: self.use_openh264,
            presets: Some(self.presets.clone()),
            search_uses: self.search_uses.clone(),
            import_options: Some(self.import_options),
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

impl SymphonyApp {
    fn frame_ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // Performance runs: open a project, and keep navigating.
        if let Some(path) = self.perf_open.take() {
            let ctx = ui.ctx().clone();
            self.open_path(path, false, &ctx);
        }
        if let Ok(how) = std::env::var("SYMPHONY_PERF_NAV") {
            let t = ui.input(|i| i.time) as f32;
            self.camera.perf_navigate(t, &how);
        }
        let ctx = ui.ctx().clone();
        if let Some(s) = self.shot.as_mut() {
            s.tick(&ctx);
        }
        // Design review only: show the shared loading screen (DESIGN D17.7).
        // Screenshot mode can import brushes as if dropped (SYMPHONY_SHOT_DROP).
        // SYMPHONY_SHOT_GRID: show presets as a grid in screenshot runs.
        if self.shot.is_some() && std::env::var_os("SYMPHONY_SHOT_GRID").is_some() {
            self.presets.grid = true;
        }
        // SYMPHONY_SHOT_IMPORT=<file>: import any file as if dropped.
        if let (Some(_), Some(p)) = (&self.shot, std::env::var_os("SYMPHONY_SHOT_IMPORT")) {
            if !self.shot_imported {
                self.shot_imported = true;
                let ctx = ui.ctx().clone();
                self.import_paths(&ctx, vec![p.into()]);
            }
        }
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

        if self.shot.is_none() && ctx.input(|i| i.viewport().close_requested()) && !self.allow_close
        {
            if self.save_dialog.is_some()
                || self
                    .welcome
                    .as_ref()
                    .is_some_and(welcome::Welcome::creating)
            {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            } else if self.s.dirty() {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.pending = Some(Pending::Quit);
            }
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
                    &|rect| self.wallpaper.glass(rect),
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
        if self.pending.is_none()
            && self.save_dialog.is_none()
            && self.recovered.is_empty()
            && self.error.is_none()
        {
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

        self.wallpaper.sync_camera_session(&self.s.key);
        self.camera.capture_wallpaper(self.wallpaper.uses_camera());
        let perf_t = std::time::Instant::now();
        self.wallpaper.paint(ui);
        let chrome = ui.painter().clone();
        perf_span("wallpaper", perf_t);
        let top_background = chrome.add(egui::Shape::Noop);
        // The Symphony profile merges Brush and Pencil into one Pen tool.
        self.s.pen_modes = self.profile == keymap::Profile::Symphony;
        if matches!(self.s.tool, Tool::Brush | Tool::Pencil) {
            self.s.last_pen = self.s.tool;
        }
        let top = egui::Panel::top("top-bar")
            .show_separator_line(false)
            .frame(egui::Frame::new().inner_margin(egui::Margin::symmetric(10, 0)))
            .exact_size(theme::TOP_BAR)
            .show(ui, |ui| {
                let perf_t = std::time::Instant::now();
                self.top_bar(ui);
                perf_span("top bar", perf_t);
            });
        chrome.set(
            top_background,
            self.wallpaper
                .chrome(top.response.rect, wallpaper::ChromeSide::Top),
        );

        // Tools follow what you're working on (a peg, or drawings).
        self.s.fit_tool_to_context();
        self.cursor_feedback(&ctx);
        // Sound follows the playhead (and plays a slice when scrubbing).
        self.player
            .sync(&ctx, &self.s.doc, self.s.frame, self.s.playing);
        // Unsaved and recovered scenes find imported files in the cache.
        if let Some(cache) = import::cache_dir() {
            if !self.s.doc.media_dirs.contains(&cache) {
                self.s.doc.media_dirs.push(cache);
            }
        }
        // Autosave finishing says so; a faded notice is cleared.
        let autosaving = self.autosaving.load(Ordering::SeqCst);
        if self.was_autosaving && !autosaving {
            self.s.notice = Some("Autosaved".into());
        }
        self.was_autosaving = autosaving;
        let now = ctx.input(|i| i.time);
        if let Some((shown, since)) = &self.notice_shown {
            if now - since > 4.0 && self.s.notice.as_deref() == Some(shown.as_str()) {
                self.s.notice = None;
                self.notice_shown = None;
            }
        }
        let bottom_background = chrome.add(egui::Shape::Noop);
        let bottom = egui::Panel::bottom("status")
            .show_separator_line(false)
            .frame(egui::Frame::new().inner_margin(egui::Margin::symmetric(12, 0)))
            .exact_size(theme::STATUS_BAR)
            .show(ui, |ui| self.status_bar(ui, device.as_ref()));
        chrome.set(
            bottom_background,
            self.wallpaper
                .chrome(bottom.response.rect, wallpaper::ChromeSide::Bottom),
        );
        let perf_t = std::time::Instant::now();
        self.tools_panel(ui);
        perf_span("tools panel", perf_t);

        if self.tool_options_open {
            let id = egui::Id::unique("floating-tool-properties");
            let glass = ctx.layer_painter(egui::LayerId::new(egui::Order::Middle, id));
            let background = glass.add(egui::Shape::Noop);
            let window = egui::Window::new("Tool Properties")
                .id(id)
                .frame(
                    egui::Frame::window(&ctx.global_style())
                        .fill(egui::Color32::TRANSPARENT)
                        .stroke(egui::Stroke::NONE),
                )
                .open(&mut self.tool_options_open)
                .default_width(310.0)
                .default_height(380.0)
                .show(&ctx, |ui| {
                    panels::tool_properties(ui, &mut self.s, &mut self.presets, device.as_ref())
                });
            if let Some(window) = window {
                let rect = window.response.rect;
                glass.set(
                    background,
                    egui::Shape::Vec(backdrop::glass(&ctx, rect, f32::from(theme::GLASS_RADIUS))),
                );
                theme::glass_rim(&glass, rect);
            }
        }

        let mut open = self.show_preferences;
        let prefs_id = egui::Id::unique("symphony-preferences");
        // While Preferences is open, the app behind softly dims, so the
        // window reads as a focused sheet (fades with Appear/Dismiss).
        let dim = symphony_ui::motion::presence(&ctx, prefs_id.with("dim"), open);
        if dim > 0.0 {
            let scrim = egui::LayerId::new(egui::Order::Middle, prefs_id.with("scrim"));
            ctx.layer_painter(scrim).rect_filled(
                ctx.content_rect(),
                0.0,
                egui::Color32::from_black_alpha((90.0 * dim) as u8),
            );
            ctx.move_to_top(scrim);
        }
        // Keep the window above its scrim and every other window, from the
        // first frame it opens. egui only paints the grey title bar on the
        // top window, so it went missing when Preferences first opened
        // under another window.
        if open || dim > 0.0 {
            ctx.move_to_top(egui::LayerId::new(egui::Order::Middle, prefs_id));
        }
        let glass = ctx.layer_painter(egui::LayerId::new(egui::Order::Middle, prefs_id));
        let background = glass.add(egui::Shape::Noop);
        let preferences = preferences_window(&ctx, prefs_id)
            .open(&mut open)
            .show(&ctx, |ui| {
                ui.horizontal_top(|ui| {
                    // Pages, like Harmony's Preferences tabs.
                    ui.vertical(|ui| {
                        ui.set_width(150.0);
                        for page in PrefsPage::ALL {
                            if ui
                                .selectable_label(self.prefs_page == page, page.title())
                                .clicked()
                            {
                                self.prefs_page = page;
                            }
                        }
                    });
                    ui.separator();
                    egui::ScrollArea::vertical()
                        .id_salt(("prefs", self.prefs_page.title()))
                        .max_height(560.0)
                        .min_scrolled_width(420.0)
                        .show(ui, |ui| {
                            ui.vertical(|ui| {
                                ui.set_width(420.0);
                                match self.prefs_page {
                                    PrefsPage::Interface => {
                                        self.wallpaper.preferences(ui);
                                        if symphony_ui::widgets::checkbox(
                                            ui,
                                            &mut self.interface_effects,
                                            "Interface effects",
                                        )
                                        .on_hover_text("Blur, glass and shadows, and animations")
                                        .changed()
                                        {
                                            theme::set_effects(&ctx, self.interface_effects);
                                            theme::set_motion(
                                                &ctx,
                                                self.interface_animations && self.interface_effects,
                                            );
                                        }
                                        ui.label(
                                            egui::RichText::new(
                                                "Turn off on slower computers: no blur, glass, shadows or animations.",
                                            )
                                            .font(theme::dense())
                                            .color(theme::TEXT_SECONDARY),
                                        );
                                        ui.add_enabled_ui(self.interface_effects, |ui| {
                                            if symphony_ui::widgets::checkbox(
                                                ui,
                                                &mut self.interface_animations,
                                                "Animate interface transitions",
                                            )
                                            .changed()
                                            {
                                                theme::set_motion(
                                                    &ctx,
                                                    self.interface_animations && self.interface_effects,
                                                );
                                            }
                                        });
                                        ui.add_space(8.0);
                                        ui.label(
                                            egui::RichText::new(
                                                "Search (Tab) ranks what you choose often first.",
                                            )
                                            .font(theme::dense())
                                            .color(theme::TEXT_SECONDARY),
                                        );
                                        if ui
                                            .add_enabled(
                                                !self.search_uses.is_empty(),
                                                egui::Button::new("Forget search history"),
                                            )
                                            .clicked()
                                        {
                                            self.search_uses.clear();
                                        }
                                    }
                                    PrefsPage::Drawing => panels::prefs_drawing(ui, &mut self.s),
                                    PrefsPage::Canvas => panels::prefs_canvas(ui, &mut self.s),
                                    PrefsPage::Workflow => {
                                        panels::prefs_workflow(ui, &mut self.profile)
                                    }
                                    PrefsPage::Video => self.video_preferences(ui),
                                    PrefsPage::Ai => panels::prefs_ai(ui, &mut self.use_no_ai),
                                }
                            })
                        });
                });
            });
        if let Some(window) = preferences {
            let rect = window.response.rect;
            glass.set(
                background,
                egui::Shape::Vec(backdrop::glass(&ctx, rect, f32::from(theme::GLASS_RADIUS))),
            );
            theme::glass_rim(&glass, rect);
        }
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
        let active: Vec<View> = layout
            .main_surface()
            .iter()
            .filter_map(|node| node.get_leaf())
            .filter_map(|leaf| leaf.tabs.get(leaf.active.0).copied())
            .collect();
        let tab_locations = layout
            .iter_all_tabs()
            .map(|(path, tab)| (path.node_path(), *tab))
            .collect();
        layout.translations.tab_context_menu.close_button = "Remove".into();
        self.hovered_view = None;
        let perf_dock = std::time::Instant::now();
        let mut viewer = Viewer {
            ctx: ctx.clone(),
            active,
            tab_locations,
            tab_request: None,
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
            hovered: &mut self.hovered_view,
        };
        let central = egui::Frame::new();
        egui::CentralPanel::default()
            .frame(central)
            .show(ui, |ui| match maximised {
                Some(v) => {
                    let painter = ui.painter().clone();
                    let background = painter.add(egui::Shape::Noop);
                    let card = egui::Frame::new()
                        .fill(theme::GLASS_PANEL)
                        .corner_radius(theme::GLASS_RADIUS)
                        .outer_margin(egui::Margin::same(theme::GUTTER as i8))
                        .inner_margin(egui::Margin::same(4))
                        .show(ui, |ui| viewer.view(ui, v));
                    let rect = card.response.rect.shrink(theme::GUTTER);
                    if let Some(mask) = self.wallpaper.glass(rect) {
                        painter.set(background, mask);
                    }
                    theme::glass_rim(&painter, rect);
                }

                None => {
                    // Reserve the fill below the dock's widgets. Leaf bounds are
                    // final only after the splitters have processed this frame.
                    let painter = ui.painter().clone();
                    let background = painter.add(egui::Shape::Noop);
                    // Floating views are glass like every other panel:
                    // the blurred wallpaper under a translucent tint.
                    // Floating views: live blur of what's behind them.
                    let blur_ctx = ui.ctx().clone();
                    let backdrop = move |rect: egui::Rect| {
                        backdrop::glass(&blur_ctx, rect, f32::from(theme::GLASS_RADIUS))
                    };
                    DockArea::new(layout)
                        .style(theme::dock_style(ui.style()))
                        .window_frame(theme::glass_window_frame(ui.style()))
                        .window_backdrop(&backdrop)
                        .show_add_buttons(false)
                        // Close views from the tab's right-click menu; no × on every tab.
                        .show_close_buttons(false)
                        .show_leaf_close_all_buttons(false)
                        .show_leaf_collapse_buttons(false)
                        .show_inside(ui, &mut viewer);
                    settle_moved_views(ui.ctx(), layout);
                    let tab_request = viewer
                        .tab_request
                        .take()
                        .or_else(|| tab_strip_menu(ui, layout, ws, &viewer.tab_locations));
                    if let Some((section, tab)) = tab_request {
                        add_tab_to(layout, section, tab);
                        ui.ctx().request_repaint();
                    }
                    let mut fills = Vec::new();
                    for leaf in layout
                        .main_surface()
                        .iter()
                        .filter_map(|node| node.get_leaf())
                    {
                        let rect = leaf.rect();
                        if rect.is_positive() && !leaf.tabs.is_empty() {
                            fills.extend(self.wallpaper.glass(rect));
                            fills.push(egui::Shape::rect_filled(
                                rect,
                                theme::GLASS_RADIUS,
                                theme::GLASS_PANEL,
                            ));
                            theme::glass_rim(&painter, rect);
                        }
                    }
                    painter.set(background, egui::Shape::Vec(fills));
                    let rects: Vec<egui::Rect> = layout
                        .main_surface()
                        .iter()
                        .filter_map(|node| node.get_leaf())
                        .filter(|leaf| !leaf.tabs.is_empty())
                        .map(|leaf| leaf.rect())
                        .collect();
                    resize_grip(ui, &rects);
                    perf_span("dock total", perf_dock);
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
        // Last session crashed: offer to send the report (the tester sees
        // it in a pre-filled issue before anything is sent).
        if let Some((path, report)) = self.crash_report.clone() {
            let mut done = false;
            theme::glass_modal(&ctx, "symphony-crash-report").show(&ctx, |ui| {
                ui.set_max_width(420.0);
                ui.heading("Symphony closed unexpectedly");
                ui.add_space(6.0);
                ui.label(
                    "Sorry about that. A report of what went wrong was saved. Sending it \
                     opens a pre-filled bug report in your browser, so you can read it and \
                     add what you were doing before you submit it.",
                );
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if symphony_ui::widgets::primary_button(ui, "Send report…", true).clicked() {
                        ctx.open_url(egui::OpenUrl::new_tab(crash::issue_url(&report)));
                        done = true;
                    }
                    if ui.button("Show report").clicked() {
                        if let Some(dir) = path.parent() {
                            ctx.open_url(egui::OpenUrl::new_tab(format!(
                                "file://{}",
                                dir.display()
                            )));
                        }
                    }
                    if ui.button("Don't send").clicked() {
                        done = true;
                    }
                });
            });
            if done {
                crash::handled(&path);
                self.crash_report = None;
            }
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
                    ui.separator();
                    ui.heading("Bricolage Grotesque");
                    ui.label(egui::RichText::new(theme::WORDMARK_LICENSE).font(theme::dense()));
                });
            });
        self.show_licences = open;
        self.file_drops(&ctx);
        // Screenshot mode: SYMPHONY_SHOT_SEARCH="query" opens search typed.
        if let (Some(_), Ok(q)) = (&self.shot, std::env::var("SYMPHONY_SHOT_SEARCH")) {
            if !self.shot_search_opened {
                let mut st =
                    views::palette::PaletteState::new(egui::pos2(520.0, 260.0), Some(View::Camera));
                st.query = q;
                // SYMPHONY_SHOT_RECORD: the "press the new shortcut" prompt.
                if std::env::var_os("SYMPHONY_SHOT_RECORD").is_some() {
                    st.record_for_review(keymap::Action::MirrorView, "Mirror view");
                }
                self.palette = Some(st);
                self.shot_search_opened = true;
            }
        }
        if self.camera.render_effects {
            let wallpaper_pixels = self.camera.wallpaper_pixels(&self.s);
            self.wallpaper
                .update_camera(&ctx, &self.s, wallpaper_pixels);
        }
        views::node::windows(&ctx, &mut self.s, &mut self.node_view);
        self.search_box(&ctx);
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
        .with_icon(app_icon::window_icon())
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
    crash::install();
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
        // Queue one frame, not two: the pen shows up a frame sooner. Our
        // GPU work per frame is small, which is when egui recommends it.
        wgpu_options: eframe::egui_wgpu::WgpuConfiguration {
            surface: eframe::egui_wgpu::SurfaceConfig::LOW_LATENCY,
            ..Default::default()
        },
        ..Default::default()
    };
    eframe::run_native(
        "Symphony",
        options,
        Box::new(|cc| Ok(Box::new(SymphonyApp::new(cc)))),
    )
}

#[cfg(test)]
mod workspace_control_tests {
    use super::*;
    use egui_kittest::Harness;

    #[test]
    fn preferences_title_bar_moves_the_window() {
        let mut harness = Harness::builder()
            .with_size(egui::vec2(760.0, 520.0))
            .build_ui_state(
                |ui, rect| {
                    *rect = preferences_window(ui.ctx(), egui::Id::unique("test-preferences"))
                        .show(ui.ctx(), |ui| {
                            ui.set_min_size(egui::vec2(300.0, 180.0));
                            ui.label("Settings");
                        })
                        .unwrap()
                        .response
                        .rect;
                },
                egui::Rect::NOTHING,
            );
        harness.run_steps(3);
        let before = *harness.state();
        let start = egui::pos2(before.center().x, before.top() + 11.0);
        let end = start + egui::vec2(62.0, 30.0);
        for (pos, pressed) in [
            (start, None),
            (start, Some(true)),
            (end, None),
            (end, Some(false)),
        ] {
            harness
                .input_mut()
                .events
                .push(egui::Event::PointerMoved(pos));
            if let Some(pressed) = pressed {
                harness.input_mut().events.push(egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                });
            }
            harness.run_steps(1);
        }
        let delta = harness.state().center() - before.center();
        assert!(
            (delta.x - 62.0).abs() < 2.0 && (delta.y - 30.0).abs() < 2.0,
            "{delta:?}"
        );
    }

    #[test]
    fn slider_click_keeps_the_rail_focused_for_precise_arrow_steps() {
        let mut harness = Harness::builder()
            .with_size(egui::vec2(500.0, 200.0))
            .build_ui_state(
                |ui, (value, position)| {
                    let response = symphony_ui::widgets::precise_slider(
                        ui,
                        egui::Slider::new(value, 0.0..=10.0)
                            .text("Test slider")
                            .step_by(0.1)
                            .show_value(false),
                    );
                    *position = response.rect.center();
                },
                (5.0f32, egui::Pos2::ZERO),
            );
        harness.run_steps(2);
        let pos = harness.state().1;
        for pressed in [true, false] {
            harness.input_mut().events.extend([
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ]);
            harness.run_steps(1);
        }
        let before = harness.state().0;
        harness.input_mut().events.push(egui::Event::Key {
            key: egui::Key::ArrowRight,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        });
        harness.run_steps(1);
        assert!((harness.state().0 - before - 0.1).abs() < 0.0001);
    }

    /// Where each left/right divider sits: (top of its area, x).
    fn divider_xs(layout: &DockState<View>) -> Vec<(f32, f32)> {
        let mut xs: Vec<(f32, f32)> = layout
            .main_surface()
            .iter()
            .filter_map(|n| match n {
                egui_dock::Node::Horizontal(s) => {
                    Some((s.rect.top(), s.rect.left() + s.rect.width() * s.fraction))
                }
                _ => None,
            })
            .collect();
        xs.sort_by(|a, b| a.0.total_cmp(&b.0));
        xs
    }

    #[test]
    fn dragging_a_divider_near_its_neighbour_snaps_them_into_line() {
        struct TestViewer;
        impl egui_dock::TabViewer for TestViewer {
            type Tab = View;
            fn id(&mut self, tab: &mut View) -> egui::Id {
                egui::Id::unique(("snap-test-tab", *tab))
            }
            fn title(&mut self, tab: &mut View) -> egui::WidgetText {
                tab.title().into()
            }
            fn ui(&mut self, _: &mut egui::Ui, _: &mut View) {}
        }
        // Two rows, each split left/right, the dividers 35 points apart.
        let mut layout = DockState::new(vec![View::Camera]);
        let surface = layout.main_surface_mut();
        let [top, bottom] =
            surface.split_below(egui_dock::NodeIndex::root(), 0.5, vec![View::Timeline]);
        surface.split_right(top, 0.5, vec![View::Drawing]);
        surface.split_right(bottom, 0.55, vec![View::Node]);
        let mut harness = Harness::builder()
            .with_size(egui::vec2(700.0, 400.0))
            .build_ui_state(
                |ui, layout| {
                    DockArea::new(layout)
                        .style(theme::dock_style(ui.style()))
                        .show_inside(ui, &mut TestViewer);
                },
                layout,
            );
        harness.run_steps(3);
        let xs = divider_xs(harness.state());
        let (target, start) = (xs[0].1, xs[1].1);
        assert!(start - target > 20.0, "{xs:?}");
        // Drag the bottom divider to 6 points short of the top one.
        let y = 300.0;
        let mut at = egui::pos2(start, y);
        harness.input_mut().events.extend([
            egui::Event::PointerMoved(at),
            egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
        ]);
        harness.run_steps(1);
        while at.x > target + 6.0 {
            at.x = (at.x - 4.0).max(target + 6.0);
            harness
                .input_mut()
                .events
                .push(egui::Event::PointerMoved(at));
            harness.run_steps(1);
        }
        harness.input_mut().events.push(egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        });
        harness.run_steps(3);
        let xs = divider_xs(harness.state());
        assert!((xs[0].1 - xs[1].1).abs() < 0.5, "snapped into line: {xs:?}");
        assert!((xs[0].1 - target).abs() < 0.5, "the other one stays put");

        // Lined up, the crossing moves all three dividers, on both axes.
        let row_y = |layout: &DockState<View>| {
            layout
                .main_surface()
                .iter()
                .find_map(|n| match n {
                    egui_dock::Node::Vertical(s) => {
                        Some(s.rect.top() + s.rect.height() * s.fraction)
                    }
                    _ => None,
                })
                .unwrap()
        };
        let y0 = row_y(harness.state());
        let mut at = egui::pos2(xs[0].1, y0);
        harness.input_mut().events.extend([
            egui::Event::PointerMoved(at),
            egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
        ]);
        harness.run_steps(1);
        for _ in 0..10 {
            at += egui::vec2(4.0, 3.0);
            harness
                .input_mut()
                .events
                .push(egui::Event::PointerMoved(at));
            harness.run_steps(1);
        }
        harness.input_mut().events.push(egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        });
        harness.run_steps(2);
        let moved = divider_xs(harness.state());
        assert!(
            (moved[0].1 - moved[1].1).abs() < 0.5,
            "still in line: {moved:?}"
        );
        assert!(moved[0].1 - xs[0].1 > 30.0, "moved across: {moved:?}");
        assert!(row_y(harness.state()) - y0 > 20.0, "moved down too");
    }

    #[test]
    fn a_lined_up_divider_segment_moves_alone() {
        struct TestViewer;
        impl egui_dock::TabViewer for TestViewer {
            type Tab = View;
            fn id(&mut self, tab: &mut View) -> egui::Id {
                egui::Id::unique(("grid-test-tab", *tab))
            }
            fn title(&mut self, tab: &mut View) -> egui::WidgetText {
                tab.title().into()
            }
            fn ui(&mut self, _: &mut egui::Ui, _: &mut View) {}
        }
        // Camera over Timeline | Tool Properties over Colour, row
        // dividers lined up.
        let mut layout = DockState::new(vec![View::Camera]);
        let surface = layout.main_surface_mut();
        let [left, right] = surface.split_right(
            egui_dock::NodeIndex::root(),
            0.6,
            vec![View::ToolProperties],
        );
        surface.split_below(left, 0.6, vec![View::Timeline]);
        surface.split_below(right, 0.6, vec![View::Colour]);
        let mut harness = Harness::builder()
            .with_size(egui::vec2(800.0, 500.0))
            .build_ui_state(
                |ui, layout| {
                    DockArea::new(layout)
                        .style(theme::dock_style(ui.style()))
                        .show_inside(ui, &mut TestViewer);
                },
                layout,
            );
        harness.run_steps(3);
        let rect_of = |layout: &DockState<View>, v: View| {
            let path = layout.find_tab(&v).unwrap().node_path();
            layout
                .iter_leaves()
                .find(|(p, _)| *p == path)
                .map(|(_, leaf)| leaf.rect())
                .unwrap()
        };
        let camera = rect_of(harness.state(), View::Camera);
        let timeline = rect_of(harness.state(), View::Timeline);
        let colour = rect_of(harness.state(), View::Colour);
        // The divider segment beside the Timeline.
        let x = (timeline.right() + colour.left()) / 2.0;
        let mut at = egui::pos2(x, timeline.center().y);
        harness
            .input_mut()
            .events
            .push(egui::Event::PointerMoved(at));
        harness.run_steps(2);
        harness.input_mut().events.push(egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        });
        harness.run_steps(1);
        for _ in 0..10 {
            at.x -= 6.0;
            harness
                .input_mut()
                .events
                .push(egui::Event::PointerMoved(at));
            harness.run_steps(1);
        }
        harness.input_mut().events.push(egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        });
        harness.run_steps(3);
        let state = harness.state();
        assert!(
            (rect_of(state, View::Camera).right() - camera.right()).abs() < 1.0,
            "the Camera view didn't move"
        );
        assert!(
            rect_of(state, View::Colour).left() < colour.left() - 40.0,
            "Colour grew left"
        );
        assert!(rect_of(state, View::Timeline).right() < timeline.right() - 40.0);
    }

    #[test]
    fn add_tab_moves_existing_views_and_adds_missing_views_here() {
        let mut layout = DockState::new(vec![View::Camera, View::Drawing]);
        layout.main_surface_mut().split_right(
            egui_dock::NodeIndex::root(),
            0.5,
            vec![View::Timeline],
        );
        let target = layout.find_tab(&View::Timeline).unwrap().node_path();
        super::add_tab_to(&mut layout, target, View::Camera);
        super::add_tab_to(&mut layout, target, View::Node);
        assert_eq!(layout.find_tab(&View::Camera).unwrap().node_path(), target);
        assert_eq!(layout.find_tab(&View::Node).unwrap().node_path(), target);
        assert_eq!(
            layout
                .iter_all_tabs()
                .filter(|(_, view)| **view == View::Camera)
                .count(),
            1
        );
        let path = layout.find_tab(&View::Camera).unwrap();
        assert_eq!(layout.remove_tab(path), Some(View::Camera));
        assert!(layout.find_tab(&View::Camera).is_none());
    }

    #[test]
    fn right_click_in_empty_tab_strip_opens_the_add_tab_menu() {
        struct TestViewer;
        impl egui_dock::TabViewer for TestViewer {
            type Tab = View;
            fn id(&mut self, tab: &mut View) -> egui::Id {
                egui::Id::unique(("workspace-test-tab", *tab))
            }
            fn title(&mut self, tab: &mut View) -> egui::WidgetText {
                tab.title().into()
            }
            fn ui(&mut self, _: &mut egui::Ui, _: &mut View) {}
        }
        let mut harness = Harness::builder()
            .with_size(egui::vec2(700.0, 360.0))
            .build_ui_state(
                |ui, (layout, pos, open)| {
                    DockArea::new(layout)
                        .style(theme::dock_style(ui.style()))
                        .show_inside(ui, &mut TestViewer);
                    let (_, leaf) = layout.iter_leaves().next().unwrap();
                    *pos = egui::pos2(leaf.rect().right() - 40.0, leaf.rect().top() + 8.0);
                    let _ = super::tab_strip_menu(ui, layout, Workspace::Default, &[]);
                    *open = egui::Popup::is_id_open(
                        ui.ctx(),
                        egui::Id::unique(("add-tab-at-strip", Workspace::Default)),
                    );
                },
                (DockState::new(vec![View::Camera]), egui::Pos2::ZERO, false),
            );
        harness.run_steps(2);
        let pos = harness.state().1;
        for pressed in [true, false] {
            harness.input_mut().events.extend([
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Secondary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ]);
            harness.run_steps(1);
        }
        assert!(harness.state().2);
    }

    #[test]
    fn horizontal_toolbar_docks_at_top_and_bottom() {
        let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 800.0));
        assert_eq!(
            ToolDock::nearest(rect, egui::pos2(630.0, 20.0)),
            ToolDock::Top
        );
        assert_eq!(
            ToolDock::nearest(rect, egui::pos2(630.0, 780.0)),
            ToolDock::Bottom
        );
        assert!(ToolDock::Top.horizontal() && ToolDock::Bottom.horizontal());
        assert_eq!(
            ToolDock::nearest(rect, egui::pos2(1240.0, 420.0)),
            ToolDock::Right
        );
    }
}
