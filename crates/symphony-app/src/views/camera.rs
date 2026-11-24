//! Camera and Drawing views: one canvas, two modes (DESIGN D3.6).
//! - Camera renders the Display node, including compositing effects.
//! - Drawing shows only the selected layer's drawing, untransformed.
//!
//! Navigation: Space+drag pans, Shift+Space+drag rotates, pinch or
//! Cmd/Ctrl+scroll zooms, scroll pans. The Hand and Zoom tools do the same
//! with the pen. Drawing on an empty cell creates the next drawing.

use eframe::egui::{self, Align2, Color32, Pos2, Rect, Sense, Stroke as EStroke, Vec2};
use symphony_core::id::Id;
use symphony_core::math::Affine;
use symphony_core::stroke::{ArtLayer, Point, Stroke, StrokeKind};
use symphony_pen::{PenSample, Phase, Tool as PenTool};
use symphony_ui::icons::{self, Icon};
use symphony_ui::{theme, widgets};
use symphony_vector::{pressure_curve, scale_of, Smoother};

use crate::keymap::{self, Action, Profile};
use crate::session::{apply_stencil, CanvasColours, Session, Stencil, Tool};
use crate::views::select::{self, SelectState};

#[path = "camera_render.rs"]
mod camera_render;

/// Eraser radius, in screen points.
const MIN_ZOOM: f32 = 0.05;
const MAX_ZOOM: f32 = 32.0;

pub struct CanvasState {
    pan: Vec2,
    /// Screen points per scene unit: absolute, as in Harmony, so 100% is
    /// actual size and resizing the view keeps the drawing's scale (and
    /// re-stamps nothing). Fitted to the view when first shown and on Fit.
    zoom: f32,
    framing: f32,
    /// Fit the stage to the view on the next frame.
    fit_pending: bool,
    /// Degrees, clockwise.
    rotation: f32,
    active: Option<Active>,
    /// An eraser drag in progress.
    erasing: Option<Erasing>,
    /// A Line Editor drag in progress.
    line_edit: Option<LineDrag>,
    /// Screen -> scene for the last frame, used by input.
    last_view: Affine,
    /// Where the canvas was on screen last frame (zoom keys aim at the
    /// mouse when it's over it).
    last_rect: Option<Rect>,
    pub select: SelectState,
    /// Mirrored left to right, to check a drawing with fresh eyes (view
    /// only; the artwork isn't changed).
    pub mirrored: bool,
    /// Stamped textured strokes, reused while only panning.
    strokes: super::textured::Cache,
    display: camera_render::Cache,
    pub render_effects: bool,
    /// The selected Mesh Warp's grid, being edited.
    warp_edit: super::warp_edit::WarpEdit,
    rig_edit: super::rig_edit::RigEdit,
    /// Text being typed with the Text tool.
    text: Option<TextDraft>,
    /// Drawings bent by Mesh Warps, by drawing: rebuilt only when the
    /// drawing or the warp changes (key), never every frame.
    bent: std::collections::HashMap<Id, (u64, std::sync::Arc<symphony_core::stroke::Drawing>)>,
}

impl Default for CanvasState {
    fn default() -> Self {
        CanvasState {
            mirrored: false,
            pan: Vec2::ZERO,
            zoom: 1.0,
            framing: 1.0,
            fit_pending: true,
            rotation: 0.0,
            active: None,
            erasing: None,
            line_edit: None,
            last_view: Affine::IDENTITY,
            last_rect: None,
            select: SelectState::default(),
            strokes: Default::default(),
            display: Default::default(),
            render_effects: true,
            warp_edit: Default::default(),
            rig_edit: Default::default(),
            text: None,
            bent: Default::default(),
        }
    }
}

/// What a Line Editor drag does: push lines, Shift-paint a point
/// selection, or move the selected points.
#[derive(Clone, Copy, PartialEq)]
enum LineMode {
    Push,
    Select,
    MoveSelected,
}

struct LineDrag {
    drawing: Id,
    to_local: Affine,
    /// Last position, drawing space.
    last: [f32; 2],
    mode: LineMode,
    moved: bool,
    /// Drawing units per screen point, for tolerances.
    local_px: f32,
}

/// One step of a Line Editor drag to scene point `to`; `radius` is in
/// drawing units.
fn line_step(s: &mut Session, le: &mut LineDrag, to: (f32, f32), radius: f32) {
    let (x, y) = le.to_local.apply(to.0, to.1);
    let delta = [x - le.last[0], y - le.last[1]];
    if delta[0] == 0.0 && delta[1] == 0.0 {
        return;
    }
    le.moved = true;
    match le.mode {
        LineMode::Push => s.line_push(le.drawing, le.last, radius, delta),
        LineMode::Select => s.line_select(le.drawing, [x, y], radius),
        LineMode::MoveSelected => s.line_move_selected(le.drawing, delta),
    }
    le.last = [x, y];
}

/// What one eraser drag erases: drawings with their scene -> drawing
/// transforms, and the last stamp (scene space) so the path is continuous.
struct Erasing {
    targets: Vec<(Id, Affine)>,
    last: Option<symphony_vector::cut::Disc>,
}

struct Active {
    layer: usize,
    frame: u32,
    /// Scene -> drawing-local.
    to_local: Affine,
    smoother: Smoother,
    min_step: f32,
    stroke: Stroke,
    /// Pencil thickness stencil, if the preset has one.
    stencil: Option<Stencil>,
    /// Stamping state while a textured brush stroke is being drawn.
    live: Option<super::textured::Live>,
    /// The Line tool's line, drawing-local: its points are rebuilt from
    /// this each move rather than following the pen's path.
    line: Option<LineDraft>,
}

/// A line being dragged: start, end, the point a bend passes through
/// (while Alt is held), and the pen pressure it was started with.
#[derive(Clone, Copy)]
struct LineDraft {
    start: [f32; 2],
    end: [f32; 2],
    bend: Option<[f32; 2]>,
    pressure: f32,
}

/// `end` turned about `start` to the nearest multiple of 22.5 degrees
/// (so 45 and 90 too), keeping its distance.
fn snap_line(start: [f32; 2], end: [f32; 2]) -> [f32; 2] {
    let (dx, dy) = (end[0] - start[0], end[1] - start[1]);
    let len = dx.hypot(dy);
    if len < 1e-6 {
        return end;
    }
    let step = std::f32::consts::PI / 8.0;
    let a = (dy.atan2(dx) / step).round() * step;
    [start[0] + a.cos() * len, start[1] + a.sin() * len]
}

/// The line's points: straight, or a curve through `bend` (a quadratic
/// whose middle passes through the bend point). Spaced about `step`.
fn line_points(d: &LineDraft, step: f32) -> Vec<Point> {
    let [sx, sy] = d.start;
    let [ex, ey] = d.end;
    // Control point so the curve's midpoint lands on the bend point.
    let c = d
        .bend
        .map(|[bx, by]| [2.0 * bx - (sx + ex) / 2.0, 2.0 * by - (sy + ey) / 2.0]);
    let reach = (ex - sx).hypot(ey - sy) + c.map_or(0.0, |[cx, cy]| (cx - sx).hypot(cy - sy));
    let n = ((reach / step.max(1e-3)) as usize).clamp(1, 96);
    (0..=n)
        .map(|i| {
            let t = i as f32 / n as f32;
            let (x, y) = match c {
                Some([cx, cy]) => {
                    let u = 1.0 - t;
                    (
                        u * u * sx + 2.0 * u * t * cx + t * t * ex,
                        u * u * sy + 2.0 * u * t * cy + t * t * ey,
                    )
                }
                None => (sx + (ex - sx) * t, sy + (ey - sy) * t),
            };
            Point {
                x,
                y,
                pressure: d.pressure,
                pen: Default::default(),
            }
        })
        .collect()
}

impl CanvasState {
    /// Performance runs (SYMPHONY_PERF_NAV): keep zooming in and out, or
    /// panning around, as an artist navigating would.
    pub fn perf_navigate(&mut self, t: f32, how: &str) {
        match how {
            "zoom" => {
                self.zoom = (self.zoom * (1.0 + 0.025 * (t * 1.2).cos())).clamp(MIN_ZOOM, MAX_ZOOM)
            }
            "pan" => self.pan += Vec2::new((t * 0.9).cos(), (t * 0.7).sin()) * 6.0,
            _ => {}
        }
    }

    /// Where scene point (`x`, `y`) was on screen last frame (UI tests).
    #[cfg(test)]
    pub fn to_screen(&self, x: f32, y: f32) -> Pos2 {
        let m = self.last_view.inverse().unwrap_or(Affine::IDENTITY);
        let (sx, sy) = m.apply(x, y);
        Pos2::new(sx, sy)
    }

    /// Set the zoom (screen points per scene unit), e.g. from search.
    pub fn set_zoom(&mut self, zoom: f32) {
        self.zoom = zoom.clamp(MIN_ZOOM, MAX_ZOOM);
        self.fit_pending = false;
    }

    pub fn capture_wallpaper(&mut self, enabled: bool) {
        self.display.capture_wallpaper(enabled);
    }

    pub fn wallpaper_pixels(&self, session: &Session) -> Option<crate::wallpaper::CameraPixels> {
        self.display.wallpaper_pixels(session)
    }

