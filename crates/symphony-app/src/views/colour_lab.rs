//! The Colour Lab view: a lit sphere or cube to find shading colours on,
//! and for any colour picked off it, how to paint it over the flat base
//! with a blend mode and opacity. The maths is `symphony_core::lab`; this
//! file is its controls. Rendering runs on a background thread, and the UI
//! keeps showing the last image until the new one is ready.

use std::sync::mpsc::{channel, Receiver};

use eframe::egui::{self, Color32, Pos2, Sense, Stroke};
use symphony_core::lab::{self, Lab, Light, Shape, Surface, RECIPE_MODES};
use symphony_ui::{theme, widgets};

use super::colour_picker as picker;
use crate::session::Session;

/// The render's size in pixels; shown scaled to fit.
const RENDER: u32 = 360;
/// Space around the render, and so its corner radius (D6.3).
const INSET: i8 = 4;

#[derive(Default)]
pub struct LabState {
    pub lab: Lab,
    /// Whether the object colour has been set yet (it starts as the
    /// current colour the first time the Lab opens).
    seeded: bool,
    shown: Option<Shown>,
    job: Option<(Lab, Receiver<lab::Image>)>,
    /// The picked pixel.
    picked: Option<[u32; 2]>,
    /// Recipe opacity, 0..=1.
    opacity: Option<f32>,
}

struct Shown {
    lab: Lab,
    image: lab::Image,
    texture: egui::TextureHandle,
}

/// Where the colour editor leaves a colour to open the Lab with.
pub fn open_request() -> egui::Id {
    egui::Id::unique("open-colour-lab")
}

impl LabState {
    /// Light this colour.
    pub fn use_colour(&mut self, rgb: [u8; 3]) {
        self.lab.object = rgb;
        self.seeded = true;
        self.picked = None;
    }

    /// Screenshot mode: pick a pixel.
    pub fn pick(&mut self, pixel: [u32; 2]) {
        self.picked = Some(pixel);
    }

    /// Whether the current settings have been rendered.
    #[cfg(test)]
    pub fn ready(&self) -> bool {
        self.shown.as_ref().is_some_and(|s| s.lab == self.lab)
    }

    /// Start a render when the settings changed and none is running; take
    /// a finished one.
    fn update(&mut self, ctx: &egui::Context) {
        if let Some((_, rx)) = &self.job {
            match rx.try_recv() {
                Ok(image) => {
                    let (lab, _) = self.job.take().expect("job");
                    let pixels = image
                        .pixels
                        .iter()
                        .map(|&[r, g, b]| Color32::from_rgb(r, g, b))
                        .collect();
                    let colour =
                        egui::ColorImage::new([image.size as usize, image.size as usize], pixels);
                    let texture = match self.shown.take() {
                        Some(mut shown) => {
                            shown.texture.set(colour, egui::TextureOptions::LINEAR);
                            shown.texture
                        }
                        None => {
                            ctx.load_texture("colour-lab", colour, egui::TextureOptions::LINEAR)
                        }
                    };
                    self.shown = Some(Shown {
                        lab,
                        image,
                        texture,
                    });
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    ctx.request_repaint_after(std::time::Duration::from_millis(16));
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => self.job = None,
            }
        }
        let current = self.shown.as_ref().is_some_and(|s| s.lab == self.lab);
        if !current && self.job.is_none() {
            let lab = self.lab.clone();
            let (tx, rx) = channel();
            let ctx = ctx.clone();
            let job = lab.clone();
            std::thread::spawn(move || {
                let _ = tx.send(lab::render(&job, RENDER));
                ctx.request_repaint();
            });
            self.job = Some((lab, rx));
        }
    }
}

pub fn ui(ui: &mut egui::Ui, s: &mut Session, state: &mut LabState) {
    if !state.seeded {
        let [r, g, b, _] = s.doc.swatch_rgba(s.swatch);
        state.lab.object = [r, g, b];
        state.seeded = true;
    }
    state.update(ui.ctx());
    egui::Frame::new()
        .inner_margin(egui::Margin::same(INSET))
        .show(ui, |ui| {
            let wide = ui.available_width() >= 620.0;
            if wide {
                ui.horizontal_top(|ui| {
                    let side = (ui.available_width() - 340.0)
                        .min(ui.available_height())
                        .max(160.0);
                    picture(ui, state, side);
                    // What was picked comes first: it's what the Lab is for.
                    ui.vertical(|ui| {
                        egui::ScrollArea::vertical()
                            .id_salt("lab-side")
                            .show(ui, |ui| {
                                picked(ui, s, state);
                                ui.separator();
                                controls(ui, s, state);
                            });
                    });
                });
            } else {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    let side = ui.available_width().min(420.0);
                    picture(ui, state, side);
                    picked(ui, s, state);
                    controls(ui, s, state);
                });
            }
        });
}

