//! Swatch editor with a hue slider and two saturation/value layouts.
use eframe::egui::{self, Color32, Pos2, Sense, Stroke};
use egui::ecolor::Hsva;
use symphony_ui::theme;

/// A swatch chip: a rounded square. Whatever frames it (a row highlight,
/// the selection ring) takes `theme::around(radius, inset)`, so the curves
/// share a centre (DESIGN D6.3).
pub const CHIP: f32 = 20.0;
pub const CHIP_RADIUS: u8 = theme::RADIUS_CONTROL;

pub fn swatch(ui: &mut egui::Ui, rgba: &mut [u8; 4]) -> egui::Response {
    picker(ui, rgba, true)
}

pub fn rgb(ui: &mut egui::Ui, rgb: &mut [u8; 3]) -> egui::Response {
    let [r, g, b] = *rgb;
    let mut rgba = [r, g, b, 255];
    let response = picker(ui, &mut rgba, false);
    *rgb = [rgba[0], rgba[1], rgba[2]];
    response
}

/// The checkerboard a see-through colour is shown over, one 2×2 texture
/// repeated, so its cells follow the chip's rounded corners.
fn checker(ctx: &egui::Context) -> egui::TextureId {
    let id = egui::Id::unique("swatch-checker");
    if let Some(t) = ctx.data(|d| d.get_temp::<egui::TextureHandle>(id)) {
        return t.id();
    }
    let (light, dark) = (Color32::from_gray(200), Color32::from_gray(120));
    let image = egui::ColorImage::new([2, 2], vec![light, dark, dark, light]);
    let texture = ctx.load_texture(
        "swatch-checker",
        image,
        egui::TextureOptions::NEAREST_REPEAT,
    );
    let tid = texture.id();
    ctx.data_mut(|d| d.insert_temp(id, texture));
    tid
}

/// Paint a swatch: the colour at its own opacity, over a checkerboard when
/// it's see-through, with a thin rim so very dark or very light colours
/// never vanish into the panel.
pub fn paint_chip(ui: &egui::Ui, rect: egui::Rect, rgba: [u8; 4], radius: u8) {
    let [r, g, b, a] = rgba;
    let painter = ui.painter();
    if a < 255 {
        const CELL: f32 = 4.0;
        let uv = egui::Rect::from_min_size(Pos2::ZERO, rect.size() / (2.0 * CELL));
        painter.add(
            egui::epaint::RectShape::filled(rect, radius, Color32::WHITE)
                .with_texture(checker(ui.ctx()), uv),
        );
    }
    painter.rect_filled(rect, radius, Color32::from_rgba_unmultiplied(r, g, b, a));
    painter.rect_stroke(
        rect,
        radius,
        Stroke::new(1.0, Color32::from_white_alpha(46)),
        egui::StrokeKind::Inside,
    );
}

/// The ring around the selected swatch, `gap` outside its edge.
pub fn paint_ring(ui: &egui::Ui, rect: egui::Rect, radius: u8, gap: u8) {
    ui.painter().rect_stroke(
        rect.expand(f32::from(gap)),
        theme::around(radius, gap),
        Stroke::new(1.5, ui.visuals().selection.stroke.color),
        egui::StrokeKind::Outside,
    );
}

pub fn opacity_tip(a: u8) -> String {
    format!("{}% opacity", (u16::from(a) * 100 + 127) / 255)
}

fn picker(ui: &mut egui::Ui, rgba: &mut [u8; 4], edit_opacity: bool) -> egui::Response {
    let (rect, chip) = ui.allocate_exact_size(egui::vec2(CHIP, CHIP), Sense::click());
    paint_chip(ui, rect, *rgba, CHIP_RADIUS);
    let mut chip = chip.on_hover_text(format!("Edit colour · {}", opacity_tip(rgba[3])));
    let toggle = chip.clicked().then_some(egui::SetOpenCommand::Toggle);
    editor(&mut chip, rgba, edit_opacity, toggle);
    chip
}

/// The editor's layouts. Wheel (a hue ring around a saturation/brightness
/// square, as in Clip Studio, Krita and SAI) is the default; Square is the
/// Photoshop layout (a big square with a hue bar); Perceptual picks in
/// OKLCH.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Layout {
    Wheel,
    Square,
    Perceptual,
}

impl Layout {
    fn from_u8(v: u8) -> Self {
        match v {
            1 => Layout::Square,
            2 => Layout::Perceptual,
            _ => Layout::Wheel,
        }
    }
}

/// The remembered layout (kept between sessions).
pub fn layout_id() -> egui::Id {
    egui::Id::unique("colour-editor-layout-v2")
}