    pub fn busy(&self) -> bool {
        self.active.is_some()
            || self.erasing.is_some()
            || self.line_edit.is_some()
            || self.select.busy()
    }

    /// Harmony's Reset View: centred, upright, and the stage fitted to the view.
    pub fn reset_view(&mut self) {
        self.pan = Vec2::ZERO;
        self.fit_pending = true;
        self.rotation = 0.0;
        self.display = Default::default();
    }

    pub fn rotate_by(&mut self, degrees: f32) {
        self.rotation = (self.rotation + degrees).rem_euclid(360.0);
    }

    /// Run a View action aimed at the mouse: zooming in or out keeps the
    /// point under the mouse in place when the mouse is over this canvas
    /// (like the scroll wheel); otherwise the view's centre stays.
    pub fn apply_at(&mut self, action: Action, pointer: Option<Pos2>) {
        let factor = match action {
            Action::ZoomIn => 1.25,
            Action::ZoomOut => 1.0 / 1.25,
            _ => return self.apply(action),
        };
        match (self.last_rect, pointer) {
            (Some(rect), Some(p)) if rect.contains(p) => zoom_at(self, rect, p, factor),
            _ => self.apply(action),
        }
    }

    /// Run a View action on this canvas (Harmony's View menu).
    pub fn apply(&mut self, action: Action) {
        match action {
            Action::ZoomIn => self.zoom = (self.zoom * 1.25).min(MAX_ZOOM),
            Action::ZoomOut => self.zoom = (self.zoom / 1.25).max(MIN_ZOOM),
            Action::RotateCw => self.rotate_by(ROTATE_STEP),
            Action::RotateCcw => self.rotate_by(-ROTATE_STEP),
            // 100%: one scene unit per screen point.
            Action::ResetZoom => self.zoom = 1.0,
            Action::ResetRotation => self.rotation = 0.0,
            Action::ResetPan => self.pan = Vec2::ZERO,
            Action::ResetView => self.reset_view(),
            Action::MirrorView => self.mirrored = !self.mirrored,
            _ => {}
        }
    }
}

/// The zoom that fits a `w` x `h` stage in `rect`, with a margin.
fn fit_zoom(rect: Rect, w: f32, h: f32) -> f32 {
    ((rect.width() - 64.0) / w)
        .min((rect.height() - 64.0) / h)
        .clamp(MIN_ZOOM, MAX_ZOOM)
}

/// Scene units -> screen points for this canvas rect. The zoom is
/// absolute: the rect only places the centre.
fn view_transform(state: &CanvasState, rect: Rect) -> Affine {
    let c = rect.center() + state.pan;
    let scale = state.zoom * state.framing;
    let mirror = if state.mirrored { -1.0 } else { 1.0 };
    Affine::translate(c.x, c.y)
        .compose(Affine::scale(mirror, 1.0))
        .compose(Affine::rotate_deg(state.rotation))
        .compose(Affine::scale(scale, scale))
}

fn to_pos((x, y): (f32, f32)) -> Pos2 {
    Pos2::new(x, y)
}

/// Text being typed: where (drawing space of `layer` at `frame`), what,
/// and its letters' triangles for the live preview (by text and style).
struct TextDraft {
    layer: usize,
    frame: u32,
    transform: Affine,
    editing: Option<symphony_core::stroke::TextObject>,
    text: String,
    focus: bool,
    preview: Option<(
        String,
        crate::vector_import::TextStyle,
        std::sync::Arc<symphony_vector::Mesh>,
    )>,
}

/// Put the typed text in the drawing (nothing if it's empty).
fn commit_text(s: &mut Session, draft: TextDraft) {
    s.write_text(
        draft.layer,
        draft.frame,
        draft.transform,
        &draft.text,
        draft.editing.map(|object| object.id),
    );
}

/// The Text tool: a click starts text there; the letters show as they'll
/// be while you type in a small field beside them. Cmd+Enter or a click
/// elsewhere puts them in the drawing (one undo step); Esc cancels.
/// Switching tools, layers or frames finishes the text too.
#[allow(clippy::too_many_arguments)]
fn text_tool(
    ui: &mut egui::Ui,
    response: &egui::Response,
    painter: &egui::Painter,
    s: &mut Session,
    state: &mut CanvasState,
    view: Affine,
    layer_world: Affine,
    space: bool,
) {
    let elsewhere = state
        .text
        .as_ref()
        .is_some_and(|d| s.tool != Tool::Text || d.layer != s.selected || d.frame != s.frame);
    if elsewhere {
        if let Some(d) = state.text.take() {
            commit_text(s, d);
        }
    }
    if s.tool != Tool::Text {
        return;
    }
    let to_screen = view.compose(layer_world);
    if response.clicked() && !space {
        if let Some(d) = state.text.take() {
            commit_text(s, d);
        } else if let (Some(p), Some(back)) = (response.interact_pointer_pos(), to_screen.inverse())
        {
            let (x, y) = back.apply(p.x, p.y);
            let hit = s
                .doc
                .exposed(s.selected, s.frame)
                .and_then(|id| s.doc.drawings.get(&id))
                .and_then(|drawing| {
                    drawing
                        .text
                        .iter()
                        .rev()
                        .find(|object| {
                            let glyphs: Vec<_> = drawing
                                .strokes
                                .iter()
                                .filter(|stroke| {
                                    stroke.art == s.art && object.glyphs.contains(&stroke.id)
                                })
                                .collect();
                            if glyphs.len() != object.glyphs.len() {
                                return false;
                            }
                            symphony_vector::bounds_of(glyphs).is_some_and(|(lo, hi)| {
                                let margin = 5.0 / scale_of(to_screen).max(0.01);
                                x >= lo[0] - margin
                                    && x <= hi[0] + margin
                                    && y >= lo[1] - margin
                                    && y <= hi[1] + margin
                            })
                        })
                        .cloned()
                });
            if let Some(object) = &hit {
                s.text_style = object.style.clone();
            }
            state.text = Some(TextDraft {
                layer: s.selected,
                frame: s.frame,
                transform: hit
                    .as_ref()
                    .map_or(Affine::translate(x, y), |object| object.transform),
                text: hit
                    .as_ref()
                    .map_or_else(String::new, |object| object.text.clone()),
                editing: hit,
                focus: true,
                preview: None,
            });
        }
    }
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Text);
    }
    let Some(draft) = state.text.as_mut() else {
        return;
    };
    // The letters, as they'll be (rebuilt only when the text or style
    // changes).
    let stale = draft
        .preview
        .as_ref()
        .is_none_or(|(t, st, _)| *t != draft.text || *st != s.text_style);
    if stale {
        let mut mesh = symphony_vector::Mesh::default();
        for shape in crate::vector_import::text_shapes(&draft.text, &s.text_style) {
            let part = symphony_vector::fill::fill_mesh(&shape, Affine::IDENTITY);
            let base = mesh.vertices.len() as u32;
            mesh.vertices.extend(part.vertices);
            mesh.indices.extend(part.indices.iter().map(|i| i + base));
        }
        draft.preview = Some((
            draft.text.clone(),
            s.text_style.clone(),
            std::sync::Arc::new(mesh),
        ));
    }
    let m = to_screen.compose(draft.transform);
    let ink = {
        let swatch = draft
            .editing
            .as_ref()
            .and_then(|object| {
                let drawing = s.doc.exposed(draft.layer, draft.frame)?;
                let glyph = s
                    .doc
                    .drawings
                    .get(&drawing)?
                    .strokes
                    .iter()
                    .find(|stroke| object.glyphs.contains(&stroke.id))?;
                Some(glyph.swatch)
            })
            .unwrap_or(s.swatch);
        let [r, g, b, a] = s.doc.swatch_rgba(swatch);
        Color32::from_rgba_unmultiplied(r, g, b, a)
    };
    if let Some((_, _, mesh)) = &draft.preview {
        let mut out = egui::Mesh::default();
        for &[x, y] in &mesh.vertices {
            out.colored_vertex(to_pos(m.apply(x, y)), ink);
        }
        out.indices = mesh.indices.clone();
        painter.add(egui::Shape::mesh(out));
    }
    // Where the text starts.
    let anchor = to_pos(m.apply(0.0, 0.0));
    let line_h = (s.text_style.size * scale_of(m)).max(8.0);
    painter.line_segment(
        [anchor, anchor + Vec2::new(0.0, line_h)],
        EStroke::new(1.5, theme::ACCENT_SELECT),
    );
    // The typing field, just below the first line.
    let mut done = false;
    let mut cancel = false;
    egui::Area::new(egui::Id::unique("text-tool-field"))
        .order(egui::Order::Foreground)
        .fixed_pos(anchor + Vec2::new(0.0, line_h + 6.0))
        .show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                // Taken before the field sees them (it would type a
                // newline for Cmd+Enter).
                ui.input_mut(|i| {
                    done = i.consume_key(egui::Modifiers::COMMAND, egui::Key::Enter);
                    cancel = i.consume_key(egui::Modifiers::NONE, egui::Key::Escape);
                });
                let field = ui.add(
                    egui::TextEdit::multiline(&mut draft.text)
                        .desired_width(240.0)
                        .desired_rows(2)
                        .hint_text("Type text"),
                );
                if draft.focus {
                    field.request_focus();
                    draft.focus = false;
                }
                ui.label(
                    egui::RichText::new(if cfg!(target_os = "macos") {
                        "Cmd+Enter to finish, Esc to cancel"
                    } else {
                        "Ctrl+Enter to finish, Esc to cancel"
                    })
                    .font(theme::dense())
                    .color(theme::TEXT_SECONDARY),
                );
            });
        });
    if cancel {
        state.text = None;
    } else if done {
        if let Some(d) = state.text.take() {
            commit_text(s, d);
        }
    }
}

