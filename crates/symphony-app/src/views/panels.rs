//! Side panels: Tool Properties, Colour, Layer Properties, Preferences, and
//! placeholders for views that aren't built yet.

use eframe::egui;
use symphony_core::id::Id;
use symphony_pen::DeviceInfo;
use symphony_ui::icons::{self, Icon};
use symphony_ui::{theme, widgets};

use crate::keymap::{Action, Profile};
use crate::presets::{self, Preset, PresetTool, Presets};
use crate::session::{stencil_at, BrushSettings, PaintMode, Session, Tool};
use symphony_core::stroke::Tip;

/// Card content padding (D6.2).
fn padded<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::Frame::new()
        .inner_margin(egui::Margin::same(theme::CARD_PAD))
        .show(ui, add)
        .inner
}

fn section(ui: &mut egui::Ui, title: &str) {
    ui.add_space(4.0);
    ui.label(
        egui::RichText::new(title)
            .font(theme::body())
            .color(theme::TEXT_PRIMARY),
    );
    ui.add_space(2.0);
}

fn hint(ui: &mut egui::Ui, text: &str) {
    ui.label(
        egui::RichText::new(text)
            .font(theme::dense())
            .color(theme::TEXT_SECONDARY),
    );
}

/// The largest tool size that can be typed. The size sliders stop at a
/// recommended size that keeps drawing fast; bigger is allowed by typing.
pub const MAX_TOOL_SIZE: f32 = 2000.0;

/// A size slider: the track covers the recommended range, and the number
/// box accepts anything up to `MAX_TOOL_SIZE`.
/// The computer's font families, loaded once in the background (it can
/// take a moment): None until ready.
fn font_families(ctx: &egui::Context) -> Option<&'static [String]> {
    static FAMILIES: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();
    static STARTED: std::sync::Once = std::sync::Once::new();
    if let Some(f) = FAMILIES.get() {
        return Some(f);
    }
    let ctx = ctx.clone();
    STARTED.call_once(move || {
        std::thread::spawn(move || {
            let _ = FAMILIES.set(crate::vector_import::font_families());
            ctx.request_repaint();
        });
    });
    None
}

fn text_controls(ui: &mut egui::Ui, s: &mut Session) {
    use crate::vector_import::TextAlign;
    section(ui, "Text");
    hint(
        ui,
        "Click in the drawing to type, or click existing text to edit it. \
         Cmd+Enter (Ctrl+Enter) finishes. Esc cancels.",
    );
    ui.add_space(4.0);
    let style = &mut s.text_style;
    ui.horizontal(|ui| {
        ui.label("Font");
        let shown = if style.family.is_empty() {
            "System sans".to_owned()
        } else {
            style.family.clone()
        };
        egui::ComboBox::from_id_salt("text-font")
            .selected_text(shown)
            .height(320.0)
            .show_ui(ui, |ui| match font_families(ui.ctx()) {
                Some(families) => {
                    ui.selectable_value(&mut style.family, String::new(), "System sans");
                    let key = ui.scope_id().with("font-filter");
                    let mut filter = ui
                        .data(|data| data.get_temp::<String>(key))
                        .unwrap_or_default();
                    ui.add(egui::TextEdit::singleline(&mut filter).hint_text("Filter fonts"));
                    ui.data_mut(|data| data.insert_temp(key, filter.clone()));
                    let filter = filter.to_lowercase();
                    for f in families
                        .iter()
                        .filter(|name| name.to_lowercase().contains(&filter))
                    {
                        ui.selectable_value(&mut style.family, f.clone(), f);
                    }
                }
                None => {
                    ui.label("Loading fonts…");
                }
            });
    });
    ui.hyperlink_to("Browse Google Fonts", "https://fonts.google.com/")
        .on_hover_text("Install a font on your computer, then reopen Symphony to use it.");
    size_slider(ui, "Size", &mut style.size, 600.0);
    style.size = style.size.max(1.0);
    ui.horizontal(|ui| {
        widgets::checkbox(ui, &mut style.bold, "Bold");
        widgets::checkbox(ui, &mut style.italic, "Italic");
    });
    ui.label("Alignment");
    ui.horizontal(|ui| {
        for (a, label) in [
            (TextAlign::Left, "Left"),
            (TextAlign::Center, "Center"),
            (TextAlign::Right, "Right"),
        ] {
            if widgets::choice(ui, style.align == a, label).clicked() {
                style.align = a;
            }
        }
    });
}

fn reduce_point_controls(ui: &mut egui::Ui, s: &mut Session) {
    if s.can_reduce_points() {
        ui.collapsing("Reduce stroke points", |ui| {
            ui.add(
                egui::DragValue::new(&mut s.point_reduction)
                    .range(0.1..=30.0)
                    .speed(0.1)
                    .prefix("Tolerance ")
                    .suffix(" px"),
            );
            if ui.button("Reduce points").clicked() {
                s.reduce_points();
            }
        });
    }
}

fn size_slider(ui: &mut egui::Ui, label: &str, v: &mut f32, recommended: f32) {
    hint(ui, label);
    ui.spacing_mut().slider_width = (ui.available_width() - 72.0).max(80.0);
    widgets::precise_slider(ui,
        egui::Slider::new(v, 0.0..=recommended)
            .step_by(0.1).clamping(egui::SliderClamping::Never),
    )
    .on_hover_text(format!(
        "The slider goes to {recommended:.0}, a size that stays fast. Type a bigger number to go up to {MAX_TOOL_SIZE:.0}."
    ));
    *v = v.clamp(0.0, MAX_TOOL_SIZE);
}

fn slider(ui: &mut egui::Ui, label: &str, v: &mut f32, range: std::ops::RangeInclusive<f32>) {
    hint(ui, label);
    ui.spacing_mut().slider_width = (ui.available_width() - 72.0).max(80.0);
    let step = f64::from((range.end() - range.start()) / 200.0);
    widgets::precise_slider(ui, egui::Slider::new(v, range).step_by(step));
}