/// The colour editor popup under `chip`; `open` opens, closes or toggles
/// it (`None` leaves it as it is). Marks `chip` changed when the colour
/// changes.
pub fn editor(
    chip: &mut egui::Response,
    rgba: &mut [u8; 4],
    edit_opacity: bool,
    open: Option<egui::SetOpenCommand>,
) {
    let id = chip.id;
    let close = if dropper::active(&chip.ctx) {
        // Picking from the screen: that click mustn't close the editor.
        egui::PopupCloseBehavior::IgnoreClicks
    } else {
        egui::PopupCloseBehavior::CloseOnClickOutside
    };
    let shown = egui::Popup::menu(chip)
        .open_memory(open)
        .close_behavior(close)
        .show(|ui| {
            if body(ui, id, rgba, edit_opacity) {
                chip.mark_changed();
            }
        });
    if shown.is_none() {
        closed(&chip.ctx, id, *rgba);
    }
}

/// The editor's contents, shared by swatch popups and the quick picker
/// (Q). Returns true when the colour changed.
pub fn body(ui: &mut egui::Ui, id: egui::Id, rgba: &mut [u8; 4], edit_opacity: bool) -> bool {
    ui.set_width(PICKER);
    let layout_id = layout_id();
    let mut layout =
        Layout::from_u8(ui.data_mut(|d| *d.get_persisted_mut_or_default::<u8>(layout_id)));
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        for (value, name, tip) in [
            (
                Layout::Wheel,
                "Wheel",
                "A hue ring around a saturation and brightness square",
            ),
            (
                Layout::Square,
                "Square",
                "A large saturation and brightness square with a hue bar",
            ),
            (
                Layout::Perceptual,
                "Perceptual",
                "Lightness and chroma in OKLCH: equal steps look equal, and making a colour \
                 darker keeps its hue",
            ),
        ] {
            ui.selectable_value(
                &mut layout,
                value,
                egui::RichText::new(name).font(theme::dense()),
            )
            .on_hover_text(tip);
        }
    });
    ui.data_mut(|d| d.insert_persisted(layout_id, layout as u8));
    // The colour when the editor opened, to compare and go back to.
    let before = ui.data_mut(|d| *d.get_temp_mut_or_insert_with(id.with("before"), || *rgba));
    let mut next = *rgba;
    if let Some([r, g, b]) = dropper::take(ui.ctx(), id) {
        next = [r, g, b, next[3]];
    }
    match layout {
        Layout::Wheel => {
            let mut hsv = hsv_of(ui, id, next);
            if wheel(ui, id, &mut hsv) {
                set_hsv(ui, id, &mut next, hsv);
            }
        }
        Layout::Square => {
            let mut hsv = hsv_of(ui, id, next);
            if square_and_bar(ui, id, &mut hsv) {
                set_hsv(ui, id, &mut next, hsv);
            }
        }
        Layout::Perceptual => perceptual(ui, id, &mut next),
    }
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        if new_and_current(ui, next, before) {
            next = before;
        }
        hex_field(ui, id, &mut next);
        if symphony_ui::icons::button(
            ui,
            symphony_ui::icons::Icon::Dropper,
            dropper::waiting_for(ui.ctx()) == Some(id),
            true,
            "Pick a colour from anywhere in the window",
        )
        .clicked()
        {
            dropper::start(ui.ctx(), id);
        }
    });
    if layout != Layout::Perceptual {
        channel_sliders(ui, id, &mut next);
    }
    if edit_opacity {
        let mut opacity = f32::from(next[3]) / 255.0 * 100.0;
        if symphony_ui::widgets::precise_slider(
            ui,
            egui::Slider::new(&mut opacity, 0.0..=100.0)
                .step_by(1.0)
                .fixed_decimals(0)
                .text("Opacity")
                .suffix("%"),
        )
        .changed()
        {
            next[3] = (opacity / 100.0 * 255.0).round() as u8;
        }
    }
    if let Some([r, g, b]) = recent(ui) {
        next = [r, g, b, next[3]];
    }
    ui.separator();
    ui.horizontal(|ui| {
        if ui
            .button("Colour Lab…")
            .on_hover_text(
                "Light this colour on a sphere or cube to find its shadows and highlights",
            )
            .clicked()
        {
            let [r, g, b, _] = next;
            ui.ctx()
                .data_mut(|d| d.insert_temp(super::colour_lab::open_request(), [r, g, b]));
            ui.close();
        }
    });
    egui::CollapsingHeader::new("Preview with a blend mode")
        .id_salt(id.with("blend-preview"))
        .default_open(false)
        .show(ui, |ui| composite_preview(ui, next));
    if next != *rgba {
        *rgba = next;
        true
    } else {
        false
    }
}

/// The editor closed: forget where it started, and remember the colour
/// among the recent ones if it changed.
fn closed(ctx: &egui::Context, id: egui::Id, now: [u8; 4]) {
    let before = ctx.data_mut(|d| d.remove_temp::<[u8; 4]>(id.with("before")));
    if let Some(before) = before.filter(|b| b[..3] != now[..3]) {
        // Both, so the colour you left is one click away; the new one first.
        remember(ctx, [before[0], before[1], before[2]]);
        remember(ctx, [now[0], now[1], now[2]]);
    }
}

