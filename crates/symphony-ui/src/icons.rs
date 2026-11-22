//! Symphony's own icon set (docs/DESIGN.md §7). Drawn in code on a 16×16
//! grid with 1.5-unit round strokes, so icons stay sharp at any UI scale.
//! No emoji, no Unicode symbols, no icon fonts (D7.1).

use std::f32::consts::PI;

use egui::{self, Color32, Painter, Pos2, Rect, Response, Sense, Shape, Stroke, Vec2};

use crate::theme;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    Select,
    Transform,
    Brush,
    Pencil,
    Eraser,
    Paint,
    Dropper,
    Hand,
    Zoom,
    FirstFrame,
    PrevFrame,
    Play,
    Pause,
    NextFrame,
    LastFrame,
    Eye,
    EyeOff,
    Onion,
    Fit,
    RotateLeft,
    RotateRight,
    Plus,
    Trash,
    ZoomOut,
    Forbidden,
    Minus,
    CloseGap,
    ArtPreview,
    ArtOverlay,
    ArtLine,
    ArtColour,
    ArtUnderlay,
}

impl Icon {
    pub const ALL: [Icon; 32] = [
        Icon::Select,
        Icon::Transform,
        Icon::Brush,
        Icon::Pencil,
        Icon::Eraser,
        Icon::Paint,
        Icon::Dropper,
        Icon::Hand,
        Icon::Zoom,
        Icon::FirstFrame,
        Icon::PrevFrame,
        Icon::Play,
        Icon::Pause,
        Icon::NextFrame,
        Icon::LastFrame,
        Icon::Eye,
        Icon::EyeOff,
        Icon::Onion,
        Icon::Fit,
        Icon::RotateLeft,
        Icon::RotateRight,
        Icon::Plus,
        Icon::Trash,
        Icon::ZoomOut,
        Icon::Forbidden,
        Icon::Minus,
        Icon::CloseGap,
        Icon::ArtPreview,
        Icon::ArtOverlay,
        Icon::ArtLine,
        Icon::ArtColour,
        Icon::ArtUnderlay,
    ];
}

/// Maps 16×16 grid units into a screen rect.
struct Grid {
    origin: Pos2,
    unit: f32,
    colour: Color32,
}

impl Grid {
    fn p(&self, x: f32, y: f32) -> Pos2 {
        self.origin + Vec2::new(x, y) * self.unit
    }
    fn pts(&self, xy: &[(f32, f32)]) -> Vec<Pos2> {
        xy.iter().map(|&(x, y)| self.p(x, y)).collect()
    }
    fn stroke(&self) -> Stroke {
        Stroke::new(1.5 * self.unit, self.colour)
    }
    fn line(&self, painter: &Painter, xy: &[(f32, f32)]) {
        // Round each vertex explicitly: the default polyline uses pointed joins.
        for edge in xy.windows(2) {
            painter.line_segment(
                [self.p(edge[0].0, edge[0].1), self.p(edge[1].0, edge[1].1)],
                self.stroke(),
            );
        }
        for &(x, y) in xy {
            self.dot(painter, x, y, 0.75);
        }
    }
    fn closed(&self, painter: &Painter, xy: &[(f32, f32)]) {
        self.line(painter, xy);
        if let (Some(&(x, y)), Some(&(u, v))) = (xy.first(), xy.last()) {
            painter.line_segment([self.p(x, y), self.p(u, v)], self.stroke());
        }
    }
    fn fill(&self, painter: &Painter, xy: &[(f32, f32)]) {
        painter.add(Shape::convex_polygon(
            self.pts(xy),
            self.colour,
            Stroke::NONE,
        ));
    }
    fn circle(&self, painter: &Painter, x: f32, y: f32, r: f32) {
        painter.circle_stroke(self.p(x, y), r * self.unit, self.stroke());
    }
    fn dot(&self, painter: &Painter, x: f32, y: f32, r: f32) {
        painter.circle_filled(self.p(x, y), r * self.unit, self.colour);
    }
}

fn eye_outline(g: &Grid, painter: &Painter) {
    let n = 12;
    let mut pts = Vec::with_capacity(2 * n + 2);
    for i in 0..=n {
        let t = i as f32 / n as f32;
        pts.push((2.0 + 12.0 * t, 8.0 - 4.2 * (PI * t).sin()));
    }
    for i in (1..n).rev() {
        let t = i as f32 / n as f32;
        pts.push((2.0 + 12.0 * t, 8.0 + 4.2 * (PI * t).sin()));
    }
    g.closed(painter, &pts);
    g.dot(painter, 8.0, 8.0, 1.75);
}

