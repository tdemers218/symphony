//! Symphony's own small controls, where egui's defaults break the design
//! rules. Unchecked checkboxes and unselected radios were invisible on our
//! surfaces, so these draw a `line.control` edge (3:1 or more, D4.4).

use egui::{self, Pos2, Rect, Response, Sense, Stroke, Vec2};

use crate::theme;

const BOX: f32 = 14.0;

/// The commit action in a dialog or workflow.
pub fn primary_button(ui: &mut egui::Ui, label: &str, enabled: bool) -> Response {
    let enabled = enabled && ui.is_enabled();
    let colour = if enabled {
        theme::ON_ACCENT
    } else {
        theme::TEXT_DISABLED
    };
    let button = egui::Button::new(egui::RichText::new(label).color(colour))
        .min_size(Vec2::new(0.0, theme::ROW))
        .fill(if enabled {
            theme::ACCENT_SELECT_FILL
        } else {
            theme::BG_RAISED
        });
    let response = ui.add_enabled(enabled, button);
    theme::focus_ring(ui, &response);
    response
}

pub const FRAME_RATES: [f32; 4] = [12.0, 24.0, 30.0, 48.0];

/// Common film and video rates. An imported scene's custom rate stays
/// visible until the artist explicitly selects a preset.
pub fn frame_rate(ui: &mut egui::Ui, id: &str, fps: &mut f32) {
    egui::ComboBox::from_id_salt(id)
        .selected_text(format!("{fps} fps"))
        .show_ui(ui, |ui| {
            for rate in FRAME_RATES {
                ui.selectable_value(fps, rate, format!("{rate:.0} fps"));
            }
        });
}

fn row(ui: &mut egui::Ui, label: &str) -> (Response, Rect) {
    let colour = if ui.is_enabled() {
        theme::TEXT_PRIMARY
    } else {
        theme::TEXT_DISABLED
    };
    let font = theme::body();
    let galley = ui.painter().layout_no_wrap(label.to_owned(), font, colour);
    let size = Vec2::new(
        BOX + 14.0 + galley.size().x,
        BOX.max(galley.size().y).max(theme::TOOLBAR_BUTTON),
    );
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let mark = Rect::from_center_size(
        Pos2::new(rect.left() + 3.0 + BOX / 2.0, rect.center().y),
        Vec2::splat(BOX),
    );
    let text_pos = Pos2::new(mark.right() + 8.0, rect.center().y - galley.size().y / 2.0);
    ui.painter().galley(text_pos, galley, colour);
    theme::focus_ring(ui, &response);
    (response, mark)
}

/// A checkbox: cel-blue fill with a white check when on, a visible edge when off.
pub fn checkbox(ui: &mut egui::Ui, value: &mut bool, label: &str) -> Response {
    let (mut response, mark) = row(ui, label);
    if response.clicked() {
        *value = !*value;
        response.mark_changed();
    }
    let p = ui.painter();
    if *value {
        p.rect_filled(
            mark,
            theme::RADIUS_CONTROL as f32,
            if response.enabled() {
                theme::ACCENT_SELECT_FILL
            } else {
                theme::BG_RAISED
            },
        );
        let pts = [
            Pos2::new(mark.left() + 3.5, mark.center().y),
            Pos2::new(mark.left() + 6.0, mark.bottom() - 3.5),
            Pos2::new(mark.right() - 3.0, mark.top() + 3.5),
        ];
        p.add(egui::Shape::line(
            pts.to_vec(),
            Stroke::new(
                1.8,
                if response.enabled() {
                    theme::ON_ACCENT
                } else {
                    theme::TEXT_DISABLED
                },
            ),
        ));
    } else {
        p.rect_filled(mark, theme::RADIUS_CONTROL as f32, theme::BG_INPUT);
        let edge = if !response.enabled() {
            theme::TEXT_DISABLED
        } else if response.hovered() {
            theme::TEXT_SECONDARY
        } else {
            theme::LINE_CONTROL
        };
        p.rect_stroke(
            mark,
            theme::RADIUS_CONTROL as f32,
            Stroke::new(1.0, edge),
            egui::StrokeKind::Inside,
        );
    }
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::Role::CheckBox, response.enabled(), *value, label)
    });
    response
}

