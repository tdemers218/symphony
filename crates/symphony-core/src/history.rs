//! Undo and redo (ENGINEERING E3). Every change to a scene is an [`Edit`]
//! that can apply and revert itself. Edits are grouped into named steps; one
//! gesture is one step (E3.2), so a whole stroke, drag or erase undoes at once.

use crate::doc::{LayerSnapshot, Scene};
use crate::graph::{Link, Node, NodeId, NodeKey, NodeKind, PegParams};
use crate::id::Id;
use crate::stroke::{Drawing, Fill, Stroke};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq)]
pub enum Edit {
    AddNode {
        id: NodeId,
        node: Node,
        position: [f32; 2],
    },
    /// Delete a node (its links go with a `RewireNodes` before this).
    RemoveNode {
        id: NodeId,
        node: Node,
        position: [f32; 2],
    },
    SetDrawingMatte {
        id: NodeId,
        before: bool,
        after: bool,
    },
    SetNodeProperties {
        id: NodeId,
        before: Node,
        after: Node,
    },
    SetNodeKeys {
        id: NodeId,
        before: BTreeMap<u32, NodeKey>,
        after: BTreeMap<u32, NodeKey>,
    },
    MoveNode {
        id: NodeId,
        before: Option<[f32; 2]>,
        after: [f32; 2],
    },
    RewireNodes {
        before: Vec<Link>,
        after: Vec<Link>,
        was_custom: bool,
    },
    SetPalette {
        before: crate::palette::Palette,
        after: crate::palette::Palette,
    },
    SetStrokeSwatch {
        drawing: Id,
        stroke: Id,
        before: Id,
        after: Id,
    },
    /// Paint a zone.
    AddFill {
        drawing: Id,
        fill: Fill,
    },
    /// Unpaint a zone.
    RemoveFill {
        drawing: Id,
        index: usize,
        fill: Fill,
    },
    /// Repaint a zone with another swatch.
    SetFillSwatch {
        drawing: Id,
        fill: Id,
        before: Id,
        after: Id,
    },
    /// Replace editable text metadata. Glyph strokes are separate edits.
    SetText {
        drawing: Id,
        before: Option<crate::stroke::TextObject>,
        after: Option<crate::stroke::TextObject>,
    },
    /// Replace strokes in place, matched by ID (move, scale, rotate, flip).
    SetStrokes {
        drawing: Id,
        before: Vec<Stroke>,
        after: Vec<Stroke>,
    },
    AddStroke {
        drawing: Id,
        stroke: Stroke,
    },
    RemoveStroke {
        drawing: Id,
        index: usize,
        stroke: Stroke,
    },
    /// Swap one stroke for others at the same depth (the Eraser cutting a
    /// line into pieces, or erasing it all when `after` is empty).
    /// Swap one paint zone for what's left of it (the Eraser).
    ReplaceFill {
        drawing: Id,
        index: usize,
        before: Fill,
        after: Vec<Fill>,
    },
    ReplaceStroke {
        drawing: Id,
        /// Depth of `before` in the drawing, where `after` goes.
        index: usize,
        before: Stroke,
        after: Vec<Stroke>,
    },
    AddDrawing {
        drawing: Drawing,
    },
    /// Replace cells `from..from+before.len()` (1-based frames). `before` and
    /// `after` can differ in length, which inserts or deletes cells and
    /// shifts everything after them.
    SetCells {
        column: Id,
        from: u32,
        before: Vec<Option<Id>>,
        after: Vec<Option<Id>>,
    },
    SetPeg {
        node: NodeId,
        before: PegParams,
        after: PegParams,
    },
    SetSwatch {
        swatch: Id,
        before: [u8; 4],
        after: [u8; 4],
    },
    SetVisible {
        column: Id,
        before: bool,
        after: bool,
    },
    Rename {
        column: Id,
        before: String,
        after: String,
    },
    SetCameraMotion {
        before: crate::doc::CameraMotion,
        after: crate::doc::CameraMotion,
    },
    SetCameraFov {
        before: crate::anim::Value,
        after: crate::anim::Value,
    },
    SetFrameRate {
        before: f32,
        after: f32,
    },
    SetLength {
        before: u32,
        after: u32,
    },
    AddLayer(Box<LayerSnapshot>),
    /// The camera frame's black bars, before and after.
    SetBars {
        before: crate::doc::Bars,
        after: crate::doc::Bars,
    },
    /// The scene's sound clips, before and after.
    SetSounds {
        before: Vec<crate::doc::SoundClip>,
        after: Vec<crate::doc::SoundClip>,
    },
    /// Move a layer from one place in the stack to another.
    MoveLayer {
        from: usize,
        to: usize,
    },
    RemoveLayer(Box<LayerSnapshot>),
}

