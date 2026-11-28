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

/// The colour editor popup under `chip`; `open` opens, closes or toggles
/// it (`None` leaves it as it is). Marks `chip` changed when the colour
/// changes.
pub fn editor(
    chip: &mut egui::Response,
    rgba: &mut [u8; 4],
    edit_opacity: bool,
    open: Option<egui::SetOpenCommand>,
) {
    egui::Popup::menu(chip)
        .open_memory(open)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| {
            let mode_id = egui::Id::unique("swatch-picker-triangle");
            let mut triangle = ui.data(|d| d.get_temp::<bool>(mode_id)).unwrap_or(false);
            ui.horizontal(|ui| {
                ui.selectable_value(&mut triangle, false, "Square");
                ui.selectable_value(&mut triangle, true, "Triangle");
            });
            ui.data_mut(|d| d.insert_temp(mode_id, triangle));
            let hue_id = chip.id.with("hue");
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
            let mut opacity = hsv.a * 100.0;
            let alpha_changed = edit_opacity
                && symphony_ui::widgets::precise_slider(
                    ui,
                    egui::Slider::new(&mut opacity, 0.0..=100.0)
                        .step_by(1.0)
                        .text("Opacity")
                        .suffix("%"),
                )
                .changed();
            if hue_changed || picked || alpha_changed {
                hsv.a = opacity / 100.0;
                let changed = hsv.to_srgba_unmultiplied();
                if changed != *rgba {
                    *rgba = changed;
                    chip.mark_changed();
                }
            }
        });
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