/// The render. Click or drag on it to pick a colour.
fn picture(ui: &mut egui::Ui, state: &mut LabState, side: f32) {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(side, side), Sense::click_and_drag());
    let radius = theme::inside(theme::GLASS_RADIUS, INSET as u8);
    let Some(shown) = &state.shown else {
        ui.painter()
            .rect_filled(rect, radius, ui.visuals().extreme_bg_color);
        ui.put(rect, egui::Spinner::new());
        return;
    };
    egui::Image::new((shown.texture.id(), rect.size()))
        .corner_radius(radius)
        .paint_at(ui, rect);
    let to_pixel = |pos: Pos2| -> [u32; 2] {
        let k = RENDER as f32 / rect.width();
        let x = ((pos.x - rect.left()) * k).clamp(0.0, RENDER as f32 - 1.0);
        let y = ((pos.y - rect.top()) * k).clamp(0.0, RENDER as f32 - 1.0);
        [x as u32, y as u32]
    };
    if response.clicked() || response.dragged() {
        if let Some(pos) = response.interact_pointer_pos() {
            state.picked = Some(to_pixel(pos));
        }
    }
    let response = response.on_hover_text("Click to pick a colour and see how to paint it");
    if let Some(pos) = response.hover_pos() {
        let [x, y] = to_pixel(pos);
        if let Some((c, _)) = shown.image.at(x, y) {
            ui.painter().circle_filled(
                pos + egui::vec2(14.0, -14.0),
                7.0,
                Color32::from_rgb(c[0], c[1], c[2]),
            );
            ui.painter().circle_stroke(
                pos + egui::vec2(14.0, -14.0),
                7.0,
                Stroke::new(1.5, Color32::WHITE),
            );
        }
    }
    if let Some([x, y]) = state.picked {
        let k = rect.width() / RENDER as f32;
        let at = rect.min + egui::vec2((x as f32 + 0.5) * k, (y as f32 + 0.5) * k);
        ui.painter()
            .circle_stroke(at, 6.0, Stroke::new(3.0, Color32::BLACK));
        ui.painter()
            .circle_stroke(at, 6.0, Stroke::new(1.5, Color32::WHITE));
    }
}

fn hex([r, g, b]: [u8; 3]) -> String {
    format!("#{r:02X}{g:02X}{b:02X}")
}

fn chip(ui: &mut egui::Ui, rgb: [u8; 3], size: f32) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(size, size), Sense::hover());
    let [r, g, b] = rgb;
    picker::paint_chip(ui, rect, [r, g, b, 255], picker::CHIP_RADIUS);
    response
}

/// The picked colour: add it as a flat colour, or see how to paint it
/// over the base colour with each blend mode.
fn picked(ui: &mut egui::Ui, s: &mut Session, state: &mut LabState) {
    egui::Frame::new()
        .inner_margin(egui::Margin::same(theme::CARD_PAD - INSET))
        .show(ui, |ui| picked_body(ui, s, state));
}