impl Edit {
    /// Build a `SetCells` that inserts or removes cells at `from`.
    pub fn splice(
        scene: &Scene,
        column: Id,
        from: u32,
        remove: u32,
        insert: Vec<Option<Id>>,
    ) -> Option<Edit> {
        let col = scene.xsheet.column(column)?;
        let tail_end = col.len().max(from + remove);
        let before: Vec<Option<Id>> = (from..=tail_end).map(|f| col.get(f)).collect();
        let mut after = insert;
        after.extend((from + remove..=tail_end).map(|f| col.get(f)));
        Some(Edit::SetCells {
            column,
            from,
            before,
            after,
        })
    }

    fn apply(&self, s: &mut Scene, forward: bool) {
        match self {
            Edit::RemoveNode { id, node, position } => {
                if forward {
                    s.graph.remove(*id);
                    s.node_positions.remove(id);
                } else {
                    s.graph.restore(*id, node.clone(), &[]);
                    s.node_positions.insert(*id, *position);
                }
            }
            Edit::AddNode { id, node, position } => {
                if forward {
                    s.graph.restore(*id, node.clone(), &[]);
                    s.node_positions.insert(*id, *position);
                } else {
                    s.graph.remove(*id);
                    s.node_positions.remove(id);
                }
            }
            Edit::SetDrawingMatte { id, before, after } => {
                s.graph
                    .set_drawing_matte(*id, if forward { *after } else { *before });
            }
            Edit::SetNodeProperties { id, before, after } => {
                let properties = if forward { after } else { before };
                if let Some(node) = s.graph.node_mut(*id) {
                    *node = properties.clone();
                    if let NodeKind::Drawing { column } = node.kind {
                        if let Some(column) = s.xsheet.column_mut(column) {
                            column.name.clone_from(&properties.name);
                        }
                    }
                }
            }
            Edit::SetNodeKeys { id, before, after } => {
                s.graph
                    .set_keys(*id, if forward { after } else { before }.clone());
            }
            Edit::MoveNode { id, before, after } => {
                if let Some(position) = if forward { Some(*after) } else { *before } {
                    s.node_positions.insert(*id, position);
                } else {
                    s.node_positions.remove(id);
                }
            }
            Edit::RewireNodes {
                before,
                after,
                was_custom,
            } => {
                s.graph
                    .replace_links(if forward { after } else { before }.clone());
                s.custom_wiring = forward || *was_custom;
            }
            Edit::SetText {
                drawing,
                before,
                after,
            } => {
                if let Some(drawing) = s.drawings.get_mut(drawing) {
                    let (previous, next) = if forward {
                        (before, after)
                    } else {
                        (after, before)
                    };
                    if let Some(previous) = previous {
                        drawing.text.retain(|object| object.id != previous.id);
                    }
                    if let Some(next) = next {
                        drawing.text.push(next.clone());
                    }
                }
            }
            Edit::AddStroke { drawing, stroke } => {
                if let Some(d) = s.drawings.get_mut(drawing) {
                    if forward {
                        d.strokes.push(stroke.clone());
                    } else {
                        d.strokes.retain(|x| x.id != stroke.id);
                    }
                }
            }
            Edit::RemoveStroke {
                drawing,
                index,
                stroke,
            } => {
                if let Some(d) = s.drawings.get_mut(drawing) {
                    if forward {
                        d.strokes.retain(|x| x.id != stroke.id);
                    } else {
                        d.strokes
                            .insert((*index).min(d.strokes.len()), stroke.clone());
                    }
                }
            }
            Edit::ReplaceFill {
                drawing,
                index,
                before,
                after,
            } => {
                if let Some(d) = s.drawings.get_mut(drawing) {
                    let (gone, new): (Vec<Id>, Vec<Fill>) = if forward {
                        (vec![before.id], after.clone())
                    } else {
                        (after.iter().map(|x| x.id).collect(), vec![before.clone()])
                    };
                    d.fills.retain(|x| !gone.contains(&x.id));
                    let at = (*index).min(d.fills.len());
                    d.fills.splice(at..at, new);
                }
            }
            Edit::ReplaceStroke {
                drawing,
                index,
                before,
                after,
            } => {
                if let Some(d) = s.drawings.get_mut(drawing) {
                    let (gone, new): (Vec<Id>, Vec<Stroke>) = if forward {
                        (vec![before.id], after.clone())
                    } else {
                        (after.iter().map(|x| x.id).collect(), vec![before.clone()])
                    };
                    d.strokes.retain(|x| !gone.contains(&x.id));
                    let at = (*index).min(d.strokes.len());
                    d.strokes.splice(at..at, new);
                }
            }
            Edit::AddFill { drawing, fill } => {
                if let Some(d) = s.drawings.get_mut(drawing) {
                    if forward {
                        d.fills.push(fill.clone());
                    } else {
                        d.fills.retain(|f| f.id != fill.id);
                    }
                }
            }
            Edit::RemoveFill {
                drawing,
                index,
                fill,
            } => {
                if let Some(d) = s.drawings.get_mut(drawing) {
                    if forward {
                        d.fills.retain(|f| f.id != fill.id);
                    } else {
                        d.fills.insert((*index).min(d.fills.len()), fill.clone());
                    }
                }
            }
            Edit::SetPalette { before, after } => {
                s.palette = if forward { after } else { before }.clone();
            }
            Edit::SetStrokeSwatch {
                drawing,
                stroke,
                before,
                after,
            } => {
                if let Some(st) = s
                    .drawings
                    .get_mut(drawing)
                    .and_then(|d| d.strokes.iter_mut().find(|st| st.id == *stroke))
                {
                    st.swatch = if forward { *after } else { *before };
                }
            }
            Edit::SetFillSwatch {
                drawing,
                fill,
                before,
                after,
            } => {
                if let Some(f) = s
                    .drawings
                    .get_mut(drawing)
                    .and_then(|d| d.fills.iter_mut().find(|f| f.id == *fill))
                {
                    f.swatch = if forward { *after } else { *before };
                }
            }
            Edit::SetStrokes {
                drawing,
                before,
                after,
            } => {
                let new = if forward { after } else { before };
                if let Some(d) = s.drawings.get_mut(drawing) {
                    for st in d.strokes.iter_mut() {
                        if let Some(n) = new.iter().find(|n| n.id == st.id) {
                            *st = n.clone();
                        }
                    }
                }
            }
            Edit::AddDrawing { drawing } => {
                if forward {
                    s.drawings.insert(drawing.id, drawing.clone());
                } else {
                    s.drawings.remove(&drawing.id);
                }
            }
            Edit::SetCells {
                column,
                from,
                before,
                after,
            } => {
                let (old, new) = if forward {
                    (before, after)
                } else {
                    (after, before)
                };
                if let Some(c) = s.xsheet.column_mut(*column) {
                    let span = old.len().max(new.len()) as u32;
                    for k in 0..span {
                        c.set(from + k, new.get(k as usize).copied().flatten());
                    }
                }
            }
            Edit::SetPeg {
                node,
                before,
                after,
            } => {
                if let Some(n) = s.graph.node_mut(*node) {
                    n.kind = NodeKind::Peg(if forward { after } else { before }.clone());
                }
            }
            Edit::SetSwatch {
                swatch,
                before,
                after,
            } => {
                if let Some(sw) = s.palette.get_mut(*swatch) {
                    let [r, g, b, a] = if forward { *after } else { *before };
                    sw.rgb = [r, g, b];
                    sw.opacity = a;
                }
            }
            Edit::SetVisible {
                column,
                before,
                after,
            } => {
                if let Some(c) = s.xsheet.column_mut(*column) {
                    c.visible = if forward { *after } else { *before };
                }
            }
            Edit::Rename {
                column,
                before,
                after,
            } => {
                let name = if forward { after } else { before };
                if let Some(c) = s.xsheet.column_mut(*column) {
                    c.name = name.clone();
                }
                if let Some(l) = s.layer_of(*column).map(|i| s.layers[i]) {
                    if let Some(n) = s.graph.node_mut(l.node) {
                        n.name = name.clone();
                    }
                    if let Some(n) = s.graph.node_mut(l.peg) {
                        n.name = format!("{name}-P");
                    }
                }
            }
            Edit::SetCameraMotion { before, after } => {
                s.camera_motion = if forward { after } else { before }.clone();
            }
            Edit::SetCameraFov { before, after } => {
                s.camera_fov = if forward { after } else { before }.clone();
            }
            Edit::SetFrameRate { before, after } => {
                s.settings.fps = if forward { *after } else { *before };
            }
            Edit::SetLength { before, after } => {
                s.settings.length = if forward { *after } else { *before }
            }
            Edit::SetBars { before, after } => {
                s.settings.bars = if forward { *after } else { *before };
            }
            Edit::SetSounds { before, after } => {
                s.sounds = if forward {
                    after.clone()
                } else {
                    before.clone()
                };
            }
            Edit::MoveLayer { from, to } => {
                let (a, b) = if forward { (*from, *to) } else { (*to, *from) };
                s.move_layer(a, b);
            }
            Edit::AddLayer(snap) | Edit::RemoveLayer(snap) => {
                let adding = matches!(self, Edit::AddLayer(_)) == forward;
                if adding {
                    s.insert_layer(snap);
                } else if let Some(i) = s.layer_of(snap.layer.column) {
                    s.take_layer(i);
                }
            }
        }
        s.version += 1;
    }