/// The width of the editor's contents.
const PICKER: f32 = 216.0;
/// How many recent colours the editor keeps.
const RECENT: usize = 12;

fn recent_id() -> egui::Id {
    egui::Id::unique("colour-recent")
}

fn remember(ctx: &egui::Context, rgb: [u8; 3]) {
    ctx.data_mut(|d| {
        let list = d.get_persisted_mut_or_default::<Vec<[u8; 3]>>(recent_id());
        list.retain(|c| *c != rgb);
        list.insert(0, rgb);
        list.truncate(RECENT);
    });
}

/// The recent colours as a row of chips; returns the one clicked.
fn recent(ui: &mut egui::Ui) -> Option<[u8; 3]> {
    let list = ui.data_mut(|d| d.get_persisted::<Vec<[u8; 3]>>(recent_id()))?;
    if list.is_empty() {
        return None;
    }
    let mut picked = None;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(3.0, 3.0);
        for c in list {
            let (rect, response) = ui.allocate_exact_size(egui::vec2(15.0, 15.0), Sense::click());
            paint_chip(ui, rect, [c[0], c[1], c[2], 255], CHIP_RADIUS / 2);
            if response.on_hover_text("A recent colour").clicked() {
                picked = Some(c);
            }
        }
    });
    picked
}

/// HSV for `rgba`, keeping the hue (and saturation) the artist set when
/// the colour is grey or black and so has none of its own.
fn hsv_of(ui: &egui::Ui, id: egui::Id, rgba: [u8; 4]) -> Hsva {
    let kept = ui
        .data(|d| d.get_temp::<[f32; 3]>(id.with("hsv")))
        .filter(|[h, s, v]| Hsva::new(*h, *s, *v, 1.0).to_srgba_unmultiplied()[..3] == rgba[..3]);
    match kept {
        Some([h, s, v]) => Hsva::new(h, s, v, f32::from(rgba[3]) / 255.0),
        None => Hsva::from_srgba_unmultiplied(rgba),
    }
}

fn set_hsv(ui: &egui::Ui, id: egui::Id, rgba: &mut [u8; 4], hsv: Hsva) {
    ui.data_mut(|d| d.insert_temp(id.with("hsv"), [hsv.h, hsv.s, hsv.v]));
    let [r, g, b, _] = Hsva::new(hsv.h, hsv.s, hsv.v, 1.0).to_srgba_unmultiplied();
    *rgba = [r, g, b, rgba[3]];
}

/// The saturation (right) by brightness (up) square at a hue, as a mesh.
fn paint_sv(painter: &egui::Painter, rect: egui::Rect, hue: f32) {
    const ROWS: u32 = 24;
    let mut mesh = egui::Mesh::default();
    for row in 0..=ROWS {
        let y = row as f32 / ROWS as f32;
        for x in [0.0, 1.0] {
            mesh.colored_vertex(
                Pos2::new(
                    rect.left() + x * rect.width(),
                    rect.top() + y * rect.height(),
                ),
                Color32::from(Hsva::new(hue, x, 1.0 - y, 1.0)),
            );
        }
        if row > 0 {
            let i = 2 * row;
            mesh.add_triangle(i - 2, i - 1, i);
            mesh.add_triangle(i, i - 1, i + 1);
        }
    }
    painter.add(egui::Shape::mesh(mesh));
}

fn marker(painter: &egui::Painter, at: Pos2, radius: f32) {
    painter.circle_stroke(at, radius, Stroke::new(3.0, Color32::BLACK));
    painter.circle_stroke(at, radius, Stroke::new(1.4, Color32::WHITE));
}

