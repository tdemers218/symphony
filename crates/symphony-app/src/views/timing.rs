//! Timeline and Xsheet: two views over the same exposure data.
//! Click a cell to go to that frame and select that layer (D8.9).
//! Double-click a layer name to rename it; right-click for layer actions.
//! The amber playhead and frame tag are Symphony's signature (D3.9).

use eframe::egui::{self, Align2, CornerRadius, Pos2, Rect, Sense, Stroke, Vec2};
use symphony_ui::icons::{self, Icon};
use symphony_ui::{theme, widgets};

use crate::keymap::{self, Action, Profile};
use crate::session::{InsertExposure, Session};
use symphony_core::{
    doc::Bookmark,
    graph::{NodeId, NodeKind},
    id::Id,
};

#[derive(Default)]
pub struct TimingState {
    bookmark: Option<(String, bool, Bookmark)>,
    renaming: Option<(usize, String)>,
    menu_row: Option<usize>,
    /// The node a right-click menu is for (a layer's drawing node too).
    menu_node: Option<NodeId>,
    /// The sound clip a right-click menu is for.
    menu_sound: Option<usize>,
    show_unanimated_nodes: bool,
    /// Parameters column: drawing name being typed, per row.
    swap_text: Option<(usize, String)>,
    /// Parameters column drag: row, starting drawing index, pixels dragged.
    swap_drag: Option<(usize, usize, f32)>,
    /// A layer being dragged to a new place in the stack (its row).
    layer_drag: Option<usize>,
    scrubbing: bool,
    /// A Smart Chart being slid along the Timeline: its node, the frame
    /// the press was on, and how far it has moved so far.
    chart_drag: Option<(NodeId, u32, i64)>,
    /// Layer order last frame (columns), and layers that just arrived or
    /// moved with when, so their row glows briefly.
    last_order: Vec<symphony_core::id::Id>,
    arrived: Vec<(symphony_core::id::Id, f64)>,
}

impl TimingState {
    pub fn new_bookmark(&mut self, s: &Session) {
        self.bookmark = Some((
            s.key.clone(),
            false,
            Bookmark {
                id: Id(0),
                name: format!("Frame {}", s.frame),
                start: s.frame,
                end: s.frame,
                colour: None,
                notes: String::new(),
            },
        ));
    }

    pub fn bookmark_window(&mut self, ctx: &egui::Context, s: &mut Session) {
        let Some((scene, existing, bookmark)) = &mut self.bookmark else {
            return;
        };
        if scene != &s.key {
            self.bookmark = None;
            return;
        }
        let mut open = true;
        let mut save = false;
        let mut remove = false;
        egui::Window::new(if *existing {
            "Edit bookmark"
        } else {
            "Add bookmark"
        })
        .id(egui::Id::unique("timeline-bookmark"))
        .collapsible(false)
        .open(&mut open)
        .show(ctx, |ui| {
            ui.label("Name");
            ui.text_edit_singleline(&mut bookmark.name);
            let max = s.length().max(bookmark.end).max(bookmark.start);
            ui.horizontal_wrapped(|ui| {
                ui.add(
                    egui::DragValue::new(&mut bookmark.start)
                        .range(1..=max)
                        .prefix("Start "),
                );
                ui.add(
                    egui::DragValue::new(&mut bookmark.end)
                        .range(bookmark.start..=max)
                        .prefix("End "),
                );
            });
            let mut custom = bookmark.colour.is_some();
            if widgets::checkbox(ui, &mut custom, "Custom colour").changed() {
                bookmark.colour = custom.then_some([125, 168, 224]);
            }
            if let Some(colour) = &mut bookmark.colour {
                ui.color_edit_button_srgb(colour);
            }
            ui.label("Notes");
            ui.text_edit_multiline(&mut bookmark.notes);
            ui.horizontal(|ui| {
                save =
                    widgets::primary_button(ui, "Save bookmark", !bookmark.name.trim().is_empty())
                        .clicked();
                if *existing {
                    remove = ui.button("Remove").clicked();
                }
            });
        });
        if save {
            bookmark.name = bookmark.name.trim().to_owned();
            bookmark.end = bookmark.end.max(bookmark.start);
            s.save_bookmark(bookmark.clone());
        } else if remove {
            s.remove_bookmark(bookmark.id);
        }
        if save || remove || !open {
            self.bookmark = None;
        }
    }
}

fn bookmark_menu(ui: &mut egui::Ui, s: &mut Session, t: &mut TimingState) {
    ui.menu_button("Bookmarks", |ui| {
        if ui.button("Add bookmark here").clicked() {
            t.new_bookmark(s);
            ui.close();
        }
        let length = s.length();
        let previous = s
            .doc
            .bookmarks
            .iter()
            .rev()
            .find(|bookmark| bookmark.start < s.frame)
            .map(|bookmark| bookmark.start);
        let next = s
            .doc
            .bookmarks
            .iter()
            .find(|bookmark| bookmark.start > s.frame && bookmark.start <= length)
            .map(|bookmark| bookmark.start);
        if !s.doc.bookmarks.is_empty() {
            ui.horizontal(|ui| {
                for (label, frame) in [("Previous", previous), ("Next", next)] {
                    if ui
                        .add_enabled(frame.is_some(), egui::Button::new(label))
                        .clicked()
                    {
                        if let Some(frame) = frame {
                            s.frame = frame.max(1);
                        }
                        ui.close();
                    }
                }
            });
            ui.separator();
        }
        egui::ScrollArea::vertical()
            .max_height(244.0)
            .show(ui, |ui| {
                for bookmark in &s.doc.bookmarks {
                    ui.horizontal(|ui| {
                        if ui
                            .add_enabled(
                                bookmark.start <= length,
                                egui::Button::new(&bookmark.name),
                            )
                            .clicked()
                        {
                            s.frame = bookmark.start.max(1);
                            ui.close();
                        }
                        if ui.small_button("Edit…").clicked() {
                            t.bookmark = Some((s.key.clone(), true, bookmark.clone()));
                            ui.close();
                        }
                    });
                }
            });
    });
}

/// Width of the Timeline's Parameters field (the exposed drawing's name).
const PARAM_W: f32 = 48.0;
/// Pixels of drag per drawing when scrubbing the Parameters field.
const PARAM_STEP: f32 = 12.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TimelineAction {
    SetKey,
    DeleteKey,
    InsertNew,
    Duplicate,
    CopyDrawing,
    PasteDrawing,
    CloneDrawing,
    Extend,
    InsertInbetween,
    DeleteLayer,
}

impl TimelineAction {
    pub const ALL: [Self; 10] = [
        Self::SetKey,
        Self::DeleteKey,
        Self::InsertNew,
        Self::Duplicate,
        Self::CopyDrawing,
        Self::PasteDrawing,
        Self::CloneDrawing,
        Self::Extend,
        Self::InsertInbetween,
        Self::DeleteLayer,
    ];

    pub fn id(self) -> &'static str {
        match self {
            Self::SetKey => "timeline.SetKey",
            Self::DeleteKey => "timeline.DeleteKey",
            Self::InsertNew => "timeline.InsertNew",
            Self::Duplicate => "timeline.Duplicate",
            Self::CopyDrawing => "timeline.CopyDrawing",
            Self::PasteDrawing => "timeline.PasteDrawing",
            Self::CloneDrawing => "timeline.CloneDrawing",
            Self::Extend => "timeline.Extend",
            Self::InsertInbetween => "timeline.InsertInbetween",
            Self::DeleteLayer => "timeline.DeleteLayer",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::SetKey => "Set Keyframe",
            Self::DeleteKey => "Delete Keyframe",
            Self::InsertNew => "Insert New Drawing Frame",
            Self::Duplicate => "Duplicate Drawing",
            Self::CopyDrawing => "Copy Drawing",
            Self::PasteDrawing => "Paste Drawing",
            Self::CloneDrawing => "Clone Drawing",
            Self::Extend => "Extend Exposure",
            Self::InsertInbetween => "Insert In-between Frame",
            Self::DeleteLayer => "Delete Layer",
        }
    }

    pub fn keywords(self) -> &'static str {
        match self {
            Self::SetKey => "add create key pose animation timeline",
            Self::DeleteKey => "remove key pose animation timeline",
            Self::InsertNew => "add new frame drawing blank empty cel cell",
            Self::Duplicate => "duplicate frame exposure independent cel",
            Self::CopyDrawing => "copy artwork from frame to clipboard",
            Self::PasteDrawing => "paste independent drawing into current cell",
            Self::CloneDrawing => "clone linked shared drawing exposure frame",
            Self::Extend => "hold extend increase exposure length one frame",
            Self::InsertInbetween => "insert new frame all layers hold timing scene",
            Self::DeleteLayer => "remove layer timeline",
        }
    }

    pub(crate) fn enabled(self, s: &Session) -> bool {
        match self {
            Self::SetKey => !s.doc.layers.is_empty() || s.selected_node.is_some(),
            Self::DeleteKey => super::node::selected_has_key(s),
            Self::InsertNew => s.can_insert_exposure(),
            Self::CopyDrawing => s.doc.exposed(s.selected, s.frame).is_some(),
            Self::PasteDrawing => s.can_paste_drawing(),
            Self::Duplicate | Self::CloneDrawing | Self::Extend => {
                s.can_insert_exposure() && s.doc.exposed(s.selected, s.frame).is_some()
            }
            Self::InsertInbetween => s.length() < 100_000,
            Self::DeleteLayer => s.layer_count() > 1,
        }
    }

    pub fn run(self, s: &mut Session) {
        match self {
            Self::SetKey => super::node::set_selected_key(s, false),
            Self::DeleteKey => super::node::set_selected_key(s, true),
            Self::InsertNew => s.insert_exposure(InsertExposure::New),
            Self::Duplicate => s.insert_exposure(InsertExposure::Duplicate),
            Self::CopyDrawing => s.copy_exposed_drawing(),
            Self::PasteDrawing => s.paste_drawing(),
            Self::CloneDrawing => s.insert_exposure(InsertExposure::Clone),
            Self::Extend => s.change_exposure(1),
            Self::InsertInbetween => s.insert_inbetweens(1),
            Self::DeleteLayer => s.delete_layer(s.selected),
        }
    }

    pub fn menu(self, ui: &mut egui::Ui, s: &mut Session, profile: Profile) {
        let action = if self == Self::Extend {
            Action::IncreaseExposure
        } else {
            Action::Timeline(self)
        };
        let button = egui::Button::new(self.label()).shortcut_text(keymap::label(profile, action));
        if ui.add_enabled(self.enabled(s), button).clicked() {
            self.run(s);
            ui.close();
        }
    }
}

