//! Phosphor Regular and the animation-specific symbols in DESIGN §7.

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

pub const PHOSPHOR_LICENSE: &str = include_str!("../assets/phosphor/LICENSE");

fn phosphor_glyph(icon: Icon) -> Option<char> {
    Some(match icon {
        Icon::Select => '\u{e1dc}',
        Icon::Transform => '\u{e6ce}',
        Icon::Brush => '\u{e6f0}',
        Icon::Pencil => '\u{e3b4}',
        Icon::Eraser => '\u{e21e}',
        Icon::Paint => '\u{e392}',
        Icon::Dropper => '\u{e568}',
        Icon::Hand => '\u{e57e}',
        Icon::Zoom => '\u{e310}',
        Icon::FirstFrame => '\u{e5a4}',
        Icon::PrevFrame => '\u{e138}',
        Icon::Play => '\u{e3d0}',
        Icon::Pause => '\u{e39e}',
        Icon::NextFrame => '\u{e13a}',
        Icon::LastFrame => '\u{e5a6}',
        Icon::Eye => '\u{e220}',
        Icon::EyeOff => '\u{e224}',
        Icon::Onion => '\u{e466}',
        Icon::Fit => '\u{e1d0}',
        Icon::RotateLeft => '\u{e038}',
        Icon::RotateRight => '\u{e036}',
        Icon::Plus => '\u{e3d4}',
        Icon::Trash => '\u{e4a6}',
        Icon::ZoomOut => '\u{e30e}',
        Icon::Forbidden => '\u{e3de}',
        Icon::Minus => '\u{e32a}',
        Icon::ArtPreview => '\u{e468}',
        Icon::ArtLine => '\u{e18a}',
        Icon::ArtColour => '\u{e18c}',
        Icon::CloseGap | Icon::ArtOverlay | Icon::ArtUnderlay => return None,
    })
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
    fn dot(&self, painter: &Painter, x: f32, y: f32, r: f32) {
        painter.circle_filled(self.p(x, y), r * self.unit, self.colour);
    }
}

/// Paint `icon` filling `rect` (square) in `colour`.
pub fn paint(painter: &Painter, rect: Rect, icon: Icon, colour: Color32) {
    let side = rect.width().min(rect.height());
    if let Some(glyph) = phosphor_glyph(icon) {
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            glyph,
            egui::FontId::new(side, theme::phosphor_family()),
            colour,
        );
        return;
    }
    let g = Grid {
        origin: rect.center() - Vec2::splat(side / 2.0),
        unit: side / 16.0,
        colour,
    };
    let p = painter;
    match icon {
        // Two line ends with a dotted bridge between them.
        Icon::CloseGap => {
            g.line(p, &[(1.5, 12.0), (3.0, 9.5), (5.0, 7.0)]);
            g.line(p, &[(11.0, 7.0), (13.2, 5.0), (14.5, 2.8)]);
            g.dot(p, 5.0, 7.0, 1.35);
            g.dot(p, 11.0, 7.0, 1.35);
            g.dot(p, 8.0, 7.0, 0.65);
        }
        // Art layers: a stack of three sheets, the current one filled.
        Icon::ArtOverlay | Icon::ArtUnderlay => {
            let sheet = |y: f32| [(2.0, y), (10.5, y), (14.0, y - 3.5), (5.5, y - 3.5)];
            for (i, y) in [6.0, 9.5, 13.0].into_iter().enumerate() {
                let filled = matches!((icon, i), (Icon::ArtOverlay, 0) | (Icon::ArtUnderlay, 2));
                if filled {
                    g.fill(p, &sheet(y));
                }
                g.closed(p, &sheet(y));
            }
        }
        _ => unreachable!("Phosphor icons are drawn above"),
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
    painter.rect_filled(rect, theme::RADIUS_CONTROL as f32, bg);
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