/// Drag inside the saturation/brightness square.
fn pick_sv(rect: egui::Rect, pos: Pos2, hsv: &mut Hsva) {
    hsv.s = ((pos.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
    hsv.v = 1.0 - ((pos.y - rect.top()) / rect.height()).clamp(0.0, 1.0);
}

/// A hue ring around a saturation/brightness square. A drag that starts on
/// the ring turns the hue; one that starts in the square picks inside it.
fn wheel(ui: &mut egui::Ui, id: egui::Id, hsv: &mut Hsva) -> bool {
    let size = PICKER;
    let (rect, response) = ui.allocate_exact_size(egui::vec2(size, size), Sense::click_and_drag());
    let centre = rect.center();
    let outer = size / 2.0 - 2.0;
    let inner = outer - 18.0;
    let half = (inner - 6.0) / std::f32::consts::SQRT_2;
    let square = egui::Rect::from_center_size(centre, egui::vec2(half * 2.0, half * 2.0));
    let painter = ui.painter_at(rect);
    // Ring, red at the top going clockwise.
    let mut mesh = egui::Mesh::default();
    const SEGMENTS: u32 = 96;
    for i in 0..=SEGMENTS {
        let hue = i as f32 / SEGMENTS as f32;
        let (sin, cos) = (hue * std::f32::consts::TAU).sin_cos();
        let dir = egui::vec2(sin, -cos);
        let colour = Color32::from(Hsva::new(hue, 1.0, 1.0, 1.0));
        mesh.colored_vertex(centre + dir * inner, colour);
        mesh.colored_vertex(centre + dir * outer, colour);
        if i > 0 {
            let k = 2 * i;
            mesh.add_triangle(k - 2, k - 1, k);
            mesh.add_triangle(k, k - 1, k + 1);
        }
    }
    painter.add(egui::Shape::mesh(mesh));
    paint_sv(&painter, square, hsv.h);

    let target = id.with("wheel-target");
    let mut changed = false;
    if let Some(pos) = response.interact_pointer_pos() {
        if response.drag_started() || response.clicked() || response.is_pointer_button_down_on() {
            // Which part the press began on decides what the drag edits.
            let on_ring = ui.data_mut(|d| {
                *d.get_temp_mut_or_insert_with(target, || (pos - centre).length() > inner - 2.0)
            });
            if on_ring {
                let v = pos - centre;
                hsv.h = (v.x.atan2(-v.y) / std::f32::consts::TAU).rem_euclid(1.0);
            } else {
                pick_sv(square, pos, hsv);
            }
            changed = true;
        }
    }
    if !response.is_pointer_button_down_on() {
        ui.data_mut(|d| d.remove::<bool>(target));
    }
    let (sin, cos) = (hsv.h * std::f32::consts::TAU).sin_cos();
    marker(
        &painter,
        centre + egui::vec2(sin, -cos) * (inner + outer) / 2.0,
        6.0,
    );
    marker(
        &painter,
        Pos2::new(
            square.left() + hsv.s * square.width(),
            square.top() + (1.0 - hsv.v) * square.height(),
        ),
        5.0,
    );
    changed
}

/// The Photoshop layout: a large square and a vertical hue bar.
fn square_and_bar(ui: &mut egui::Ui, id: egui::Id, hsv: &mut Hsva) -> bool {
    let bar_width = 18.0;
    let gap = 8.0;
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(PICKER, 170.0), Sense::click_and_drag());
    let square = egui::Rect::from_min_max(
        rect.min,
        Pos2::new(rect.right() - bar_width - gap, rect.bottom()),
    );
    let bar = egui::Rect::from_min_max(Pos2::new(rect.right() - bar_width, rect.top()), rect.max);
    let painter = ui.painter_at(rect);
    paint_sv(&painter, square, hsv.h);
    let mut mesh = egui::Mesh::default();
    const STEPS: u32 = 36;
    for i in 0..=STEPS {
        let hue = i as f32 / STEPS as f32;
        let y = bar.top() + hue * bar.height();
        let colour = Color32::from(Hsva::new(hue, 1.0, 1.0, 1.0));
        mesh.colored_vertex(Pos2::new(bar.left(), y), colour);
        mesh.colored_vertex(Pos2::new(bar.right(), y), colour);
        if i > 0 {
            let k = 2 * i;
            mesh.add_triangle(k - 2, k - 1, k);
            mesh.add_triangle(k, k - 1, k + 1);
        }
    }
    painter.add(egui::Shape::mesh(mesh));
    let target = id.with("square-target");
    let mut changed = false;
    if let Some(pos) = response.interact_pointer_pos() {
        if response.clicked() || response.is_pointer_button_down_on() {
            let on_bar = ui.data_mut(|d| {
                *d.get_temp_mut_or_insert_with(target, || pos.x > square.right() + gap / 2.0)
            });
            if on_bar {
                hsv.h = ((pos.y - bar.top()) / bar.height()).clamp(0.0, 0.9999);
            } else {
                pick_sv(square, pos, hsv);
            }
            changed = true;
        }
    }
    if !response.is_pointer_button_down_on() {
        ui.data_mut(|d| d.remove::<bool>(target));
    }
    let y = bar.top() + hsv.h * bar.height();
    painter.rect_stroke(
        egui::Rect::from_center_size(
            Pos2::new(bar.center().x, y),
            egui::vec2(bar_width + 4.0, 5.0),
        ),
        2.0,
        Stroke::new(1.4, Color32::WHITE),
        egui::StrokeKind::Middle,
    );
    marker(
        &painter,
        Pos2::new(
            square.left() + hsv.s * square.width(),
            square.top() + (1.0 - hsv.v) * square.height(),
        ),
        5.0,
    );
    changed
}

/// One chip split in two: the new colour on the left, the colour the
/// editor opened with on the right (click it to go back). Returns true
/// when the old half is clicked.
fn new_and_current(ui: &mut egui::Ui, now: [u8; 4], before: [u8; 4]) -> bool {
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(CHIP * 2.6, CHIP + 4.0), Sense::click());
    let radius = CHIP_RADIUS;
    paint_chip(ui, rect, before, radius);
    let left = egui::Rect::from_min_max(rect.min, Pos2::new(rect.center().x, rect.bottom()));
    let mut clipped = ui.new_child(egui::UiBuilder::new().max_rect(rect));
    clipped.set_clip_rect(left.intersect(ui.clip_rect()));
    paint_chip(&clipped, rect, now, radius);
    let back = response.clicked()
        && response
            .interact_pointer_pos()
            .is_some_and(|p| p.x > rect.center().x);
    response.on_hover_text(
        "New · current: click the right half to go back to the colour you started with",
    );
    back
}