fn drawing_clipboard_menu(ui: &mut egui::Ui, s: &mut Session, profile: Profile) {
    for action in [
        TimelineAction::CopyDrawing,
        TimelineAction::PasteDrawing,
        TimelineAction::Duplicate,
        TimelineAction::CloneDrawing,
    ] {
        action.menu(ui, s, profile);
    }
}

fn frame_menu(ui: &mut egui::Ui, s: &mut Session, profile: Profile) {
    for action in [
        TimelineAction::InsertNew,
        TimelineAction::Duplicate,
        TimelineAction::CloneDrawing,
        TimelineAction::CopyDrawing,
        TimelineAction::PasteDrawing,
        TimelineAction::Extend,
    ] {
        action.menu(ui, s, profile);
    }
    ui.menu_button("Reuse Existing Drawing", |ui| {
        let drawings = s.element_drawings(s.selected);
        if drawings.is_empty() {
            ui.label("This layer has no drawings yet.");
        }
        egui::ScrollArea::vertical()
            .max_height(280.0)
            .show(ui, |ui| {
                for (id, name) in drawings {
                    if ui
                        .add_enabled(s.can_insert_exposure(), egui::Button::new(name))
                        .clicked()
                    {
                        s.insert_exposure(InsertExposure::Reuse(id));
                        ui.close();
                    }
                }
            });
    });
    ui.separator();
    TimelineAction::InsertInbetween.menu(ui, s, profile);
    TimelineAction::DeleteKey.menu(ui, s, profile);
    ui.separator();
    if ui
        .button("Clear exposure")
        .on_hover_text("Empty this cell; the drawing stays in the layer's drawings.")
        .clicked()
    {
        s.clear_cell();
        ui.close();
    }
    if ui
        .button("Remove frame")
        .on_hover_text("Take out this cell and pull the rest of the layer one frame earlier.")
        .clicked()
    {
        s.remove_frame();
        ui.close();
    }
}

/// View header: transport, timing fields and layer buttons (D3.2).
pub fn header(ui: &mut egui::Ui, s: &mut Session, profile: Profile) {
    egui::Frame::new()
        .inner_margin(egui::Margin::symmetric(theme::CARD_PAD, 4))
        .show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                let available = ui.available_width();
                let icon = if s.playing { Icon::Pause } else { Icon::Play };
                if icons::button(
                    ui,
                    icon,
                    s.playing,
                    true,
                    &keymap::tooltip(profile, Action::PlayPause),
                )
                .clicked()
                {
                    s.playing = !s.playing;
                }
                ui.spacing_mut().item_spacing.x = 6.0;
                let len = s.length();
                // Keep scene settings in the menu when the panel is narrow.
                let compact = available < 535.0;
                ui.add(
                    egui::DragValue::new(&mut s.frame)
                        .range(1..=len)
                        .prefix("Frame "),
                );
                let keyed = super::node::selected_has_key(s);
                let (icon, action) = if keyed {
                    (Icon::RemoveKeyframe, TimelineAction::DeleteKey)
                } else {
                    (Icon::AddKeyframe, TimelineAction::SetKey)
                };
                let tooltip = keymap::tooltip(profile, Action::Timeline(action));
                if icons::button(ui, icon, keyed, action.enabled(s), &tooltip).clicked() {
                    action.run(s);
                }
                ui.menu_button("Frames", |ui| {
                    frame_menu(ui, s, profile);
                    ui.separator();
                    ui.menu_button("Timing options", |ui| timing_options(ui, s));
                });
                if !compact {
                    scene_timing(ui, s);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 2.0;
                        if icons::button(
                            ui,
                            Icon::Trash,
                            false,
                            s.layer_count() > 1,
                            "Delete layer",
                        )
                        .clicked()
                        {
                            s.delete_layer(s.selected);
                        }
                        if icons::button(
                            ui,
                            Icon::Plus,
                            false,
                            true,
                            &keymap::tooltip(profile, Action::AddLayer),
                        )
                        .clicked()
                        {
                            s.add_layer();
                        }
                    });
                }
            });
        });
}

fn timing_options(ui: &mut egui::Ui, s: &mut Session) {
    ui.checkbox(&mut s.animate, "Auto-key")
        .on_hover_text("Record animated property edits at the current frame.");
    ui.separator();
    let len = s.length();
    for (label, frame) in [
        ("First frame", 1),
        ("Previous frame", s.frame.saturating_sub(1).max(1)),
        ("Next frame", (s.frame + 1).min(len)),
        ("Last frame", len),
    ] {
        if ui.button(label).clicked() {
            s.frame = frame;
            ui.close();
        }
    }
    ui.separator();
    scene_timing(ui, s);
    ui.separator();
    if ui.button("Add layer").clicked() {
        s.add_layer();
        ui.close();
    }
    if ui
        .add_enabled(s.layer_count() > 1, egui::Button::new("Delete layer"))
        .clicked()
    {
        s.delete_layer(s.selected);
        ui.close();
    }
}

fn scene_timing(ui: &mut egui::Ui, s: &mut Session) {
    let mut length = s.length();
    let response = ui.add(
        egui::DragValue::new(&mut length)
            .range(1..=100_000)
            .prefix("Length "),
    );
    if response.changed() {
        s.set_length(length);
        s.frame = s.frame.min(length);
    }
    if response.drag_stopped() || response.lost_focus() {
        s.end_gesture();
    }
    let mut fps = s.doc.settings.fps;
    widgets::frame_rate(ui, "timeline-fps", &mut fps);
    s.set_frame_rate(fps);
}

/// Keyframe: amber diamond (amber = time, plus a shape, D4.5).
fn key_diamond(painter: &egui::Painter, c: Pos2, r: f32) {
    let pts = vec![
        c + Vec2::new(0.0, -r),
        c + Vec2::new(r, 0.0),
        c + Vec2::new(0.0, r),
        c + Vec2::new(-r, 0.0),
    ];
    painter.add(egui::Shape::convex_polygon(
        pts,
        theme::ACCENT_TIME,
        Stroke::new(1.0, theme::BG_PANEL),
    ));
}

/// The amber frame tag on a ruler (D3.9).
fn frame_tag(painter: &egui::Painter, centre: Pos2, frame: u32) {
    let text = frame.to_string();
    let w = 10.0 + 7.0 * text.len() as f32;
    let r = Rect::from_center_size(centre, Vec2::new(w, 16.0));
    painter.rect_filled(r, theme::RADIUS_CONTROL as f32, theme::ACCENT_TIME);
    painter.text(
        r.center(),
        Align2::CENTER_CENTER,
        text,
        theme::numbers(),
        theme::BG_BASE,
    );
}

/// The Timeline ruler's beats for a frame rate: [small step, half second,
/// second], in frames. 24 fps: 6, 12, 24; 30 fps: 5, 15, 30; 25 fps: 5,
/// 25 (no whole half), 25; 12 fps: 3, 6, 12.
fn ruler_steps(fps: f32) -> [u32; 3] {
    let second = (fps.round() as u32).max(1);
    let half = if second.is_multiple_of(2) {
        second / 2
    } else {
        second
    };
    let small = [4, 6, 5, 3, 2]
        .into_iter()
        .find(|d| second.is_multiple_of(*d) && second / d >= 3)
        .map_or(half, |d| second / d);
    [small.max(1), half.max(1), second]
}

/// Indent per level of the node hierarchy, in the Timeline's name column.
const INDENT: f32 = 12.0;