/// A sample stroke that shows what the settings will do: pressure rises
/// and falls along it, unless a thickness stencil sets the thickness.
fn paint_sample(
    p: &egui::Painter,
    rect: egui::Rect,
    ink: egui::Color32,
    b: &BrushSettings,
    gamma: f32,
    pencil: bool,
) {
    if let (Some(tex), false) = (&b.texture, pencil) {
        super::textured::sample(p, rect, tex, b.min_width, b.max_width, ink);
        return;
    }
    let n = 48;
    let (minw, maxw) = (b.min_width, b.max_width.min(rect.height() - 8.0).max(0.5));
    let minw = minw.min(maxw);
    let wave = (rect.height() * 0.15).min(8.0);
    let pt = |i: usize| {
        let t = i as f32 / n as f32;
        let x = rect.left() + 12.0 + t * (rect.width() - 24.0);
        let y = rect.center().y + (t * std::f32::consts::TAU).sin() * wave;
        let pressure = match (&b.stencil, pencil) {
            (Some(st), true) => stencil_at(st, t),
            _ => symphony_vector::pressure_curve((t * std::f32::consts::PI).sin(), gamma),
        };
        (egui::pos2(x, y), minw + (maxw - minw) * pressure)
    };
    let round = |tip: Tip| !pencil || tip == Tip::Round;
    for i in 0..n {
        let ((a, wa), (c, wc)) = (pt(i), pt(i + 1));
        p.line_segment([a, c], egui::Stroke::new((wa + wc) * 0.5, ink));
        if i + 1 < n || round(b.tips.end) {
            p.circle_filled(c, wc * 0.5, ink);
        }
    }
    if round(b.tips.start) {
        let (a, wa) = pt(0);
        p.circle_filled(a, wa * 0.5, ink);
    }
}

fn preview(ui: &mut egui::Ui, s: &Session, b: &BrushSettings, pencil: bool) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 56.0), egui::Sense::hover());
    let p = ui.painter();
    p.rect_filled(
        rect,
        theme::RADIUS_CONTROL as f32,
        crate::session::CanvasColours::get(s.colours.stage),
    );
    let [r, g, bl, a] = s.doc.swatch_rgba(s.swatch);
    paint_sample(
        p,
        rect,
        egui::Color32::from_rgba_unmultiplied(r, g, bl, a),
        b,
        s.pressure_gamma,
        pencil,
    );
}