fn parse_hex(text: &str) -> Option<[u8; 3]> {
    let t = text.trim().trim_start_matches('#');
    let digits: Vec<u8> = t
        .chars()
        .map(|c| c.to_digit(16).map(|d| d as u8))
        .collect::<Option<_>>()?;
    match digits.as_slice() {
        [r, g, b] => Some([r * 17, g * 17, b * 17]),
        [r1, r2, g1, g2, b1, b2] => Some([r1 * 16 + r2, g1 * 16 + g2, b1 * 16 + b2]),
        _ => None,
    }
}

/// The colour as hex, to read, type or paste.
fn hex_field(ui: &mut egui::Ui, id: egui::Id, rgba: &mut [u8; 4]) {
    let [r, g, b, _] = *rgba;
    let draft_id = id.with("hex");
    let mut text = ui
        .data(|d| d.get_temp::<String>(draft_id))
        .unwrap_or_else(|| format!("#{r:02X}{g:02X}{b:02X}"));
    let edit = ui.add(
        egui::TextEdit::singleline(&mut text)
            .font(egui::TextStyle::Monospace)
            .desired_width(70.0)
            .char_limit(7),
    );
    if edit.has_focus() {
        ui.data_mut(|d| d.insert_temp(draft_id, text.clone()));
        if let Some([r, g, b]) = parse_hex(&text) {
            *rgba = [r, g, b, rgba[3]];
        }
    } else {
        ui.data_mut(|d| d.remove::<String>(draft_id));
    }
    edit.on_hover_text("Hex colour: type or paste, like #4C3A31 or 4c3");
}

/// HSB or RGB sliders, with exact values.
fn channel_sliders(ui: &mut egui::Ui, id: egui::Id, rgba: &mut [u8; 4]) {
    let mode_id = egui::Id::unique("colour-editor-rgb");
    let mut rgb_mode = ui.data_mut(|d| *d.get_persisted_mut_or_default::<bool>(mode_id));
    symphony_ui::widgets::segmented(
        ui,
        "colour-channels",
        &mut rgb_mode,
        &[(false, "HSB"), (true, "RGB")],
        false,
    );
    ui.data_mut(|d| d.insert_persisted(mode_id, rgb_mode));
    ui.spacing_mut().slider_width = PICKER - 92.0;
    let slider = |ui: &mut egui::Ui, value: &mut f32, max: f32, name: &str, suffix: &str| {
        symphony_ui::widgets::precise_slider(
            ui,
            egui::Slider::new(value, 0.0..=max)
                .step_by(1.0)
                .fixed_decimals(0)
                .suffix(suffix)
                .text(name),
        )
        .changed()
    };
    if rgb_mode {
        let mut c = rgba.map(f32::from);
        let mut changed = false;
        for (i, name) in ["R", "G", "B"].into_iter().enumerate() {
            changed |= slider(ui, &mut c[i], 255.0, name, "");
        }
        if changed {
            *rgba = [c[0] as u8, c[1] as u8, c[2] as u8, rgba[3]];
        }
    } else {
        let hsv = hsv_of(ui, id, *rgba);
        let (mut h, mut sat, mut v) = (hsv.h * 360.0, hsv.s * 100.0, hsv.v * 100.0);
        let changed = slider(ui, &mut h, 360.0, "H", "°")
            | slider(ui, &mut sat, 100.0, "S", "%")
            | slider(ui, &mut v, 100.0, "B", "%");
        if changed {
            let hsv = Hsva::new((h / 360.0).rem_euclid(1.0), sat / 100.0, v / 100.0, 1.0);
            set_hsv(ui, id, rgba, hsv);
        }
    }
}

/// The chroma axis of the Perceptual plane. sRGB's most chromatic colour
/// (blue-magenta) sits just under 0.33.
const MAX_CHROMA: f32 = 0.33;

