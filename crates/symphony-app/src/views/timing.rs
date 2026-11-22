//! Timeline and Xsheet: two views over the same exposure data.
//! Click a cell to go to that frame and select that layer (D8.9).
//! Double-click a layer name to rename it; right-click for layer actions.
//! The amber playhead and frame tag are Symphony's signature (D3.9).

use eframe::egui::{self, Align2, CornerRadius, Pos2, Rect, Sense, Stroke, Vec2};
use symphony_ui::icons::{self, Icon};
use symphony_ui::{theme, widgets};

use crate::keymap::{self, Action, Profile};
use crate::session::Session;

#[derive(Default)]
pub struct TimingState {
    renaming: Option<(usize, String)>,
    menu_row: Option<usize>,
    /// Parameters column: drawing name being typed, per row.
    swap_text: Option<(usize, String)>,
    /// Parameters column drag: row, starting drawing index, pixels dragged.
    swap_drag: Option<(usize, usize, f32)>,
}

/// Width of the Timeline's Parameters field (the exposed drawing's name).
const PARAM_W: f32 = 48.0;
/// Pixels of drag per drawing when scrubbing the Parameters field.
const PARAM_STEP: f32 = 12.0;

/// View header: transport, timing fields and layer buttons (D3.2).
pub fn header(ui: &mut egui::Ui, s: &mut Session, profile: Profile) {
    egui::Frame::new()
        .inner_margin(egui::Margin::symmetric(theme::CARD_PAD, 4))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                if icons::button(ui, Icon::FirstFrame, false, true, "First frame").clicked() {
                    s.frame = 1;
                }
                if icons::button(
                    ui,
                    Icon::PrevFrame,
                    false,
                    true,
                    &keymap::tooltip(profile, Action::PrevFrame),
                )
                .clicked()
                {
                    s.frame = s.frame.saturating_sub(1).max(1);
                }
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
                if icons::button(
                    ui,
                    Icon::NextFrame,
                    false,
                    true,
                    &keymap::tooltip(profile, Action::NextFrame),
                )
                .clicked()
                {
                    s.frame = (s.frame + 1).min(s.length());
                }
                if icons::button(ui, Icon::LastFrame, false, true, "Last frame").clicked() {
                    s.frame = s.length();
                }
                ui.add_space(16.0);
                ui.spacing_mut().item_spacing.x = 8.0;
                let len = s.length();
                // In a narrow card (like the Hand-Drawn Xsheet) keep only the
                // frame field, so nothing runs past the card edge.
                let compact = ui.available_width() < 330.0;
                ui.add(
                    egui::DragValue::new(&mut s.frame)
                        .range(1..=len)
                        .prefix("Frame "),
                );
                if compact {
                    return;
                }
                let mut length = len;
                let r = ui.add(
                    egui::DragValue::new(&mut length)
                        .range(1..=100_000)
                        .prefix("Length "),
                );
                if r.changed() {
                    s.set_length(length);
                    s.frame = s.frame.min(length);
                }
                if r.drag_stopped() || r.lost_focus() {
                    s.end_gesture();
                }
                let mut fps = s.doc.settings.fps;
                widgets::frame_rate(ui, "timeline-fps", &mut fps);
                s.set_frame_rate(fps);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    if icons::button(ui, Icon::Trash, false, s.layer_count() > 1, "Delete layer")
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
            });
        });
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
        s.selected = row;
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