    /// Fold a later edit of the same target into this one (E3.2).
    fn absorb(&mut self, later: &Edit) -> bool {
        match (self, later) {
            (
                Edit::SetPeg { node, after, .. },
                Edit::SetPeg {
                    node: n2,
                    after: a2,
                    ..
                },
            ) if node == n2 => {
                *after = a2.clone();
                true
            }
            (
                Edit::SetSwatch { swatch, after, .. },
                Edit::SetSwatch {
                    swatch: s2,
                    after: a2,
                    ..
                },
            ) if swatch == s2 => {
                *after = *a2;
                true
            }
            // Live property edits on one node: first before, latest after.
            (
                Edit::SetNodeProperties { id, after, .. },
                Edit::SetNodeProperties {
                    id: i2, after: a2, ..
                },
            ) if id == i2 => {
                *after = a2.clone();
                true
            }
            // A gesture that edits strokes again and again (the Line
            // Editor, a drag) keeps one entry: each stroke's first "before"
            // and latest "after", instead of a copy per frame.
            (
                Edit::SetStrokes {
                    drawing,
                    before,
                    after,
                },
                Edit::SetStrokes {
                    drawing: d2,
                    before: b2,
                    after: a2,
                },
            ) if drawing == d2 => {
                for (old, new) in b2.iter().zip(a2) {
                    match after.iter_mut().find(|s| s.id == new.id) {
                        Some(slot) => *slot = new.clone(),
                        None => {
                            before.push(old.clone());
                            after.push(new.clone());
                        }
                    }
                }
                true
            }
            _ => false,
        }
    }
}

