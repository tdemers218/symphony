//! Views and workspaces. A workspace is a saved docking layout. Like
//! Harmony, layout changes auto-save into the active workspace, and Reset
//! puts it back to its preset.
//!
//! Presets follow the workspaces Harmony ships (Default, Hand-Drawn
//! Animation, Compositing). The exact view placement is our own
//! approximation and still needs checking against Harmony 27.

use egui_dock::{DockState, NodeIndex};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum View {
    Camera,
    Drawing,
    Timeline,
    Xsheet,
    Node,
    ToolProperties,
    Colour,
    LayerProperties,
    Function,
    Library,
    NodeLibrary,
    Multiplane,
    UndoHistory,
    Manual,
}

impl View {
    pub const ALL: [View; 14] = [
        View::Camera,
        View::Drawing,
        View::Timeline,
        View::Xsheet,
        View::Node,
        View::ToolProperties,
        View::Colour,
        View::LayerProperties,
        View::Function,
        View::Library,
        View::NodeLibrary,
        View::Multiplane,
        View::UndoHistory,
        View::Manual,
    ];

    pub fn title(self) -> &'static str {
        match self {
            View::Camera => "Camera",
            View::Drawing => "Drawing",
            View::Timeline => "Timeline",
            View::Xsheet => "Xsheet",
            View::Node => "Node",
            View::Multiplane => "3D View",
            View::ToolProperties => "Tool Properties",
            View::Colour => "Colour",
            View::LayerProperties => "Node Properties",
            View::Function => "Function",
            View::NodeLibrary => "Node Library",
            View::Library => "Library",
            View::UndoHistory => "Undo History",
            View::Manual => "Manual",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Workspace {
    Default,
    HandDrawn,
    Compositing,
}

impl Workspace {
    pub const ALL: [Workspace; 3] = [
        Workspace::Default,
        Workspace::HandDrawn,
        Workspace::Compositing,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Workspace::Default => "Default",
            Workspace::HandDrawn => "Hand-Drawn Animation",
            Workspace::Compositing => "Compositing",
        }
    }

    pub fn preset(self) -> DockState<View> {
        use View::*;
        match self {
            Workspace::Default => {
                let mut d = DockState::new(vec![Camera, Drawing, Node]);
                let s = d.main_surface_mut();
                let [main, right] = s.split_right(
                    NodeIndex::root(),
                    0.78,
                    vec![ToolProperties, LayerProperties],
                );
                let [_tools, _colour] = s.split_below(right, 0.5, vec![Colour]);
                let [_cam, _bottom] = s.split_below(main, 0.68, vec![Timeline, Xsheet, Function]);
                d
            }
            Workspace::HandDrawn => {
                let mut d = DockState::new(vec![Camera, Drawing]);
                let s = d.main_surface_mut();
                let [main, _xsheet] = s.split_right(NodeIndex::root(), 0.72, vec![Xsheet]);
                let [left, _cam] = s.split_left(main, 0.24, vec![ToolProperties]);
                let [_tools, _colour] = s.split_below(left, 0.5, vec![Colour]);
                d
            }
            Workspace::Compositing => {
                let mut d = DockState::new(vec![Node, Multiplane]);
                let s = d.main_surface_mut();
                let [node, cam] = s.split_right(NodeIndex::root(), 0.5, vec![Camera]);
                let [_node, _bottom] = s.split_below(node, 0.65, vec![LayerProperties, Library]);
                let [_, _timeline] = s.split_below(cam, 0.65, vec![Timeline]);
                d
            }
        }
    }
}