pub fn timeline(ui: &mut egui::Ui, s: &mut Session, t: &mut TimingState, profile: Profile) {
    header(ui, s, profile);
    const HEAD: f32 = 176.0;
    const RULER: f32 = 28.0;
    let cw = theme::FRAME_CELL;
    let rh = theme::ROW;
    let rows = s.layer_count();
    let len = s.length();
    let width = HEAD + len as f32 * cw + 16.0;

    egui::ScrollArea::both()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let size = Vec2::new(
                width.max(ui.available_width()),
                (RULER + rows as f32 * rh + 8.0).max(ui.available_height()),
            );
            let (resp, painter) = ui.allocate_painter(size, Sense::click_and_drag());
            let o = resp.rect.min;
            let frame_x = |f: u32| o.x + HEAD + (f - 1) as f32 * cw;
            let rows_bottom = o.y + RULER + rows as f32 * rh;

            for f in (5..=len).step_by(5) {
                let x = frame_x(f) + cw;
                painter.line_segment(
                    [Pos2::new(x, o.y + RULER), Pos2::new(x, rows_bottom)],
                    Stroke::new(1.0, theme::LINE_SUBTLE),
                );
            }
            for f in 1..=len {
                let x = frame_x(f) + cw / 2.0;
                let major = f % 5 == 0 || f == 1;
                let top = if major { 16.0 } else { 21.0 };
                painter.line_segment(
                    [Pos2::new(x, o.y + top), Pos2::new(x, o.y + RULER - 2.0)],
                    Stroke::new(1.0, theme::LINE_STRONG),
                );
                if major {
                    painter.text(
                        Pos2::new(x, o.y + 4.0),
                        Align2::CENTER_TOP,
                        f.to_string(),
                        theme::numbers(),
                        theme::TEXT_SECONDARY,
                    );
                }
            }

            for i in 0..rows {
                let y = o.y + RULER + i as f32 * rh;
                let sel = i == s.selected;
                let visible = s.layer_visible(i);
                let row = Rect::from_min_size(
                    Pos2::new(o.x + 4.0, y + 1.0),
                    Vec2::new(width - 8.0, rh - 2.0),
                );
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
                let eye_rect =
                    Rect::from_center_size(Pos2::new(o.x + 22.0, y + rh / 2.0), Vec2::splat(16.0));
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
                        Pos2::new(o.x + 40.0, y + rh / 2.0),
                        Align2::LEFT_CENTER,
                        s.layer_name(i),
                        theme::body(),
                        name_col,
                        HEAD - 56.0 - PARAM_W,
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
                            .filter(|fr| {
                                s.doc.peg(i).is_some_and(|p| p.has_key(*fr)) || *fr == s.frame
                            })
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
                if let Some(peg) = s.doc.peg(i) {
                    for f in 1..=len {
                        if peg.has_key(f) {
                            key_diamond(
                                &painter,
                                Pos2::new(frame_x(f) + cw / 2.0, y + rh / 2.0),
                                4.5,
                            );
                        }
                    }
                }
            }

            let cx = frame_x(s.frame.min(len)) + cw / 2.0;
            painter.line_segment(
                [Pos2::new(cx, o.y + RULER - 4.0), Pos2::new(cx, rows_bottom)],
                Stroke::new(1.5, theme::ACCENT_TIME),
            );
            frame_tag(&painter, Pos2::new(cx, o.y + RULER - 12.0), s.frame);

            let row_at = |p: Pos2| -> Option<usize> {
                let r = ((p.y - o.y - RULER) / rh).floor();
                (r >= 0.0 && (r as usize) < rows).then_some(r as usize)
            };
            if let Some(p) = resp.interact_pointer_pos() {
                let rel = p - o;
                if rel.x >= HEAD {
                    s.frame = (((rel.x - HEAD) / cw) as u32 + 1).clamp(1, len);
                }
                if let Some(row) = row_at(p) {
                    if resp.clicked() || resp.drag_started() {
                        if rel.x < 32.0 && resp.clicked() {
                            s.toggle_visible(row);
                        }
                        s.selected = row;
                    }
                    if resp.double_clicked() && rel.x < HEAD - PARAM_W - 8.0 {
                        t.renaming = Some((row, s.layer_name(row).to_owned()));
                    }
                }
            }
            if resp.secondary_clicked() {
                t.menu_row = resp.interact_pointer_pos().and_then(row_at);
                if let Some(r) = t.menu_row {
                    s.selected = r;
                }
            }
            if let Some(row) = t.menu_row {
                resp.context_menu(|ui| layer_menu(ui, s, t, row));
            }

            if let Some((row, _)) = t.renaming {
                let y = o.y + RULER + row as f32 * rh;
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
        });
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
                        s.selected = c;
                    }
                    if resp.double_clicked() && rel.y < HEAD {
                        t.renaming = Some((c, s.layer_name(c).to_owned()));
                    }
                }
            }
            if resp.secondary_clicked() {
                t.menu_row = resp.interact_pointer_pos().and_then(col_at);
                if let Some(c) = t.menu_row {
                    s.selected = c;
                }
            }
            if let Some(c) = t.menu_row {
                resp.context_menu(|ui| layer_menu(ui, s, t, c));
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