#[derive(Clone, Debug)]
struct Step {
    name: String,
    edits: Vec<Edit>,
    /// Steps with the same open key merge until `seal` (one gesture).
    open: Option<String>,
}

impl Step {
    fn bytes(&self) -> usize {
        64 + self.name.len() + self.edits.iter().map(Edit::approx_bytes).sum::<usize>()
    }
}

#[derive(Clone, Debug, Default)]
pub struct History {
    undo: std::collections::VecDeque<Step>,
    redo: Vec<Step>,
    /// Roughly how much memory the undo steps hold, in bytes.
    bytes: usize,
}

/// Undo is bounded by memory, not a small count (E3.4): up to this many
/// steps, as long as they fit in `BUDGET`. Oldest steps go first. A stroke
/// costs a few kilobytes, so thousands of steps fit; only very large edits
/// make room by dropping old ones.
const LIMIT: usize = 10_000;
const BUDGET: usize = 512 * 1024 * 1024;

fn stroke_bytes(s: &Stroke) -> usize {
    96 + s.points.len() * 12 + s.shape.iter().map(|c| c.len() * 8 + 24).sum::<usize>()
}

fn fill_bytes(f: &Fill) -> usize {
    64 + f.contours.iter().map(|c| c.len() * 8 + 24).sum::<usize>()
        + f.linked
            .as_ref()
            .map_or(0, |link| 48 + link.strokes.len() * 8)
}