/// One Timeline row: a layer, a node track (index into the shown node
/// tracks) or a sound clip.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Slot {
    Layer(usize),
    Node(usize),
    Sound(usize),
}

fn kind_of(s: &Session, id: NodeId) -> Option<&NodeKind> {
    s.doc.graph.node(id).map(|n| &n.kind)
}

/// The pegs above a layer's drawing (outermost first) and the effects
/// after it, in order.
fn layer_chain(s: &Session, layer: usize) -> (Vec<NodeId>, Vec<NodeId>) {
    let graph = &s.doc.graph;
    let node = s.doc.layers[layer].node;
    let mut pegs = Vec::new();
    let mut cur = node;
    while let Some(up) = graph.input(cur, 0) {
        if matches!(
            kind_of(s, up),
            Some(
                NodeKind::Peg(_)
                    | NodeKind::MeshWarp(_)
                    | NodeKind::Skeleton(_)
                    | NodeKind::Bone(_)
            )
        ) && !pegs.contains(&up)
        {
            pegs.push(up);
            cur = up;
        } else {
            break;
        }
    }
    pegs.reverse();
    let mut effects = Vec::new();
    let mut cur = node;
    while let Some(next) = graph
        .links()
        .iter()
        .find(|l| {
            l.from == cur && l.port == 0 && matches!(kind_of(s, l.to), Some(NodeKind::Effect(_)))
        })
        .map(|l| l.to)
    {
        if effects.contains(&next) || effects.len() > 32 {
            break;
        }
        effects.push(next);
        cur = next;
    }
    (pegs, effects)
}

/// Nodes that are part of some layer's hierarchy.
fn attached_nodes(s: &Session) -> std::collections::HashSet<NodeId> {
    (0..s.layer_count())
        .flat_map(|i| {
            let (pegs, effects) = layer_chain(s, i);
            pegs.into_iter().chain(effects)
        })
        .collect()
}

/// Timeline rows in hierarchy order: for each layer, its pegs above it
/// (outermost first, indented like parents), the layer, then its effects;
/// then sound; then the remaining nodes. A peg shared by several layers
/// shows once, above the first.
fn hierarchy_rows(s: &Session, tracks: &[NodeId], sounds: usize) -> Vec<(Slot, usize)> {
    let index = |id: NodeId| tracks.iter().position(|&x| x == id);
    let mut placed = std::collections::HashSet::new();
    let mut out = Vec::new();
    for i in 0..s.layer_count() {
        let (pegs, effects) = layer_chain(s, i);
        let mut depth = 0;
        for peg in pegs {
            if let Some(j) = index(peg) {
                if placed.insert(peg) {
                    out.push((Slot::Node(j), depth));
                }
            }
            depth += 1;
        }
        out.push((Slot::Layer(i), depth));
        for fx in effects {
            if let Some(j) = index(fx) {
                if placed.insert(fx) {
                    out.push((Slot::Node(j), depth + 1));
                }
            }
        }
    }
    out.extend((0..sounds).map(|k| (Slot::Sound(k), 0)));
    for (j, id) in tracks.iter().enumerate() {
        if !placed.contains(id) {
            out.push((Slot::Node(j), 0));
        }
    }
    out
}

/// "Add node" from a Timeline row: the new node goes in the nearest place
/// in the hierarchy that fits (a peg above, an effect after), so the
/// Timeline can work like a layer stack or like nodes.
fn add_node_menu(ui: &mut egui::Ui, s: &mut Session, target: NodeId) {
    ui.menu_button("Add node", |ui| {
        if ui.button("Drawing layer").clicked() {
            s.add_layer();
            ui.close();
        }
        ui.separator();
        for kind in super::node::addable_nodes() {
            if ui.button(super::node::kind_label(&kind)).clicked() {
                super::node::insert_near(s, target, kind);
                ui.close();
            }
        }
    });
}

fn sound_menu(ui: &mut egui::Ui, s: &mut Session, k: usize) {
    let muted = s.doc.sounds.get(k).is_some_and(|c| c.muted);
    if ui
        .button(if muted { "Unmute sound" } else { "Mute sound" })
        .clicked()
    {
        s.toggle_sound_mute(k);
        ui.close();
    }
    ui.separator();
    if ui.button("Delete sound").clicked() {
        s.delete_sound(k);
        ui.close();
    }
}

fn layer_menu(ui: &mut egui::Ui, s: &mut Session, t: &mut TimingState, row: usize) {
    if ui.button("Rename layer").clicked() {
        t.renaming = Some((row, s.layer_name(row).to_owned()));
        ui.close();
    }
    let vis = if s.layer_visible(row) {
        "Hide layer"
    } else {
        "Show layer"
    };
    if ui.button(vis).clicked() {
        s.toggle_visible(row);
        ui.close();
    }
    ui.separator();
    if ui
        .add_enabled(s.layer_count() > 1, egui::Button::new("Delete layer"))
        .clicked()
    {
        s.delete_layer(row);
        ui.close();
    }
}

/// Inline rename field over a layer name. Enter or clicking away keeps it,
/// Escape cancels (D8.6).
/// Harmony's Timeline Parameters column for a drawing layer: the drawing
/// exposed at the current frame. Drag sideways to cycle through the
/// layer's drawings, or double-click to type a drawing's name.
fn parameters_field(
    ui: &mut egui::Ui,
    painter: &egui::Painter,
    s: &mut Session,
    t: &mut TimingState,
    row: usize,
    rect: Rect,
) {
    let frame = s.frame;
    if let Some((_, text)) = t.swap_text.as_mut().filter(|(r, _)| *r == row) {
        let resp = ui.put(rect, egui::TextEdit::singleline(text).font(theme::dense()));
        resp.request_focus();
        let (enter, esc) = ui.input(|i| {
            (
                i.key_pressed(egui::Key::Enter),
                i.key_pressed(egui::Key::Escape),
            )
        });
        if esc {
            t.swap_text = None;
        } else if enter || resp.lost_focus() {
            let wanted = text.trim().to_lowercase();
            t.swap_text = None;
            let found = s
                .element_drawings(row)
                .into_iter()
                .find(|(_, n)| n.to_lowercase() == wanted);
            match found {
                Some((id, _)) => {
                    s.swap_drawing(row, frame, id);
                    s.end_gesture();
                }
                None if !wanted.is_empty() => {
                    s.notice = Some(format!("This layer has no drawing called {wanted}"));
                }
                None => {}
            }
        }
        return;
    }
    let exposed = s.doc.exposed(row, frame);
    let name = exposed
        .and_then(|d| s.doc.drawings.get(&d))
        .map(|d| d.name.clone());
    painter.rect_filled(rect, theme::RADIUS_CONTROL as f32, theme::BG_INPUT);
    widgets::fitted_text(
        ui,
        painter,
        rect.center(),
        Align2::CENTER_CENTER,
        name.as_deref().unwrap_or(""),
        theme::dense(),
        theme::TEXT_PRIMARY,
        rect.width() - 6.0,
    );
    if exposed.is_none() {
        return;
    }
    let resp = ui
        .interact(
            rect,
            ui.scope_id().with(("swap", row)),
            Sense::click_and_drag(),
        )
        .on_hover_cursor(egui::CursorIcon::ResizeHorizontal)
        .on_hover_text(
            "Drawing at this frame. Drag sideways to swap it, or double-click to type a name.",
        );
    if resp.drag_started() || resp.clicked() {
        s.select_graph_node(s.doc.layers[row].node, false);
    }
    if resp.double_clicked() {
        t.swap_text = Some((row, name.unwrap_or_default()));
        return;
    }
    let list = s.element_drawings(row);
    if resp.drag_started() {
        let start = list
            .iter()
            .position(|(id, _)| Some(*id) == exposed)
            .unwrap_or(0);
        t.swap_drag = Some((row, start, 0.0));
    }
    if resp.dragged() {
        if let Some((_, start, moved)) = t.swap_drag.as_mut().filter(|(r, ..)| *r == row) {
            *moved += resp.drag_delta().x;
            let steps = (*moved / PARAM_STEP).round() as isize;
            let i = start
                .saturating_add_signed(steps)
                .min(list.len().saturating_sub(1));
            if let Some((id, _)) = list.get(i) {
                s.swap_drawing(row, frame, *id);
            }
        }
    }
    if resp.drag_stopped() {
        t.swap_drag = None;
        s.end_gesture();
    }
}

fn rename_field(ui: &mut egui::Ui, s: &mut Session, t: &mut TimingState, rect: Rect) {
    let Some((row, text)) = t.renaming.as_mut() else {
        return;
    };
    let row = *row;
    let resp = ui.put(rect, egui::TextEdit::singleline(text).font(theme::body()));
    resp.request_focus();
    let (enter, esc) = ui.input(|i| {
        (
            i.key_pressed(egui::Key::Enter),
            i.key_pressed(egui::Key::Escape),
        )
    });
    if esc {
        t.renaming = None;
    } else if enter || resp.lost_focus() {
        let name = text.clone();
        t.renaming = None;
        s.rename_layer(row, &name);
    }
}

