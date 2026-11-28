//! Actions registry and per-profile shortcuts (docs/ENGINEERING.md E4.3–E4.6,
//! docs/DESIGN.md §11). Every shortcut in the app is defined here and
//! nowhere else.
//!
//! Harmony-profile bindings are checked against Harmony 27's published
//! default shortcuts, kept as facts in docs/reference/harmony-27-shortcuts.tsv (E4.4). The Symphony profile starts as a
//! copy of Harmony; any difference must be logged in DESIGN.md §11.1.

use eframe::egui::{self, Key, Modifiers};
use serde::{Deserialize, Serialize};

use crate::views::timing::TimelineAction;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Profile {
    Harmony,
    #[default]
    Symphony,
}

impl Profile {
    pub fn title(self) -> &'static str {
        match self {
            Profile::Harmony => "Harmony",
            Profile::Symphony => "Symphony",
        }
    }

    pub fn blurb(self) -> &'static str {
        match self {
            Profile::Harmony => "Shortcuts and workflow as in the latest Harmony.",
            Profile::Symphony => "Contextual shortcuts with plain-letter tool keys.",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Action {
    NewScene,
    Open,
    Save,
    SaveAs,
    /// Import anything: images, sequences, sound, video, brushes, scenes.
    Import,
    Undo,
    Redo,
    SelectAll,
    DeselectAll,
    Delete,
    Cut,
    Copy,
    Paste,
    FlipHorizontal,
    FlipVertical,
    PrevFrame,
    NextFrame,
    FirstFrame,
    LastFrame,
    PlayPause,
    IncreaseExposure,
    DecreaseExposure,
    IncreaseBrushSize,
    DecreaseBrushSize,
    CreateEmptyDrawing,
    AddLayer,
    ToggleOnionSkin,
    ResetView,
    /// Mirror the view left to right (the artwork isn't changed).
    MirrorView,
    /// Playback range: start here, end here, the whole scene, loop on/off.
    SetPlayStart,
    SetPlayEnd,
    ClearPlayRange,
    ToggleLoop,
    ZoomIn,
    ZoomOut,
    RotateCw,
    RotateCcw,
    ResetZoom,
    ResetRotation,
    ResetPan,
    OnionMorePrevious,
    OnionLessPrevious,
    OnionMoreNext,
    OnionLessNext,
    OnionPrevious1,
    OnionPrevious2,
    OnionPrevious3,
    OnionNoPrevious,
    OnionNext1,
    OnionNext2,
    OnionNext3,
    OnionNoNext,
    MaximiseView,
    ToolSelect,
    ToolTransform,
    ToolBrush,
    ToolLine,
    ToolText,
    ToolPencil,
    ToolEraser,
    ToolPaint,
    ToolPaintUnpainted,
    ToolRepaint,
    ToolUnpaint,
    ToolCloseGap,
    ShowStrokes,
    RespectProtectedColours,
    SwitchLineColourArt,
    SwitchOverlayUnderlay,
    PreviewArtLayers,
    CreateColourArt,
    SubstituteNext,
    SubstitutePrevious,
    GoToNextDrawing,
    GoToPreviousDrawing,
    Search,
    ToolDropper,
    ToolLineEditor,
    ToolHand,
    ToolZoom,
    SelectCutter,
    ToggleSelectedPoints,
    ArrangeNodes,
    ShowNodeProperties,
    RenderNodePreview,
    ResetNodeView,
    ToggleCameraEffects,
    ToggleAutoKey,
    ToggleLayerVisible,
    /// Smart Charts, while one is edited (selected, Transform tool).
    ChartRole,
    ChartHold,
    ChartEarlier,
    ChartLater,
    ChartPose,
    ChartEvenSpacing,
    ChartUnfollow,
    ChartsVisible,
    Timeline(TimelineAction),
}

impl Action {
    pub const ALL: [Action; 106] = [
        Action::NewScene,
        Action::Open,
        Action::Save,
        Action::SaveAs,
        Action::Import,
        Action::Undo,
        Action::Redo,
        Action::SelectAll,
        Action::DeselectAll,
        Action::Delete,
        Action::Cut,
        Action::Copy,
        Action::Paste,
        Action::FlipHorizontal,
        Action::FlipVertical,
        Action::PrevFrame,
        Action::NextFrame,
        Action::FirstFrame,
        Action::LastFrame,
        Action::PlayPause,
        Action::IncreaseExposure,
        Action::DecreaseExposure,
        Action::IncreaseBrushSize,
        Action::DecreaseBrushSize,
        Action::CreateEmptyDrawing,
        Action::AddLayer,
        Action::ToggleOnionSkin,
        Action::ResetView,
        Action::MirrorView,
        Action::SetPlayStart,
        Action::SetPlayEnd,
        Action::ClearPlayRange,
        Action::ToggleLoop,
        Action::ZoomIn,
        Action::ZoomOut,
        Action::RotateCw,
        Action::RotateCcw,
        Action::ResetZoom,
        Action::ResetRotation,
        Action::ResetPan,
        Action::OnionMorePrevious,
        Action::OnionLessPrevious,
        Action::OnionMoreNext,
        Action::OnionLessNext,
        Action::OnionPrevious1,
        Action::OnionPrevious2,
        Action::OnionPrevious3,
        Action::OnionNoPrevious,
        Action::OnionNext1,
        Action::OnionNext2,
        Action::OnionNext3,
        Action::OnionNoNext,
        Action::MaximiseView,
        Action::ToolSelect,
        Action::ToolTransform,
        Action::ToolBrush,
        Action::ToolLine,
        Action::ToolText,
        Action::ToolPencil,
        Action::ToolEraser,
        Action::ToolPaint,
        Action::ToolPaintUnpainted,
        Action::ToolRepaint,
        Action::ToolUnpaint,
        Action::ToolCloseGap,
        Action::ShowStrokes,
        Action::RespectProtectedColours,
        Action::SwitchLineColourArt,
        Action::SwitchOverlayUnderlay,
        Action::PreviewArtLayers,
        Action::CreateColourArt,
        Action::SubstituteNext,
        Action::SubstitutePrevious,
        Action::GoToNextDrawing,
        Action::GoToPreviousDrawing,
        Action::Search,
        Action::ToolDropper,
        Action::ToolLineEditor,
        Action::ToolHand,
        Action::ToolZoom,
        Action::SelectCutter,
        Action::ToggleSelectedPoints,
        Action::ArrangeNodes,
        Action::ShowNodeProperties,
        Action::RenderNodePreview,
        Action::ResetNodeView,
        Action::ToggleCameraEffects,
        Action::ToggleAutoKey,
        Action::ToggleLayerVisible,
        Action::ChartRole,
        Action::ChartHold,
        Action::ChartEarlier,
        Action::ChartLater,
        Action::ChartPose,
        Action::ChartEvenSpacing,
        Action::ChartUnfollow,
        Action::ChartsVisible,
        Action::Timeline(TimelineAction::SetKey),
        Action::Timeline(TimelineAction::DeleteKey),
        Action::Timeline(TimelineAction::InsertNew),
        Action::Timeline(TimelineAction::Duplicate),
        Action::Timeline(TimelineAction::CopyDrawing),
        Action::Timeline(TimelineAction::PasteDrawing),
        Action::Timeline(TimelineAction::CloneDrawing),
        Action::Timeline(TimelineAction::InsertInbetween),
        Action::Timeline(TimelineAction::DeleteLayer),
    ];

    /// Stable ID, used for settings and scripting.
    pub fn id(self) -> &'static str {
        match self {
            Action::NewScene => "file.new",
            Action::Open => "file.open",
            Action::Save => "file.save",
            Action::SaveAs => "file.save_as",
            Action::Import => "file.import",
            Action::Undo => "edit.undo",
            Action::Redo => "edit.redo",
            Action::SelectAll => "edit.select_all",
            Action::DeselectAll => "edit.deselect_all",
            Action::Delete => "edit.delete",
            Action::Cut => "edit.cut",
            Action::Copy => "edit.copy",
            Action::Paste => "edit.paste",
            Action::FlipHorizontal => "edit.flip_horizontal",
            Action::FlipVertical => "edit.flip_vertical",
            Action::PrevFrame => "frame.previous",
            Action::NextFrame => "frame.next",
            Action::FirstFrame => "frame.first",
            Action::LastFrame => "frame.last",
            Action::PlayPause => "playback.toggle",
            Action::IncreaseExposure => "timing.increase_exposure",
            Action::DecreaseExposure => "timing.decrease_exposure",
            Action::IncreaseBrushSize => "drawing.increase_brush_size",
            Action::DecreaseBrushSize => "drawing.decrease_brush_size",
            Action::CreateEmptyDrawing => "drawing.create_empty",
            Action::AddLayer => "layer.add",
            Action::ToggleOnionSkin => "view.onion_skin",
            Action::ResetView => "view.reset",
            Action::MirrorView => "view.mirror",
            Action::SetPlayStart => "playback.start_here",
            Action::SetPlayEnd => "playback.end_here",
            Action::ClearPlayRange => "playback.whole_scene",
            Action::ToggleLoop => "playback.loop",
            Action::ZoomIn => "view.zoom_in",
            Action::ZoomOut => "view.zoom_out",
            Action::RotateCw => "view.rotate_cw",
            Action::RotateCcw => "view.rotate_ccw",
            Action::ResetZoom => "view.reset_zoom",
            Action::ResetRotation => "view.reset_rotation",
            Action::ResetPan => "view.reset_pan",
            Action::OnionMorePrevious => "onion.previous_more",
            Action::OnionLessPrevious => "onion.previous_less",
            Action::OnionMoreNext => "onion.next_more",
            Action::OnionLessNext => "onion.next_less",
            Action::OnionPrevious1 => "onion.previous_1",
            Action::OnionPrevious2 => "onion.previous_2",
            Action::OnionPrevious3 => "onion.previous_3",
            Action::OnionNoPrevious => "onion.previous_none",
            Action::OnionNext1 => "onion.next_1",
            Action::OnionNext2 => "onion.next_2",
            Action::OnionNext3 => "onion.next_3",
            Action::OnionNoNext => "onion.next_none",
            Action::MaximiseView => "view.maximise",
            Action::ToolSelect => "tool.select",
            Action::ToolTransform => "tool.transform",
            Action::ToolBrush => "tool.brush",
            Action::ToolLine => "tool.line",
            Action::ToolText => "tool.text",
            Action::ToolPencil => "tool.pencil",
            Action::ToolEraser => "tool.eraser",
            Action::ToolPaint => "tool.paint",
            Action::ToolPaintUnpainted => "tool.paint_unpainted",
            Action::ToolRepaint => "tool.repaint",
            Action::ToolUnpaint => "tool.unpaint",
            Action::ToolCloseGap => "tool.close_gap",
            Action::ShowStrokes => "view.show_strokes",
            Action::RespectProtectedColours => "drawing.respect_protected_colours",
            Action::SwitchLineColourArt => "drawing.switch_line_colour_art",
            Action::SwitchOverlayUnderlay => "drawing.switch_overlay_underlay",
            Action::PreviewArtLayers => "drawing.preview_art_layers",
            Action::CreateColourArt => "drawing.create_colour_art",
            Action::SubstituteNext => "timing.substitute_next",
            Action::SubstitutePrevious => "timing.substitute_previous",
            Action::GoToNextDrawing => "timing.next_drawing",
            Action::GoToPreviousDrawing => "timing.previous_drawing",
            Action::Search => "app.search",
            Action::ToolDropper => "tool.dropper",
            Action::ToolLineEditor => "tool.line_editor",
            Action::ToolHand => "tool.hand",
            Action::ToolZoom => "tool.zoom",
            Action::SelectCutter => "drawing.cutter",
            Action::ToggleSelectedPoints => "drawing.show-points",
            Action::ArrangeNodes => "node.arrange",
            Action::ShowNodeProperties => "node.properties",
            Action::RenderNodePreview => "node.preview",
            Action::ResetNodeView => "node.reset",
            Action::ToggleCameraEffects => "camera.effects",
            Action::ToggleAutoKey => "timing.animate",
            Action::ToggleLayerVisible => "layer.visible",
            Action::ChartRole => "chart.role",
            Action::ChartHold => "chart.hold",
            Action::ChartEarlier => "chart.earlier",
            Action::ChartLater => "chart.later",
            Action::ChartPose => "chart.rotate_scale",
            Action::ChartEvenSpacing => "chart.even_spacing",
            Action::ChartUnfollow => "chart.unfollow",
            Action::ChartsVisible => "chart.visibility",
            Action::Timeline(action) => action.id(),
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Action::NewScene => "New scene",
            Action::Open => "Open",
            Action::Save => "Save",
            Action::SaveAs => "Save as",
            Action::Import => "Import…",
            Action::Undo => "Undo",
            Action::Redo => "Redo",
            Action::SelectAll => "Select all",
            Action::DeselectAll => "Deselect all",
            Action::Delete => "Delete",
            Action::Cut => "Cut",
            Action::Copy => "Copy",
            Action::Paste => "Paste",
            Action::FlipHorizontal => "Flip horizontal",
            Action::FlipVertical => "Flip vertical",
            Action::PrevFrame => "Previous frame",
            Action::NextFrame => "Next frame",
            Action::FirstFrame => "First frame",
            Action::LastFrame => "Last frame",
            Action::PlayPause => "Play / pause",
            Action::IncreaseExposure => "Increase exposure",
            Action::DecreaseExposure => "Decrease exposure",
            Action::IncreaseBrushSize => "Increase brush size",
            Action::DecreaseBrushSize => "Decrease brush size",
            Action::CreateEmptyDrawing => "Create empty drawing",
            Action::AddLayer => "Add drawing layer",
            Action::ToggleOnionSkin => "Onion skin",
            Action::ResetView => "Reset view",
            Action::MirrorView => "Mirror view",
            Action::SetPlayStart => "Playback starts here",
            Action::SetPlayEnd => "Playback ends here",
            Action::ClearPlayRange => "Play the whole scene",
            Action::ToggleLoop => "Loop playback",
            Action::ZoomIn => "Zoom in",
            Action::ZoomOut => "Zoom out",
            Action::RotateCw => "Rotate view CW",
            Action::RotateCcw => "Rotate view CCW",
            Action::ResetZoom => "Reset zoom",
            Action::ResetRotation => "Reset rotation",
            Action::ResetPan => "Reset pan",
            Action::OnionMorePrevious => "Onion skin: add previous drawing",
            Action::OnionLessPrevious => "Onion skin: remove previous drawing",
            Action::OnionMoreNext => "Onion skin: add next drawing",
            Action::OnionLessNext => "Onion skin: remove next drawing",
            Action::OnionPrevious1 => "Onion skin: 1 previous",
            Action::OnionPrevious2 => "Onion skin: 2 previous",
            Action::OnionPrevious3 => "Onion skin: 3 previous",
            Action::OnionNoPrevious => "Onion skin: no previous",
            Action::OnionNext1 => "Onion skin: 1 next",
            Action::OnionNext2 => "Onion skin: 2 next",
            Action::OnionNext3 => "Onion skin: 3 next",
            Action::OnionNoNext => "Onion skin: no next",
            Action::MaximiseView => "Maximise view",
            Action::ToolSelect => "Select",
            Action::ToolTransform => "Transform",
            Action::ToolBrush => "Brush",
            Action::ToolLine => "Line",
            Action::ToolText => "Text",
            Action::ToolPencil => "Pencil",
            Action::ToolEraser => "Eraser",
            Action::ToolPaint => "Paint",
            Action::ToolPaintUnpainted => "Paint Unpainted",
            Action::ToolRepaint => "Repaint",
            Action::ToolUnpaint => "Unpaint",
            Action::ToolCloseGap => "Close Gap",
            Action::ShowStrokes => "Show Strokes",
            Action::RespectProtectedColours => "Respect Protected Colours",
            Action::SwitchLineColourArt => "Switch to Line Art/Colour Art",
            Action::SwitchOverlayUnderlay => "Switch to Overlay/Underlay Art",
            Action::PreviewArtLayers => "Preview All Art Layers",
            Action::CreateColourArt => "Create Colour Art from Line Art",
            Action::SubstituteNext => "Substitute Drawing Next",
            Action::SubstitutePrevious => "Substitute Drawing Previous",
            Action::GoToNextDrawing => "Go to Next Drawing",
            Action::GoToPreviousDrawing => "Go to Previous Drawing",
            Action::Search => "Search everything",
            Action::ToolDropper => "Dropper",
            Action::ToolLineEditor => "Line Editor",
            Action::ToolHand => "Hand",
            Action::ToolZoom => "Zoom",
            Action::SelectCutter => "Cutter",
            Action::ToggleSelectedPoints => "Show selected points",
            Action::ArrangeNodes => "Arrange nodes",
            Action::ShowNodeProperties => "Open Node Properties",
            Action::RenderNodePreview => "Render node preview",
            Action::ResetNodeView => "Reset Node view",
            Action::ToggleCameraEffects => "Toggle camera effects",
            Action::ToggleAutoKey => "Toggle auto-key",
            Action::ToggleLayerVisible => "Hide or show the selected layer",
            Action::ChartRole => "Smart Chart: key or breakdown",
            Action::ChartHold => "Smart Chart: hold until the next point",
            Action::ChartEarlier => "Move Smart Chart one frame earlier",
            Action::ChartLater => "Move Smart Chart one frame later",
            Action::ChartPose => "Smart Chart: rotate and scale",
            Action::ChartEvenSpacing => "Smart Chart: even spacing",
            Action::ChartUnfollow => "Stop following Smart Chart",
            Action::ChartsVisible => "Show or hide Smart Charts",
            Action::Timeline(action) => action.label(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Binding {
    pub key: Key,
    pub mods: Modifiers,
    /// Checked against the latest Harmony's published list (E4.4).
    pub verified: bool,
}

const fn b(key: Key, mods: Modifiers) -> Option<Binding> {
    Some(Binding {
        key,
        mods,
        verified: true,
    })
}

const ALT: Modifiers = Modifiers::ALT;
const SHIFT: Modifiers = Modifiers::SHIFT;
const NONE: Modifiers = Modifiers::NONE;
const CMD: Modifiers = Modifiers::COMMAND;
const CMD_SHIFT: Modifiers = Modifiers::COMMAND.plus(Modifiers::SHIFT);
const CTRL_SHIFT: Modifiers = Modifiers::CTRL.plus(Modifiers::SHIFT);
const ALT_SHIFT: Modifiers = Modifiers::ALT.plus(Modifiers::SHIFT);

fn harmony(action: Action) -> Option<Binding> {
    match action {
        Action::NewScene => b(Key::N, CMD),
        Action::Open => b(Key::O, CMD),
        Action::Save => b(Key::S, CMD),
        Action::SaveAs => None, // Harmony's Ctrl+Shift+S is Advanced Save
        // Not bound until checked against Harmony 27.1's shortcut list.
        Action::Import => None,
        Action::Undo => b(Key::Z, CMD),
        // Harmony 27 documents Redo as Ctrl+Shift+Z on macOS too.
        Action::Redo => b(
            Key::Z,
            if cfg!(target_os = "macos") {
                CTRL_SHIFT
            } else {
                CMD_SHIFT
            },
        ),
        Action::SelectAll => b(Key::A, CMD),
        Action::DeselectAll => b(Key::Escape, NONE),
        Action::Delete => b(
            if cfg!(target_os = "macos") {
                Key::Backspace
            } else {
                Key::Delete
            },
            NONE,
        ),
        Action::Cut => b(Key::X, CMD),
        Action::Copy => b(Key::C, CMD),
        Action::Paste => b(Key::V, CMD),
        Action::FlipHorizontal => b(Key::Num4, NONE),
        Action::FlipVertical => b(Key::Num5, NONE),
        Action::PrevFrame => b(Key::Comma, NONE),
        Action::NextFrame => b(Key::Period, NONE),
        Action::FirstFrame => b(Key::Comma, SHIFT), // "<"
        Action::LastFrame => b(Key::Period, SHIFT), // ">"
        Action::PlayPause => b(Key::Enter, SHIFT),  // Toggle Playback
        Action::IncreaseExposure => b(Key::Plus, NONE),
        Action::DecreaseExposure => b(Key::Minus, NONE),
        Action::IncreaseBrushSize | Action::DecreaseBrushSize => None,
        Action::CreateEmptyDrawing => b(Key::R, ALT_SHIFT),
        Action::AddLayer => b(Key::R, CMD),
        Action::ToggleOnionSkin => b(Key::O, ALT),
        Action::ResetView => b(Key::M, SHIFT),
        // Not bound until checked against Harmony 27.1's shortcut list.
        Action::MirrorView => None,
        Action::SetPlayStart | Action::SetPlayEnd | Action::ClearPlayRange | Action::ToggleLoop => {
            None
        }
        Action::ZoomIn => b(Key::Num2, NONE),
        Action::ZoomOut => b(Key::Num1, NONE),
        Action::ResetZoom => None,
        Action::ResetRotation => b(Key::X, SHIFT),
        Action::ResetPan => b(Key::N, SHIFT),
        Action::RotateCw | Action::RotateCcw => None,
        Action::OnionMorePrevious
        | Action::OnionLessPrevious
        | Action::OnionMoreNext
        | Action::OnionLessNext => None,
        // Harmony 27: ! @ # ~ for previous, Cmd+1/2/3/` for next.
        Action::OnionPrevious1 => b(Key::Exclamationmark, SHIFT),
        Action::OnionPrevious2 => b(Key::Num2, SHIFT),
        Action::OnionPrevious3 => b(Key::Num3, SHIFT),
        Action::OnionNoPrevious => b(Key::Backtick, SHIFT),
        Action::OnionNext1 => b(Key::Num1, CMD),
        Action::OnionNext2 => b(Key::Num2, CMD),
        Action::OnionNext3 => b(Key::Num3, CMD),
        Action::OnionNoNext => b(Key::Backtick, CMD),
        Action::MaximiseView => b(Key::F, CMD), // Toggle Full Screen
        Action::ToolSelect => b(Key::S, ALT),
        Action::ToolTransform => b(Key::T, SHIFT),
        Action::ToolBrush => b(Key::B, ALT),
        // Unbound until checked against Harmony 27.1's Line tool.
        Action::ToolLine => None,
        // Unbound until checked against Harmony 27.1's Text tool.
        Action::ToolText => None,
        Action::ToolPencil => b(Key::Slash, ALT),
        Action::ToolEraser => b(Key::E, ALT),
        Action::ToolPaint => b(Key::I, ALT),
        Action::ToolPaintUnpainted => b(Key::Y, ALT),
        Action::ToolRepaint => b(Key::R, ALT),
        Action::ToolUnpaint => b(Key::U, ALT),
        Action::ToolCloseGap => b(Key::C, ALT),
        Action::ShowStrokes => b(Key::K, NONE),
        Action::RespectProtectedColours => b(Key::S, SHIFT),
        Action::SwitchLineColourArt => b(Key::L, NONE),
        Action::SwitchOverlayUnderlay => b(Key::Colon, NONE),
        Action::PreviewArtLayers => b(Key::P, NONE),
        // "*" has no key of its own on many layouts: matched by the typed
        // character instead (see `pressed`).
        Action::CreateColourArt => None,
        Action::SubstituteNext => b(Key::CloseBracket, NONE),
        Action::SubstitutePrevious => b(Key::OpenBracket, NONE),
        Action::GoToNextDrawing => b(Key::G, NONE),
        Action::GoToPreviousDrawing => b(Key::F, NONE),
        // Symphony's search (Tristan). Harmony's published list leaves Tab
        // free, so both profiles have it.
        Action::Search => b(Key::Tab, NONE),
        Action::ToolDropper => b(Key::D, ALT),
        // Symphony's own tool: no Harmony shortcut to follow. Tab search finds it.
        Action::ToolLineEditor => None,
        Action::ToolHand => None,
        Action::ToolZoom => b(Key::Z, ALT),
        Action::Timeline(TimelineAction::SetKey) => b(Key::K, NONE),
        Action::Timeline(TimelineAction::CopyDrawing) => b(Key::C, CMD),
        Action::Timeline(TimelineAction::PasteDrawing) => b(Key::V, CMD),
        Action::Timeline(_)
        | Action::SelectCutter
        | Action::ToggleSelectedPoints
        | Action::ArrangeNodes
        | Action::ShowNodeProperties
        | Action::RenderNodePreview
        | Action::ResetNodeView
        | Action::ToggleCameraEffects
        | Action::ToggleAutoKey
        | Action::ToggleLayerVisible
        // Harmony has no Smart Charts: these are search-only there.
        | Action::ChartRole
        | Action::ChartHold
        | Action::ChartEarlier
        | Action::ChartLater
        | Action::ChartPose
        | Action::ChartEvenSpacing
        | Action::ChartUnfollow
        | Action::ChartsVisible => None,
    }
}

/// Symphony uses mnemonic tool keys and scopes them to the workspace under the pointer.
fn symphony(action: Action) -> Option<Binding> {
    match action {
        Action::Redo => b(Key::Z, CMD_SHIFT),
        Action::ZoomIn => b(Key::Num1, NONE),
        Action::ZoomOut => b(Key::Num2, NONE),
        Action::IncreaseBrushSize => b(Key::Plus, NONE),
        Action::DecreaseBrushSize => b(Key::Minus, NONE),
        Action::ToolSelect => b(Key::S, NONE),
        Action::ToolTransform => b(Key::X, NONE),
        Action::ToolBrush => b(Key::B, NONE),
        Action::ToolLine => b(Key::L, NONE),
        Action::ToolText => b(Key::T, NONE),
        Action::ToolPencil => None,
        Action::ToolEraser => b(Key::E, NONE),
        Action::ToolPaint => b(Key::P, NONE),
        Action::ToolCloseGap => b(Key::G, NONE),
        Action::ToolDropper => b(Key::D, NONE),
        Action::ToolLineEditor => b(Key::N, NONE),
        Action::ToolHand => b(Key::H, NONE),
        Action::ToolZoom => b(Key::Z, NONE),
        Action::SelectCutter => b(Key::C, NONE),
        Action::ToggleSelectedPoints => b(Key::I, NONE),
        Action::ArrangeNodes => b(Key::A, NONE),
        Action::ShowNodeProperties => b(Key::P, NONE),
        Action::RenderNodePreview => b(Key::V, NONE),
        Action::ResetNodeView => b(Key::R, NONE),
        Action::ToggleCameraEffects => b(Key::R, SHIFT),
        Action::ToggleAutoKey => b(Key::A, NONE),
        Action::ToggleLayerVisible => b(Key::V, NONE),
        Action::ToolRepaint => b(Key::R, NONE),
        Action::ToolUnpaint => b(Key::U, NONE),
        Action::ToolPaintUnpainted => b(Key::Y, NONE),
        Action::ToggleOnionSkin => b(Key::O, NONE),
        Action::MirrorView => b(Key::M, NONE),
        Action::AddLayer => b(Key::L, NONE),
        Action::Timeline(TimelineAction::DeleteKey) => b(Key::K, SHIFT),
        Action::Timeline(TimelineAction::InsertNew) => b(Key::N, NONE),
        Action::Timeline(TimelineAction::Duplicate) => b(Key::D, NONE),
        Action::Timeline(TimelineAction::CloneDrawing) => b(Key::C, NONE),
        Action::Timeline(TimelineAction::InsertInbetween) => b(Key::I, NONE),
        Action::SwitchLineColourArt => b(Key::L, SHIFT),
        Action::PreviewArtLayers => b(Key::V, NONE),
        // Smart Charts (DESIGN §11.1): only while a chart is edited.
        Action::ChartRole => b(Key::B, SHIFT),
        Action::ChartHold => b(Key::H, SHIFT),
        Action::ChartEarlier => b(Key::OpenBracket, NONE),
        Action::ChartLater => b(Key::CloseBracket, NONE),
        _ => harmony(action),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    Global,
    OutsideTiming,
    OutsideTimeline,
    Canvas,
    Camera,
    Node,
    Timing,
    Timeline,
    Viewport,
}

pub fn scope(profile: Profile, action: Action) -> Scope {
    use Action::*;
    match action {
        IncreaseBrushSize | DecreaseBrushSize | SelectCutter | ToggleSelectedPoints => {
            Scope::Canvas
        }
        ArrangeNodes | ShowNodeProperties | RenderNodePreview | ResetNodeView => Scope::Node,
        ChartRole | ChartHold | ChartEarlier | ChartLater => Scope::Canvas,
        ToggleCameraEffects => Scope::Camera,
        Timeline(TimelineAction::SetKey) if profile == Profile::Harmony => Scope::Timeline,
        Timeline(_) | ToggleAutoKey | ToggleLayerVisible => Scope::Timing,
        Copy | Paste if profile == Profile::Harmony => Scope::OutsideTiming,
        ShowStrokes if profile == Profile::Harmony => Scope::OutsideTimeline,
        _ if profile == Profile::Harmony => Scope::Global,
        ToolSelect
        | ToolTransform
        | ToolBrush
        | ToolLine
        | ToolText
        | ToolPencil
        | ToolEraser
        | ToolPaint
        | ToolPaintUnpainted
        | ToolRepaint
        | ToolUnpaint
        | ToolCloseGap
        | ToolDropper
        | ToolLineEditor
        | ToolHand
        | ToolZoom
        | ShowStrokes
        | RespectProtectedColours
        | SwitchLineColourArt
        | SwitchOverlayUnderlay
        | PreviewArtLayers
        | CreateColourArt
        | FlipHorizontal
        | ToggleOnionSkin
        | MirrorView
        | FlipVertical
        | SelectAll
        | DeselectAll
        | Delete
        | Cut
        | Copy
        | Paste => Scope::Canvas,
        AddLayer | IncreaseExposure | DecreaseExposure | SubstituteNext | SubstitutePrevious
        | GoToNextDrawing | GoToPreviousDrawing => Scope::Timing,
        ZoomIn | ZoomOut => Scope::Viewport,
        _ => Scope::Global,
    }
}

fn scopes_overlap(profile: Profile, a: Action, b: Action) -> bool {
    fn mask(scope: Scope) -> u8 {
        match scope {
            Scope::Global => 0b11111,
            Scope::OutsideTiming => 0b00111,
            Scope::OutsideTimeline => 0b10111,
            Scope::Canvas => 0b0011,
            Scope::Camera => 0b0010,
            Scope::Node => 0b0100,
            Scope::Timing => 0b11000,
            Scope::Timeline => 0b01000,
            Scope::Viewport => 0b0111,
        }
    }
    mask(scope(profile, a)) & mask(scope(profile, b)) != 0
}

pub fn binding(profile: Profile, action: Action) -> Option<Binding> {
    if let Some(custom) = custom_binding(profile, action) {
        return custom;
    }
    default_binding(profile, action)
}

/// The profile's own binding, ignoring anything the user remapped.
pub fn default_binding(profile: Profile, action: Action) -> Option<Binding> {
    match profile {
        Profile::Harmony => harmony(action),
        Profile::Symphony => symphony(action),
    }
}

// ---- Remapping ---------------------------------------------------------
//
// Shortcuts the user changed (from search's right-click menu), per profile,
// on top of the profile's defaults. Saved with the app's settings.

/// One remapped shortcut, as saved: profile, action id, and the key name
/// plus modifiers (None = no shortcut).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Remap {
    pub profile: Profile,
    pub action: String,
    pub key: Option<(String, Mods)>,
}

/// Modifiers as saved.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mods {
    pub alt: bool,
    pub ctrl: bool,
    pub shift: bool,
    pub command: bool,
}

impl Mods {
    fn of(m: Modifiers) -> Self {
        Mods {
            alt: m.alt,
            ctrl: m.ctrl && !m.command,
            shift: m.shift,
            command: m.command,
        }
    }

    fn to_egui(self) -> Modifiers {
        let mut m = Modifiers::NONE;
        if self.alt {
            m = m.plus(Modifiers::ALT);
        }
        if self.ctrl {
            m = m.plus(Modifiers::CTRL);
        }
        if self.shift {
            m = m.plus(Modifiers::SHIFT);
        }
        if self.command {
            m = m.plus(Modifiers::COMMAND);
        }
        m
    }
}

static REMAPS: std::sync::RwLock<Vec<Remap>> = std::sync::RwLock::new(Vec::new());

fn custom_binding(profile: Profile, action: Action) -> Option<Option<Binding>> {
    let remaps = REMAPS.read().ok()?;
    let r = remaps
        .iter()
        .find(|r| r.profile == profile && r.action == action.id())?;
    Some(r.key.as_ref().and_then(|(name, mods)| {
        Some(Binding {
            key: Key::from_name(name)?,
            mods: mods.to_egui(),
            verified: false,
        })
    }))
}

/// Every remapped shortcut, to save.
pub fn remaps() -> Vec<Remap> {
    REMAPS.read().map(|r| r.clone()).unwrap_or_default()
}

/// Load saved remaps (at start-up).
/// Tests that change or depend on shortcut remaps hold this, since the
/// remaps are one global and tests run in parallel.
#[cfg(test)]
pub static TEST_REMAPS: std::sync::Mutex<()> = std::sync::Mutex::new(());

pub fn set_remaps(list: Vec<Remap>) {
    if let Ok(mut r) = REMAPS.write() {
        *r = list;
    }
}

/// Has the user changed this action's shortcut in this profile?
pub fn is_remapped(profile: Profile, action: Action) -> bool {
    custom_binding(profile, action).is_some()
}

fn put(profile: Profile, action: Action, key: Option<(String, Mods)>) {
    if let Ok(mut r) = REMAPS.write() {
        r.retain(|x| !(x.profile == profile && x.action == action.id()));
        r.push(Remap {
            profile,
            action: action.id().to_owned(),
            key,
        });
    }
}

/// Which other actions already use `key` + `mods` in `profile` (nothing
/// changes).
pub fn clashes(profile: Profile, action: Action, key: Key, mods: Modifiers) -> Vec<Action> {
    let wanted = (key, Mods::of(mods));
    Action::ALL
        .into_iter()
        .filter(|a| *a != action && scopes_overlap(profile, action, *a))
        .filter(|a| binding(profile, *a).is_some_and(|b| (b.key, Mods::of(b.mods)) == wanted))
        .collect()
}

/// A readable name for a key combination ("Option+K"), written the same
/// way as shortcut labels.
pub fn combo_label(key: Key, mods: Modifiers) -> String {
    let mac = cfg!(target_os = "macos");
    let mut parts: Vec<&str> = Vec::new();
    if mods.command || mods.ctrl || mods.mac_cmd {
        parts.push(if mac { "Cmd" } else { "Ctrl" });
    }
    if mods.alt {
        parts.push(if mac { "Option" } else { "Alt" });
    }
    if mods.shift {
        parts.push("Shift");
    }
    let mut s = parts.join("+");
    if !s.is_empty() {
        s.push('+');
    }
    s.push_str(key.symbol_or_name());
    s
}

/// Give `action` the shortcut `key` + `mods` in `profile`. Any other action
/// using that shortcut loses it, and is returned so the caller can say so.
pub fn remap(profile: Profile, action: Action, key: Key, mods: Modifiers) -> Vec<Action> {
    let taken = clashes(profile, action, key, mods);
    for other in &taken {
        put(profile, *other, None);
    }
    put(
        profile,
        action,
        Some((key.name().to_owned(), Mods::of(mods))),
    );
    taken
}

/// Remove `action`'s shortcut in `profile`.
pub fn unbind(profile: Profile, action: Action) {
    put(profile, action, None);
}

/// Back to the profile's own shortcut.
pub fn reset(profile: Profile, action: Action) {
    if let Ok(mut r) = REMAPS.write() {
        r.retain(|x| !(x.profile == profile && x.action == action.id()));
    }
}

/// Did the user just trigger `action`? Modifiers must match exactly, so
/// Alt+E (Eraser) never fires on plain E.
pub fn pressed(ctx: &egui::Context, profile: Profile, action: Action) -> bool {
    if action == Action::CreateColourArt {
        // Harmony's "*", wherever the layout puts it.
        return ctx.input(|i| {
            !i.modifiers.command
                && !i.modifiers.alt
                && i.events
                    .iter()
                    .any(|e| matches!(e, egui::Event::Text(t) if t == "*"))
        });
    }
    // Mac users press Cmd+Shift+Z for Redo out of habit, whatever the
    // profile; Harmony's own Ctrl+Shift+Z still works too.
    if action == Action::Redo
        && cfg!(target_os = "macos")
        && ctx.input(|i| i.key_pressed(Key::Z) && i.modifiers.matches_exact(CMD_SHIFT))
    {
        return true;
    }
    let Some(bind) = binding(profile, action) else {
        return false;
    };
    ctx.input(|i| {
        // + and - are typed with Shift on some layouts, so ignore Shift there.
        let mods_ok = if matches!(
            bind.key,
            Key::Plus | Key::Minus | Key::Exclamationmark | Key::Colon
        ) {
            i.modifiers.matches_logically(bind.mods)
        } else {
            i.modifiers.matches_exact(bind.mods)
        };
        i.key_pressed(bind.key) && mods_ok
    })
}

/// Shortcut text for menus and tooltips, such as "Alt+B" or "Cmd+Z".
pub fn label(profile: Profile, action: Action) -> String {
    if action == Action::CreateColourArt {
        return "*".into();
    }
    let Some(bind) = binding(profile, action) else {
        return String::new();
    };
    let mac = cfg!(target_os = "macos");
    let mut parts: Vec<&str> = Vec::new();
    if bind.mods.command || bind.mods.ctrl || bind.mods.mac_cmd {
        parts.push(if mac { "Cmd" } else { "Ctrl" });
    }
    if bind.mods.alt {
        parts.push(if mac { "Option" } else { "Alt" });
    }
    if bind.mods.shift {
        parts.push("Shift");
    }
    let key = bind.key.symbol_or_name();
    let mut s = parts.join("+");
    if !s.is_empty() {
        s.push('+');
    }
    s.push_str(key);
    s
}

/// "Name (shortcut)" for tooltips (D7.5).
pub fn tooltip(profile: Profile, action: Action) -> String {
    match label(profile, action) {
        l if l.is_empty() => action.name().to_owned(),
        l => format!("{} ({l})", action.name()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Bindings may be shared by views that cannot both handle them.
    #[test]
    fn no_overlapping_bindings() {
        let _remaps = TEST_REMAPS.lock().unwrap_or_else(|e| e.into_inner());
        for profile in [Profile::Harmony, Profile::Symphony] {
            for action in Action::ALL {
                if let Some(bind) = binding(profile, action) {
                    assert!(
                        clashes(profile, action, bind.key, bind.mods).is_empty(),
                        "{profile:?}: {} clashes",
                        action.id()
                    );
                }
            }
        }
    }

    #[test]
    fn symphony_uses_mnemonics_and_reuses_exposure_keys_on_canvas() {
        let _remaps = TEST_REMAPS.lock().unwrap_or_else(|e| e.into_inner());
        let profile = Profile::Symphony;
        assert_eq!(fired(profile, Key::E, NONE), [Action::ToolEraser]);
        assert_eq!(fired(profile, Key::Num1, NONE), [Action::ZoomIn]);
        assert_eq!(fired(profile, Key::Num2, NONE), [Action::ZoomOut]);
        assert_eq!(fired(profile, Key::Tab, NONE), [Action::Search]);
        assert_eq!(scope(profile, Action::ToolEraser), Scope::Canvas);
        for (key, exposure, brush) in [
            (
                Key::Plus,
                Action::IncreaseExposure,
                Action::IncreaseBrushSize,
            ),
            (
                Key::Minus,
                Action::DecreaseExposure,
                Action::DecreaseBrushSize,
            ),
        ] {
            assert_eq!(fired(profile, key, NONE), [exposure, brush]);
            assert_eq!(scope(profile, exposure), Scope::Timing);
            assert_eq!(scope(profile, brush), Scope::Canvas);
        }
    }

    #[test]
    fn letter_shortcuts_can_be_reused_across_panels() {
        let _remaps = TEST_REMAPS.lock().unwrap_or_else(|e| e.into_inner());
        let profile = Profile::Symphony;
        for (key, actions) in [
            (
                Key::A,
                &[
                    (Action::ArrangeNodes, Scope::Node),
                    (Action::ToggleAutoKey, Scope::Timing),
                ][..],
            ),
            (
                Key::C,
                &[
                    (Action::SelectCutter, Scope::Canvas),
                    (
                        Action::Timeline(TimelineAction::CloneDrawing),
                        Scope::Timing,
                    ),
                ],
            ),
            (
                Key::D,
                &[
                    (Action::ToolDropper, Scope::Canvas),
                    (Action::Timeline(TimelineAction::Duplicate), Scope::Timing),
                ],
            ),
            (
                Key::V,
                &[
                    (Action::PreviewArtLayers, Scope::Canvas),
                    (Action::RenderNodePreview, Scope::Node),
                    (Action::ToggleLayerVisible, Scope::Timing),
                ],
            ),
        ] {
            let fired = fired(profile, key, NONE);
            assert_eq!(fired.len(), actions.len());
            for &(action, context) in actions {
                assert!(fired.contains(&action));
                assert_eq!(scope(profile, action), context);
                assert!(clashes(profile, action, key, NONE).is_empty());
            }
        }
    }

    /// Which actions fire for one key press with `mods`.
    fn fired(profile: Profile, key: Key, mods: egui::Modifiers) -> Vec<Action> {
        let ctx = egui::Context::default();
        let mut got = Vec::new();
        let input = egui::RawInput {
            events: vec![
                egui::Event::ModifiersChanged(mods),
                egui::Event::Key {
                    key,
                    physical_key: Some(key),
                    pressed: true,
                    repeat: false,
                    modifiers: mods,
                },
            ],
            ..Default::default()
        };
        let mut out = ctx.run_ui(input, |ui| {
            for a in Action::ALL {
                if pressed(ui.ctx(), profile, a) {
                    got.push(a);
                }
            }
        });
        out.textures_delta.clear();
        got
    }

    #[test]
    fn redo_fires_from_its_shortcut_in_both_profiles() {
        let _remaps = TEST_REMAPS.lock().unwrap_or_else(|e| e.into_inner());
        for profile in [Profile::Harmony, Profile::Symphony] {
            let bind = binding(profile, Action::Redo).unwrap();
            assert_eq!(
                fired(profile, bind.key, bind.mods),
                vec![Action::Redo],
                "{profile:?}"
            );
        }
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn mac_redo_also_answers_cmd_shift_z_in_the_harmony_profile() {
        assert_eq!(
            fired(Profile::Harmony, Key::Z, CMD_SHIFT),
            vec![Action::Redo]
        );
    }

    #[test]
    fn ids_are_unique() {
        let mut ids: Vec<_> = Action::ALL.iter().map(|a| a.id()).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), Action::ALL.len());
    }
}