/// Harmony's preset list, in its Stroke View: a sample stroke and a name
/// per preset, with New, Delete and a menu for the rest.
fn preset_list(ui: &mut egui::Ui, s: &mut Session, presets: &mut Presets, tool: PresetTool) {
    let pencil = tool == PresetTool::Pencil;
    let state = ui.scope_id().with(("presets", pencil));
    let rename = state.with("rename");
    let removed = state.with("removed");
    let current_settings = if pencil { s.pencil } else { s.brush };
    let current = presets.current(tool).map(str::to_owned);
    ui.horizontal(|ui| {
        hint(ui, "Presets");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.menu_button("More", |ui| {
                if ui
                    .add_enabled(current.is_some(), egui::Button::new("Rename preset"))
                    .clicked()
                {
                    if let Some(name) = &current {
                        ui.data_mut(|d| d.insert_temp(rename, (name.clone(), name.clone())));
                    }
                    ui.close();
                }
                if ui
                    .add_enabled(current.is_some(), egui::Button::new("Update preset"))
                    .on_hover_text("Save the current settings into the selected preset")
                    .clicked()
                {
                    if let Some(p) = current.as_ref().and_then(|name| {
                        presets.list_mut(tool).iter_mut().find(|p| &p.name == name)
                    }) {
                        // Updating a bitmap brush from its vector version
                        // keeps the brush's stamps.
                        let texture = match current_settings.texture {
                            Some(t) if !t.vector => Some(t),
                            _ => p.settings.texture,
                        };
                        p.settings = current_settings;
                        p.settings.texture = texture;
                    }
                    ui.close();
                }
                ui.separator();
                if ui.button("Import presets…").clicked() {
                    ui.close();
                    import_presets(s, presets);
                }
                if ui.button("Export presets…").clicked() {
                    ui.close();
                    export_presets(s, presets);
                }
            });
            if icons::button(ui, Icon::Minus, false, current.is_some(), "Delete preset").clicked() {
                if let Some(gone) = current.as_ref().and_then(|n| presets.remove(tool, n)) {
                    ui.data_mut(|d| d.insert_temp(removed, gone));
                }
            }
            if icons::button(
                ui,
                Icon::Plus,
                false,
                true,
                "New preset from these settings",
            )
            .clicked()
            {
                let name = presets.add(
                    tool,
                    if pencil { "Pencil" } else { "Brush" },
                    current_settings,
                );
                ui.data_mut(|d| d.insert_temp(rename, (name.clone(), name)));
            }
        });
    });
    // List (name beside its stroke) or Grid (more presets at a glance).
    ui.horizontal(|ui| {
        hint(ui, "View");
        widgets::segmented(
            ui,
            "preset-view",
            &mut presets.grid,
            &[(false, "List"), (true, "Grid")],
            false,
        );
    });
    // Two kinds of brush, as Tristan set out: Bitmap lists the brushes
    // made for bitmap painting (textured stamps, like Krita's and
    // Photoshop's). Vector lists clean vector brushes plus a vector version
    // of every bitmap brush: the same name, size and smoothing as a solid
    // line (an approximation; stamps don't map exactly onto curves).
    let list = if pencil {
        presets.list(tool).to_vec()
    } else {
        brush_list(presets.list(tool), is_bitmap(&current_settings))
    };
    let [r, g, b, a] = s.doc.swatch_rgba(s.swatch);
    let ink = egui::Color32::from_rgba_unmultiplied(r, g, b, a);
    egui::ScrollArea::vertical()
        .id_salt(("preset-scroll", pencil))
        .max_height(if presets.grid { 240.0 } else { 168.0 })
        .show(ui, |ui| {
            let grid = presets.grid;
            let mut body = |ui: &mut egui::Ui| {
                for preset in &list {
                    let selected = current.as_deref() == Some(preset.name.as_str());
                    let draft = ui.data(|d| d.get_temp::<(String, String)>(rename));
                    if let Some((from, mut text)) = draft.filter(|(from, _)| *from == preset.name) {
                        let edit = ui.text_edit_singleline(&mut text);
                        if !edit.has_focus() && !edit.lost_focus() {
                            edit.request_focus();
                        }
                        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                            ui.data_mut(|d| d.remove::<(String, String)>(rename));
                        } else if edit.lost_focus() {
                            presets.rename(tool, &from, &text);
                            ui.data_mut(|d| d.remove::<(String, String)>(rename));
                        } else {
                            ui.data_mut(|d| d.insert_temp(rename, (from, text)));
                        }
                        continue;
                    }
                    let (rect, row) = if grid {
                        ui.allocate_exact_size(egui::vec2(88.0, 70.0), egui::Sense::click())
                    } else {
                        let width = ui.available_width();
                        ui.allocate_exact_size(egui::vec2(width, 30.0), egui::Sense::click())
                    };
                    let fill = if selected {
                        theme::ACCENT_SELECT_FILL
                    } else if row.hovered() {
                        theme::BG_HOVER
                    } else {
                        egui::Color32::TRANSPARENT
                    };
                    let p = ui.painter();
                    p.rect_filled(rect, theme::RADIUS_CONTROL as f32, fill);
                    let sample = if grid {
                        egui::Rect::from_min_size(
                            rect.min + egui::vec2(4.0, 4.0),
                            egui::vec2(rect.width() - 8.0, 42.0),
                        )
                    } else {
                        egui::Rect::from_min_size(
                            rect.min + egui::vec2(4.0, 3.0),
                            egui::vec2(72.0, rect.height() - 6.0),
                        )
                    };
                    p.rect_filled(
                        sample,
                        3.0,
                        crate::session::CanvasColours::get(s.colours.stage),
                    );
                    paint_sample(p, sample, ink, &preset.settings, 1.0, pencil);
                    let name_colour = if selected {
                        theme::ON_ACCENT
                    } else {
                        theme::TEXT_PRIMARY
                    };
                    if grid {
                        // Name under the stroke; the tooltip below has it in full.
                        widgets::fit_text(
                            ui,
                            p,
                            egui::pos2(rect.center().x, sample.bottom() + 11.0),
                            egui::Align2::CENTER_CENTER,
                            &preset.name,
                            theme::dense(),
                            name_colour,
                            rect.width() - 8.0,
                        );
                    } else {
                        // Long names end in an ellipsis; the row's tooltip below
                        // starts with the full name (D5.5).
                        let tag = if preset.settings.stencil.is_some() && pencil {
                            56.0
                        } else {
                            0.0
                        };
                        let left = sample.right() + 8.0;
                        widgets::fit_text(
                            ui,
                            p,
                            egui::pos2(left, rect.center().y),
                            egui::Align2::LEFT_CENTER,
                            &preset.name,
                            theme::body(),
                            name_colour,
                            rect.right() - 6.0 - tag - left,
                        );
                        if preset.settings.stencil.is_some() && pencil {
                            p.text(
                                egui::pos2(rect.right() - 6.0, rect.center().y),
                                egui::Align2::RIGHT_CENTER,
                                "Stencil",
                                theme::dense(),
                                if selected {
                                    theme::ON_ACCENT
                                } else {
                                    theme::TEXT_SECONDARY
                                },
                            );
                        }
                    }
                    let mut tip = preset.name.clone();
                    if let Some(src) = &preset.source {
                        tip.push_str(&format!("\nImported from {src}"));
                    }
                    tip.push_str(if pencil && preset.settings.stencil.is_some() {
                        "\nThickness stencil: ignores pen pressure"
                    } else {
                        "\nClick to use. Double-click to rename."
                    });
                    let row = row.on_hover_text(tip);
                    if row.clicked() {
                        if pencil {
                            s.pencil = preset.settings;
                        } else {
                            s.brush = preset.settings;
                        }
                        presets.set_current(tool, Some(preset.name.clone()));
                    }
                    if row.double_clicked() {
                        ui.data_mut(|d| {
                            d.insert_temp(rename, (preset.name.clone(), preset.name.clone()))
                        });
                    }
                }
            };
            if grid {
                ui.horizontal_wrapped(|ui| body(ui));
            } else {
                body(ui);
            }
        });
    if let Some((index, gone)) = ui.data(|d| d.get_temp::<(usize, Preset)>(removed)) {
        ui.horizontal(|ui| {
            hint(ui, &format!("Deleted {}.", gone.name));
            if ui.small_button("Restore").clicked() {
                presets.restore(tool, index, gone.clone());
                ui.data_mut(|d| d.remove::<(usize, Preset)>(removed));
            }
        });
    }
    if let Some(name) = &current {
        let unchanged = presets
            .list(tool)
            .iter()
            .any(|p| &p.name == name && p.settings == current_settings);
        if !unchanged {
            hint(
                ui,
                &format!("Changed from {name}. Update it in More, or add a new preset."),
            );
        }
    }
}

fn import_presets(s: &mut Session, presets: &mut Presets) {
    let Some(path) = rfd::FileDialog::new()
        .set_title("Import presets")
        .add_filter("Symphony presets", &[presets::EXTENSION])
        .add_filter("Harmony brush presets", &["xml", "tbr"])
        .pick_file()
    else {
        return;
    };
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase);
    if matches!(ext.as_deref(), Some("xml" | "tbr")) {
        s.notice = Some(
            "Symphony can't read Harmony brush files yet. That import is being worked on.".into(),
        );
        return;
    }
    s.notice = Some(match presets.import(&path) {
        Ok(1) => "Imported 1 preset".into(),
        Ok(n) => format!("Imported {n} presets"),
        Err(e) => e,
    });
}