impl Edit {
    /// Rough memory held by this edit, for the undo budget.
    fn approx_bytes(&self) -> usize {
        let strokes = |v: &[Stroke]| v.iter().map(stroke_bytes).sum::<usize>();
        let fills = |v: &[Fill]| v.iter().map(fill_bytes).sum::<usize>();
        64 + match self {
            Edit::SetText { before, after, .. } => before
                .iter()
                .chain(after)
                .map(|text| 104 + text.text.len() + text.style.family.len() + text.glyphs.len() * 8)
                .sum(),
            Edit::SetStrokes { before, after, .. } => strokes(before) + strokes(after),
            Edit::AddStroke { stroke, .. } | Edit::RemoveStroke { stroke, .. } => {
                stroke_bytes(stroke)
            }
            Edit::ReplaceStroke { before, after, .. } => stroke_bytes(before) + strokes(after),
            Edit::AddFill { fill, .. } | Edit::RemoveFill { fill, .. } => fill_bytes(fill),
            Edit::ReplaceFill { before, after, .. } => fill_bytes(before) + fills(after),
            Edit::AddDrawing { drawing } => strokes(&drawing.strokes) + fills(&drawing.fills),
            Edit::SetCells { before, after, .. } => (before.len() + after.len()) * 16,
            Edit::Rename { before, after, .. } => before.len() + after.len(),
            _ => 512,
        }
    }
}

impl History {
    /// Apply `edits` as one undo step called `name`.
    pub fn apply(&mut self, scene: &mut Scene, name: &str, edits: Vec<Edit>) {
        if edits.is_empty() {
            return;
        }
        self.seal();
        for e in &edits {
            e.apply(scene, true);
        }
        self.push(Step {
            name: name.into(),
            edits,
            open: None,
        });
    }

    /// Apply `edit` as part of an ongoing gesture `key` (a drag, an erase).
    /// Everything until `seal` becomes one undo step.
    pub fn apply_merged(&mut self, scene: &mut Scene, name: &str, key: &str, edit: Edit) {
        edit.apply(scene, true);
        if let Some(top) = self
            .undo
            .back_mut()
            .filter(|t| t.open.as_deref() == Some(key))
        {
            let before = top.bytes();
            if !top.edits.iter_mut().any(|e| e.absorb(&edit)) {
                top.edits.push(edit);
            }
            let after = top.bytes();
            self.bytes = (self.bytes + after).saturating_sub(before);
            self.trim();
            return;
        }
        self.seal();
        self.push(Step {
            name: name.into(),
            edits: vec![edit],
            open: Some(key.into()),
        });
    }

    /// End the current gesture.
    pub fn seal(&mut self) {
        if let Some(top) = self.undo.back_mut() {
            top.open = None;
        }
    }

    fn push(&mut self, step: Step) {
        self.bytes += step.bytes();
        self.undo.push_back(step);
        self.redo.clear();
        self.trim();
    }

    /// Drop the oldest steps until within the step limit and memory budget
    /// (always keeping the newest).
    fn trim(&mut self) {
        while self.undo.len() > 1 && (self.undo.len() > LIMIT || self.bytes > BUDGET) {
            if let Some(old) = self.undo.pop_front() {
                self.bytes = self.bytes.saturating_sub(old.bytes());
            }
        }
    }

    /// How many steps can be undone.
    pub fn undo_len(&self) -> usize {
        self.undo.len()
    }

    /// Undo the last step. Returns its name.
    pub fn undo(&mut self, scene: &mut Scene) -> Option<String> {
        self.seal();
        let step = self.undo.pop_back()?;
        self.bytes = self.bytes.saturating_sub(step.bytes());
        for e in step.edits.iter().rev() {
            e.apply(scene, false);
        }
        let name = step.name.clone();
        self.redo.push(step);
        Some(name)
    }