/// A drawing bent by its Mesh Warps, rebuilt only when the drawing (the
/// scene version) or the warp changed. Bent strokes get their own ids per
/// warp, so the line caches keep the unbent and bent shapes apart.
fn bent_drawing(
    cache: &mut std::collections::HashMap<
        Id,
        (u64, std::sync::Arc<symphony_core::stroke::Drawing>),
    >,
    version: u64,
    d: &symphony_core::stroke::Drawing,
    warp: &symphony_core::warp::Warp,
) -> std::sync::Arc<symphony_core::stroke::Drawing> {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    version.hash(&mut h);
    warp.key().hash(&mut h);
    let key = h.finish();
    if let Some((k, bent)) = cache.get(&d.id) {
        if *k == key {
            return bent.clone();
        }
    }
    let mut bent = symphony_vector::bend::drawing(d, warp);
    let tag = (key >> 1) | (1 << 63);
    for st in &mut bent.strokes {
        st.id = Id(st.id.0 ^ tag);
    }
    for f in &mut bent.fills {
        f.id = Id(f.id.0 ^ tag);
    }
    let bent = std::sync::Arc::new(bent);
    cache.insert(d.id, (key, bent.clone()));
    bent
}

fn paint_stroke(
    painter: &egui::Painter,
    cache: &mut super::textured::Cache,
    textures: &std::collections::BTreeMap<u64, symphony_core::stroke::Texture>,
    stroke: &Stroke,
    colour: Color32,
    m: Affine,
) {
    if let Some(tex) = stroke.texture.filter(|t| t.vector) {
        cache.paint_stamped(painter, textures, stroke, &tex, colour, m);
        return;
    }
    if let Some(tex) = &stroke.texture {
        cache.paint(painter.ctx(), painter, textures, stroke, tex, colour, m);
        return;
    }
    cache.vectors.paint(painter, stroke, colour, m);
}

/// Black bars: the ring between the frame (`w` x `h`, centred) and the
/// opening `outline` (scaled by `k`). Each opening point pairs with the
/// matching point of the opening stretched to the frame (so straight edges
/// meet straight edges), and each rounded corner is filled with a fan from
/// the frame's corner. `outline` runs corner by corner (top-right,
/// bottom-right, bottom-left, top-left), the same number of points each.
fn bars_mesh(
    outline: &[[f32; 2]],
    w: f32,
    h: f32,
    k: f32,
    to_screen: impl Fn(f32, f32) -> Pos2,
) -> egui::Shape {
    let mut mesh = egui::Mesh::default();
    let n = outline.len() as u32;
    let (ow, oh) = outline.iter().fold((0.0f32, 0.0f32), |(a, b), p| {
        (a.max(p[0].abs() * k), b.max(p[1].abs() * k))
    });
    let (sx, sy) = (w / 2.0 / ow.max(1e-6), h / 2.0 / oh.max(1e-6));
    for p in outline {
        let (x, y) = (p[0] * k, p[1] * k);
        mesh.colored_vertex(to_screen(x, y), Color32::BLACK);
        mesh.colored_vertex(to_screen(x * sx, y * sy), Color32::BLACK);
    }
    for i in 0..n {
        let j = (i + 1) % n;
        let (a, b, c, d) = (2 * i, 2 * i + 1, 2 * j, 2 * j + 1);
        mesh.add_triangle(a, b, d);
        mesh.add_triangle(a, d, c);
    }
    // Rounded corners: fan from the frame corner over the stretched arc.
    let per = n / 4;
    if per > 1 {
        for (q, (cx, cy)) in [(1.0, -1.0), (1.0, 1.0), (-1.0, 1.0), (-1.0, -1.0)]
            .into_iter()
            .enumerate()
        {
            let v = mesh.vertices.len() as u32;
            mesh.colored_vertex(to_screen(cx * w / 2.0, cy * h / 2.0), Color32::BLACK);
            let start = q as u32 * per;
            for i in start..start + per - 1 {
                mesh.add_triangle(2 * i + 1, 2 * (i + 1) + 1, v);
            }
        }
    }
    egui::Shape::mesh(mesh)
}

/// When there's no native pen backend, egui's pointer becomes pen samples (E4.2).
pub fn pointer_samples(ctx: &egui::Context) -> Vec<PenSample> {
    ctx.input(|i| {
        let Some(pos) = i.pointer.latest_pos() else {
            return Vec::new();
        };
        let phase = if i.pointer.primary_pressed() {
            Phase::Down
        } else if i.pointer.primary_released() {
            Phase::Up
        } else if i.pointer.primary_down() {
            Phase::Move
        } else {
            Phase::Hover
        };
        vec![PenSample {
            x: pos.x as f64,
            y: pos.y as f64,
            pressure: if phase == Phase::Hover { 0.0 } else { 1.0 },
            tilt_x: 0.0,
            tilt_y: 0.0,
            rotation: 0.0,
            tangential: 0.0,
            phase,
            tool: PenTool::Mouse,
            buttons: 0,
            time: i.time,
        }]
    })
}

/// Rotate View steps 30 degrees, like an animation table (D3.5a).
pub const ROTATE_STEP: f32 = 30.0;

fn dense_label(ui: &mut egui::Ui, text: impl Into<String>) {
    ui.label(
        egui::RichText::new(text.into())
            .font(theme::dense())
            .color(theme::TEXT_SECONDARY),
    );
}

/// Top toolbar (D3.5a): display options, here onion skin.
fn top_toolbar(
    ui: &mut egui::Ui,
    s: &mut Session,
    profile: Profile,
    drawing_only: bool,
    render_effects: &mut bool,
) {
    egui::Frame::new()
        .inner_margin(egui::Margin::symmetric(theme::CARD_PAD, 4))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                let tip = keymap::tooltip(profile, Action::ToggleOnionSkin);
                if icons::button(ui, Icon::Onion, s.onion.on, true, &tip).clicked() {
                    s.onion.on = !s.onion.on;
                }
                // The details live in a small menu, so the bar keeps its
                // room for view and drawing controls.
                let summary = if s.onion.on {
                    format!("{} | {}", s.onion.before, s.onion.after)
                } else {
                    "Off".to_owned()
                };
                ui.menu_button(egui::RichText::new(summary).font(theme::dense()), |ui| {
                    ui.set_min_width(220.0);
                    ui.checkbox(&mut s.onion.on, "Onion skin");
                    let counts: [(&str, Action, Action, bool); 2] = [
                        (
                            "Previous drawings",
                            Action::OnionLessPrevious,
                            Action::OnionMorePrevious,
                            true,
                        ),
                        (
                            "Next drawings",
                            Action::OnionLessNext,
                            Action::OnionMoreNext,
                            false,
                        ),
                    ];
                    for (label, less, more, before) in counts {
                        ui.horizontal(|ui| {
                            dense_label(ui, label);
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    let n = if before {
                                        s.onion.before
                                    } else {
                                        s.onion.after
                                    };
                                    if icons::button(
                                        ui,
                                        Icon::Plus,
                                        false,
                                        n < 9,
                                        &keymap::tooltip(profile, more),
                                    )
                                    .clicked()
                                    {
                                        s.onion_step(before, 1);
                                    }
                                    dense_label(ui, n.to_string());
                                    if icons::button(
                                        ui,
                                        Icon::Minus,
                                        false,
                                        n > 0,
                                        &keymap::tooltip(profile, less),
                                    )
                                    .clicked()
                                    {
                                        s.onion_step(before, -1);
                                    }
                                },
                            );
                        });
                    }
                    let mut percent = s.onion.opacity * 100.0;
                    if symphony_ui::widgets::precise_slider(
                        ui,
                        egui::Slider::new(&mut percent, 5.0..=100.0)
                            .step_by(1.0)
                            .suffix("%")
                            .text("Opacity"),
                    )
                    .changed()
                    {
                        s.onion.opacity = percent / 100.0;
                    }
                    ui.separator();
                    ui.checkbox(&mut s.light_table, "Light table")
                        .on_hover_text(
                            "Show the other layers' drawings faintly behind the one you're drawing",
                        );
                    dense_label(ui, "Tints: Preferences, Canvas");
                });
                ui.add_space(10.0);
                art_layer_toolbar(ui, s, profile);
                if !drawing_only {
                    bars_menu(ui, s);
                    ui.toggle_value(render_effects, "Render effects")
                        .on_hover_text(
                            "Preview Camera effects. Turn off to position the unprocessed artwork.",
                        );
                }
                if drawing_only {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        dense_label(ui, "Selected drawing only, without pegs");
                    });
                }
            });
        });
}