fn export_presets(s: &mut Session, presets: &Presets) {
    let Some(mut path) = rfd::FileDialog::new()
        .set_title("Export presets")
        .set_file_name(format!("My presets.{}", presets::EXTENSION))
        .add_filter("Symphony presets", &[presets::EXTENSION])
        .save_file()
    else {
        return;
    };
    if path.extension().is_none() {
        path.set_extension(presets::EXTENSION);
    }
    s.notice = Some(match presets.export(&path) {
        Ok(()) => format!(
            "Exported {} brush and {} pencil presets",
            presets.brush.len(),
            presets.pencil.len()
        ),
        Err(e) => format!("Couldn't export the presets: {e}"),
    });
}

/// The Brush presets of one kind: Bitmap is the textured (bitmap-native)
/// ones; Vector is the clean ones plus a vector version of each bitmap one.
pub fn brush_list(all: &[Preset], bitmap: bool) -> Vec<Preset> {
    if bitmap {
        all.iter()
            .filter(|p| is_bitmap(&p.settings))
            .cloned()
            .collect()
    } else {
        all.iter().map(vector_version).collect()
    }
}

/// Does this brush paint bitmap stamps (rather than vector lines)?
pub fn is_bitmap(b: &BrushSettings) -> bool {
    b.texture.is_some_and(|t| !t.vector)
}

/// A brush as a true vector brush: a bitmap brush keeps its name, size,
/// tip and dynamics, but its tip's shape is stamped along the line and
/// merged into one clean outline instead of painting pixels.
fn vector_version(p: &Preset) -> Preset {
    let mut v = p.clone();
    if let Some(t) = &mut v.settings.texture {
        t.vector = true;
    }
    v
}

fn brush_controls(ui: &mut egui::Ui, s: &mut Session, presets: &mut Presets, pencil: bool) {
    let tool = if pencil {
        PresetTool::Pencil
    } else {
        PresetTool::Brush
    };
    // First the kind of brush, since it decides the list below.
    if !pencil {
        let bitmap = is_bitmap(&s.brush);
        let mut want = bitmap;
        widgets::segmented(
            ui,
            "brush-kind",
            &mut want,
            &[(false, "Vector"), (true, "Bitmap")],
            false,
        );
        if want != bitmap {
            // Keep the same brush where it exists in the other kind: a
            // bitmap brush's vector version, or back to the bitmap one.
            let current = presets.current(tool).map(str::to_owned);
            let same = current
                .as_ref()
                .and_then(|name| presets.list(tool).iter().find(|p| &p.name == name).cloned());
            let pick = match (want, same) {
                (false, Some(p)) => Some(vector_version(&p)),
                (true, Some(p)) if is_bitmap(&p.settings) => Some(p),
                _ => presets
                    .list(tool)
                    .iter()
                    .find(|p| is_bitmap(&p.settings) == want)
                    .cloned()
                    .map(|p| if want { p } else { vector_version(&p) }),
            };
            match pick {
                Some(p) => {
                    s.brush = p.settings;
                    presets.set_current(tool, Some(p.name));
                }
                None => {
                    s.brush.texture = want.then(|| symphony_core::stroke::StrokeTexture {
                        spacing: 0.1,
                        ..Default::default()
                    });
                    presets.set_current(tool, None);
                }
            }
        }
        ui.add_space(6.0);
    }
    // The preview is pinned above (tool_properties); then the presets,
    // then the settings, as in Harmony.
    preset_list(ui, s, presets, tool);
    ui.add_space(8.0);
    let mut b = if pencil { s.pencil } else { s.brush };
    size_slider(ui, "Maximum size", &mut b.max_width, 60.0);
    b.max_width = b.max_width.max(0.5);
    size_slider(ui, "Minimum size", &mut b.min_width, 60.0);
    b.min_width = b.min_width.min(b.max_width);
    slider(ui, "Smoothing", &mut b.smoothing, 0.0..=0.95);
    hint(
        ui,
        if pencil && b.stencil.is_some() {
            "This preset's thickness stencil sets the thickness between minimum and maximum, ignoring pen pressure."
        } else {
            "Pen pressure sets the size between minimum and maximum. Smoothing steadies the line as you draw."
        },
    );
    if !pencil {
        if let Some(t) = &mut b.texture {
            slider(ui, "Spacing", &mut t.spacing, 0.01..=3.0);
            slider(ui, "Opacity", &mut t.opacity, 0.0..=1.0);
            slider(ui, "Flow", &mut t.flow, 0.01..=1.0);
            hint(
                ui,
                "Below 1, each stamp lays down a little and overlaps build up.",
            );
            if t.tip.is_none() {
                slider(ui, "Hardness", &mut t.hardness, 0.0..=1.0);
            }
            // One lever for how much light pressure fades the stroke.
            let mut fade = 1.0 - t.opacity_by_pressure[0];
            slider(ui, "Pressure fades opacity", &mut fade, 0.0..=1.0);
            let fade = fade.clamp(0.0, 1.0);
            let linear = [1.0 - fade, 1.0 - fade * 2.0 / 3.0, 1.0 - fade / 3.0, 1.0];
            if (linear[0] - t.opacity_by_pressure[0]).abs() > 1e-4 {
                t.opacity_by_pressure = linear;
            }
            slider(ui, "Tip angle", &mut t.angle, -180.0..=180.0);
            widgets::checkbox(ui, &mut t.follow, "Turn tip with the stroke");
            if t.grain.is_some() {
                slider(ui, "Paper grain scale", &mut t.grain_scale, 0.1..=8.0);
            }
            hint(
                ui,
                "Strokes stay vector lines: move, scale or reshape them and the texture is redrawn, not stretched.",
            );
        }
    }
    if pencil {
        ui.add_space(4.0);
        egui::Grid::new("pencil-tips")
            .num_columns(3)
            .spacing([8.0, 4.0])
            .show(ui, |ui| {
                for (label, tip) in [
                    ("Start tip", &mut b.tips.start),
                    ("End tip", &mut b.tips.end),
                ] {
                    hint(ui, label);
                    ui.selectable_value(tip, Tip::Round, "Round");
                    ui.selectable_value(tip, Tip::Flat, "Flat");
                    ui.end_row();
                }
            });
        if b.stencil.is_some() && ui.small_button("Remove thickness stencil").clicked() {
            b.stencil = None;
        }
    }
    if pencil {
        s.pencil = b;
    } else {
        s.brush = b;
    }
}