fn reveal_track(ui: &egui::Ui, row: Rect, reveal: bool) {
    let visible = ui.clip_rect();
    if reveal && (row.top() < visible.top() || row.bottom() > visible.bottom()) {
        let x = visible.center().x;
        ui.scroll_to_rect(
            Rect::from_min_max(Pos2::new(x, row.top()), Pos2::new(x, row.bottom())),
            None,
        );
    }
}

pub fn timeline(ui: &mut egui::Ui, s: &mut Session, t: &mut TimingState, profile: Profile) {
    header(ui, s, profile);
    bookmark_menu(ui, s, t);
    const HEAD: f32 = 176.0;
    const RULER: f32 = 28.0;
    let cw = theme::FRAME_CELL;
    let rh = theme::ROW;
    let rows = s.layer_count();
    let len = s.length();
    let reveal = s.reveal_node_track.take();
    let visible_node = super::node::selected_node(s);
    let mut node_tracks = Vec::new();
    let mut has_unanimated_nodes = false;
    for (id, node) in s.doc.graph.nodes() {
        use symphony_core::graph::NodeKind;
        if matches!(node.kind, NodeKind::Drawing { .. }) {
            continue;
        }
        let mut frames = std::collections::BTreeSet::new();
        frames.extend(
            s.doc
                .graph
                .keys(id)
                .into_iter()
                .flat_map(|keys| keys.keys().copied()),
        );
        match &node.kind {
            NodeKind::Peg(peg) | NodeKind::Skeleton(peg) => {
                for value in [&peg.x, &peg.y, &peg.z, &peg.rotation]
                    .into_iter()
                    .chain(peg.scale_x.iter())
                    .chain(peg.scale_y.iter())
                    .chain(peg.tilt.iter().flat_map(|angles| angles.iter()))
                {
                    frames.extend(value.keys().iter().map(|key| key.frame));
                }
            }
            NodeKind::Bone(bone) => {
                for value in bone.values() {
                    frames.extend(value.keys().iter().map(|key| key.frame));
                }
            }
            NodeKind::MeshWarp(warp) => {
                for value in [&warp.x, &warp.y, &warp.width, &warp.height]
                    .into_iter()
                    .chain(warp.points.iter().flatten())
                {
                    frames.extend(value.keys().iter().map(|key| key.frame));
                }
            }
            NodeKind::Chart(chart) => frames.extend(chart.key_frames()),
            NodeKind::Display => {
                let motion = &s.doc.camera_motion;
                for value in [&s.doc.camera_fov, &motion.x, &motion.y, &motion.dolly] {
                    frames.extend(value.keys().iter().map(|key| key.frame));
                }
            }
            _ => {}
        }
        has_unanimated_nodes |= frames.is_empty();
        node_tracks.push((id, super::node::node_name(node), frames));
    }
    if has_unanimated_nodes {
        ui.toggle_value(&mut t.show_unanimated_nodes, "Show all node tracks")
            .on_hover_text("Animated nodes and the selected node are always shown.");
    }
    // Pegs and effects that belong to a layer always show: they're part
    // of the layer's hierarchy. The filter is for loose nodes.
    let attached = attached_nodes(s);
    node_tracks.retain(|(id, _, frames)| {
        t.show_unanimated_nodes
            || !frames.is_empty()
            || Some(*id) == visible_node
            || attached.contains(id)
    });
    // Own labels because timeline interactions can mutate the document.
    let node_tracks: Vec<_> = node_tracks
        .into_iter()
        .map(|(id, name, frames)| (id, name.to_owned(), frames))
        .collect();
    let sound_rows = s.doc.sounds.len();
    let track_count = rows + node_tracks.len() + sound_rows;
    // Where each row goes, following the node hierarchy.
    let track_ids: Vec<NodeId> = node_tracks.iter().map(|(id, _, _)| *id).collect();
    let layout = hierarchy_rows(s, &track_ids, sound_rows);
    let mut layer_slot = vec![0usize; rows];
    let mut layer_depth = vec![0usize; rows];
    let mut node_slot = vec![0usize; node_tracks.len()];
    let mut node_depth = vec![0usize; node_tracks.len()];
    let mut sound_slot = vec![0usize; sound_rows];
    for (slot, &(row, depth)) in layout.iter().enumerate() {
        match row {
            Slot::Layer(i) => {
                layer_slot[i] = slot;
                layer_depth[i] = depth;
            }
            Slot::Node(j) => {
                node_slot[j] = slot;
                node_depth[j] = depth;
            }
            Slot::Sound(k) => sound_slot[k] = slot,
        }
    }
    let width = HEAD + len as f32 * cw + 16.0;
    let lane_height = if s.doc.bookmarks.iter().any(|bookmark| bookmark.start <= len) {
        24.0
    } else {
        0.0
    };
    let header_height = RULER + lane_height;

    let out = egui::ScrollArea::both()
        .auto_shrink([false, false])
        .show_viewport(ui, |ui, scroll_viewport| {
            let size = Vec2::new(
                width.max(ui.available_width()),
                (header_height + track_count as f32 * rh + 8.0).max(ui.available_height()),
            );
            let (resp, viewport_painter) = ui.allocate_painter(size, Sense::click_and_drag());
            let o = resp.rect.min;
            let frame_x = |f: u32| o.x + HEAD + (f - 1) as f32 * cw;
            let rows_bottom = o.y + header_height + track_count as f32 * rh;
            // The ruler shares the tracks' horizontal scroll origin.
            let viewport = ui.clip_rect();
            let lane_top = o.y + scroll_viewport.top().max(0.0);
            let ruler_top = lane_top + lane_height;
            let ruler_bottom = ruler_top + RULER;
            let ruler = viewport_painter.with_clip_rect(Rect::from_min_max(
                Pos2::new(viewport.left(), ruler_top),
                Pos2::new(viewport.right(), ruler_bottom),
            ));
            if lane_height > 0.0 {
                let lane = viewport_painter.with_clip_rect(Rect::from_min_max(
                    Pos2::new(viewport.left(), lane_top),
                    Pos2::new(viewport.right(), ruler_top),
                ));
                let mut jump = None;
                for bookmark in s.doc.bookmarks.iter().filter(|entry| entry.start <= len) {
                    let x = frame_x(bookmark.start.max(1));
                    let available = viewport.right() - x;
                    let span =
                        (bookmark.end.min(len).saturating_sub(bookmark.start) + 1) as f32 * cw;
                    let marker = Rect::from_min_size(
                        Pos2::new(x, lane_top + 2.0),
                        Vec2::new(span.max(68.0).min(available.max(0.0)), lane_height - 4.0),
                    );
                    let clipped = marker.intersect(lane.clip_rect());
                    if !clipped.is_positive() {
                        continue;
                    }
                    let colour = bookmark.colour.map_or(theme::ACCENT_SELECT, |[r, g, b]| {
                        egui::Color32::from_rgb(r, g, b)
                    });
                    lane.rect_filled(marker, theme::RADIUS_CONTROL, theme::BG_RAISED);
                    lane.rect_filled(
                        Rect::from_min_size(marker.min, Vec2::new(3.0, marker.height())),
                        1.0,
                        colour,
                    );
                    widgets::fitted_text(
                        ui,
                        &lane,
                        marker.left_center() + Vec2::new(7.0, 0.0),
                        Align2::LEFT_CENTER,
                        &bookmark.name,
                        theme::dense(),
                        theme::TEXT_PRIMARY,
                        marker.width() - 10.0,
                    );
                    let response = ui.interact(
                        clipped,
                        resp.id.with(("bookmark", bookmark.id)),
                        Sense::click(),
                    );
                    if response.clicked() {
                        jump = Some(bookmark.start.max(1));
                    }
                    if response.double_clicked() {
                        t.bookmark = Some((s.key.clone(), true, bookmark.clone()));
                    }
                    if !bookmark.notes.is_empty() {
                        response.on_hover_text(&bookmark.notes);
                    }
                }
                if let Some(frame) = jump {
                    s.frame = frame;
                }
            }
            let track_clip = Rect::from_min_max(
                Pos2::new(viewport.left(), ruler_bottom.min(viewport.bottom())),
                viewport.max,
            );
            ui.set_clip_rect(track_clip);
            let painter = viewport_painter.with_clip_rect(track_clip);

            // Beats follow the frame rate: the second, its half and a
            // small step (24 fps: 24, 12, 6; 30 fps: 30, 15, 5). Lines and
            // numbers sit on the frame's centre, like the playhead.
            let [small, half, second] = ruler_steps(s.doc.settings.fps);
            // Number every small step if there's room, else every half.
            let label_step = if small as f32 * cw >= 22.0 {
                small
            } else {
                half
            };
            for f in 1..=len {
                let x = frame_x(f) + cw / 2.0;
                let level = if f % second == 0 {
                    3
                } else if f % half == 0 {
                    2
                } else if f % small == 0 {
                    1
                } else {
                    0
                };
                if level > 0 {
                    let alpha = [0.0, 0.35, 0.6, 1.0][level];
                    painter.line_segment(
                        [Pos2::new(x, o.y + header_height), Pos2::new(x, rows_bottom)],
                        Stroke::new(1.0, theme::LINE_STRONG.gamma_multiply(alpha)),
                    );
                }
                let top = match level {
                    0 => 22.0,
                    1 => 18.0,
                    _ => 15.0,
                };
                ruler.line_segment(
                    [
                        Pos2::new(x, ruler_top + top),
                        Pos2::new(x, ruler_bottom - 2.0),
                    ],
                    Stroke::new(1.0, theme::LINE_STRONG),
                );
                if f == 1 || f % label_step == 0 {
                    ruler.text(
                        Pos2::new(x, ruler_top + 4.0),
                        Align2::CENTER_TOP,
                        f.to_string(),
                        theme::numbers(),
                        if level == 3 {
                            theme::TEXT_PRIMARY
                        } else {
                            theme::TEXT_SECONDARY
                        },
                    );
                }
            }

            for i in 0..rows {
                let y = o.y + header_height + layer_slot[i] as f32 * rh;
                let indent = layer_depth[i] as f32 * INDENT;
                let node = s.doc.layers[i].node;
                let sel = visible_node == Some(node);
                let visible = s.layer_visible(i);
                let row = Rect::from_min_size(
                    Pos2::new(o.x + 4.0, y + 1.0),
                    Vec2::new(width - 8.0, rh - 2.0),
                );
                reveal_track(ui, row, reveal == Some(node));
                if sel {
                    painter.rect_filled(row, theme::RADIUS_CONTROL as f32, theme::BG_RAISED);
                    let edge = Rect::from_min_size(row.min, Vec2::new(3.0, row.height()));
                    painter.rect_filled(
                        edge,
                        CornerRadius {
                            nw: 2,
                            sw: 2,
                            ne: 0,
                            se: 0,
                        },
                        theme::ACCENT_SELECT,
                    );
                }
                let eye = if visible { Icon::Eye } else { Icon::EyeOff };
                let eye_rect = Rect::from_center_size(
                    Pos2::new(o.x + 22.0 + indent, y + rh / 2.0),
                    Vec2::splat(16.0),
                );
                icons::paint(
                    &painter,
                    eye_rect,
                    eye,
                    if visible {
                        theme::TEXT_SECONDARY
                    } else {
                        theme::TEXT_DISABLED
                    },
                );
                if !matches!(t.renaming, Some((r, _)) if r == i) {
                    let name_col = if visible {
                        theme::TEXT_PRIMARY
                    } else {
                        theme::TEXT_SECONDARY
                    };
                    widgets::fitted_text(
                        ui,
                        &painter,
                        Pos2::new(o.x + 40.0 + indent, y + rh / 2.0),
                        Align2::LEFT_CENTER,
                        s.layer_name(i),
                        theme::body(),
                        name_col,
                        HEAD - 56.0 - PARAM_W - indent,
                    );
                }
                parameters_field(
                    ui,
                    &painter,
                    s,
                    t,
                    i,
                    Rect::from_min_size(
                        Pos2::new(o.x + HEAD - PARAM_W - 6.0, y + 4.0),
                        Vec2::new(PARAM_W, rh - 8.0),
                    ),
                );

                if let Some(col) = s.doc.column(i) {
                    let mut f = 1;
                    while f <= len {
                        let Some(d) = col.get(f) else {
                            f += 1;
                            continue;
                        };
                        let start = f;
                        while f <= len && col.get(f) == Some(d) {
                            f += 1;
                        }
                        let block = Rect::from_min_max(
                            Pos2::new(frame_x(start) + 1.0, y + 5.0),
                            Pos2::new(frame_x(f) - 1.0, y + rh - 5.0),
                        );
                        let fill = if sel {
                            theme::LINE_STRONG
                        } else {
                            theme::BG_HOVER
                        };
                        painter.rect_filled(block, theme::RADIUS_CONTROL as f32, fill);
                        let name = s
                            .doc
                            .drawings
                            .get(&d)
                            .map(|d| d.name.as_str())
                            .unwrap_or("?");
                        // The drawing name must stay readable: step it past the
                        // playhead and any keyframe diamond it would sit under.
                        let text_w = 7.0 * name.len() as f32;
                        let mut obstacles: Vec<f32> = (start..f)
                            .filter(|fr| s.doc.graph.has_key(node, *fr) || *fr == s.frame)
                            .map(|fr| frame_x(fr) + cw / 2.0)
                            .collect();
                        obstacles.sort_by(f32::total_cmp);
                        let mut x = block.left() + 5.0;
                        for ob in obstacles {
                            if ob + 6.0 > x - 2.0 && ob - 6.0 < x + text_w {
                                x = ob + 7.0;
                            }
                        }
                        if x + text_w < block.right() - 2.0 {
                            painter.text(
                                Pos2::new(x, block.center().y),
                                Align2::LEFT_CENTER,
                                name,
                                theme::dense(),
                                theme::TEXT_PRIMARY,
                            );
                        }
                    }
                }
                for f in s
                    .doc
                    .graph
                    .keys(node)
                    .into_iter()
                    .flat_map(|keys| keys.keys().copied())
                    .filter(|f| (1..=len).contains(f))
                {
                    key_diamond(
                        &painter,
                        Pos2::new(frame_x(f) + cw / 2.0, y + rh / 2.0),
                        4.5,
                    );
                }
            }

            // Sound clips: a row each, with the waveform from their frame.
            let fps = s.doc.settings.fps.max(1.0);
            for (k, clip) in s.doc.sounds.iter().enumerate() {
                // Right under the layers, where it's easy to see.
                let y = o.y + header_height + sound_slot[k] as f32 * rh;
                widgets::fitted_text(
                    ui,
                    &painter,
                    Pos2::new(o.x + 18.0, y + rh / 2.0),
                    Align2::LEFT_CENTER,
                    &format!("♪ {}", clip.name),
                    theme::body(),
                    if clip.muted {
                        theme::TEXT_DISABLED
                    } else {
                        theme::TEXT_SECONDARY
                    },
                    HEAD - 30.0,
                );
                let wave = s
                    .doc
                    .media_path(clip.media)
                    .and_then(|p| crate::audio::sound(&p, ui.ctx()));
                let Some(wave) = wave else { continue };
                let bins_per_frame = crate::audio::PEAKS_PER_SECOND / fps;
                let ink = if clip.muted {
                    theme::TEXT_DISABLED
                } else {
                    theme::ACCENT_TIME.gamma_multiply(0.75)
                };
                let frames = (wave.seconds() * fps).ceil() as u32;
                for f in 0..frames {
                    let frame = clip.start + f;
                    if frame > len {
                        break;
                    }
                    let a = (f as f32 * bins_per_frame) as usize;
                    let b = (((f + 1) as f32 * bins_per_frame) as usize).max(a + 1);
                    let peak = wave.peaks[a.min(wave.peaks.len())..b.min(wave.peaks.len())]
                        .iter()
                        .fold(0.0f32, |m, p| m.max(*p));
                    let x = frame_x(frame) + cw / 2.0;
                    let half = (peak * (rh / 2.0 - 3.0)).max(0.5);
                    painter.line_segment(
                        [
                            Pos2::new(x, y + rh / 2.0 - half),
                            Pos2::new(x, y + rh / 2.0 + half),
                        ],
                        Stroke::new((cw * 0.6).clamp(1.0, 6.0), ink),
                    );
                }
            }
            for (index, (node, name, frames)) in node_tracks.iter().enumerate() {
                let y = o.y + header_height + node_slot[index] as f32 * rh;
                let indent = node_depth[index] as f32 * INDENT;
                let row = Rect::from_min_size(
                    Pos2::new(o.x + 4.0, y + 1.0),
                    Vec2::new(width - 8.0, rh - 2.0),
                );
                reveal_track(ui, row, reveal == Some(*node));
                if visible_node == Some(*node) {
                    painter.rect_filled(row, theme::RADIUS_CONTROL, theme::BG_RAISED);
                }
                widgets::fitted_text(
                    ui,
                    &painter,
                    Pos2::new(o.x + 18.0 + indent, y + rh / 2.0),
                    Align2::LEFT_CENTER,
                    name,
                    theme::body(),
                    theme::TEXT_SECONDARY,
                    HEAD - 30.0 - indent,
                );
                // A chart's span: what you grab to move it in time.
                if let Some(symphony_core::graph::NodeKind::Chart(chart)) =
                    s.doc.graph.node(*node).map(|n| &n.kind)
                {
                    if let (Some(&a), Some(&b)) = (frames.first(), frames.last()) {
                        let colour = super::chart_edit::colour(chart, *node);
                        let span = Rect::from_min_max(
                            Pos2::new(frame_x(a.min(len)) + 2.0, y + rh / 2.0 - 5.0),
                            Pos2::new(frame_x(b.min(len)) + cw - 2.0, y + rh / 2.0 + 5.0),
                        );
                        painter.rect_filled(span, 5.0, colour.gamma_multiply(0.28));
                    }
                }
                for f in frames.iter().copied().filter(|f| (1..=len).contains(f)) {
                    key_diamond(
                        &painter,
                        Pos2::new(frame_x(f) + cw / 2.0, y + rh / 2.0),
                        4.5,
                    );
                }
            }

            // A layer that was just added or moved glows briefly, so the
            // eye finds it instead of rows simply jumping.
            let now = ui.input(|i| i.time);
            let order: Vec<symphony_core::id::Id> = s.doc.layers.iter().map(|l| l.column).collect();
            if !t.last_order.is_empty() && order != t.last_order {
                for (i, id) in order.iter().enumerate() {
                    if t.last_order.get(i) != Some(id) && !t.last_order.is_empty() {
                        let moved_or_new = t.last_order.iter().position(|c| c == id) != Some(i);
                        if moved_or_new && s.selected == i {
                            t.arrived.retain(|(c, _)| c != id);
                            t.arrived.push((*id, now));
                        }
                    }
                }
            }
            t.last_order = order.clone();
            t.arrived.retain(|(_, at)| now - at < 0.8);
            for (id, at) in &t.arrived {
                if let Some(row) = order.iter().position(|c| c == id) {
                    let k = 1.0 - ((now - at) / 0.8) as f32;
                    let y = o.y + header_height + layer_slot[row] as f32 * rh;
                    painter.rect_filled(
                        Rect::from_min_size(
                            Pos2::new(o.x + 4.0, y + 1.0),
                            Vec2::new(width - 8.0, rh - 2.0),
                        ),
                        theme::RADIUS_CONTROL,
                        theme::ACCENT_SELECT.gamma_multiply(0.22 * k * k),
                    );
                    ui.ctx().request_repaint();
                }
            }
            // The playback range, as a band along the ruler.
            if let Some((a, b)) = s.play_range {
                let (a, b) = (a.clamp(1, len), b.clamp(1, len));
                let band = Rect::from_min_max(
                    Pos2::new(frame_x(a.min(b)), ruler_top + 2.0),
                    Pos2::new(frame_x(a.max(b)) + cw, ruler_bottom - 2.0),
                );
                ruler.rect_filled(
                    band,
                    theme::RADIUS_CONTROL,
                    theme::ACCENT_TIME.gamma_multiply(0.16),
                );
            }
            // A chart's track between its first and last frame is a handle
            // for the whole chart: drag it to move the chart in time.
            let chart_span_at = |p: Pos2| -> Option<(NodeId, u32)> {
                if p.x < o.x + HEAD || p.y < ruler_bottom {
                    return None;
                }
                let slot = ((p.y - o.y - header_height) / rh).floor();
                if slot < 0.0 {
                    return None;
                }
                let j = node_slot.iter().position(|&n| n == slot as usize)?;
                let (id, _, frames) = &node_tracks[j];
                let s_node = s.doc.graph.node(*id)?;
                if !matches!(s_node.kind, symphony_core::graph::NodeKind::Chart(_)) {
                    return None;
                }
                let frame = ((p.x - o.x - HEAD) / cw).floor() as i64 + 1;
                let (first, last) = (*frames.first()?, *frames.last()?);
                (i64::from(first)..=i64::from(last))
                    .contains(&frame)
                    .then_some((*id, frame as u32))
            };
            // Determine the gesture from the press, even when its current
            // pointer has crossed into the headers or past the last frame.
            if resp.drag_started_by(egui::PointerButton::Primary) {
                let origin = ui.input(|i| i.pointer.press_origin());
                t.chart_drag = origin
                    .and_then(chart_span_at)
                    .map(|(id, frame)| (id, frame, 0));
                t.scrubbing = t.chart_drag.is_none()
                    && origin.is_some_and(|p| p.x >= o.x + HEAD && p.y >= ruler_top);
            }
            if let Some((id, from, moved)) = &mut t.chart_drag {
                if let Some(p) = resp.interact_pointer_pos() {
                    let frame = ((p.x - o.x - HEAD) / cw).floor() as i64 + 1;
                    let step = frame - i64::from(*from) - *moved;
                    if step != 0 {
                        let mut done = 0;
                        super::chart_edit::edit(
                            s,
                            *id,
                            "Move chart in time",
                            Some(format!("chart-shift-{}", id.0)),
                            |c| done = c.shift_frames(step),
                        );
                        *moved += done;
                    }
                }
                ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
                if resp.drag_stopped() || !ui.input(|i| i.pointer.primary_down()) {
                    s.end_gesture();
                    t.chart_drag = None;
                }
            } else if resp.hover_pos().and_then(chart_span_at).is_some() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
            }
            let scrub = t.scrubbing && (resp.dragged() || resp.drag_stopped());
            if let Some(p) = resp.interact_pointer_pos().filter(|p| {
                scrub
                    || (resp.clicked_by(egui::PointerButton::Primary)
                        && p.x >= o.x + HEAD
                        && p.y >= ruler_top)
            }) {
                let frame =
                    (((p.x - o.x - HEAD) / cw).floor() as i64 + 1).clamp(1, i64::from(len)) as u32;
                s.frame = frame;
            }
            if resp.drag_stopped() || !ui.input(|i| i.pointer.primary_down()) {
                t.scrubbing = false;
            }
            // A click jumps straight to the frame (only that frame is ever
            // shown or rendered); just the line glides there (Select
            // motion). Dragging and playing keep it exactly on the frame:
            // scrubbing must feel snappy, not trail the mouse.
            let target = s.frame.min(len) as f32;
            let playhead = egui::Id::unique("timeline-playhead");
            let shown = if s.playing || scrub || s.frame == 1 {
                symphony_ui::motion::start(ui.ctx(), playhead, target);
                target
            } else {
                symphony_ui::motion::value(
                    ui.ctx(),
                    playhead,
                    target,
                    symphony_ui::motion::Motion::Select,
                )
            };
            let whole = (shown.floor() as u32).clamp(1, len);
            let cx = frame_x(whole)
                + (frame_x((whole + 1).min(len)) - frame_x(whole)) * shown.fract()
                + cw / 2.0;
            painter.line_segment(
                [
                    Pos2::new(cx, ruler_bottom),
                    Pos2::new(cx, rows_bottom.max(ruler_bottom)),
                ],
                Stroke::new(1.5, theme::ACCENT_TIME),
            );
            ruler.line_segment(
                [
                    Pos2::new(cx, ruler_bottom - 4.0),
                    Pos2::new(cx, ruler_bottom),
                ],
                Stroke::new(1.5, theme::ACCENT_TIME),
            );
            frame_tag(&ruler, Pos2::new(cx, ruler_bottom - 12.0), s.frame);

            let slot_at = |p: Pos2| -> Option<Slot> {
                if p.y < ruler_bottom {
                    return None;
                }
                let r = ((p.y - o.y - header_height) / rh).floor();
                (r >= 0.0)
                    .then(|| layout.get(r as usize).map(|&(row, _)| row))
                    .flatten()
            };
            let row_at = |p: Pos2| -> Option<usize> {
                match slot_at(p) {
                    Some(Slot::Layer(i)) => Some(i),
                    _ => None,
                }
            };
            // Drag a layer by its name to move it in the stack; a line
            // shows where it will land.
            if resp.drag_started_by(egui::PointerButton::Primary) && !t.scrubbing {
                let origin = ui.input(|i| i.pointer.press_origin());
                if let (Some(p), Some(row)) = (origin, origin.and_then(row_at)) {
                    let x = p.x - o.x;
                    if (32.0..HEAD - PARAM_W).contains(&x) {
                        t.layer_drag = Some(row);
                    }
                }
            }
            if let (Some(from), Some(p)) = (t.layer_drag, resp.interact_pointer_pos()) {
                // The gap in the layer stack: layers whose row centre is
                // above the pointer come before it.
                let at = (p.y - o.y - header_height) / rh;
                let gap = (0..rows)
                    .filter(|&i| (layer_slot[i] as f32 + 0.5) < at)
                    .count();
                let line_slot = if gap < rows {
                    layer_slot[gap]
                } else {
                    layer_slot.iter().max().map_or(0, |m| m + 1)
                };
                let y = o.y + header_height + line_slot as f32 * rh;
                painter.line_segment(
                    [Pos2::new(o.x + 8.0, y), Pos2::new(o.x + width - 8.0, y)],
                    Stroke::new(2.0, theme::ACCENT_SELECT),
                );
                if resp.drag_stopped() {
                    t.layer_drag = None;
                    let to = if gap > from { gap - 1 } else { gap };
                    s.move_layer(from, to.min(rows.saturating_sub(1)));
                }
            }
            if !resp.dragged() && !resp.drag_stopped() {
                t.layer_drag = None;
            }
            if let Some(p) = resp
                .interact_pointer_pos()
                .filter(|_| t.layer_drag.is_none())
            {
                let rel = p - o;
                if let Some(row) = row_at(p) {
                    if resp.clicked() || resp.drag_started() {
                        if rel.x < 32.0 && resp.clicked() {
                            s.toggle_visible(row);
                        }
                        s.select_graph_node(s.doc.layers[row].node, false);
                    }
                    if resp.double_clicked() && rel.x < HEAD - PARAM_W - 8.0 {
                        t.renaming = Some((row, s.layer_name(row).to_owned()));
                    }
                } else if resp.clicked() || resp.drag_started() {
                    if let Some(Slot::Node(j)) = slot_at(p) {
                        s.select_graph_node(node_tracks[j].0, false);
                    }
                }
            }
            // Right-click a layer or node: its menu, with Add node, so the
            // Timeline works as a layer stack or as nodes.
            if resp.secondary_clicked() {
                t.menu_row = None;
                t.menu_node = None;
                t.menu_sound = None;
                match resp.interact_pointer_pos().and_then(slot_at) {
                    Some(Slot::Layer(r)) => {
                        t.menu_row = Some(r);
                        if let Some(p) = resp.interact_pointer_pos().filter(|p| p.x >= o.x + HEAD) {
                            s.frame = (((p.x - o.x - HEAD) / cw) as u32 + 1).clamp(1, len);
                        }
                        t.menu_node = Some(s.doc.layers[r].node);
                    }
                    Some(Slot::Node(j)) => t.menu_node = Some(node_tracks[j].0),
                    Some(Slot::Sound(k)) => t.menu_sound = Some(k),
                    _ => {}
                }
                if let Some(node) = t.menu_node {
                    s.select_graph_node(node, false);
                }
            }
            if t.menu_node.is_some() || t.menu_sound.is_some() {
                resp.context_menu(|ui| {
                    if let Some(k) = t.menu_sound {
                        sound_menu(ui, s, k);
                        return;
                    }
                    if let Some(node) = t.menu_node {
                        add_node_menu(ui, s, node);
                    }
                    if let Some(row) = t.menu_row {
                        ui.separator();
                        drawing_clipboard_menu(ui, s, profile);
                        ui.separator();
                        layer_menu(ui, s, t, row);
                    } else if let Some(node) = t.menu_node {
                        // A peg, Mesh Warp or effect row.
                        ui.separator();
                        let why = super::node::undeletable(s, node);
                        let b = ui.add_enabled(why.is_none(), egui::Button::new("Delete node"));
                        let b = match why {
                            Some(w) => b.on_disabled_hover_text(w),
                            None => b,
                        };
                        if b.clicked() {
                            super::node::delete_nodes(s, &[node]);
                            ui.close();
                        }
                    }
                });
            }

            if let Some((row, _)) = t.renaming {
                let y =
                    o.y + header_height + layer_slot.get(row).copied().unwrap_or(row) as f32 * rh;
                rename_field(
                    ui,
                    s,
                    t,
                    Rect::from_min_size(
                        Pos2::new(o.x + 36.0, y + 3.0),
                        Vec2::new(HEAD - 52.0 - PARAM_W, rh - 6.0),
                    ),
                );
            }
            ui.set_clip_rect(viewport);
        });
    // More tracks below (or above): a soft fade at that edge says so.
    let view = out.inner_rect;
    let (offset, content) = (out.state.clamped_offset().y, out.content_size.y);
    let fade = 24.0_f32.min(view.height() * 0.25);
    if offset + view.height() < content - 1.0 {
        theme::fade_strip(
            ui.painter(),
            Rect::from_min_max(Pos2::new(view.left(), view.bottom() - fade), view.max),
            theme::GLASS_PANEL,
            false,
        );
    }
}