/// Black bars over the camera frame: rounded edges or a cinematic shape.
/// They're part of the picture, so exports draw them too.
fn bars_menu(ui: &mut egui::Ui, s: &mut Session) {
    let mut bars = s.doc.settings.bars;
    let label = if bars.on { "Bars: on" } else { "Bars" };
    ui.menu_button(label, |ui| {
        ui.set_min_width(220.0);
        widgets::checkbox(ui, &mut bars.on, "Black bars");
        ui.add_enabled_ui(bars.on, |ui| {
            ui.add_space(4.0);
            dense_label(ui, "Shape");
            for (aspect, name) in [
                (0.0, "Frame (rounded edges only)"),
                (1.85, "Cinematic 1.85"),
                (2.39, "Cinematic 2.39"),
            ] {
                widgets::radio_value(ui, &mut bars.aspect, aspect, name);
            }
            ui.add_space(4.0);
            ui.add(egui::Slider::new(&mut bars.radius, 0.0..=200.0).text("Corner radius"));
        });
        dense_label(ui, "Bars are part of the picture: exports draw them too.");
    })
    .response
    .on_hover_text("Black bars over the camera frame");
    s.set_bars(bars);
}

/// Harmony's Art Layer toolbar: preview all, then the art layers back to
/// front as Harmony lists them (Overlay and Underlay only when enabled).
fn art_layer_toolbar(ui: &mut egui::Ui, s: &mut Session, profile: Profile) {
    use crate::session::art_name;
    // Turning the preference off leaves nothing on a hidden art layer.
    if !s.overlay_underlay && matches!(s.art, ArtLayer::Overlay | ArtLayer::Underlay) {
        s.set_art(ArtLayer::LineArt);
    }
    ui.separator();
    let tip = keymap::tooltip(profile, Action::PreviewArtLayers);
    if icons::button(ui, Icon::ArtPreview, s.art_preview, true, &tip).clicked() {
        s.art_preview = !s.art_preview;
    }
    let mut layers = vec![
        (ArtLayer::LineArt, Icon::ArtLine),
        (ArtLayer::ColourArt, Icon::ArtColour),
    ];
    if s.overlay_underlay {
        layers.insert(0, (ArtLayer::Overlay, Icon::ArtOverlay));
        layers.push((ArtLayer::Underlay, Icon::ArtUnderlay));
    }
    for (art, icon) in layers {
        let key = match art {
            ArtLayer::LineArt | ArtLayer::ColourArt => Action::SwitchLineColourArt,
            _ => Action::SwitchOverlayUnderlay,
        };
        let shortcut = keymap::label(profile, key);
        let tip = if shortcut.is_empty() {
            art_name(art).to_owned()
        } else {
            format!("{} ({shortcut} switches)", art_name(art))
        };
        if icons::button(ui, icon, s.art == art, true, &tip).clicked() {
            s.set_art(art);
        }
    }
    ui.add_space(10.0);
}

/// Bottom toolbar (D3.5a): navigation.
fn bottom_toolbar(ui: &mut egui::Ui, state: &mut CanvasState, profile: Profile) {
    egui::Frame::new()
        .inner_margin(egui::Margin::symmetric(theme::CARD_PAD, 4))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                let mut zoom = state.zoom;
                egui::ComboBox::from_id_salt(ui.scope_id().with("zoom"))
                    .width(72.0)
                    .selected_text(format!("{:.0}%", state.zoom * 100.0))
                    .show_ui(ui, |ui| {
                        for z in [0.25, 0.5, 1.0, 2.0, 4.0, 8.0] {
                            ui.selectable_value(&mut zoom, z, format!("{:.0}%", z * 100.0));
                        }
                    })
                    .response
                    // Screen readers announce it as "Zoom".
                    .widget_info(|| egui::WidgetInfo::labeled(egui::Role::ComboBox, true, "Zoom"));
                state.zoom = zoom;
                ui.add_space(8.0);
                for (label, action) in [
                    ("Reset zoom", Action::ResetZoom),
                    ("Reset rotation", Action::ResetRotation),
                    ("Reset pan", Action::ResetPan),
                ] {
                    if ui
                        .small_button(label)
                        .on_hover_text(keymap::tooltip(profile, action))
                        .clicked()
                    {
                        state.apply(action);
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    if icons::button(
                        ui,
                        Icon::Fit,
                        false,
                        true,
                        &keymap::tooltip(profile, Action::ResetView),
                    )
                    .clicked()
                    {
                        state.apply(Action::ResetView);
                    }
                    if icons::button(
                        ui,
                        Icon::RotateRight,
                        false,
                        true,
                        &keymap::tooltip(profile, Action::RotateCw),
                    )
                    .clicked()
                    {
                        state.apply(Action::RotateCw);
                    }
                    if icons::button(
                        ui,
                        Icon::RotateLeft,
                        false,
                        true,
                        &keymap::tooltip(profile, Action::RotateCcw),
                    )
                    .clicked()
                    {
                        state.apply(Action::RotateCcw);
                    }
                    if state.mirrored {
                        dense_label(ui, "Mirrored".to_owned());
                    }
                    if state.rotation != 0.0 {
                        dense_label(ui, format!("{:.0} deg", state.rotation));
                    }
                });
            });
        });
}