pub fn tool_properties(
    ui: &mut egui::Ui,
    s: &mut Session,
    presets: &mut Presets,
    device: Option<&DeviceInfo>,
) {
    // Brush and Pencil: the name and stroke preview stay pinned at the top,
    // so changing any setting below shows its effect right away.
    if matches!(s.tool, Tool::Brush | Tool::Line | Tool::Pencil) {
        let pencil = s.tool == Tool::Pencil;
        padded(ui, |ui| {
            section(ui, if pencil { "Pencil" } else { "Brush" });
            let pencil = s.tool == Tool::Pencil;
            let b = if pencil { s.pencil } else { s.brush };
            preview(ui, s, &b, pencil);
        });
        ui.separator();
    }
    // Long tool settings scroll instead of being cut off, and a soft fade
    // at an edge says there's more that way.
    let out = egui::ScrollArea::vertical()
        .id_salt("tool-properties")
        .auto_shrink([false, false])
        .show(ui, |ui| tool_properties_body(ui, s, presets, device));
    let view = out.inner_rect;
    let (offset, content) = (out.state.clamped_offset().y, out.content_size.y);
    let fade = 28.0_f32.min(view.height() * 0.25);
    let tint = theme::GLASS_PANEL;
    if offset > 1.0 {
        theme::fade_strip(
            ui.painter(),
            egui::Rect::from_min_size(view.min, egui::vec2(view.width(), fade)),
            tint,
            true,
        );
    }
    if offset + view.height() < content - 1.0 {
        theme::fade_strip(
            ui.painter(),
            egui::Rect::from_min_max(egui::pos2(view.left(), view.bottom() - fade), view.max),
            tint,
            false,
        );
    }
}