pub fn xsheet(ui: &mut egui::Ui, s: &mut Session, t: &mut TimingState, profile: Profile) {
    header(ui, s, profile);
    const NUM: f32 = 48.0;
    const CW: f32 = 72.0;
    const HEAD: f32 = 28.0;
    let rh = theme::XSHEET_ROW;
    let cols = s.layer_count();
    let len = s.length();
    let width = NUM + cols as f32 * CW + 8.0;

    egui::ScrollArea::both()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let size = Vec2::new(
                width.max(ui.available_width()),
                HEAD + len as f32 * rh + 8.0,
            );
            let (resp, painter) = ui.allocate_painter(size, Sense::click_and_drag());
            let o = resp.rect.min;
            let row_y = |f: u32| o.y + HEAD + (f - 1) as f32 * rh;

            for f in 1..=len {
                let y = row_y(f);
                if f % 2 == 0 {
                    painter.rect_filled(
                        Rect::from_min_size(
                            Pos2::new(o.x + NUM, y),
                            Vec2::new(cols as f32 * CW, rh),
                        ),
                        0.0,
                        theme::BG_RAISED,
                    );
                }
                if f % 8 == 0 {
                    // Paper exposure sheets mark every 8 frames; we keep that rhythm.
                    painter.line_segment(
                        [
                            Pos2::new(o.x + NUM, y + rh),
                            Pos2::new(o.x + NUM + cols as f32 * CW, y + rh),
                        ],
                        Stroke::new(1.0, theme::LINE_STRONG),
                    );
                }
                if f != s.frame {
                    painter.text(
                        Pos2::new(o.x + NUM - 10.0, y + rh / 2.0),
                        Align2::RIGHT_CENTER,
                        f.to_string(),
                        theme::numbers(),
                        theme::TEXT_SECONDARY,
                    );
                }
            }

            for i in 0..cols {
                let x = o.x + NUM + i as f32 * CW;
                let sel = i == s.selected;
                let head = Rect::from_min_size(
                    Pos2::new(x + 2.0, o.y + 2.0),
                    Vec2::new(CW - 4.0, HEAD - 6.0),
                );
                painter.rect_filled(
                    head,
                    theme::RADIUS_CONTROL as f32,
                    if sel {
                        theme::ACCENT_SELECT_FILL
                    } else {
                        theme::BG_RAISED
                    },
                );
                if !matches!(t.renaming, Some((r, _)) if r == i) {
                    let text = if sel {
                        theme::ON_ACCENT
                    } else if s.layer_visible(i) {
                        theme::TEXT_PRIMARY
                    } else {
                        theme::TEXT_SECONDARY
                    };
                    widgets::fitted_text(
                        ui,
                        &painter,
                        head.center(),
                        Align2::CENTER_CENTER,
                        s.layer_name(i),
                        theme::body(),
                        text,
                        head.width() - 8.0,
                    );
                }
                painter.line_segment(
                    [Pos2::new(x, o.y + HEAD), Pos2::new(x, o.y + size.y)],
                    Stroke::new(1.0, theme::LINE_SUBTLE),
                );

                let Some(col) = s.doc.column(i) else { continue };
                for f in 1..=len {
                    let Some(d) = col.get(f) else { continue };
                    let y = row_y(f);
                    if f > 1 && col.get(f - 1) == Some(d) {
                        // Held cell: a vertical line, like a paper exposure sheet.
                        let cx = x + CW / 2.0;
                        painter.line_segment(
                            [Pos2::new(cx, y), Pos2::new(cx, y + rh)],
                            Stroke::new(1.5, theme::TEXT_SECONDARY),
                        );
                    } else {
                        let name = s
                            .doc
                            .drawings
                            .get(&d)
                            .map(|d| d.name.as_str())
                            .unwrap_or("?");
                        painter.text(
                            Pos2::new(x + CW / 2.0, y + rh / 2.0),
                            Align2::CENTER_CENTER,
                            name,
                            theme::numbers(),
                            theme::TEXT_PRIMARY,
                        );
                    }
                }
            }

            let y = row_y(s.frame.min(len)) + rh / 2.0;
            painter.line_segment(
                [
                    Pos2::new(o.x + NUM, y),
                    Pos2::new(o.x + NUM + cols as f32 * CW, y),
                ],
                Stroke::new(1.0, theme::ACCENT_TIME.gamma_multiply(0.6)),
            );
            frame_tag(&painter, Pos2::new(o.x + NUM / 2.0 + 2.0, y), s.frame);

            let col_at = |p: Pos2| -> Option<usize> {
                let c = ((p.x - o.x - NUM) / CW).floor();
                (c >= 0.0 && (c as usize) < cols).then_some(c as usize)
            };
            if let Some(p) = resp.interact_pointer_pos() {
                let rel = p - o;
                if rel.y >= HEAD {
                    s.frame = (((rel.y - HEAD) / rh) as u32 + 1).clamp(1, len);
                }
                if let Some(c) = col_at(p) {
                    if resp.clicked() || resp.drag_started() {
                        s.select_graph_node(s.doc.layers[c].node, false);
                    }
                    if resp.double_clicked() && rel.y < HEAD {
                        t.renaming = Some((c, s.layer_name(c).to_owned()));
                    }
                }
            }
            if resp.secondary_clicked() {
                t.menu_row = resp.interact_pointer_pos().and_then(col_at);
                if let Some(c) = t.menu_row {
                    if let Some(p) = resp.interact_pointer_pos().filter(|p| p.y >= o.y + HEAD) {
                        s.frame = (((p.y - o.y - HEAD) / rh) as u32 + 1).clamp(1, len);
                    }
                    s.select_graph_node(s.doc.layers[c].node, false);
                }
            }
            if let Some(c) = t.menu_row {
                resp.context_menu(|ui| {
                    drawing_clipboard_menu(ui, s, profile);
                    ui.separator();
                    layer_menu(ui, s, t, c);
                });
            }
            if let Some((c, _)) = t.renaming {
                let x = o.x + NUM + c as f32 * CW;
                rename_field(
                    ui,
                    s,
                    t,
                    Rect::from_min_size(
                        Pos2::new(x + 2.0, o.y + 2.0),
                        Vec2::new(CW - 4.0, HEAD - 6.0),
                    ),
                );
            }
        });
}