pub fn ui(
    ui: &mut egui::Ui,
    s: &mut Session,
    state: &mut CanvasState,
    samples: &[PenSample],
    drawing_only: bool,
    profile: Profile,
) {
    top_toolbar(ui, s, profile, drawing_only, &mut state.render_effects);
    let canvas_size = ui.available_size() - Vec2::new(0.0, theme::TAB_ROW);
    let (response, painter) = ui.allocate_painter(canvas_size, Sense::click_and_drag());
    let rect = response.rect;
    let painter = painter.with_clip_rect(rect);
    state.strokes.begin_frame(ui.ctx());
    state.strokes.vectors.set_version(s.doc.version);
    let (w, h) = (s.doc.settings.width as f32, s.doc.settings.height as f32);

    state.framing = if drawing_only {
        1.0
    } else {
        s.doc.camera_scale(s.frame)
    };

    // Navigation (before drawing, so this frame already shows the result).
    let (space, shift, zoom_delta, scroll, alt) = ui.input(|i| {
        (
            i.key_down(egui::Key::Space),
            i.modifiers.shift,
            i.zoom_delta(),
            i.smooth_scroll_delta,
            i.modifiers.alt,
        )
    });
    let navigating = space || s.tool == Tool::Hand;
    if response.hovered() || response.dragged() {
        if let Some(p) = response.hover_pos() {
            if zoom_delta != 1.0 {
                zoom_at(state, rect, p, zoom_delta);
            }
        }
        if scroll != Vec2::ZERO && zoom_delta == 1.0 {
            state.pan += scroll;
        }
    }
    if response.dragged() && navigating {
        if space && shift {
            // Rotate around the view centre by the angle the pointer swept.
            if let Some(p) = response.interact_pointer_pos() {
                let c = rect.center() + state.pan;
                let (a, b) = (p - response.drag_delta() - c, p - c);
                state.rotate_by((b.y.atan2(b.x) - a.y.atan2(a.x)).to_degrees());
            }
        } else {
            state.pan += response.drag_delta();
        }
    }
    if s.tool == Tool::Zoom && response.clicked() {
        if let Some(p) = response.interact_pointer_pos() {
            zoom_at(state, rect, p, if alt { 0.8 } else { 1.25 });
        }
    }

    if state.fit_pending && rect.width() > 80.0 && rect.height() > 80.0 {
        state.zoom = fit_zoom(rect, w, h);
        state.fit_pending = false;
    }
    let view = view_transform(state, rect);
    state.last_view = view.inverse().unwrap_or(Affine::IDENTITY);
    state.last_rect = Some(rect);
    let px = 1.0 / scale_of(view).max(1e-6);

    let show_surround = drawing_only || s.doc.settings.bars.is_off();
    // D4.2: neutral surround. D3.10: the stage is paper with a soft shadow.
    let colours = s.colours;
    // The view's corners are rounded like its panel; the stage stays sharp.
    painter.rect_filled(
        rect,
        f32::from(theme::GLASS_RADIUS) - 4.0,
        if show_surround {
            CanvasColours::get(colours.surround)
        } else {
            Color32::BLACK
        },
    );
    let (w, h) = (w / state.framing, h / state.framing);
    let corners = [
        (-w / 2.0, -h / 2.0),
        (w / 2.0, -h / 2.0),
        (w / 2.0, h / 2.0),
        (-w / 2.0, h / 2.0),
    ]
    .map(|(x, y)| to_pos(view.apply(x, y)));
    if drawing_only && state.rotation == 0.0 {
        let stage = Rect::from_two_pos(corners[0], corners[2]);
        let shadow = egui::Shadow {
            offset: [0, 10],
            blur: 36,
            spread: 0,
            color: theme::SHADOW,
        };
        painter.add(shadow.as_shape(stage, 0));
    }
    painter.add(egui::Shape::convex_polygon(
        corners.to_vec(),
        CanvasColours::get(colours.stage),
        EStroke::NONE,
    ));

    let colour = |s: &Session, swatch: Id| {
        let [r, g, b, a] = s.doc.swatch_rgba(swatch);
        Color32::from_rgba_unmultiplied(r, g, b, a)
    };

    // Use the same depth projection as the rendered stage for ink and hit tests.
    let projected_layer = if drawing_only {
        Some(Affine::IDENTITY)
    } else {
        s.doc
            .layers
            .get(s.selected)
            .and_then(|layer| s.doc.camera_transform(layer.node, s.frame))
    };
    let layer_world = projected_layer.unwrap_or(Affine::IDENTITY);

    // Onion skin: neighbouring drawings of the selected layer, tinted.
    if s.onion.on && projected_layer.is_some() {
        let (before, after) = s.onion_drawings();
        let tints = [
            CanvasColours::get(colours.onion_before),
            CanvasColours::get(colours.onion_after),
        ];
        for (list, tint) in [(before, tints[0]), (after, tints[1])] {
            for (k, d) in list.iter().enumerate().rev() {
                let alpha = s.onion.opacity.clamp(0.05, 1.0) / (k as f32 + 1.0);
                if let Some(dr) = s.doc.drawings.get(d) {
                    // Paint first, faint, so the tinted drawing reads whole.
                    for f in &dr.fills {
                        state.strokes.vectors.paint_fill(
                            &painter,
                            f,
                            tint.gamma_multiply(alpha * 0.35),
                            view.compose(layer_world),
                        );
                    }
                    for st in &dr.strokes {
                        paint_stroke(
                            &painter,
                            &mut state.strokes,
                            &s.doc.textures,
                            st,
                            tint.gamma_multiply(alpha),
                            view.compose(layer_world),
                        );
                    }
                }
            }
        }
    }

    // Light table (Drawing view): the other layers' drawings at this frame,
    // washed out behind the one being drawn, placed where they sit
    // relative to it.
    if drawing_only && s.light_table {
        let own = projected_layer.and_then(|m| m.inverse());
        let wash = theme::TEXT_SECONDARY.gamma_multiply(0.4);
        for (i, layer) in s.doc.layers.iter().enumerate() {
            if i == s.selected || !s.layer_visible(i) {
                continue;
            }
            let (Some(d), Some(world)) = (
                s.doc.exposed(i, s.frame),
                s.doc.camera_transform(layer.node, s.frame),
            ) else {
                continue;
            };
            let rel = own.map_or(world, |inv| inv.compose(world));
            if let Some(dr) = s.doc.drawings.get(&d) {
                for f in &dr.fills {
                    state.strokes.vectors.paint_fill(
                        &painter,
                        f,
                        wash.gamma_multiply(0.4),
                        view.compose(rel),
                    );
                }
                for st in &dr.strokes {
                    paint_stroke(
                        &painter,
                        &mut state.strokes,
                        &s.doc.textures,
                        st,
                        wash,
                        view.compose(rel),
                    );
                }
            }
        }
    }

    // Collect the graph once for both the draft artwork and the effects check.
    use symphony_core::graph::RenderCommand;
    let camera_commands = if drawing_only {
        None
    } else {
        s.doc
            .graph
            .render_commands(s.doc.display, &s.doc.xsheet, s.frame)
            .ok()
    };
    // Drawings bent by Mesh Warps: (drawing, its warp).
    // (drawing, its warp, projection for both vector and raster artwork)
    let mut warps: Vec<(Id, std::sync::Arc<symphony_core::warp::Warp>, Affine)> = Vec::new();
    let items: Vec<select::Item> = if drawing_only {
        s.doc
            .exposed(s.selected, s.frame)
            .map(|d| vec![(s.selected, d, Affine::IDENTITY)])
            .unwrap_or_default()
    } else {
        camera_commands
            .iter()
            .flatten()
            .filter_map(|command| {
                let RenderCommand::Drawing(item) = command else {
                    return None;
                };
                if item.matte_pass {
                    return None;
                }
                let layer = s.doc.layers.iter().position(|l| l.node == item.node)?;
                let plane = s.doc.project_plane(s.frame, item.depth)?;
                let transform = plane.compose(item.transform);
                if let Some(w) = &item.warp {
                    warps.push((item.drawing, w.clone(), plane));
                }
                Some((layer, item.drawing, transform))
            })
            .collect()
    };
    // Plain artwork uses cached GPU geometry. Effects go through an
    // independent stage-sized raster cache when the preview is enabled.
    let preview = state.select.preview.filter(|_| s.tool == Tool::Select);
    let raw_only = drawing_only
        || !state.render_effects
        || camera_commands.as_ref().is_some_and(|commands| {
            commands
                .iter()
                .all(|c| matches!(c, RenderCommand::Drawing(_)))
        });
    let raw_art = raw_only || show_surround;
    if raw_art {
        state
            .bent
            .retain(|d, _| warps.iter().any(|(w, _, _)| w == d));
        for (_, d, m) in &items {
            let Some(original) = s.doc.drawings.get(d) else {
                continue;
            };
            let world = view.compose(*m);
            if let Some(img) = original.image {
                let image_world = warps
                    .iter()
                    .find(|(w, _, _)| w == d)
                    .and_then(|(_, warp, stage)| {
                        Some(view.compose(*stage).compose(warp.raster_transform()?))
                    })
                    .unwrap_or(world);
                state
                    .strokes
                    .vectors
                    .paint_image(&painter, &s.doc, img, image_world);
            }
            // Under a Mesh Warp: the bent copy, drawn with the warp's
            // outer transform (on the same plane).
            let bent_copy;
            let (dr, world) = match warps.iter().find(|(w, _, _)| w == d) {
                Some((_, warp, stage)) => {
                    bent_copy = bent_drawing(&mut state.bent, s.doc.version, original, warp);
                    (&*bent_copy, view.compose(*stage).compose(warp.outer()))
                }
                None => (original, world),
            };
            for art in ArtLayer::STACK {
                if drawing_only && !s.art_preview && art != s.art {
                    continue;
                }
                for f in dr.fills.iter().filter(|f| f.art == art) {
                    // Selected zones follow the Select tool's live preview.
                    let transform = match preview {
                        Some(l)
                            if s.selection.drawing == Some(*d)
                                && s.selection.strokes.contains(&f.id) =>
                        {
                            world.compose(l)
                        }
                        _ => world,
                    };
                    state
                        .strokes
                        .vectors
                        .paint_fill(&painter, f, colour(s, f.swatch), transform);
                }
                for st in dr
                    .strokes
                    .iter()
                    .filter(|st| st.art == art && st.kind != StrokeKind::Invisible)
                    .filter(|st| {
                        !state
                            .text
                            .as_ref()
                            .and_then(|draft| draft.editing.as_ref())
                            .is_some_and(|object| object.glyphs.contains(&st.id))
                    })
                {
                    let transform = match preview {
                        Some(l)
                            if s.selection.drawing == Some(*d)
                                && s.selection.strokes.contains(&st.id) =>
                        {
                            world.compose(l)
                        }
                        _ => world,
                    };
                    paint_stroke(
                        &painter,
                        &mut state.strokes,
                        &s.doc.textures,
                        st,
                        colour(s, st.swatch),
                        transform,
                    );
                }
            }
        }
    }
    if !raw_only {
        if show_surround {
            // The raw pass is a positioning aid outside the stage. Cover its
            // inside portion before painting the composited stage texture.
            painter.add(egui::Shape::convex_polygon(
                corners.to_vec(),
                CanvasColours::get(colours.stage),
                EStroke::NONE,
            ));
        }
        let hidden_text = state.text.as_ref().and_then(|draft| {
            let drawing = s.doc.exposed(draft.layer, draft.frame)?;
            let object = draft.editing.as_ref()?;
            Some((drawing, object.glyphs.clone()))
        });
        if !state
            .display
            .paint(&painter, s, corners, preview, hidden_text)
        {
            painter.text(
                rect.center(),
                Align2::CENTER_CENTER,
                "Couldn’t render the Display output.",
                theme::body(),
                theme::TEXT_PRIMARY,
            );
        }
    }
    if s.show_strokes {
        for (_, d, m) in &items {
            let Some(dr) = s.doc.drawings.get(d) else {
                continue;
            };
            let world = view.compose(*m);
            for st in dr.strokes.iter().filter(|st| {
                st.kind == StrokeKind::Invisible
                    && (!drawing_only || s.art_preview || st.art == s.art)
            }) {
                let transform = match preview {
                    Some(l)
                        if s.selection.drawing == Some(*d)
                            && s.selection.strokes.contains(&st.id) =>
                    {
                        world.compose(l)
                    }
                    _ => world,
                };
                painter.add(egui::Shape::line(
                    st.points
                        .iter()
                        .map(|p| to_pos(transform.apply(p.x, p.y)))
                        .collect(),
                    EStroke::new(1.0, theme::ACCENT_SELECT),
                ));
            }
        }
    }
    // Line Editor: the picked points, as dots; Esc clears them.
    if s.tool == Tool::LineEditor {
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            s.line_points.clear();
        }
        if let Some(d) = s
            .doc
            .exposed(s.selected, s.frame)
            .and_then(|d| s.doc.drawings.get(&d))
        {
            let m = view.compose(layer_world);
            for (id, i) in &s.line_points {
                let Some(p) = d
                    .strokes
                    .iter()
                    .find(|st| st.id == *id)
                    .and_then(|st| st.points.get(*i))
                else {
                    continue;
                };
                let at = to_pos(m.apply(p.x, p.y));
                painter.circle_filled(at, 4.5, theme::BG_BASE);
                painter.circle_filled(at, 3.0, theme::ACCENT_SELECT);
            }
        }
    }
    state.select.camera_space = !drawing_only;
    let navigating_now = space || s.tool == Tool::Hand;
    let warp_busy = !navigating_now
        && super::warp_edit::ui(ui, &response, &painter, s, &mut state.warp_edit, view);
    let rig_busy = if !navigating_now && !drawing_only {
        super::rig_edit::ui(ui, &response, &painter, s, &mut state.rig_edit, view)
    } else {
        super::rig_edit::finish(s, &mut state.rig_edit);
        false
    };
    if matches!(s.tool, Tool::Select | Tool::Transform)
        && !navigating_now
        && !warp_busy
        && !rig_busy
    {
        select::ui(ui, &response, &painter, s, &mut state.select, view, &items);
    } else {
        state.select.hover = None;
    }
    if let Some(a) = state
        .active
        .as_ref()
        .filter(|a| a.stroke.kind == StrokeKind::Invisible)
    {
        if let Some(back) = a.to_local.inverse() {
            let pts: Vec<Pos2> = a
                .stroke
                .points
                .iter()
                .map(|p| to_pos(view.compose(back).apply(p.x, p.y)))
                .collect();
            painter.add(egui::Shape::line(
                pts,
                EStroke::new(1.5, theme::ACCENT_SELECT),
            ));
        }
    }
    if let Some(a) = state
        .active
        .as_mut()
        .filter(|a| a.stroke.kind != StrokeKind::Invisible)
    {
        if let Some(back) = a.to_local.inverse() {
            let m = view.compose(back);
            let ink = colour(s, a.stroke.swatch);
            match a.stroke.texture {
                // Textured: stamp only the new part each frame.
                Some(tex) => super::textured::Live::paint(
                    &mut a.live,
                    ui.ctx(),
                    &painter,
                    &s.doc.textures,
                    &a.stroke,
                    &tex,
                    ink,
                    m,
                ),
                None => state
                    .strokes
                    .vectors
                    .paint_live(&painter, &a.stroke, ink, m),
            }
        }
    }

    // The scene's black bars, over the artwork, as export draws them.
    if !drawing_only {
        let settings = &s.doc.settings;
        let (sw, sh) = (settings.width as f32, settings.height as f32);
        if let Some(outline) = settings.bars.outline(sw, sh) {
            let k = 1.0 / state.framing;
            painter.add(bars_mesh(&outline, sw * k, sh * k, k, |x, y| {
                to_pos(view.apply(x, y))
            }));
        }
    }

    text_tool(ui, &response, &painter, s, state, view, layer_world, space);

    // Pen input.
    let zoom_factor = ui.ctx().zoom_factor();
    let draws = matches!(
        s.tool,
        Tool::Brush
            | Tool::Line
            | Tool::Pencil
            | Tool::Eraser
            | Tool::Paint
            | Tool::CloseGap
            | Tool::Dropper
            | Tool::LineEditor
    ) && !space
        && projected_layer.is_some();
    let locked = s.doc.column(s.selected).is_some_and(|c| c.locked);
    for sm in samples {
        let pos = Pos2::new(sm.x as f32 / zoom_factor, sm.y as f32 / zoom_factor);
        let (sx, sy) = state.last_view.apply(pos.x, pos.y);
        let erasing = sm.tool == PenTool::Eraser || s.tool == Tool::Eraser;
        match sm.phase {
            Phase::Down
                if draws
                    && !locked
                    && (response.hovered() || response.contains_pointer())
                    && rect.contains(pos) =>
            {
                let Some(to_local) = layer_world.inverse() else {
                    continue;
                };
                let local_px = px / scale_of(layer_world).max(1e-6);
                let (lx, ly) = to_local.apply(sx, sy);
                if !erasing && s.tool == Tool::Paint {
                    s.paint_at(s.selected, s.frame, lx, ly, local_px);
                    continue;
                }
                if !erasing && s.tool == Tool::Dropper {
                    s.pick_colour(s.selected, s.frame, lx, ly, local_px);
                    continue;
                }
                if !erasing && s.tool == Tool::LineEditor {
                    let Some(drawing) = s.doc.exposed(s.selected, s.frame) else {
                        continue;
                    };
                    let mode = if shift {
                        LineMode::Select
                    } else if !s.line_points.is_empty() {
                        LineMode::MoveSelected
                    } else {
                        LineMode::Push
                    };
                    if mode == LineMode::Select {
                        let r =
                            s.line_edit_radius * size_unit(s, px) / scale_of(layer_world).max(1e-6);
                        s.line_select(drawing, [lx, ly], r);
                    }
                    state.line_edit = Some(LineDrag {
                        drawing,
                        to_local,
                        last: [lx, ly],
                        mode,
                        moved: false,
                        local_px,
                    });
                    continue;
                }
                if erasing {
                    let targets = eraser_targets(s, &items, layer_world);
                    if !targets.is_empty() {
                        state.erasing = Some(Erasing {
                            targets,
                            last: None,
                        });
                        let r = eraser_radius(s, sm.pressure) * size_unit(s, px);
                        erase_at(s, state, (sx, sy), r);
                    }
                } else {
                    let settings = if s.tool == Tool::Pencil {
                        s.pencil
                    } else {
                        s.brush
                    };
                    let kind = match s.tool {
                        Tool::Pencil => StrokeKind::Pencil,
                        Tool::CloseGap => StrokeKind::Invisible,
                        _ => StrokeKind::Brush,
                    };
                    // A textured brush brings its tip and grain into the
                    // scene, so the scene keeps working on any machine.
                    if let (Some(tex), StrokeKind::Brush) = (settings.texture, kind) {
                        for k in [tex.tip, tex.grain].into_iter().flatten() {
                            crate::brush_import::ensure_texture(&mut s.doc, k);
                        }
                    }
                    let (x, y) = to_local.apply(sx, sy);
                    let p = Point {
                        x,
                        y,
                        pressure: pressure_curve(sm.pressure, s.pressure_gamma),
                        pen: symphony_core::stroke::Pen::from_tilt(
                            sm.tilt_x,
                            sm.tilt_y,
                            sm.rotation,
                        ),
                    };
                    let mut smoother = Smoother::new(settings.smoothing);
                    let first = smoother.push(p, 0.0).unwrap_or(p);
                    // Sizes are scene units by default, so zooming in makes
                    // the brush look bigger, as on paper. With "Brush size
                    // follows zoom" they are screen points instead.
                    let k = size_unit(s, px) / scale_of(layer_world).max(1e-6);
                    state.active = Some(Active {
                        layer: s.selected,
                        frame: s.frame,
                        to_local,
                        smoother,
                        min_step: 0.5 * local_px,
                        stroke: Stroke {
                            id: s.doc.ids.fresh(),
                            kind,
                            art: s.art,
                            swatch: s.swatch,
                            min_width: settings.min_width * k,
                            max_width: settings.max_width * k,
                            points: vec![first],
                            // Start and end tips are a pencil property.
                            tips: if kind == StrokeKind::Pencil {
                                settings.tips
                            } else {
                                Default::default()
                            },
                            texture: settings.texture.filter(|_| kind == StrokeKind::Brush),
                            shape: Vec::new(),
                        },
                        stencil: settings.stencil.filter(|_| kind == StrokeKind::Pencil),
                        live: None,
                        line: (s.tool == Tool::Line).then_some(LineDraft {
                            start: [first.x, first.y],
                            end: [first.x, first.y],
                            bend: None,
                            pressure: first.pressure,
                        }),
                    });
                }
            }
            Phase::Move => {
                if let Some(a) = state.active.as_mut().filter(|a| a.line.is_some()) {
                    // The Line tool: the pen sets the end; Shift snaps its
                    // angle; Alt bends the line through the pen instead.
                    let (x, y) = a.to_local.apply(sx, sy);
                    if let Some(d) = a.line.as_mut() {
                        if alt {
                            d.bend = Some([x, y]);
                        } else {
                            d.end = if shift {
                                snap_line(d.start, [x, y])
                            } else {
                                [x, y]
                            };
                        }
                        a.stroke.points = line_points(d, a.min_step * 2.0);
                    }
                    // Rebuilt, not extended: stamp the textured preview anew.
                    a.live = None;
                } else if let Some(a) = state.active.as_mut() {
                    let (x, y) = a.to_local.apply(sx, sy);
                    let raw = Point {
                        x,
                        y,
                        pressure: pressure_curve(sm.pressure, s.pressure_gamma),
                        pen: symphony_core::stroke::Pen::from_tilt(
                            sm.tilt_x,
                            sm.tilt_y,
                            sm.rotation,
                        ),
                    };
                    if let Some(p) = a.smoother.push(raw, a.min_step) {
                        a.stroke.points.push(p);
                        // A thickness stencil ignores pressure; show it live.
                        if let Some(st) = &a.stencil {
                            apply_stencil(&mut a.stroke.points, st);
                        }
                    }
                } else if state.erasing.is_some() {
                    let r = eraser_radius(s, sm.pressure) * size_unit(s, px);
                    erase_at(s, state, (sx, sy), r);
                } else if let Some(le) = state.line_edit.as_mut() {
                    let r = s.line_edit_radius * size_unit(s, px) / scale_of(layer_world).max(1e-6);
                    line_step(s, le, (sx, sy), r);
                }
            }
            Phase::Up => {
                if let Some(mut le) = state.line_edit.take() {
                    // The last stretch, up to where the pen lifted.
                    let r = s.line_edit_radius * size_unit(s, px) / scale_of(layer_world).max(1e-6);
                    line_step(s, &mut le, (sx, sy), r);
                    match le.mode {
                        LineMode::Push if le.moved => {
                            // Slim the pushed lines back to the detail setting.
                            let tolerance =
                                (1.0 - s.stroke_detail).clamp(0.0, 0.9) * 3.0 * le.local_px;
                            s.line_edit_finish(le.drawing, tolerance);
                        }
                        // A click without dragging clears the point selection.
                        LineMode::MoveSelected if !le.moved => s.line_points.clear(),
                        LineMode::MoveSelected => s.line_edit_finish(le.drawing, 0.0),
                        _ => s.end_gesture(),
                    }
                }
                if state.active.as_ref().is_some_and(|a| a.line.is_some()) {
                    let Some(mut a) = state.active.take() else {
                        continue;
                    };
                    // Where the pen lifted is the line's end (or bend).
                    let (x, y) = a.to_local.apply(sx, sy);
                    if let Some(d) = a.line.as_mut() {
                        if alt {
                            d.bend = Some([x, y]);
                        } else {
                            d.end = if shift {
                                snap_line(d.start, [x, y])
                            } else {
                                [x, y]
                            };
                        }
                        a.stroke.points = line_points(d, a.min_step * 2.0);
                    }
                    // A line is exactly its draft: no smoothing or thinning.
                    if a.stroke.points.len() >= 2 {
                        if let Some(live) = a.live.take() {
                            state.strokes.adopt(a.stroke.id, live);
                        }
                        s.commit_stroke(a.layer, a.frame, a.stroke);
                    }
                } else if let Some(mut a) = state.active.take() {
                    // Where the pen lifted is part of the line: without it,
                    // every stroke lost its last stretch and drawn shapes
                    // didn't close. Lifting reports ~0 pressure, so keep
                    // the last pressure.
                    let (x, y) = a.to_local.apply(sx, sy);
                    let (pressure, pen) = a
                        .stroke
                        .points
                        .last()
                        .map_or((1.0, Default::default()), |p| (p.pressure, p.pen));
                    if let Some(p) = a.smoother.push(
                        Point {
                            x,
                            y,
                            pressure,
                            pen,
                        },
                        a.min_step,
                    ) {
                        a.stroke.points.push(p);
                    }
                    if let Some(p) = a.smoother.finish() {
                        a.stroke.points.push(p);
                    }
                    if let Some(st) = &a.stencil {
                        apply_stencil(&mut a.stroke.points, st);
                    }
                    // Keep only the points that matter (Stroke detail): the
                    // tolerance is in screen pixels, so it means the same at
                    // any zoom, and the line is drawn as a curve through them.
                    if a.stroke.kind != StrokeKind::Invisible {
                        let tolerance_px = (1.0 - s.stroke_detail).clamp(0.0, 0.9) * 3.0;
                        let local_px = a.min_step * 2.0;
                        let span = a.stroke.max_width - a.stroke.min_width;
                        a.stroke.points = symphony_vector::curve::simplify(
                            &a.stroke.points,
                            tolerance_px * local_px,
                            span,
                        );
                    }
                    if a.stroke.kind == StrokeKind::Invisible {
                        // Close Gap: the rough stroke only says where; it isn't kept.
                        let reach = 40.0 * a.min_step / 0.5;
                        s.close_gap(a.layer, a.frame, &a.stroke.points, reach);
                    } else {
                        // The live image stands in until the committed
                        // stroke is stamped, so it never flickers.
                        if let Some(live) = a.live.take() {
                            state.strokes.adopt(a.stroke.id, live);
                        }
                        s.commit_stroke(a.layer, a.frame, a.stroke);
                    }
                }
                if state.erasing.take().is_some() {
                    s.end_gesture();
                }
            }
            _ => {}
        }
    }

    // D10.4: an empty cell invites drawing. On the surround for contrast (D4.4).
    let hint = if locked {
        Some(format!("{} is locked", s.layer_name(s.selected)))
    } else if s.doc.exposed(s.selected, s.frame).is_none() {
        Some(format!(
            "Draw here to create a drawing on {}",
            s.layer_name(s.selected)
        ))
    } else {
        None
    };
    if let Some(hint) = hint.filter(|_| drawing_only) {
        painter.text(
            rect.left_top() + Vec2::new(10.0, 8.0),
            Align2::LEFT_TOP,
            hint,
            theme::dense(),
            theme::TEXT_PRIMARY,
        );
    }

    let pen_eraser = samples.last().is_some_and(|l| l.tool == PenTool::Eraser);
    draw_cursor(
        ui,
        &response,
        rect,
        s,
        Mods {
            space,
            shift,
            alt,
            pen_eraser,
            locked,
            hint: state.select.hover,
            screen_per_size: size_unit(s, px) / px,
        },
    );
    if !show_surround {
        camera_render::mask_outside_stage(&painter, rect, corners);
    } else if !drawing_only {
        painter.add(egui::Shape::closed_line(
            corners.to_vec(),
            EStroke::new(1.0, theme::LINE_CONTROL),
        ));
    }
    bottom_toolbar(ui, state, profile);
}