fn tool_properties_body(
    ui: &mut egui::Ui,
    s: &mut Session,
    presets: &mut Presets,
    device: Option<&DeviceInfo>,
) {
    padded(ui, |ui| {
        match s.tool {
            Tool::Brush => brush_controls(ui, s, presets, false),
            Tool::Line => {
                hint(
                    ui,
                    "Drag a straight line in the Brush's style. Hold Shift to snap to 22.5° steps, hold Alt to bend it into a curve.",
                );
                ui.add_space(4.0);
                brush_controls(ui, s, presets, false);
            }
            Tool::Pencil => brush_controls(ui, s, presets, true),
            Tool::Text => text_controls(ui, s),
            Tool::Eraser => {
                section(ui, "Eraser");
                let e = &mut s.eraser;
                size_slider(ui, "Maximum size", &mut e.max_width, 120.0);
                e.max_width = e.max_width.max(1.0);
                size_slider(ui, "Minimum size", &mut e.min_width, 120.0);
                e.min_width = e.min_width.min(e.max_width);
                hint(ui, "Erases the lines and paint it passes over, on the current art layer unless the options below say otherwise. Pen pressure sets the size. The eraser end of your pen works with any tool.");
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    hint(ui, "Tip style");
                    widgets::segmented(
                        ui,
                        "eraser-tip",
                        &mut s.eraser_tip,
                        &[(Tip::Round, "Round"), (Tip::Flat, "Flat")],
                        false,
                    );
                });
                hint(ui, "The shape left at the ends of a pencil line where you cut it. Brush lines and paint lose exactly the eraser's shape.");
                ui.add_space(6.0);
                widgets::checkbox(ui, &mut s.erase_all_art, "Apply to all art layers");
                hint(ui, "Erase Line Art, Colour Art, Overlay and Underlay together, like Harmony's Apply to Line and Colour Art.");
                widgets::checkbox(ui, &mut s.erase_all_layers, "Erase all layers");
                hint(ui, "Erase through every visible layer under the eraser, not only the selected one.");
                widgets::checkbox(ui, &mut s.erase_all_frames, "Erase all frames");
                hint(ui, "Erase the same place in every drawing of the layer, not only the one at this frame.");
            }
            Tool::Select | Tool::Transform => {
                reduce_point_controls(ui, s);
                let select = s.tool == Tool::Select;
                section(ui, if select { "Select" } else { "Transform" });
                widgets::segmented(
                    ui,
                    "select-mode",
                    &mut s.marquee_mode,
                    &[(false, "Lasso"), (true, "Marquee")],
                    false,
                );
                hint(
                    ui,
                    "Hold Option/Alt to use the other mode for one selection.",
                );
                widgets::checkbox(ui, &mut s.easy_drag, "Easy drag");
                hint(
                    ui,
                    "Drag the selection from anywhere inside its box, not only on the artwork.",
                );
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui.button("Flip horizontal").clicked() {
                        if select {
                            s.flip_selection(true);
                        } else {
                            let l = s.selected;
                            s.adjust_peg(l, |v| v.scale_x = -v.scale_x);
                            s.end_gesture();
                        }
                    }
                    if ui.button("Flip vertical").clicked() {
                        if select {
                            s.flip_selection(false);
                        } else {
                            let l = s.selected;
                            s.adjust_peg(l, |v| v.scale_y = -v.scale_y);
                            s.end_gesture();
                        }
                    }
                });
                if select {
                    let n = s.selection.strokes.len();
                    hint(
                        ui,
                        &format!("{n} stroke{} selected", if n == 1 { "" } else { "s" }),
                    );
                    if ui.add_enabled(n > 0, egui::Button::new("Delete")).clicked() {
                        s.delete_selection();
                    }
                    let stencil = s.selected_stencil();
                    let new_stencil = ui
                        .add_enabled(
                            stencil.is_some(),
                            egui::Button::new("New thickness stencil"),
                        )
                        .on_hover_text("Save this pencil line's thickness as a Pencil preset")
                        .on_disabled_hover_text("Select one pencil line first");
                    if new_stencil.clicked() {
                        if let Some(st) = stencil {
                            let mut settings = s.pencil;
                            settings.stencil = Some(st);
                            let name = presets.add(PresetTool::Pencil, "Stencil", settings);
                            s.notice = Some(format!("Added the pencil preset {name}"));
                        }
                    }
                } else {
                    hint(ui, "Click artwork to pick its layer. Drag to move, corner handles to scale, just outside a corner to rotate. Auto-key records changes at the current frame.");
                }
            }
            Tool::Paint => {
                section(ui, "Paint");
                for mode in [
                    PaintMode::Paint,
                    PaintMode::PaintUnpainted,
                    PaintMode::Repaint,
                    PaintMode::Unpaint,
                ] {
                    widgets::radio_value(ui, &mut s.paint_mode, mode, mode.title());
                }
                hint(ui, match s.paint_mode {
                    PaintMode::Paint => "Fills a closed area, or recolours it if it's already painted. Click a line to recolour the line.",
                    PaintMode::PaintUnpainted => "Fills only empty areas and leaves painted ones alone.",
                    PaintMode::Repaint => "Recolours only areas that are already painted.",
                    PaintMode::Unpaint => "Removes the colour from an area.",
                });
                widgets::checkbox(
                    ui,
                    &mut s.link_bucket_fills,
                    "Keep new fills linked to lines",
                )
                .on_hover_text(
                    "Refits the fill when its boundary moves or is reshaped with the Line Editor.",
                );
                ui.add_space(6.0);
                widgets::checkbox(ui, &mut s.respect_protected, "Respect protected colours");
                hint(
                    ui,
                    "Leaves colours marked as protected in the Colour view alone (Shift+S).",
                );
                widgets::checkbox(ui, &mut s.show_strokes, "Show strokes");
                hint(ui, "Shows the invisible lines that close areas (K).");
            }
            Tool::CloseGap => {
                section(ui, "Close Gap");
                hint(ui, "Draw a rough line near a gap. Symphony joins the two nearest line ends with an invisible stroke, so the area can be painted.");
                widgets::checkbox(ui, &mut s.show_strokes, "Show strokes");
            }
            Tool::LineEditor => {
                section(ui, "Line Editor");
                reduce_point_controls(ui, s);
                size_slider(ui, "Radius", &mut s.line_edit_radius, 200.0);
                s.line_edit_radius = s.line_edit_radius.max(1.0);
                slider(ui, "Strength", &mut s.line_edit_strength, 0.05..=1.0);
                hint(ui, "Drag to push the lines under the brush: points in the middle move most and the effect fades out towards the edge.");
                ui.add_space(4.0);
                hint(ui, "Shift-drag to paint over points and select them. Then drag to move only those points.");
                let n = s.line_points.len();
                if ui
                    .add_enabled(
                        n > 0,
                        egui::Button::new(format!("Clear point selection ({n})")),
                    )
                    .clicked()
                {
                    s.line_points.clear();
                }
            }
            Tool::Dropper => {
                section(ui, "Dropper");
                hint(ui, "Click a line or painted area to pick up its colour.");
                #[cfg(target_os = "macos")]
                if ui.button("Pick colour from screen").clicked() {
                    crate::screen_colour::pick(ui.ctx(), s.key.clone());
                }
            }
            Tool::Hand => {
                section(ui, "Hand");
                hint(
                    ui,
                    "Drag to pan. Hold Space with any tool to pan, Shift+Space to rotate.",
                );
            }
            Tool::Zoom => {
                section(ui, "Zoom");
                hint(ui, "Click to zoom in, Option/Alt-click to zoom out. Pinch or Cmd/Ctrl+scroll also zoom.");
            }
        }
        ui.add_space(12.0);
        section(ui, "Tablet");
        slider(ui, "Pressure response", &mut s.pressure_gamma, 0.3..=3.0);
        hint(ui, "Lower makes light strokes heavier. 1.0 is linear.");
        let text = match device {
            Some(d) if d.in_proximity => "Pen in range",
            Some(_) => "Pen input ready. Bring the pen near the tablet.",
            None => "No pen support on this system yet. Using the mouse.",
        };
        hint(ui, text);
    });
}