#[cfg(test)]
mod hierarchy_tests {
    use super::*;

    /// A chart's Timeline track is a handle: dragging inside its span
    /// moves the whole chart in time, as one undo step, without scrubbing.
    #[test]
    fn dragging_a_chart_track_moves_the_chart_in_time() {
        use symphony_core::chart::{Chart, Role};
        use symphony_core::graph::NodeKind;
        let mut session = Session::new_scene();
        let mut chart = Chart::default();
        chart.set_point(3, [0.0, 0.0], Role::Key);
        chart.set_point(9, [50.0, 0.0], Role::Key);
        let id = session
            .doc
            .graph
            .add("Chart", NodeKind::Chart(Box::new(chart)));
        session.frame = 1;
        let mut harness = egui_kittest::Harness::builder()
            .with_size(Vec2::new(800.0, 380.0))
            .with_accessibility_check(false)
            .build_ui_state(
                |ui, (session, timing)| timeline(ui, session, timing, Profile::Symphony),
                (session, TimingState::default()),
            );
        harness.run_steps(3);
        // Find the chart's row: the cursor offers to grab its span.
        let x = |frame: f32| 8.0 + 176.0 + (frame - 1.0) * theme::FRAME_CELL + 8.0;
        let y = (0..95)
            .map(|i| i as f32 * 4.0)
            .find(|&y| {
                harness
                    .input_mut()
                    .events
                    .push(egui::Event::PointerMoved(Pos2::new(x(5.0), y)));
                harness.run_steps(1);
                harness.output().platform_output.cursor_icon == egui::CursorIcon::Grab
            })
            .expect("the chart's span is grabbable");
        let button = |pos, pressed| egui::Event::PointerButton {
            pos,
            pressed,
            button: egui::PointerButton::Primary,
            modifiers: egui::Modifiers::NONE,
        };
        let (press, to) = (Pos2::new(x(5.0), y), Pos2::new(x(8.0), y));
        harness.input_mut().events.push(button(press, true));
        harness.run_steps(1);
        for step in 1..=6 {
            let p = press.lerp(to, step as f32 / 6.0);
            harness
                .input_mut()
                .events
                .push(egui::Event::PointerMoved(p));
            harness.run_steps(1);
        }
        harness.input_mut().events.push(button(to, false));
        harness.run_steps(2);
        let frames = |s: &Session| match &s.doc.graph.node(id).unwrap().kind {
            NodeKind::Chart(c) => c.points.iter().map(|p| p.frame).collect::<Vec<_>>(),
            _ => unreachable!(),
        };
        let session = &mut harness.state_mut().0;
        assert_eq!(frames(session), vec![6, 12]);
        assert_eq!(session.frame, 1, "moving the chart doesn't scrub");
        session.undo();
        assert_eq!(frames(session), vec![3, 9]);
    }