fn picked_body(ui: &mut egui::Ui, s: &mut Session, state: &mut LabState) {
    let Some(shown) = &state.shown else { return };
    let Some(([x, y], (colour, surface))) = state
        .picked
        .and_then(|p| shown.image.at(p[0], p[1]).map(|hit| (p, hit)))
    else {
        ui.weak("Click the picture to pick a colour. You can add it to the palette, or see how to paint it over the base colour with a blend mode.");
        return;
    };
    let _ = (x, y);
    let (base, base_name) = match surface {
        Surface::Object => (shown.lab.object, "the object's colour"),
        Surface::Floor => (shown.lab.floor, "the floor's colour"),
    };
    ui.horizontal(|ui| {
        chip(ui, colour, 32.0);
        ui.vertical(|ui| {
            ui.strong(hex(colour));
            if ui
                .button("Add to palette")
                .on_hover_text("Add this colour as a flat swatch and make it current")
                .clicked()
            {
                s.add_colour(
                    "Add Lab colour",
                    hex(colour),
                    [colour[0], colour[1], colour[2], 255],
                );
            }
        });
    });
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.label("Or paint it over");
        chip(ui, base, 16.0);
        ui.label(base_name);
    });
    let mut percent = state.opacity.unwrap_or(1.0) * 100.0;
    if widgets::precise_slider(
        ui,
        egui::Slider::new(&mut percent, 1.0..=100.0)
            .step_by(1.0)
            .fixed_decimals(0)
            .suffix("%")
            .text("Opacity"),
    )
    .changed()
    {
        state.opacity = Some(percent / 100.0);
    }
    let opacity = percent / 100.0;
    let mut cannot = Vec::new();
    egui::Grid::new("lab-recipes")
        .num_columns(4)
        .spacing(egui::vec2(8.0, 4.0))
        .show(ui, |ui| {
            for mode in RECIPE_MODES {
                let r = lab::recipe(mode, base, colour, opacity);
                if !r.exact {
                    match lab::lowest_opacity(mode, base, colour) {
                        Some(p) => cannot.push(format!("{} needs at least {p}%", mode.title())),
                        None => cannot.push(format!("{} can't", mode.title())),
                    }
                    continue;
                }
                ui.label(mode.title());
                chip(ui, r.colour, 16.0).on_hover_text(hex(r.colour));
                ui.monospace(hex(r.colour));
                if ui
                    .small_button("Add")
                    .on_hover_text(format!(
                        "Add {} to the palette, to paint in a {} layer at {percent:.0}%",
                        hex(r.colour),
                        mode.title()
                    ))
                    .clicked()
                {
                    s.add_colour(
                        "Add Lab colour",
                        format!("{} {}", mode.title(), hex(r.colour)),
                        [r.colour[0], r.colour[1], r.colour[2], 255],
                    );
                }
                ui.end_row();
            }
        });
    if !cannot.is_empty() {
        ui.add_space(4.0);
        ui.weak(format!("At {percent:.0}%: {}.", cannot.join(", ")));
    }
}

/// Where a light comes from, seen from above: the viewer is at the
/// bottom, the centre is straight overhead. Drag the dot.
fn direction_dial(ui: &mut egui::Ui, light: &mut Light) {
    let size = 64.0;
    let (rect, response) = ui.allocate_exact_size(egui::vec2(size, size), Sense::click_and_drag());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Slider, true, "Light direction"));
    let centre = rect.center();
    let reach = size / 2.0 - 6.0;
    let p = ui.painter();
    p.circle_stroke(
        centre,
        reach,
        Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color),
    );
    // The object, and a tick where the viewer stands.
    p.circle_filled(centre, 5.0, ui.visuals().widgets.inactive.bg_fill);
    p.line_segment(
        [
            centre + egui::vec2(0.0, reach - 3.0),
            centre + egui::vec2(0.0, reach + 3.0),
        ],
        Stroke::new(1.5, ui.visuals().weak_text_color()),
    );
    if response.clicked() || response.dragged() {
        if let Some(pos) = response.interact_pointer_pos() {
            let v = (pos - centre) / reach;
            let distance = v.length().min(1.0);
            light.height = (1.0 - distance) * 90.0;
            if distance > 0.02 {
                light.around = v.x.atan2(v.y).to_degrees();
            }
        }
    }
    let distance = reach * (1.0 - light.height / 90.0);
    let (sa, ca) = light.around.to_radians().sin_cos();
    let dot = centre + egui::vec2(sa, ca) * distance;
    let [r, g, b] = light.colour;
    p.circle_filled(dot, 6.0, Color32::from_rgb(r, g, b));
    p.circle_stroke(dot, 6.0, Stroke::new(1.5, Color32::BLACK));
    response.on_hover_text(
        "Where the light comes from, seen from above. You stand at the bottom; the middle is \
         straight overhead, the top edge is behind the object.",
    );
}