/// A colour chip edits the paint; its name selects the swatch for drawing.
pub fn colour(ui: &mut egui::Ui, s: &mut Session) {
    padded(ui, |ui| {
        let state = ui.scope_id().with(("colour-edit", &s.key));
        let rename = state.with("rename");
        let removal = state.with("remove");
        ui.horizontal(|ui| {
            if icons::button(ui, Icon::Plus, false, true, "Add colour").clicked() {
                s.add_swatch();
                ui.data_mut(|d| d.insert_temp(rename, (s.swatch, String::from("New colour"))));
            }
            let options = &s.doc.palette.swatches;
            if icons::button(ui, Icon::Minus, false, options.len() > 1, "Remove colour").clicked() {
                if let Some(other) = options.iter().find(|sw| sw.id != s.swatch) {
                    ui.data_mut(|d| d.insert_temp(removal, (s.swatch, other.id)));
                }
            }
            ui.label(&s.doc.palette.name);
        });
        let swatches = s.doc.palette.swatches.clone();
        egui::ScrollArea::vertical().show(ui, |ui| {
            for (index, sw) in swatches.iter().enumerate() {
                ui.push_id(sw.id.0, |ui| {
                    ui.horizontal(|ui| {
                        // Harmony marks a protected colour with a red bar
                        // beside it. The space is kept so rows line up.
                        let (bar, _) = ui.allocate_exact_size(
                            egui::vec2(3.0, ui.spacing().interact_size.y),
                            egui::Sense::hover(),
                        );
                        if sw.protected {
                            ui.painter().rect_filled(bar, 1.0, theme::STATE_DANGER);
                            ui.interact(bar, ui.scope_id().with("protected"), egui::Sense::hover())
                                .on_hover_text("Protected colour");
                        }
                        let mut rgba = sw.rgba();
                        let chip = super::colour_picker::swatch(ui, &mut rgba);
                        if chip.clicked() || chip.changed() {
                            s.swatch = sw.id;
                        }
                        if chip.changed() {
                            s.set_swatch(sw.id, rgba);
                        }
                        let draft = ui.data(|d| d.get_temp::<(Id, String)>(rename));
                        if let Some((id, mut text)) = draft.filter(|(id, _)| *id == sw.id) {
                            let edit = ui.text_edit_singleline(&mut text);
                            if !edit.has_focus() && !edit.lost_focus() {
                                edit.request_focus();
                            }
                            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                                ui.data_mut(|d| d.remove::<(Id, String)>(rename));
                            } else if edit.lost_focus() {
                                s.rename_swatch(id, &text);
                                ui.data_mut(|d| d.remove::<(Id, String)>(rename));
                            } else {
                                ui.data_mut(|d| d.insert_temp(rename, (id, text)));
                            }
                        } else {
                            let row = ui.selectable_label(s.swatch == sw.id, &sw.name);
                            if row.clicked() {
                                s.swatch = sw.id;
                            }
                            if row.double_clicked() {
                                ui.data_mut(|d| d.insert_temp(rename, (sw.id, sw.name.clone())));
                            }
                            row.context_menu(|ui| {
                                if ui.button("Rename").clicked() {
                                    ui.data_mut(|d| {
                                        d.insert_temp(rename, (sw.id, sw.name.clone()))
                                    });
                                    ui.close();
                                }
                                let protect = if sw.protected {
                                    "Unprotect Colour"
                                } else {
                                    "Protect Colour"
                                };
                                if ui.button(protect).clicked() {
                                    s.toggle_protect_swatch(sw.id);
                                    ui.close();
                                }
                                for (label, delta, enabled) in [
                                    ("Move up", -1, index > 0),
                                    ("Move down", 1, index + 1 < swatches.len()),
                                ] {
                                    if ui.add_enabled(enabled, egui::Button::new(label)).clicked() {
                                        s.move_swatch(sw.id, delta);
                                        ui.close();
                                    }
                                }
                            });
                        }
                    })
                });
            }
        });
        let pending = ui.data(|d| d.get_temp::<(Id, Id)>(removal));
        if let Some((id, mut next)) = pending {
            if let Some(sw_name) = s.doc.palette.get(id).map(|sw| sw.name.clone()) {
                theme::glass_modal(ui.ctx(), removal).show(ui.ctx(), |ui| {
                    ui.set_max_width(400.0);
                    ui.heading(format!("Remove {}?", sw_name));
                    ui.add_space(6.0);
                    ui.label("Use this colour for its strokes and fills:");
                    egui::ComboBox::from_id_salt("replacement")
                        .selected_text(s.doc.palette.get(next).map_or("", |sw| sw.name.as_str()))
                        .show_ui(ui, |ui| {
                            for item in swatches.iter().filter(|item| item.id != id) {
                                ui.selectable_value(&mut next, item.id, &item.name);
                            }
                        });
                    ui.data_mut(|d| d.insert_temp(removal, (id, next)));
                    ui.horizontal(|ui| {
                        if ui.button("Cancel").clicked() {
                            ui.data_mut(|d| d.remove::<(Id, Id)>(removal));
                        }
                        if ui.button("Remove colour").clicked() {
                            s.delete_swatch(id, next);
                            ui.data_mut(|d| d.remove::<(Id, Id)>(removal));
                        }
                    });
                });
            }
        }
    });
}

/// Preferences: Workflow (DESIGN §11) and AI (DESIGN §16).
/// Preferences > Drawing.
pub fn prefs_drawing(ui: &mut egui::Ui, s: &mut Session) {
    section(ui, "Lines");
    hint(ui, "Stroke detail");
    ui.spacing_mut().slider_width = (ui.available_width() - 72.0).max(80.0);
    widgets::precise_slider(
        ui,
        egui::Slider::new(&mut s.stroke_detail, 0.1..=1.0)
            .step_by(0.01)
            .custom_formatter(|v, _| format!("{:.0}%", v * 100.0))
            .custom_parser(|t| {
                t.trim_end_matches('%')
                    .trim()
                    .parse::<f64>()
                    .ok()
                    .map(|v| v / 100.0)
            }),
    );
    hint(
        ui,
        "How many of a line's pen points to keep. Lines are always drawn as smooth curves; lower detail makes them lighter and steadier, 100% keeps every point.",
    );
    slider(
        ui,
        "Pen pressure response",
        &mut s.pressure_gamma,
        0.3..=3.0,
    );
    hint(ui, "Lower makes light strokes heavier. 1.0 is linear.");
    ui.add_space(8.0);
    section(ui, "Tools");
    widgets::checkbox(ui, &mut s.brush_size_cursor, "Brush size cursor");
    hint(
        ui,
        "Shows the brush size as a circle around the cursor, as in Harmony.",
    );
    widgets::checkbox(ui, &mut s.size_follows_zoom, "Brush size follows zoom");
    hint(
        ui,
        "On: a brush is the same size on screen at any zoom, so zoomed-in lines are finer. Off (default): sizes belong to the drawing, so zooming in makes the brush look bigger.",
    );
    widgets::checkbox(ui, &mut s.respect_protected, "Respect protected colours");
    hint(ui, "Painting leaves protected colours alone (Shift+S).");
    ui.add_space(8.0);
    section(ui, "Art layers");
    widgets::checkbox(
        ui,
        &mut s.overlay_underlay,
        "Support Overlay and Underlay Arts",
    );
    hint(
        ui,
        "Adds the Overlay and Underlay art layers to Line Art and Colour Art, as in Harmony's Advanced preferences.",
    );
}