    #[test]
    fn timeline_scrub_can_cross_into_headers_and_reach_frame_one() {
        let mut session = Session::new_scene();
        session.frame = 6;
        let mut harness = egui_kittest::Harness::builder()
            .with_size(Vec2::new(800.0, 380.0))
            .with_accessibility_check(false)
            .build_ui_state(
                |ui, (session, timing)| timeline(ui, session, timing, Profile::Symphony),
                (session, TimingState::default()),
            );
        harness.run_steps(3);
        let press = Pos2::new(320.0, 96.0);
        let moved = Pos2::new(144.0, 96.0);
        let button = |pos, pressed| egui::Event::PointerButton {
            pos,
            pressed,
            button: egui::PointerButton::Primary,
            modifiers: egui::Modifiers::NONE,
        };
        harness
            .input_mut()
            .events
            .extend([egui::Event::PointerMoved(press), button(press, true)]);
        harness.run_steps(1);
        harness
            .input_mut()
            .events
            .push(egui::Event::PointerMoved(moved));
        harness.run_steps(1);
        assert_eq!(harness.state().0.frame, 1);
        harness.input_mut().events.push(button(moved, false));
        harness.run_steps(1);
        assert_eq!(harness.state().0.frame, 1);
        assert!(!harness.state().1.scrubbing);
    }