/// Paint `icon` filling `rect` (square) in `colour`.
pub fn paint(painter: &Painter, rect: Rect, icon: Icon, colour: Color32) {
    let side = rect.width().min(rect.height());
    let g = Grid {
        origin: rect.center() - Vec2::splat(side / 2.0),
        unit: side / 16.0,
        colour,
    };
    let p = painter;
    match icon {
        Icon::Select => g.closed(
            p,
            &[
                (4.0, 2.5),
                (4.0, 13.0),
                (6.8, 10.4),
                (9.0, 14.0),
                (10.8, 13.0),
                (8.7, 9.5),
                (12.5, 9.2),
            ],
        ),
        Icon::Transform => {
            g.closed(p, &[(4.0, 4.0), (12.0, 4.0), (12.0, 12.0), (4.0, 12.0)]);
            for (x, y) in [(4.0, 4.0), (12.0, 4.0), (12.0, 12.0), (4.0, 12.0)] {
                g.fill(
                    p,
                    &[
                        (x - 1.6, y - 1.6),
                        (x + 1.6, y - 1.6),
                        (x + 1.6, y + 1.6),
                        (x - 1.6, y + 1.6),
                    ],
                );
            }
        }
        Icon::Brush => {
            g.closed(
                p,
                &[
                    (7.1, 7.8),
                    (12.7, 2.4),
                    (13.5, 2.5),
                    (13.7, 3.3),
                    (9.1, 9.5),
                ],
            );
            g.closed(
                p,
                &[
                    (7.0, 8.2),
                    (8.8, 9.8),
                    (8.0, 11.6),
                    (6.4, 12.8),
                    (4.3, 13.3),
                    (2.4, 13.1),
                    (3.7, 11.8),
                    (4.3, 9.9),
                    (5.4, 8.7),
                ],
            );
        }
        Icon::Pencil => {
            g.closed(
                p,
                &[
                    (2.2, 13.8),
                    (3.0, 10.1),
                    (10.8, 2.3),
                    (12.0, 2.0),
                    (14.0, 4.0),
                    (13.7, 5.2),
                    (5.9, 13.0),
                ],
            );
            g.line(p, &[(9.4, 3.7), (12.3, 6.6)]);
        }
        Icon::Eraser => {
            g.closed(
                p,
                &[
                    (2.0, 9.4),
                    (9.3, 2.1),
                    (14.0, 6.8),
                    (7.4, 13.4),
                    (5.9, 13.4),
                ],
            );
            g.line(p, &[(5.9, 5.5), (10.5, 10.1)]);
            g.line(p, &[(7.2, 13.5), (14.0, 13.5)]);
        }
        Icon::Paint => {
            g.closed(p, &[(2.4, 8.0), (7.4, 3.0), (12.2, 7.8), (7.0, 13.0)]);
            g.line(p, &[(4.1, 2.1), (7.4, 5.4)]);
            g.line(p, &[(3.2, 8.8), (11.2, 8.8)]);
            g.fill(
                p,
                &[
                    (13.0, 9.6),
                    (14.4, 12.2),
                    (14.1, 13.3),
                    (13.0, 13.8),
                    (11.9, 13.3),
                    (11.6, 12.2),
                ],
            );
        }
        Icon::Dropper => {
            g.closed(
                p,
                &[
                    (2.0, 14.0),
                    (2.8, 10.5),
                    (8.8, 4.5),
                    (11.5, 7.2),
                    (5.5, 13.2),
                ],
            );
            g.line(p, &[(7.5, 3.8), (12.1, 8.4)]);
            g.line(p, &[(10.0, 5.8), (12.6, 3.2)]);
            g.dot(p, 12.2, 3.8, 2.3);
        }
        Icon::Hand => {
            // Side-on thumb and grouped fingers leave a single open palm at 16 px.
            g.closed(
                p,
                &[
                    (6.4, 13.4),
                    (2.2, 8.3),
                    (2.3, 7.5),
                    (3.2, 7.2),
                    (5.2, 8.9),
                    (4.8, 4.2),
                    (5.3, 3.4),
                    (6.1, 3.5),
                    (6.8, 7.0),
                    (6.8, 2.7),
                    (7.5, 2.1),
                    (8.3, 2.6),
                    (8.6, 6.7),
                    (8.9, 3.4),
                    (9.6, 2.9),
                    (10.3, 3.5),
                    (10.5, 7.0),
                    (11.0, 5.2),
                    (11.8, 4.8),
                    (12.6, 5.5),
                    (12.9, 9.1),
                    (12.4, 11.3),
                    (11.0, 13.4),
                ],
            );
        }
        Icon::Zoom => {
            g.circle(p, 7.0, 7.0, 4.2);
            g.line(p, &[(10.2, 10.2), (13.6, 13.6)]);
            g.line(p, &[(5.0, 7.0), (9.0, 7.0)]);
            g.line(p, &[(7.0, 5.0), (7.0, 9.0)]);
        }
        Icon::ZoomOut => {
            g.circle(p, 7.0, 7.0, 4.2);
            g.line(p, &[(10.2, 10.2), (13.6, 13.6)]);
            g.line(p, &[(5.0, 7.0), (9.0, 7.0)]);
        }
        Icon::Forbidden => {
            g.circle(p, 8.0, 8.0, 5.5);
            g.line(p, &[(4.1, 11.9), (11.9, 4.1)]);
        }
        Icon::FirstFrame => {
            g.line(p, &[(3.5, 3.5), (3.5, 12.5)]);
            g.fill(p, &[(12.5, 3.5), (12.5, 12.5), (5.5, 8.0)]);
        }
        Icon::PrevFrame => g.line(p, &[(10.5, 3.2), (5.6, 8.0), (10.5, 12.8)]),
        Icon::Play => g.fill(p, &[(4.5, 2.8), (4.5, 13.2), (13.0, 8.0)]),
        Icon::Pause => {
            g.fill(p, &[(4.0, 3.0), (6.8, 3.0), (6.8, 13.0), (4.0, 13.0)]);
            g.fill(p, &[(9.2, 3.0), (12.0, 3.0), (12.0, 13.0), (9.2, 13.0)]);
        }
        Icon::NextFrame => g.line(p, &[(5.5, 3.2), (10.4, 8.0), (5.5, 12.8)]),
        Icon::LastFrame => {
            g.fill(p, &[(3.5, 3.5), (3.5, 12.5), (10.5, 8.0)]);
            g.line(p, &[(12.5, 3.5), (12.5, 12.5)]);
        }
        Icon::Eye => eye_outline(&g, p),
        Icon::EyeOff => {
            g.line(
                p,
                &[
                    (2.2, 7.4),
                    (4.7, 9.8),
                    (7.8, 10.7),
                    (11.1, 9.9),
                    (13.8, 7.4),
                ],
            );
            for (x, y, dx) in [(4.5, 9.9, -1.1), (8.0, 10.8, 0.0), (11.5, 9.9, 1.1)] {
                g.line(p, &[(x, y), (x + dx, y + 1.8)]);
            }
        }
        // Three overlapping sheets: onion skin.
        Icon::Onion => {
            g.closed(p, &[(2.2, 2.2), (10.4, 2.2), (10.4, 10.4), (2.2, 10.4)]);
            g.line(p, &[(12.6, 4.8), (12.6, 12.6), (4.8, 12.6)]);
            g.line(p, &[(14.8, 7.2), (14.8, 14.8), (7.2, 14.8)]);
        }
        Icon::Fit => {
            for (x, y, dx, dy) in [
                (2.5, 2.5, 1.0, 1.0),
                (13.5, 2.5, -1.0, 1.0),
                (2.5, 13.5, 1.0, -1.0),
                (13.5, 13.5, -1.0, -1.0),
            ] {
                g.line(p, &[(x, y + 3.5 * dy), (x, y), (x + 3.5 * dx, y)]);
            }
            g.closed(p, &[(6.0, 6.0), (10.0, 6.0), (10.0, 10.0), (6.0, 10.0)]);
        }
        Icon::RotateLeft | Icon::RotateRight => {
            let direction = if icon == Icon::RotateRight { 1.0 } else { -1.0 };
            let arc: Vec<_> = (0..=20)
                .map(|i| {
                    let angle = (65.0 + 245.0 * i as f32 / 20.0).to_radians();
                    (8.0 + direction * 5.0 * angle.cos(), 8.5 + 5.0 * angle.sin())
                })
                .collect();
            g.line(p, &arc);
            g.line(
                p,
                &[
                    (8.0 + direction * 3.3, 2.1),
                    (8.0 + direction * 3.3, 4.8),
                    (8.0 + direction * 0.6, 4.8),
                ],
            );
        }
        Icon::Minus => g.line(p, &[(3.0, 8.0), (13.0, 8.0)]),
        // Two line ends with a dotted bridge between them.
        Icon::CloseGap => {
            g.line(p, &[(1.5, 12.0), (3.0, 9.5), (5.0, 7.0)]);
            g.line(p, &[(11.0, 7.0), (13.2, 5.0), (14.5, 2.8)]);
            g.dot(p, 5.0, 7.0, 1.35);
            g.dot(p, 11.0, 7.0, 1.35);
            g.dot(p, 8.0, 7.0, 0.65);
        }
        // Art layers: a stack of three sheets, the current one filled.
        Icon::ArtPreview | Icon::ArtOverlay | Icon::ArtUnderlay => {
            let sheet = |y: f32| [(2.0, y), (10.5, y), (14.0, y - 3.5), (5.5, y - 3.5)];
            for (i, y) in [6.0, 9.5, 13.0].into_iter().enumerate() {
                let filled = matches!((icon, i), (Icon::ArtOverlay, 0) | (Icon::ArtUnderlay, 2));
                if filled {
                    g.fill(p, &sheet(y));
                }
                g.closed(p, &sheet(y));
            }
        }
        // Line Art: an outline. Colour Art: the paint inside it.
        Icon::ArtLine => g.circle(p, 8.0, 8.0, 5.0),
        Icon::ArtColour => {
            g.dot(p, 8.0, 8.0, 5.0);
        }
        Icon::Plus => {
            g.line(p, &[(8.0, 3.0), (8.0, 13.0)]);
            g.line(p, &[(3.0, 8.0), (13.0, 8.0)]);
        }
        Icon::Trash => {
            g.line(p, &[(2.5, 4.5), (13.5, 4.5)]);
            g.line(p, &[(6.0, 4.5), (6.5, 2.5), (9.5, 2.5), (10.0, 4.5)]);
            g.closed(p, &[(4.0, 4.5), (12.0, 4.5), (11.2, 13.5), (4.8, 13.5)]);
            g.line(p, &[(6.8, 7.0), (6.8, 11.0)]);
            g.line(p, &[(9.2, 7.0), (9.2, 11.0)]);
        }
    }
}