fn zoom_at(state: &mut CanvasState, rect: Rect, p: Pos2, factor: f32) {
    let before = view_transform(state, rect)
        .inverse()
        .map(|inv| inv.apply(p.x, p.y));
    state.zoom = (state.zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);
    if let Some((x, y)) = before {
        let after = view_transform(state, rect).apply(x, y);
        state.pan += p - to_pos(after);
    }
}

/// Erase whole strokes under the eraser. (Cutting strokes in two comes
/// with the vector editing tools.)
/// Eraser radius in screen points for this pen pressure.
/// Scene units per unit of tool size: 1 (sizes are scene units), or the
/// scene size of a screen point when sizes follow the zoom.
fn size_unit(s: &Session, px: f32) -> f32 {
    if s.size_follows_zoom {
        px
    } else {
        1.0
    }
}

fn eraser_radius(s: &Session, pressure: f32) -> f32 {
    s.eraser
        .width_at_pressure(pressure_curve(pressure, s.pressure_gamma))
        * 0.5
}

/// Erase from the last stamp to this one: discs along the segment, so a
/// fast drag leaves no gaps.
/// The drawings one eraser drag works on: the selected layer, or every
/// visible layer in the view ("Erase all layers"), and the drawing at this
/// frame, or all of each layer's drawings ("Erase all frames").
fn eraser_targets(s: &Session, items: &[select::Item], layer_world: Affine) -> Vec<(Id, Affine)> {
    let layers: Vec<(usize, Affine)> = if s.erase_all_layers {
        items
            .iter()
            .filter(|(layer, _, _)| s.layer_visible(*layer))
            .map(|(layer, _, world)| (*layer, *world))
            .collect()
    } else {
        vec![(s.selected, layer_world)]
    };
    let mut out: Vec<(Id, Affine)> = Vec::new();
    for (layer, world) in layers {
        let Some(to_local) = world.inverse() else {
            continue;
        };
        let drawings: Vec<Id> = if s.erase_all_frames {
            s.element_drawings(layer).into_iter().map(|d| d.0).collect()
        } else {
            s.doc.exposed(layer, s.frame).into_iter().collect()
        };
        for d in drawings {
            if !out.iter().any(|(o, _)| *o == d) {
                out.push((d, to_local));
            }
        }
    }
    out
}