    pub fn redo(&mut self, scene: &mut Scene) -> Option<String> {
        let step = self.redo.pop()?;
        for e in &step.edits {
            e.apply(scene, true);
        }
        let name = step.name.clone();
        self.bytes += step.bytes();
        self.undo.push_back(step);
        self.trim();
        Some(name)
    }

    pub fn undo_name(&self) -> Option<&str> {
        self.undo.back().map(|s| s.name.as_str())
    }

    pub fn redo_name(&self) -> Option<&str> {
        self.redo.last().map(|s| s.name.as_str())
    }

    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
        self.bytes = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::Settings;
    use crate::stroke::{ArtLayer, Point, StrokeKind};

    fn scene_with_layer() -> (Scene, History) {
        let mut s = Scene::new(Settings::default());
        let mut h = History::default();
        let snap = s.make_layer("Head", 0);
        h.apply(&mut s, "Add layer", vec![Edit::AddLayer(Box::new(snap))]);
        (s, h)
    }

    fn stroke(s: &mut Scene) -> Stroke {
        let swatch = s.palette.swatches[0].id;
        Stroke {
            id: s.ids.fresh(),
            kind: StrokeKind::Brush,
            art: ArtLayer::LineArt,
            swatch,
            min_width: 1.0,
            max_width: 8.0,
            points: vec![Point {
                x: 0.0,
                y: 0.0,
                pressure: 1.0,
                pen: Default::default(),
            }],
            tips: Default::default(),
            texture: None,
            shape: Vec::new(),
        }
    }

    #[test]
    fn stroke_on_new_drawing_undoes_in_one_step() {
        let (mut s, mut h) = scene_with_layer();
        let col = s.layers[0].column;
        let d = s.make_drawing(col);
        let st = stroke(&mut s);
        let did = d.id;
        let cells = Edit::splice(&s, col, 1, 0, vec![Some(did)]).unwrap();
        h.apply(
            &mut s,
            "Brush stroke",
            vec![
                Edit::AddDrawing { drawing: d },
                cells,
                Edit::AddStroke {
                    drawing: did,
                    stroke: st,
                },
            ],
        );
        assert_eq!(s.exposed(0, 1), Some(did));
        assert_eq!(s.drawings[&did].strokes.len(), 1);

        assert_eq!(h.undo(&mut s).as_deref(), Some("Brush stroke"));
        assert_eq!(s.exposed(0, 1), None);
        assert!(!s.drawings.contains_key(&did));

        h.redo(&mut s);
        assert_eq!(s.drawings[&did].strokes.len(), 1);
    }

    #[test]
    fn remove_layer_undo_restores_order_and_wiring() {
        let (mut s, mut h) = scene_with_layer();
        let snap = s.make_layer("Body", 1);
        h.apply(&mut s, "Add layer", vec![Edit::AddLayer(Box::new(snap))]);
        let before: Vec<_> = s.layers.clone();
        let taken = s.take_layer(0).unwrap();
        s.insert_layer(&taken);
        assert_eq!(s.layers, before);

        let snap = s.take_layer(0).unwrap();
        s.insert_layer(&snap);
        h.apply(
            &mut s,
            "Delete layer",
            vec![Edit::RemoveLayer(Box::new(snap))],
        );
        assert_eq!(s.layers.len(), 1);
        h.undo(&mut s);
        assert_eq!(s.layers, before);
        assert_eq!(s.graph.input(s.composite, 0), Some(before[0].node));
    }

    #[test]
    fn drag_merges_into_one_step() {
        let (mut s, mut h) = scene_with_layer();
        let peg = s.layers[0].peg;
        let p0 = s.peg(0).unwrap().clone();
        let mut p = p0.clone();
        for x in 1..=5 {
            let before = s.peg(0).unwrap().clone();
            p.x = crate::anim::Value::Constant(x as f32);
            h.apply_merged(
                &mut s,
                "Move peg",
                "drag",
                Edit::SetPeg {
                    node: peg,
                    before,
                    after: p.clone(),
                },
            );
        }
        h.seal();
        assert_eq!(s.peg(0).unwrap().x.at(1), 5.0);
        h.undo(&mut s);
        assert_eq!(s.peg(0).unwrap(), &p0);
    }