/// Preferences > Canvas.
pub fn prefs_canvas(ui: &mut egui::Ui, s: &mut Session) {
    section(ui, "Colours");
    let defaults = crate::session::CanvasColours::default();
    let c = &mut s.colours;
    for (label, value, default, tip) in [
        (
            "Stage",
            &mut c.stage,
            defaults.stage,
            "The empty stage behind your drawing. Exports still render on white.",
        ),
        (
            "Around the stage",
            &mut c.surround,
            defaults.surround,
            "The area outside the stage.",
        ),
        (
            "Onion skin: previous",
            &mut c.onion_before,
            defaults.onion_before,
            "Tint of earlier drawings.",
        ),
        (
            "Onion skin: next",
            &mut c.onion_after,
            defaults.onion_after,
            "Tint of later drawings.",
        ),
    ] {
        ui.horizontal(|ui| {
            super::colour_picker::rgb(ui, value).on_hover_text(tip);
            ui.label(label);
            if *value != default && ui.small_button("Reset").clicked() {
                *value = default;
            }
        });
    }
    ui.add_space(8.0);
    section(ui, "Display");
    widgets::checkbox(ui, &mut s.show_strokes, "Show strokes");
    hint(ui, "Shows the invisible lines that close paint areas (K).");
    widgets::checkbox(ui, &mut s.art_preview, "Preview all art layers");
    hint(
        ui,
        "The Drawing view shows the other art layers as reference (P).",
    );
}

/// Preferences > Workflow and shortcuts. The list can be filtered.
pub fn prefs_workflow(ui: &mut egui::Ui, profile: &mut Profile) {
    section(ui, "Shortcut profile");
    for p in [Profile::Harmony, Profile::Symphony] {
        widgets::radio_value(ui, profile, p, p.title());
        hint(ui, p.blurb());
        ui.add_space(4.0);
    }
    ui.add_space(8.0);
    section(ui, "Shortcuts");
    let id = ui.scope_id().with("shortcut-filter");
    let mut filter = ui.data(|d| d.get_temp::<String>(id)).unwrap_or_default();
    ui.add(
        egui::TextEdit::singleline(&mut filter)
            .hint_text("Filter, e.g. onion or Alt")
            .desired_width(f32::INFINITY),
    );
    ui.data_mut(|d| d.insert_temp(id, filter.clone()));
    let needle = filter.to_lowercase();
    egui::Grid::new("shortcuts")
        .num_columns(2)
        .striped(true)
        .show(ui, |ui| {
            for a in Action::ALL {
                let l = crate::keymap::label(*profile, a);
                let hay = format!("{} {} {}", a.name(), a.id(), l).to_lowercase();
                if !needle.is_empty() && !hay.contains(&needle) {
                    continue;
                }
                ui.label(a.name()).on_hover_text(a.id());
                ui.colored_label(
                    theme::TEXT_SECONDARY,
                    if l.is_empty() { "None".to_owned() } else { l },
                );
                ui.end_row();
            }
        });
    hint(
        ui,
        "Harmony shortcuts follow Harmony 27's published defaults.",
    );
}

/// Preferences > AI.
pub fn prefs_ai(ui: &mut egui::Ui, use_no_ai: &mut bool) {
    widgets::checkbox(ui, use_no_ai, "Use no AI");
    hint(ui, "Symphony has no generative AI. It has no assisted tools yet either, so nothing in it uses AI. Search understands plain words with a hand-written word list, not AI.");
    ui.hyperlink_to(
        "How Symphony uses AI",
        "https://github.com/tdemers218/symphony/blob/main/docs/AI-POLICY.md",
    );
}

/// Help > Icon sheet: every icon side by side for review (D7.7).
pub fn icon_sheet(ui: &mut egui::Ui) {
    egui::Grid::new("icon-proof")
        .spacing(egui::vec2(6.0, 4.0))
        .show(ui, |ui| {
            for group in Icon::ALL.chunks(9) {
                for &icon in group {
                    ui.vertical_centered(|ui| {
                        ui.set_width(74.0);
                        icons::button(ui, icon, false, true, &format!("{icon:?}"));
                        icons::tool_button(ui, icon, true, true, &format!("{icon:?}"));
                        let label = match icon {
                            Icon::FirstFrame => "First frame",
                            Icon::PrevFrame => "Previous frame",
                            Icon::NextFrame => "Next frame",
                            Icon::LastFrame => "Last frame",
                            Icon::EyeOff => "Hidden",
                            Icon::Eye => "Visible",
                            Icon::Onion => "Onion skin",
                            Icon::RotateLeft => "Rotate left",
                            Icon::RotateRight => "Rotate right",
                            Icon::ZoomOut => "Zoom out",
                            Icon::CloseGap => "Close gap",
                            _ => &format!("{icon:?}"),
                        };
                        ui.small(label);
                    });
                }
                ui.end_row();
            }
        });
}

pub fn planned(ui: &mut egui::Ui, what: &str, milestone: &str) {
    ui.vertical_centered(|ui| {
        ui.add_space(32.0);
        ui.label(egui::RichText::new(what).strong());
        ui.colored_label(
            theme::TEXT_SECONDARY,
            format!("Coming in {milestone}. See docs/ROADMAP.md."),
        );
    });
}