    #[test]
    fn timeline_ruler_stays_above_scrolling_tracks() {
        let mut session = Session::new_scene();
        for _ in 0..12 {
            session.add_layer();
        }
        let track = session.layer_name(3).to_owned();
        let mut harness = egui_kittest::Harness::builder()
            .with_size(Vec2::new(800.0, 380.0))
            .with_accessibility_check(false)
            .build_ui_state(
                |ui, (session, timing)| timeline(ui, session, timing, Profile::Symphony),
                (session, TimingState::default()),
            );
        let text_y = |output: &egui::FullOutput, label: &str| {
            output
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::Shape::Text(text) if text.galley.text() == label => Some(text.pos.y),
                    _ => None,
                })
                .unwrap_or_else(|| panic!("missing timeline label {label}"))
        };
        harness.run_steps(3);
        let ruler_before = text_y(harness.output(), "6");
        let track_before = text_y(harness.output(), &track);
        harness.input_mut().events.extend([
            egui::Event::PointerMoved(Pos2::new(320.0, 220.0)),
            egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: Vec2::new(0.0, -56.0),
                phase: egui::TouchPhase::Move,
                modifiers: egui::Modifiers::NONE,
                source: egui::MouseWheelSource::Unknown,
            },
        ]);
        harness.run_steps(12);
        assert!((text_y(harness.output(), "6") - ruler_before).abs() < 0.1);
        assert!(text_y(harness.output(), &track) < track_before - 10.0);
    }

    #[test]
    fn ruler_beats_follow_the_frame_rate() {
        assert_eq!(ruler_steps(24.0), [6, 12, 24]);
        assert_eq!(ruler_steps(30.0), [5, 15, 30]);
        assert_eq!(ruler_steps(12.0), [3, 6, 12]);
        assert_eq!(ruler_steps(25.0), [5, 25, 25]);
        assert_eq!(ruler_steps(60.0), [15, 30, 60]);
    }
    use symphony_core::graph::Effect;

    /// Nodes added from the Timeline go to the nearest fitting place, and
    /// the Timeline lists each layer's pegs right above it.
    #[test]
    fn added_nodes_join_the_hierarchy_and_the_timeline_follows_it() {
        let mut s = Session::new_scene();
        let layer = s.doc.layers[0];
        let old_peg = s
            .doc
            .graph
            .input(layer.node, 0)
            .expect("layers start with a peg");
        // A peg on the drawing: a new parent between it and its old peg.
        super::super::node::insert_near(&mut s, layer.node, NodeKind::Peg(Default::default()));
        let new_peg = s.doc.graph.input(layer.node, 0).unwrap();
        assert_ne!(new_peg, old_peg);
        assert_eq!(s.doc.graph.input(new_peg, 0), Some(old_peg));
        // An effect from the (old) peg: after that layer's drawing.
        let out_before = s
            .doc
            .graph
            .links()
            .iter()
            .find(|l| l.from == layer.node)
            .copied()
            .unwrap();
        super::super::node::insert_near(
            &mut s,
            old_peg,
            NodeKind::Effect(Effect::Blur {
                radius: 2.0,
                mode: symphony_core::graph::BlurMode::Box,
            }),
        );
        let fx = s
            .doc
            .graph
            .links()
            .iter()
            .find(|l| l.from == layer.node)
            .unwrap()
            .to;
        assert!(matches!(kind_of(&s, fx), Some(NodeKind::Effect(_))));
        assert!(s
            .doc
            .graph
            .links()
            .iter()
            .any(|l| l.from == fx && l.to == out_before.to));
        // The rows: old peg, new peg, then the layer, then its effect.
        let (pegs, effects) = layer_chain(&s, 0);
        assert_eq!(pegs, vec![old_peg, new_peg]);
        assert_eq!(effects, vec![fx]);
        let tracks = vec![fx, new_peg, old_peg];
        let rows = hierarchy_rows(&s, &tracks, 0);
        assert_eq!(
            &rows[..4],
            &[
                (Slot::Node(2), 0),
                (Slot::Node(1), 1),
                (Slot::Layer(0), 2),
                (Slot::Node(0), 3)
            ]
        );
        // One undo each.
        s.undo();
        s.undo();
        assert_eq!(s.doc.graph.input(layer.node, 0), Some(old_peg));
    }
}