/// Erase from the last stamp to this one: discs along the segment, so a
/// fast drag leaves no gaps. `radius` is in scene units.
fn erase_at(s: &mut Session, state: &mut CanvasState, scene_pt: (f32, f32), radius: f32) {
    let Some(e) = state.erasing.as_mut() else {
        return;
    };
    let (x, y) = scene_pt;
    let here = ([x, y], radius.max(0.01));
    // In path order, from the last stamp to this one: the eraser shape
    // joins them into one smooth capsule, and pencil cutting checks along it.
    let mut discs = Vec::new();
    if let Some((p, r)) = e.last {
        let len = (x - p[0]).hypot(y - p[1]);
        let step = (radius.min(r) * 0.5).max(0.01);
        let n = ((len / step).ceil() as usize).min(2000);
        for i in 0..n {
            let t = i as f32 / n as f32;
            discs.push((
                [p[0] + (x - p[0]) * t, p[1] + (y - p[1]) * t],
                r + (radius - r) * t,
            ));
        }
    }
    discs.push(here);
    e.last = Some(here);
    let targets = e.targets.clone();
    for (d, to_local) in targets {
        let k = scale_of(to_local);
        let local: Vec<symphony_vector::cut::Disc> = discs
            .iter()
            .map(|(c, r)| {
                let (lx, ly) = to_local.apply(c[0], c[1]);
                ([lx, ly], r * k)
            })
            .collect();
        s.erase(d, &local);
    }
}

#[derive(Clone, Copy)]
struct Mods {
    space: bool,
    shift: bool,
    alt: bool,
    /// The pen's eraser end is in use.
    pen_eraser: bool,
    /// The selected layer is locked.
    locked: bool,
    /// Handle hint from Select/Transform.
    hint: Option<Icon>,
    /// Screen points per unit of tool size, for the size circle.
    screen_per_size: f32,
}