/// Lightness (up) by chroma (right) at one OKLCH hue, then a hue strip and
/// exact values. Only colours sRGB can show are drawn, so the plane's
/// right edge is the gamut edge at that hue.
fn perceptual(ui: &mut egui::Ui, id: egui::Id, rgba: &mut [u8; 4]) {
    use symphony_core::colour::Oklch;
    let [r, g, b, a] = *rgba;
    // Greys have no hue and 8-bit rounding loses a little: keep what the
    // artist set while the colour they see is still the one it makes.
    let lch_id = id.with("oklch");
    let mut lch = ui
        .data(|d| d.get_temp::<Oklch>(lch_id))
        .filter(|kept| kept.to_srgb() == [r, g, b])
        .unwrap_or_else(|| Oklch::from_srgb([r, g, b]));
    let mut changed = false;

    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(PICKER, 150.0), Sense::click_and_drag());
    let at = |l: f32, c: f32| {
        Pos2::new(
            rect.left() + c / MAX_CHROMA * rect.width(),
            rect.top() + (1.0 - l) * rect.height(),
        )
    };
    // Each row runs from grey to the gamut edge at its lightness, so the
    // mesh is exactly the in-gamut shape.
    const ROWS: u32 = 32;
    const COLUMNS: u32 = 12;
    let mut mesh = egui::Mesh::default();
    for row in 0..=ROWS {
        let l = 1.0 - row as f32 / ROWS as f32;
        let edge = Oklch::max_chroma(l, lch.h);
        for column in 0..=COLUMNS {
            let c = edge * column as f32 / COLUMNS as f32;
            let [r, g, b] = Oklch { l, c, h: lch.h }.to_srgb();
            mesh.colored_vertex(at(l, c), Color32::from_rgb(r, g, b));
        }
        if row > 0 {
            let width = COLUMNS + 1;
            for column in 0..COLUMNS {
                let (top, bottom) = ((row - 1) * width + column, row * width + column);
                mesh.add_triangle(top, top + 1, bottom);
                mesh.add_triangle(bottom, top + 1, bottom + 1);
            }
        }
    }
    ui.painter().add(egui::Shape::mesh(mesh));
    if response.clicked() || response.dragged() {
        if let Some(pos) = response.interact_pointer_pos() {
            let l = 1.0 - ((pos.y - rect.top()) / rect.height()).clamp(0.0, 1.0);
            let c = ((pos.x - rect.left()) / rect.width()).clamp(0.0, 1.0) * MAX_CHROMA;
            lch.l = l;
            // Past the edge picks the edge.
            lch.c = c.min(Oklch::max_chroma(l, lch.h));
            changed = true;
        }
    }
    let marker = at(lch.l, lch.c.min(MAX_CHROMA));
    ui.painter()
        .circle_stroke(marker, 5.0, Stroke::new(3.0, Color32::BLACK));
    ui.painter()
        .circle_stroke(marker, 5.0, Stroke::new(1.4, Color32::WHITE));

    // Hue strip, at a lightness and chroma every hue can show.
    let (strip, response) =
        ui.allocate_exact_size(egui::vec2(200.0, 14.0), Sense::click_and_drag());
    let mut mesh = egui::Mesh::default();
    const STEPS: u32 = 36;
    for step in 0..=STEPS {
        let h = step as f32 * 360.0 / STEPS as f32;
        let [r, g, b] = Oklch {
            l: 0.72,
            c: 0.12,
            h,
        }
        .to_srgb();
        let x = strip.left() + strip.width() * step as f32 / STEPS as f32;
        let colour = Color32::from_rgb(r, g, b);
        mesh.colored_vertex(Pos2::new(x, strip.top()), colour);
        mesh.colored_vertex(Pos2::new(x, strip.bottom()), colour);
        if step > 0 {
            let i = 2 * step;
            mesh.add_triangle(i - 2, i - 1, i);
            mesh.add_triangle(i, i - 1, i + 1);
        }
    }
    ui.painter().add(egui::Shape::mesh(mesh));
    if response.clicked() || response.dragged() {
        if let Some(pos) = response.interact_pointer_pos() {
            lch.h = ((pos.x - strip.left()) / strip.width()).clamp(0.0, 1.0) * 360.0;
            changed = true;
        }
    }
    let x = strip.left() + strip.width() * lch.h / 360.0;
    ui.painter().rect_stroke(
        egui::Rect::from_center_size(Pos2::new(x, strip.center().y), egui::vec2(5.0, 18.0)),
        2.0,
        Stroke::new(1.4, Color32::WHITE),
        egui::StrokeKind::Middle,
    );

    let mut lightness = lch.l * 100.0;
    let mut chroma = lch.c;
    let mut hue = lch.h;
    let sliders = [
        symphony_ui::widgets::precise_slider(
            ui,
            egui::Slider::new(&mut lightness, 0.0..=100.0)
                .step_by(0.5)
                .text("Lightness")
                .suffix("%"),
        ),
        symphony_ui::widgets::precise_slider(
            ui,
            egui::Slider::new(&mut chroma, 0.0..=MAX_CHROMA)
                .step_by(0.001)
                .text("Chroma"),
        ),
        symphony_ui::widgets::precise_slider(
            ui,
            egui::Slider::new(&mut hue, 0.0..=360.0)
                .step_by(1.0)
                .text("Hue")
                .suffix("°"),
        ),
    ];
    if sliders.iter().any(egui::Response::changed) {
        lch = Oklch {
            l: lightness / 100.0,
            c: chroma,
            h: hue,
        };
        changed = true;
    }
    if changed {
        let [r, g, b] = lch.to_srgb();
        *rgba = [r, g, b, a];
        // Kept as set (a chroma past the edge comes back when the hue or
        // lightness allows it).
        ui.data_mut(|d| d.insert_temp(lch_id, lch));
    }
    if !lch.in_gamut() {
        ui.weak("Past what screens can show: using the nearest colour.");
    }
}