    #[test]
    fn splice_inserts_and_shifts() {
        let (mut s, mut h) = scene_with_layer();
        let col = s.layers[0].column;
        let (a, b) = (Id(100), Id(101));
        s.xsheet.column_mut(col).unwrap().expose(a, 1, 2);
        s.xsheet.column_mut(col).unwrap().expose(b, 3, 1);
        // Extend the first exposure by one frame: insert a cell at 3.
        let e = Edit::splice(&s, col, 3, 0, vec![Some(a)]).unwrap();
        h.apply(&mut s, "Extend exposure", vec![e]);
        let cells: Vec<_> = (1..=4).map(|f| s.exposed(0, f)).collect();
        assert_eq!(cells, [Some(a), Some(a), Some(a), Some(b)]);
        h.undo(&mut s);
        let cells: Vec<_> = (1..=4).map(|f| s.exposed(0, f)).collect();
        assert_eq!(cells, [Some(a), Some(a), Some(b), None]);
    }

    #[test]
    fn set_strokes_round_trips() {
        let (mut s, mut h) = scene_with_layer();
        let col = s.layers[0].column;
        let d = s.make_drawing(col);
        let did = d.id;
        let st = stroke(&mut s);
        h.apply(
            &mut s,
            "Draw",
            vec![
                Edit::AddDrawing { drawing: d },
                Edit::AddStroke {
                    drawing: did,
                    stroke: st.clone(),
                },
            ],
        );
        let mut moved = st.clone();
        moved.points[0].x += 10.0;
        h.apply(
            &mut s,
            "Move",
            vec![Edit::SetStrokes {
                drawing: did,
                before: vec![st.clone()],
                after: vec![moved],
            }],
        );
        assert_eq!(s.drawings[&did].strokes[0].points[0].x, 10.0);
        h.undo(&mut s);
        assert_eq!(s.drawings[&did].strokes[0].points[0].x, 0.0);
    }

    #[test]
    fn undo_keeps_thousands_of_strokes_within_a_memory_budget() {
        let mut s = Scene::new(Settings::default());
        let mut h = History::default();
        let snap = s.make_layer("A", 0);
        h.apply(&mut s, "Add layer", vec![Edit::AddLayer(Box::new(snap))]);
        let d = s.make_drawing(s.layers[0].column);
        let did = d.id;
        h.apply(&mut s, "Draw", vec![Edit::AddDrawing { drawing: d }]);
        let stroke = |id| Stroke {
            id,
            kind: StrokeKind::Brush,
            art: Default::default(),
            swatch: Id(0),
            min_width: 1.0,
            max_width: 4.0,
            points: (0..80)
                .map(|k| Point {
                    x: k as f32,
                    y: 0.0,
                    pressure: 1.0,
                    pen: Default::default(),
                })
                .collect(),
            tips: Default::default(),
            texture: None,
            shape: Vec::new(),
        };
        for _ in 0..3000 {
            let st = stroke(s.ids.fresh());
            h.apply(
                &mut s,
                "Draw",
                vec![Edit::AddStroke {
                    drawing: did,
                    stroke: st,
                }],
            );
        }
        // The old 500-step limit would have kept 500; all 3000 fit now.
        assert!(h.undo_len() >= 3000, "{}", h.undo_len());
        assert!(h.bytes < BUDGET);
        // Undo and redo keep the count of held memory in step.
        let held = h.bytes;
        h.undo(&mut s);
        assert!(h.bytes < held);
        h.redo(&mut s);
        assert_eq!(h.bytes, held);
    }