/// Our canvas cursor (D8.11): a crosshair plus the active tool's icon, or a
/// navigation cursor while a shortcut is held. Drawn on the foreground
/// layer, white over a dark outline so it reads on paper and on grey.
/// Draw a brush tip image as the cursor: `r` is half the diameter in
/// screen points. False if the tip isn't available (fall back to a circle).
fn paint_tip_cursor(
    ui: &egui::Ui,
    painter: &egui::Painter,
    at: Pos2,
    r: f32,
    key: u64,
    angle_deg: f32,
    colour: Color32,
) -> bool {
    let id = egui::Id::unique(("tip-cursor", key));
    let handle = ui
        .ctx()
        .data(|d| d.get_temp::<egui::TextureHandle>(id))
        .or_else(|| {
            let t = crate::brush_import::library_texture(key)?;
            let pixels = t
                .pixels
                .iter()
                .map(|&a| Color32::from_white_alpha(a))
                .collect();
            let image = egui::ColorImage {
                size: [t.width as usize, t.height as usize],
                source_size: Vec2::new(t.width as f32, t.height as f32),
                pixels,
            };
            let h = ui
                .ctx()
                .load_texture("tip-cursor", image, egui::TextureOptions::LINEAR);
            ui.ctx().data_mut(|d| d.insert_temp(id, h.clone()));
            Some(h)
        });
    let Some(handle) = handle else {
        return false;
    };
    // The tip's longer side spans the diameter, as when stamping.
    let [w, h] = handle.size();
    let side = w.max(h).max(1) as f32;
    let (hw, hh) = (r * w as f32 / side, r * h as f32 / side);
    let (sin, cos) = (-angle_deg.to_radians()).sin_cos();
    let corner = |x: f32, y: f32| at + Vec2::new(x * cos - y * sin, x * sin + y * cos);
    let mut mesh = egui::Mesh::with_texture(handle.id());
    for (x, y, u, v) in [
        (-hw, -hh, 0.0, 0.0),
        (hw, -hh, 1.0, 0.0),
        (hw, hh, 1.0, 1.0),
        (-hw, hh, 0.0, 1.0),
    ] {
        mesh.vertices.push(egui::epaint::Vertex {
            pos: corner(x, y),
            uv: Pos2::new(u, v),
            color: colour.gamma_multiply(0.55),
        });
    }
    mesh.indices = vec![0, 1, 2, 0, 2, 3];
    painter.add(egui::Shape::mesh(mesh));
    true
}

fn draw_cursor(ui: &egui::Ui, response: &egui::Response, rect: Rect, s: &Session, m: Mods) {
    let (pen_eraser, locked, hint) = (m.pen_eraser, m.locked, m.hint);
    let Some(p) = ui
        .input(|i| i.pointer.latest_pos())
        .filter(|p| rect.contains(*p))
    else {
        return;
    };
    if !(response.hovered() || response.dragged() || response.contains_pointer()) {
        return;
    }
    ui.ctx().set_cursor_icon(egui::CursorIcon::None);
    let painter = ui
        .ctx()
        .layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::unique("symphony-canvas-cursor"),
        ))
        .with_clip_rect(rect);
    let dark = theme::BG_BASE;
    let light = theme::ON_ACCENT;

    // A navigation cursor replaces the crosshair: the icon itself is the pointer.
    let nav = if m.space && m.shift {
        Some(Icon::RotateRight)
    } else if m.space || s.tool == Tool::Hand {
        Some(Icon::Hand)
    } else if s.tool == Tool::Zoom {
        Some(if m.alt { Icon::ZoomOut } else { Icon::Zoom })
    } else {
        None
    };
    let badge = |icon: Icon, centre: Pos2, size: f32| {
        let r = Rect::from_center_size(centre, Vec2::splat(size + 6.0));
        painter.rect_filled(r, theme::RADIUS_CONTROL as f32, dark.gamma_multiply(0.85));
        icons::paint(
            &painter,
            Rect::from_center_size(centre, Vec2::splat(size)),
            icon,
            light,
        );
    };
    if let Some(icon) = nav {
        badge(icon, p, 18.0);
        return;
    }
    // Over a Select/Transform handle: show what dragging it will do.
    if let Some(icon) = hint.filter(|_| matches!(s.tool, Tool::Select | Tool::Transform)) {
        badge(icon, p, 16.0);
        return;
    }

    let draws = matches!(s.tool, Tool::Brush | Tool::Pencil | Tool::Eraser) || pen_eraser;
    if locked && draws {
        badge(Icon::Forbidden, p, 18.0);
        return;
    }

    // Size circle: always for the eraser, and for brush tools when the
    // Brush Size Cursor preference is on (as in Harmony).
    let erasing = pen_eraser || s.tool == Tool::Eraser;
    let radius = if erasing {
        Some((s.eraser.max_width * 0.5 * m.screen_per_size).max(1.5))
    } else if s.tool == Tool::LineEditor {
        Some((s.line_edit_radius * m.screen_per_size).max(1.5))
    } else if s.brush_size_cursor && matches!(s.tool, Tool::Brush | Tool::Pencil) {
        let b = if s.tool == Tool::Pencil {
            s.pencil
        } else {
            s.brush
        };
        Some((b.max_width * 0.5 * m.screen_per_size).max(1.5))
    } else {
        None
    };
    // A textured brush with a tip image shows the tip itself: its shape,
    // size and angle, faintly, so you see what a stamp will look like.
    let tip = match (erasing, s.tool) {
        (false, Tool::Brush) => s.brush.texture.and_then(|t| t.tip.map(|k| (k, t.angle))),
        _ => None,
    };
    // A soft shadow under a white ring keeps it visible on any surface.
    let ring_with_shadow = |r: f32, colour: Color32| {
        for (w, a, dy) in [(4.0, 0.18, 1.0), (2.5, 0.35, 0.5)] {
            painter.circle_stroke(
                p + Vec2::new(0.0, dy),
                r,
                EStroke::new(w, dark.gamma_multiply(a)),
            );
        }
        painter.circle_stroke(p, r, EStroke::new(1.25, colour));
    };
    match (radius, tip) {
        (Some(r), Some((key, angle))) => {
            if !paint_tip_cursor(ui, &painter, p, r, key, angle, light) {
                ring_with_shadow(r, light);
            }
        }
        (Some(r), None) => {
            ring_with_shadow(r, if erasing { theme::STATE_DANGER } else { light });
        }
        _ => {}
    }
    if radius.is_some() {
        // The ring is the cursor (no crosshair); a tiny dot marks the centre
        // so even a very small brush shows where it lands.
        painter.circle_filled(p, 2.0, dark.gamma_multiply(0.5));
        painter.circle_filled(p, 1.0, light);
    } else {
        // Tools without a size ring keep a crosshair for aiming.
        let (gap, arm) = (3.0, 9.0);
        for (a, b) in [
            (Vec2::new(-arm, 0.0), Vec2::new(-gap, 0.0)),
            (Vec2::new(gap, 0.0), Vec2::new(arm, 0.0)),
            (Vec2::new(0.0, -arm), Vec2::new(0.0, -gap)),
            (Vec2::new(0.0, gap), Vec2::new(0.0, arm)),
        ] {
            painter.line_segment([p + a, p + b], EStroke::new(3.0, dark));
            painter.line_segment([p + a, p + b], EStroke::new(1.0, light));
        }
    }

    // Tool icon beside the crosshair, so the active tool is always clear.
    let icon = if erasing {
        Icon::Eraser
    } else {
        match s.tool {
            Tool::Select => Icon::Select,
            Tool::Transform => Icon::Transform,
            Tool::Pencil => Icon::Pencil,
            Tool::Paint => Icon::Paint,
            Tool::CloseGap => Icon::CloseGap,
            Tool::Dropper => Icon::Dropper,
            Tool::LineEditor => Icon::LineEdit,
            Tool::Text => Icon::Text,
            _ => Icon::Brush,
        }
    };
    badge(icon, p + Vec2::new(16.0, 16.0), 12.0);
}

#[cfg(test)]
mod zoom_key_tests {
    use super::*;

    /// The 1 and 2 keys zoom on the mouse: the scene point under it stays.
    #[test]
    fn zoom_keys_keep_the_point_under_the_mouse() {
        let mut st = CanvasState::default();
        let rect = Rect::from_min_size(Pos2::ZERO, egui::vec2(800.0, 600.0));
        st.last_rect = Some(rect);
        let mouse = Pos2::new(620.0, 140.0);
        let under = |st: &CanvasState| {
            let inv = view_transform(st, rect).inverse().unwrap();
            inv.apply(mouse.x, mouse.y)
        };
        let before = under(&st);
        st.apply_at(Action::ZoomIn, Some(mouse));
        st.apply_at(Action::ZoomIn, Some(mouse));
        let after = under(&st);
        assert!((st.zoom - 1.5625).abs() < 1e-4, "{}", st.zoom);
        assert!((before.0 - after.0).abs() < 0.01 && (before.1 - after.1).abs() < 0.01);
        // Outside the canvas: the centre stays put instead.
        st.apply_at(Action::ZoomOut, Some(Pos2::new(2000.0, 2000.0)));
        assert!((st.zoom - 1.25).abs() < 1e-4);
    }
}