/// What the colour looks like when painted: over a backdrop, with a blend
/// mode, at its opacity. Uses the renderer's own blending
/// (`symphony_core::colour`), so the preview is what renders.
fn composite_preview(ui: &mut egui::Ui, rgba: [u8; 4]) {
    use symphony_core::graph::BlendMode;
    let state = egui::Id::unique("colour-editor-preview");
    let (mut backdrop, mut mode) = ui
        .data(|d| d.get_temp::<([u8; 3], BlendMode)>(state))
        .unwrap_or(([255; 3], BlendMode::Normal));
    ui.horizontal(|ui| {
        // The result: the paint, inset in its backdrop with concentric
        // corners.
        const INSET: u8 = 4;
        let outer = CHIP + 2.0 * f32::from(INSET);
        let (rect, _) = ui.allocate_exact_size(egui::vec2(outer, outer), Sense::hover());
        let [r, g, b, a] = rgba;
        let result =
            symphony_core::colour::over_opaque(mode, backdrop, [r, g, b], f32::from(a) / 255.0);
        let [br, bg, bb] = backdrop;
        paint_chip(
            ui,
            rect,
            [br, bg, bb, 255],
            theme::around(CHIP_RADIUS, INSET),
        );
        let [rr, rg, rb] = result;
        paint_chip(
            ui,
            rect.shrink(f32::from(INSET)),
            [rr, rg, rb, 255],
            CHIP_RADIUS,
        );
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                ui.weak("Over");
                for (colour, name) in [
                    ([255; 3], "White paper"),
                    ([128; 3], "Mid grey"),
                    ([24; 3], "Near black"),
                ] {
                    let (rect, response) =
                        ui.allocate_exact_size(egui::vec2(14.0, 14.0), Sense::click());
                    let [r, g, b] = colour;
                    paint_chip(ui, rect, [r, g, b, 255], CHIP_RADIUS / 2);
                    if backdrop == colour {
                        paint_ring(ui, rect, CHIP_RADIUS / 2, 2);
                    }
                    if response.on_hover_text(name).clicked() {
                        backdrop = colour;
                    }
                }
            });
            ui.menu_button(format!("Blend: {}", mode.title()), |ui| {
                for m in BlendMode::ALL {
                    ui.selectable_value(&mut mode, m, m.title());
                }
            })
            .response
            .on_hover_text("Preview the colour as a layer with this blend mode would paint it");
        });
    });
    ui.data_mut(|d| d.insert_temp(state, (backdrop, mode)));
}

/// The quick colour picker: the editor for the current colour, summoned
/// at the pointer with a shortcut (Q in the Symphony profile) and gone
/// again with the same key, Escape or a click outside.
#[derive(Default)]
pub struct Quick {
    at: Option<Pos2>,
    /// Its size last frame, to keep it on screen.
    size: Option<egui::Vec2>,
}

impl Quick {
    pub fn toggle(&mut self, ctx: &egui::Context) {
        if self.at.is_some() {
            self.close(ctx);
        } else {
            self.at = Some(
                ctx.pointer_hover_pos()
                    .unwrap_or_else(|| ctx.content_rect().center()),
            );
        }
    }

    pub fn is_open(&self) -> bool {
        self.at.is_some()
    }

    fn close(&mut self, ctx: &egui::Context) {
        self.at = None;
        dropper::cancel(ctx);
    }

    pub fn ui(&mut self, ctx: &egui::Context, s: &mut crate::session::Session) {
        let Some(at) = self.at else { return };
        let id = egui::Id::unique("quick-colour-picker");
        let picking = dropper::active(ctx);
        if !picking && ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.close(ctx);
            closed(ctx, id, s.doc.swatch_rgba(s.swatch));
            return;
        }
        // A sheet over the workspace takes the click that closes the picker,
        // so it doesn't also draw on the canvas.
        let screen = ctx.content_rect();
        if !picking {
            let dismissed = egui::Area::new(id.with("sheet"))
                .order(egui::Order::Foreground)
                .fixed_pos(screen.min)
                .show(ctx, |ui| {
                    ui.allocate_response(screen.size(), Sense::click_and_drag())
                        .is_pointer_button_down_on()
                })
                .inner;
            if dismissed {
                self.close(ctx);
                closed(ctx, id, s.doc.swatch_rgba(s.swatch));
                return;
            }
        }
        let name = s
            .doc
            .palette
            .get(s.swatch)
            .map_or_else(String::new, |sw| sw.name.clone());
        let mut rgba = s.doc.swatch_rgba(s.swatch);
        // Open centred on the pointer, kept on screen.
        let size = self.size.unwrap_or(egui::vec2(PICKER + 16.0, 560.0));
        // Clamped by hand: on the first frames the window can be smaller
        // than the picker (or have no size yet), where f32::clamp panics.
        let fit = |want: f32, low: f32, high: f32| want.min(high).max(low);
        let pos = Pos2::new(
            fit(
                at.x - size.x / 2.0,
                screen.left() + 8.0,
                screen.right() - size.x - 8.0,
            ),
            fit(
                at.y - 120.0,
                screen.top() + 8.0,
                screen.bottom() - size.y - 8.0,
            ),
        );
        let area = egui::Area::new(id)
            .order(egui::Order::Foreground)
            .fixed_pos(pos)
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.strong(&name);
                        ui.weak("· current colour");
                    })
                    .response
                    .on_hover_text("Changes here recolour everything painted with this swatch");
                    body(ui, id, &mut rgba, true)
                })
            });
        ctx.move_to_top(area.response.layer_id);
        self.size = Some(area.response.rect.size());
        if area.inner.inner {
            s.set_swatch(s.swatch, rgba);
        }
    }
}