fn light_controls(ui: &mut egui::Ui, light: &mut Light) {
    ui.horizontal(|ui| {
        direction_dial(ui, light);
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                picker::rgb(ui, &mut light.colour).on_hover_text("Light colour");
                ui.weak("Colour");
            });
            ui.spacing_mut().slider_width = 110.0;
            widgets::precise_slider(
                ui,
                egui::Slider::new(&mut light.strength, 0.0..=2.0).text("Strength"),
            );
            widgets::precise_slider(
                ui,
                egui::Slider::new(&mut light.height, 0.0..=90.0)
                    .step_by(1.0)
                    .suffix("°")
                    .text("Height"),
            );
        });
    });
}

fn heading(ui: &mut egui::Ui, text: &str) {
    ui.add_space(8.0);
    ui.label(
        egui::RichText::new(text)
            .font(theme::body())
            .color(theme::TEXT_PRIMARY),
    );
}

fn controls(ui: &mut egui::Ui, s: &mut Session, state: &mut LabState) {
    egui::Frame::new()
        .inner_margin(egui::Margin::same(theme::CARD_PAD - INSET))
        .show(ui, |ui| {
            let lab = &mut state.lab;
            ui.horizontal(|ui| {
                widgets::segmented(
                    ui,
                    "lab-shape",
                    &mut lab.shape,
                    &[(Shape::Sphere, "Sphere"), (Shape::Cube, "Cube")],
                    false,
                );
                let mut cel = lab.bands >= 2;
                if widgets::segmented(
                    ui,
                    "lab-shading",
                    &mut cel,
                    &[(false, "Smooth"), (true, "Cel")],
                    false,
                ) {
                    lab.bands = if cel { 2 } else { 0 };
                }
            });
            if lab.bands >= 2 {
                let mut tones = lab.bands;
                widgets::precise_slider(ui, egui::Slider::new(&mut tones, 2..=5).text("Tones"));
                lab.bands = tones;
            }

            heading(ui, "Object");
            ui.horizontal(|ui| {
                picker::rgb(ui, &mut lab.object).on_hover_text("The flat base colour");
                if ui
                    .button("Use current colour")
                    .on_hover_text("The colour selected in the Colour view")
                    .clicked()
                {
                    let [r, g, b, _] = s.doc.swatch_rgba(s.swatch);
                    lab.object = [r, g, b];
                }
            });
            widgets::precise_slider(
                ui,
                egui::Slider::new(&mut lab.highlight, 0.0..=1.0).text("Highlight"),
            );
            widgets::precise_slider(
                ui,
                egui::Slider::new(&mut lab.gloss, 0.0..=1.0).text("Gloss"),
            );

            heading(ui, "Main light");
            light_controls(ui, &mut lab.key);

            ui.add_space(8.0);
            ui.checkbox(&mut lab.second.on, "Second light")
                .on_hover_text(
                    "A rim or fill light, often a cool colour against a warm main light",
                );
            if lab.second.on {
                light_controls(ui, &mut lab.second);
            }

            heading(ui, "Surroundings");
            ui.horizontal(|ui| {
                picker::rgb(ui, &mut lab.ambient)
                    .on_hover_text("Sky colour: light from all around");
                ui.spacing_mut().slider_width = 110.0;
                widgets::precise_slider(
                    ui,
                    egui::Slider::new(&mut lab.ambient_strength, 0.0..=1.0).text("Sky light"),
                );
            });
            ui.horizontal(|ui| {
                picker::rgb(ui, &mut lab.floor).on_hover_text("Floor colour");
                ui.spacing_mut().slider_width = 110.0;
                widgets::precise_slider(
                    ui,
                    egui::Slider::new(&mut lab.bounce, 0.0..=1.0).text("Bounce"),
                )
                .on_hover_text("Light the floor throws back up onto the object");
            });
            ui.checkbox(&mut lab.shadows, "Cast shadows");
            ui.add_space(8.0);
            if ui.button("Reset lights").clicked() {
                let object = lab.object;
                *lab = Lab {
                    object,
                    shape: lab.shape,
                    ..Lab::default()
                };
            }
        });
}