/// A toolbar icon button (D6.2: 24 px target, 16 px icon). `active` shows
/// the on state (D7.3). Disabled buttons still show their tooltip.
pub fn button(
    ui: &mut egui::Ui,
    icon: Icon,
    active: bool,
    enabled: bool,
    tooltip: &str,
) -> Response {
    sized_button(
        ui,
        icon,
        theme::TOOLBAR_BUTTON,
        16.0,
        active,
        enabled,
        tooltip,
    )
}

/// A Tools-toolbar button (D6.2: 32 px pen target, 20 px icon).
pub fn tool_button(
    ui: &mut egui::Ui,
    icon: Icon,
    active: bool,
    enabled: bool,
    tooltip: &str,
) -> Response {
    sized_button(
        ui,
        icon,
        theme::TOOL_BUTTON,
        theme::TOOL_ICON,
        active,
        enabled,
        tooltip,
    )
}

fn sized_button(
    ui: &mut egui::Ui,
    icon: Icon,
    size: f32,
    icon_size: f32,
    active: bool,
    enabled: bool,
    tooltip: &str,
) -> Response {
    let enabled = enabled && ui.is_enabled();
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(size), sense);
    let painter = ui.painter();
    let transition = |key, on| {
        if ui.style().animation_time > 0.0 {
            ui.ctx().animate_bool_responsive(response.id.with(key), on)
        } else {
            f32::from(on)
        }
    };
    let hover = transition("hover", enabled && response.hovered());
    let selected = transition("selected", enabled && active);
    let bg = egui::Color32::TRANSPARENT
        .lerp_to_gamma(theme::BG_HOVER, hover)
        .lerp_to_gamma(theme::ACCENT_SELECT_FILL, selected);
    let radius = if size == theme::TOOL_BUTTON {
        if selected > 0.0 {
            painter.add(
                egui::epaint::Shadow {
                    offset: [0, 0],
                    blur: 8,
                    spread: 0,
                    color: theme::ACCENT_SELECT.gamma_multiply(0.22 * selected),
                }
                .as_shape(rect, theme::GLASS_CONTROL_RADIUS),
            );
        }
        theme::GLASS_CONTROL_RADIUS
    } else {
        theme::RADIUS_CONTROL
    };
    painter.rect_filled(rect, radius, bg);
    let fg = if enabled {
        theme::TEXT_PRIMARY.lerp_to_gamma(theme::ON_ACCENT, selected)
    } else {
        theme::TEXT_DISABLED
    };
    paint(
        painter,
        Rect::from_center_size(rect.center(), Vec2::splat(icon_size)),
        icon,
        fg,
    );
    theme::focus_ring(ui, &response);
    response
        .widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, enabled, active, tooltip));
    response.on_hover_text(tooltip)
}