/// Picking a colour from anywhere in the window: a click asks for a
/// screenshot, and the pixel under the click becomes the editor's colour.
pub mod dropper {
    use std::sync::Arc;

    use eframe::egui;

    fn waiting_id() -> egui::Id {
        egui::Id::unique("colour-dropper-waiting")
    }
    fn click_id() -> egui::Id {
        egui::Id::unique("colour-dropper-click")
    }
    fn result_id(editor: egui::Id) -> egui::Id {
        editor.with("dropped")
    }

    pub fn start(ctx: &egui::Context, editor: egui::Id) {
        ctx.data_mut(|d| d.insert_temp(waiting_id(), editor));
    }

    pub fn cancel(ctx: &egui::Context) {
        ctx.data_mut(|d| {
            d.remove::<egui::Id>(waiting_id());
            d.remove::<egui::Pos2>(click_id());
        });
    }

    pub fn waiting_for(ctx: &egui::Context) -> Option<egui::Id> {
        ctx.data(|d| d.get_temp::<egui::Id>(waiting_id()))
    }

    pub fn active(ctx: &egui::Context) -> bool {
        waiting_for(ctx).is_some()
    }

    /// The colour picked for `editor`, once.
    pub fn take(ctx: &egui::Context, editor: egui::Id) -> Option<[u8; 3]> {
        ctx.data_mut(|d| d.remove_temp::<[u8; 3]>(result_id(editor)))
    }

    /// Every frame: while waiting, cover the window to catch the click
    /// (so it doesn't draw), then read the screenshot it asked for.
    pub fn update(ctx: &egui::Context) {
        let Some(editor) = waiting_for(ctx) else {
            return;
        };
        if ctx.data(|d| d.get_temp::<egui::Pos2>(click_id())).is_some() {
            // Clicked; the screenshot's callback finishes the pick.
            return;
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            cancel(ctx);
            return;
        }
        let screen = ctx.content_rect();
        egui::Area::new(egui::Id::unique("colour-dropper-cover"))
            .order(egui::Order::Tooltip)
            .fixed_pos(screen.min)
            .show(ctx, |ui| {
                let response = ui.allocate_response(screen.size(), egui::Sense::click());
                ctx.set_cursor_icon(egui::CursorIcon::Crosshair);
                if let Some(pos) = response.hover_pos() {
                    // Off to the side, so it isn't what gets picked.
                    ui.painter().text(
                        pos + egui::vec2(18.0, 18.0),
                        egui::Align2::LEFT_TOP,
                        "Click to pick a colour · Esc cancels",
                        egui::FontId::proportional(13.0),
                        egui::Color32::WHITE,
                    );
                }
                if response.clicked() {
                    if let Some(pos) = response.interact_pointer_pos() {
                        ctx.data_mut(|d| d.insert_temp(click_id(), pos));
                        let ppp = ctx.pixels_per_point();
                        let later = ctx.clone();
                        ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(
                            egui::ScreenshotCallback::new(move |image: Arc<egui::ColorImage>| {
                                let [w, h] = image.size;
                                let x = ((pos.x * ppp) as usize).min(w.saturating_sub(1));
                                let y = ((pos.y * ppp) as usize).min(h.saturating_sub(1));
                                if let Some(c) = image.pixels.get(y * w + x) {
                                    let rgb = [c.r(), c.g(), c.b()];
                                    later.data_mut(|d| d.insert_temp(result_id(editor), rgb));
                                }
                                cancel(&later);
                                later.request_repaint();
                            }),
                        ));
                    }
                }
            });
    }
}

#[cfg(test)]
mod tests {
    use super::parse_hex;

    #[test]
    fn hex_accepts_the_usual_spellings() {
        assert_eq!(parse_hex("#4C3A31"), Some([0x4c, 0x3a, 0x31]));
        assert_eq!(parse_hex("4c3a31"), Some([0x4c, 0x3a, 0x31]));
        assert_eq!(parse_hex(" #fff "), Some([255, 255, 255]));
        assert_eq!(parse_hex("#12345"), None);
        assert_eq!(parse_hex("#zzzzzz"), None);
    }
}