/// One option of a choice (a radio button). Returns the response; the
/// caller sets the value when it's clicked.
pub fn choice(ui: &mut egui::Ui, selected: bool, label: &str) -> Response {
    let (response, mark) = row(ui, label);
    let p = ui.painter();
    let r = BOX / 2.0;
    if selected {
        let (fill, dot) = if response.enabled() {
            (theme::ACCENT_SELECT_FILL, theme::ON_ACCENT)
        } else {
            (theme::BG_RAISED, theme::TEXT_DISABLED)
        };
        p.circle_filled(mark.center(), r, fill);
        p.circle_filled(mark.center(), r * 0.4, dot);
    } else {
        p.circle_filled(mark.center(), r, theme::BG_INPUT);
        let edge = if !response.enabled() {
            theme::TEXT_DISABLED
        } else if response.hovered() {
            theme::TEXT_SECONDARY
        } else {
            theme::LINE_CONTROL
        };
        p.circle_stroke(mark.center(), r - 0.5, Stroke::new(1.0, edge));
    }
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::Role::RadioButton, response.enabled(), selected, label)
    });
    response
}

/// `choice` bound to a value.
pub fn radio_value<T: PartialEq + Copy>(
    ui: &mut egui::Ui,
    current: &mut T,
    value: T,
    label: &str,
) -> Response {
    let mut r = choice(ui, *current == value, label);
    if r.clicked() && *current != value {
        *current = value;
        r.mark_changed();
    }
    r
}

/// Paint text cut to `max_width` with an ellipsis (DESIGN D5.5). `pos` is
/// placed by `align`, as in `Painter::text`. Returns the drawn rectangle
/// and whether the text was cut.
#[allow(clippy::too_many_arguments)]
pub fn fit_text(
    ui: &egui::Ui,
    painter: &egui::Painter,
    pos: Pos2,
    align: egui::Align2,
    text: &str,
    font: egui::FontId,
    colour: egui::Color32,
    max_width: f32,
) -> (Rect, bool) {
    let mut job = egui::text::LayoutJob::single_section(
        text.to_owned(),
        egui::TextFormat::simple(font, colour),
    );
    job.wrap = egui::text::TextWrapping::truncate_at_width(max_width.max(8.0));
    let galley = ui.fonts_mut(|f| f.layout_job(job));
    let rect = align.anchor_size(pos, galley.size());
    let elided = galley.elided;
    painter.galley(rect.min, galley, colour);
    (rect, elided)
}

/// [`fit_text`] plus the full text on hover when it was cut (D5.5). Use
/// this unless the surrounding widget's own tooltip already names it.
#[allow(clippy::too_many_arguments)]
pub fn fitted_text(
    ui: &egui::Ui,
    painter: &egui::Painter,
    pos: Pos2,
    align: egui::Align2,
    text: &str,
    font: egui::FontId,
    colour: egui::Color32,
    max_width: f32,
) -> Rect {
    let (rect, elided) = fit_text(ui, painter, pos, align, text, font, colour, max_width);
    if elided && ui.rect_contains_pointer(rect) {
        let rested = ui.input(|i| i.pointer.time_since_last_movement())
            >= ui.style().interaction.tooltip_delay;
        if rested {
            egui::Tooltip::always_open(
                ui.ctx().clone(),
                ui.layer_id(),
                ui.scope_id().with(("fitted-text", text)),
                egui::PopupAnchor::Pointer,
            )
            .show(|ui| ui.label(text));
        } else {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_secs_f32(
                    ui.style().interaction.tooltip_delay,
                ));
        }
    }
    rect
}