    #[test]
    fn repeated_stroke_edits_in_one_gesture_fold_into_one_entry() {
        let mut s = Scene::new(Settings::default());
        let mut h = History::default();
        let snap = s.make_layer("A", 0);
        h.apply(&mut s, "Add layer", vec![Edit::AddLayer(Box::new(snap))]);
        let d = s.make_drawing(s.layers[0].column);
        let did = d.id;
        h.apply(&mut s, "Draw", vec![Edit::AddDrawing { drawing: d }]);
        let st = Stroke {
            id: s.ids.fresh(),
            kind: StrokeKind::Brush,
            art: Default::default(),
            swatch: Id(0),
            min_width: 1.0,
            max_width: 1.0,
            points: vec![Point {
                x: 0.0,
                y: 0.0,
                pressure: 1.0,
                pen: Default::default(),
            }],
            tips: Default::default(),
            texture: None,
            shape: Vec::new(),
        };
        h.apply(
            &mut s,
            "Draw",
            vec![Edit::AddStroke {
                drawing: did,
                stroke: st.clone(),
            }],
        );
        let mut cur = st.clone();
        for i in 1..=50 {
            let mut next = cur.clone();
            next.points[0].x = i as f32;
            h.apply_merged(
                &mut s,
                "Edit lines",
                "line-edit",
                Edit::SetStrokes {
                    drawing: did,
                    before: vec![cur.clone()],
                    after: vec![next.clone()],
                },
            );
            cur = next;
        }
        assert_eq!(s.drawings[&did].strokes[0].points[0].x, 50.0);
        let step = h.undo.back().unwrap();
        assert_eq!(step.edits.len(), 1, "one entry for the whole drag");
        h.undo(&mut s);
        assert_eq!(s.drawings[&did].strokes[0], st, "undo restores the start");
    }

    #[test]
    fn replacing_a_stroke_keeps_its_depth_through_undo() {
        let mut s = Scene::new(Settings::default());
        let mut h = History::default();
        let snap = s.make_layer("A", 0);
        h.apply(&mut s, "Add layer", vec![Edit::AddLayer(Box::new(snap))]);
        let d = s.make_drawing(s.layers[0].column);
        let did = d.id;
        h.apply(&mut s, "Draw", vec![Edit::AddDrawing { drawing: d }]);
        let mk = |s: &mut Scene, x: f32| Stroke {
            id: s.ids.fresh(),
            kind: StrokeKind::Brush,
            art: Default::default(),
            swatch: Id(0),
            min_width: 1.0,
            max_width: 1.0,
            points: vec![Point {
                x,
                y: 0.0,
                pressure: 1.0,
                pen: Default::default(),
            }],
            tips: Default::default(),
            texture: None,
            shape: Vec::new(),
        };
        let (a, b, c) = (mk(&mut s, 0.0), mk(&mut s, 1.0), mk(&mut s, 2.0));
        for st in [&a, &b, &c] {
            h.apply(
                &mut s,
                "Draw",
                vec![Edit::AddStroke {
                    drawing: did,
                    stroke: st.clone(),
                }],
            );
        }
        let ids = |s: &Scene| {
            s.drawings[&did]
                .strokes
                .iter()
                .map(|x| x.id)
                .collect::<Vec<_>>()
        };
        let (p, q) = (mk(&mut s, 5.0), mk(&mut s, 6.0));
        h.apply(
            &mut s,
            "Erase",
            vec![Edit::ReplaceStroke {
                drawing: did,
                index: 1,
                before: b.clone(),
                after: vec![p.clone(), q.clone()],
            }],
        );
        assert_eq!(ids(&s), vec![a.id, p.id, q.id, c.id]);
        h.undo(&mut s);
        assert_eq!(ids(&s), vec![a.id, b.id, c.id]);
        h.apply(
            &mut s,
            "Erase",
            vec![Edit::ReplaceStroke {
                drawing: did,
                index: 1,
                before: b.clone(),
                after: vec![],
            }],
        );
        assert_eq!(ids(&s), vec![a.id, c.id]);
        h.undo(&mut s);
        assert_eq!(
            ids(&s),
            vec![a.id, b.id, c.id],
            "fully erased line comes back in place"
        );
    }

    #[test]
    fn scene_survives_json() {
        let (mut s, _) = scene_with_layer();
        let col = s.layers[0].column;
        let d = s.make_drawing(col);
        s.drawings.insert(d.id, d);
        let json = serde_json::to_string(&s).unwrap();
        let back: Scene = serde_json::from_str(&json).unwrap();
        assert_eq!(back.layers, s.layers);
        assert_eq!(back.drawings.len(), 1);
    }
}
