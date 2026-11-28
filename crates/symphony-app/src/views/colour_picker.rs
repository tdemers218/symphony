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

/// The editor's layouts. Square and Triangle pick saturation and value at
/// a hue (HSV); Perceptual picks chroma and lightness at a hue in OKLCH,
/// where equal steps look equal and darkening keeps the hue.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Layout {
    Square,
    Triangle,
    Perceptual,
}

impl Layout {
    fn from_u8(v: u8) -> Self {
        match v {
            1 => Layout::Triangle,
            2 => Layout::Perceptual,
            _ => Layout::Square,
        }
    }
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
    let before_id = chip.id.with("before");
    let shown = egui::Popup::menu(chip)
        .open_memory(open)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| {
            // The layout is the artist's preference, kept between sessions.
            let layout_id = egui::Id::unique("colour-editor-layout");
            let mut layout =
                Layout::from_u8(ui.data_mut(|d| *d.get_persisted_mut_or_default::<u8>(layout_id)));
            ui.horizontal(|ui| {
                ui.selectable_value(&mut layout, Layout::Square, "Square");
                ui.selectable_value(&mut layout, Layout::Triangle, "Triangle");
                ui.selectable_value(&mut layout, Layout::Perceptual, "Perceptual")
                    .on_hover_text(
                        "Lightness and chroma in OKLCH: equal steps look equal, and \
                         making a colour darker keeps its hue",
                    );
            });
            ui.data_mut(|d| d.insert_persisted(layout_id, layout as u8));
            // The colour when the editor opened, to compare and go back to.
            let before = ui.data_mut(|d| *d.get_temp_mut_or_insert_with(before_id, || *rgba));
            let mut next = *rgba;
            if before_and_now(ui, before, *rgba) {
                next = before;
            }
            match layout {
                Layout::Perceptual => {
                    perceptual(ui, chip.id, &mut next);
                }
                Layout::Square | Layout::Triangle => {
                    hsv_layout(ui, chip.id, &mut next, layout == Layout::Triangle);
                }
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
            ui.separator();
            composite_preview(ui, next);
            if next != *rgba {
                *rgba = next;
                chip.mark_changed();
            }
        });
    if shown.is_none() {
        chip.ctx.data_mut(|d| d.remove::<[u8; 4]>(before_id));
    }
}

/// The colour the editor opened with beside the current one; clicking the
/// first goes back to it. Returns true when clicked.
fn before_and_now(ui: &mut egui::Ui, before: [u8; 4], now: [u8; 4]) -> bool {
    let mut back = false;
    ui.horizontal(|ui| {
        let (rect, response) = ui.allocate_exact_size(egui::vec2(CHIP * 2.0, CHIP), Sense::click());
        paint_chip(ui, rect, before, CHIP_RADIUS);
        back = response
            .on_hover_text("The colour before: click to go back to it")
            .clicked();
        let (rect, _) = ui.allocate_exact_size(egui::vec2(CHIP * 2.0, CHIP), Sense::hover());
        paint_chip(ui, rect, now, CHIP_RADIUS);
        ui.weak("Before · Now");
    });
    back
}

fn hsv_layout(ui: &mut egui::Ui, id: egui::Id, rgba: &mut [u8; 4], triangle: bool) {
    let hue_id = id.with("hue");
    let mut hsv = Hsva::from_srgba_unmultiplied(*rgba);
    if hsv.s == 0.0 || hsv.v == 0.0 {
        hsv.h = ui.data(|d| d.get_temp::<f32>(hue_id)).unwrap_or(hsv.h);
    }
    let mut hue = hsv.h * 360.0;
    let hue_changed = symphony_ui::widgets::precise_slider(
        ui,
        egui::Slider::new(&mut hue, 0.0..=360.0)
            .step_by(1.0)
            .text("Hue")
            .suffix("°"),
    )
    .changed();
    hsv.h = hue / 360.0;
    ui.data_mut(|d| d.insert_temp(hue_id, hsv.h));
    let picked = saturation_value(ui, &mut hsv, triangle);
    if hue_changed || picked {
        let [r, g, b, _] = hsv.to_srgba_unmultiplied();
        *rgba = [r, g, b, rgba[3]];
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
        ui.allocate_exact_size(egui::vec2(200.0, 130.0), Sense::click_and_drag());
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

fn saturation_value(ui: &mut egui::Ui, hsv: &mut Hsva, triangle: bool) -> bool {
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(200.0, 130.0), Sense::click_and_drag());
    let point = |x: f32, y: f32| {
        Pos2::new(
            rect.left() + x * rect.width(),
            rect.top() + y * rect.height(),
        )
    };
    let pure = Color32::from(Hsva::new(hsv.h, 1.0, 1.0, 1.0));
    let mut mesh = egui::Mesh::default();
    if triangle {
        // Barycentric weights: hue and white form the top edge, black the tip.
        mesh.colored_vertex(point(0.0, 0.0), Color32::WHITE);
        mesh.colored_vertex(point(1.0, 0.0), pure);
        mesh.colored_vertex(point(0.5, 1.0), Color32::BLACK);
        mesh.add_triangle(0, 1, 2);
    } else {
        const ROWS: u32 = 24;
        for row in 0..=ROWS {
            let y = row as f32 / ROWS as f32;
            for x in [0.0, 1.0] {
                mesh.colored_vertex(
                    point(x, y),
                    Color32::from(Hsva::new(hsv.h, x, 1.0 - y, 1.0)),
                );
            }
            if row > 0 {
                let i = 2 * row;
                mesh.add_triangle(i - 2, i - 1, i);
                mesh.add_triangle(i, i - 1, i + 1);
            }
        }
    }
    ui.painter().add(egui::Shape::mesh(mesh));
    let mut changed = false;
    if response.clicked() || response.dragged() {
        if let Some(pos) = response.interact_pointer_pos() {
            let y = ((pos.y - rect.top()) / rect.height()).clamp(0.0, 1.0);
            let x = ((pos.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
            hsv.v = 1.0 - y;
            hsv.s = if triangle {
                if hsv.v > 0.0 {
                    ((x - y * 0.5) / hsv.v).clamp(0.0, 1.0)
                } else {
                    hsv.s
                }
            } else {
                x
            };
            changed = true;
        }
    }
    let x = if triangle {
        hsv.s * hsv.v + (1.0 - hsv.v) / 2.0
    } else {
        hsv.s
    };
    let marker = point(x, 1.0 - hsv.v);
    ui.painter()
        .circle_stroke(marker, 5.0, Stroke::new(3.0, Color32::BLACK));
    ui.painter()
        .circle_stroke(marker, 5.0, Stroke::new(1.4, Color32::WHITE));
    changed
}
