//! Design tokens and the egui theme built from them (docs/DESIGN.md §4–6).
//! UI code uses these tokens only. A colour literal anywhere else is a bug (D4.1).

use std::sync::Arc;

use egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Margin, Stroke,
    TextStyle, Vec2,
};

const fn hex(rgb: u32) -> Color32 {
    Color32::from_rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
}

// §4.1 colour tokens.
pub const BG_BASE: Color32 = hex(0x17181A);
pub const BG_PANEL: Color32 = hex(0x242628);
pub const BG_RAISED: Color32 = hex(0x303336);
pub const BG_HOVER: Color32 = hex(0x3C4044);
pub const BG_INPUT: Color32 = hex(0x1D1F21);
pub const LINE_SUBTLE: Color32 = hex(0x393C40);
pub const LINE_STRONG: Color32 = hex(0x595F66);
/// Edges of controls that would otherwise vanish (checkbox, radio): 3:1 or more on every surface (D4.4).
pub const LINE_CONTROL: Color32 = hex(0x949CA4);
pub const TEXT_PRIMARY: Color32 = hex(0xE8EAEC);
pub const TEXT_SECONDARY: Color32 = hex(0xB5BAC0);
pub const TEXT_DISABLED: Color32 = hex(0x747980);
pub const ON_ACCENT: Color32 = hex(0xFFFFFF);
pub const ACCENT_SELECT: Color32 = hex(0x81B4F8);
pub const ACCENT_SELECT_FILL: Color32 = hex(0x3D63B8);
pub const ACCENT_TIME: Color32 = hex(0xF0A63A);
pub const STATE_DANGER: Color32 = hex(0xE86A62);
pub const STATE_OK: Color32 = hex(0x62C484);
pub const CANVAS_SURROUND: Color32 = hex(0x464749);
/// The empty stage: a light neutral grey, not white (Tristan, 2026-11-22),
/// so a blank scene doesn't glare and white artwork still shows. Black
/// ink on it is 11:1. Exports still render on white paper.
pub const CANVAS_STAGE: Color32 = hex(0xD4D5D7);
/// Onion skin tints for previous and next drawings (DESIGN §4.3). Defaults
/// follow the red-before, green-after convention; to verify against Harmony 27.
pub const ONION_BEFORE: Color32 = hex(0xE0483E);
pub const ONION_AFTER: Color32 = hex(0x2FA35A);

// Function view curves: one hue per peg function, readable on BG_INPUT.
pub const FUNCTION_X: Color32 = hex(0xEE7A6B);
pub const FUNCTION_Y: Color32 = hex(0x6CCB8E);
pub const FUNCTION_ANGLE: Color32 = hex(0x6FA8F5);
pub const FUNCTION_SCALE_X: Color32 = hex(0xC99AF2);
pub const FUNCTION_SCALE_Y: Color32 = hex(0x5CCFD0);
/// Neutral shadow under the stage (D3.10) and floating windows (D6.4).
pub const SHADOW: Color32 = Color32::from_rgba_premultiplied(0, 0, 0, 150);

// §5.2 type scale.
pub const SIZE_DENSE: f32 = 11.0;
pub const SIZE_BODY: f32 = 13.0;
pub const SIZE_HEADING: f32 = 15.0;

// §6.2 standard sizes.
pub const TOP_BAR: f32 = 40.0;
pub const TAB_ROW: f32 = 32.0;
pub const STATUS_BAR: f32 = 24.0;
pub const TOOLBAR_BUTTON: f32 = 24.0;
pub const TOOL_BUTTON: f32 = 32.0;
pub const TOOL_ICON: f32 = 20.0;
pub const TOOL_GROUP_GAP: f32 = 12.0;
pub const TOOLS_STRIP: f32 = 44.0;
pub const ROW: f32 = 28.0;
pub const XSHEET_ROW: f32 = 20.0;
pub const FRAME_CELL: f32 = 16.0;
pub const CARD_PAD: i8 = 12;
pub const GUTTER: f32 = 6.0;

// Workspace glass surfaces composite over the cached wallpaper.
pub const GLASS_PANEL: Color32 = Color32::from_rgba_premultiplied(21, 22, 25, 179);
pub const GLASS_CHROME: Color32 = Color32::from_rgba_premultiplied(14, 15, 17, 158);
pub const GLASS_EDGE: Color32 = Color32::from_rgba_premultiplied(23, 23, 23, 23);
pub const GLASS_RADIUS: u8 = 12;
pub const GLASS_TOOL_INSET: i8 = 6;
pub const GLASS_CONTROL_RADIUS: u8 = GLASS_RADIUS - GLASS_TOOL_INSET as u8;
const GLASS_TAB_INSET: i8 = 5;

// §6.3 radii.
pub const RADIUS_CONTROL: u8 = 4;
pub const RADIUS_CARD: u8 = 8;
pub const RADIUS_FLOATING: u8 = 8;

pub fn dense() -> FontId {
    FontId::proportional(SIZE_DENSE)
}

pub fn body() -> FontId {
    FontId::proportional(SIZE_BODY)
}

/// Digits in time grids line up (D5.1).
pub fn numbers() -> FontId {
    FontId::monospace(SIZE_DENSE)
}

fn fonts() -> FontDefinitions {
    let mut f = FontDefinitions::default();
    f.font_data.insert(
        "Inter".into(),
        Arc::new(FontData::from_static(include_bytes!(
            "../assets/fonts/Inter.ttf"
        ))),
    );
    f.font_data.insert(
        "JetBrainsMono".into(),
        Arc::new(FontData::from_static(include_bytes!(
            "../assets/fonts/JetBrainsMono.ttf"
        ))),
    );
    f.families
        .entry(FontFamily::Proportional)
        .or_default()
        .insert(0, "Inter".into());
    f.families
        .entry(FontFamily::Monospace)
        .or_default()
        .insert(0, "JetBrainsMono".into());
    f
}

fn widget(bg: Color32, fg: Color32, stroke: Stroke) -> egui::style::WidgetVisuals {
    egui::style::WidgetVisuals {
        bg_fill: bg,
        weak_bg_fill: bg,
        bg_stroke: stroke,
        corner_radius: CornerRadius::same(RADIUS_CONTROL),
        fg_stroke: Stroke::new(1.0, fg),
        expansion: 0.0,
    }
}

/// Shared duration for controls, menus and windows (DESIGN D9).
pub fn set_motion(ctx: &egui::Context, enabled: bool) {
    ctx.clear_animations();
    ctx.all_styles_mut(|style| style.animation_time = if enabled { 0.12 } else { 0.0 });
}

/// One inner rim on the same rounded rectangle as the glass fill.
pub fn glass_rim(painter: &egui::Painter, rect: egui::Rect) {
    painter.rect_stroke(
        rect,
        GLASS_RADIUS,
        Stroke::new(1.0, GLASS_EDGE),
        egui::StrokeKind::Inside,
    );
    let inset = f32::from(GLASS_RADIUS);
    if rect.width() > inset * 2.0 {
        let y = rect.top() + 0.5;
        painter.hline(
            rect.left() + inset..=rect.right() - inset,
            y,
            Stroke::new(1.0, Color32::from_white_alpha(15)),
        );
    }
}

/// Paint a keyboard focus ring inside a custom control's target.
pub(crate) fn focus_ring(ui: &egui::Ui, response: &egui::Response) {
    if response.enabled() && response.has_focus() {
        ui.painter().rect_stroke(
            response.rect,
            RADIUS_CONTROL,
            Stroke::new(1.0, ACCENT_SELECT),
            egui::StrokeKind::Inside,
        );
    }
}

pub fn apply(ctx: &egui::Context) {
    ctx.set_fonts(fonts());
    ctx.all_styles_mut(|style| {
        style.text_styles = [
            (TextStyle::Small, FontId::proportional(SIZE_DENSE)),
            (TextStyle::Body, FontId::proportional(SIZE_BODY)),
            (TextStyle::Button, FontId::proportional(SIZE_BODY)),
            (TextStyle::Heading, FontId::proportional(SIZE_HEADING)),
            (TextStyle::Monospace, FontId::monospace(SIZE_DENSE)),
        ]
        .into();

        let s = &mut style.spacing;
        s.item_spacing = Vec2::new(8.0, 4.0);
        s.button_padding = Vec2::new(8.0, 4.0);
        s.interact_size = Vec2::new(24.0, 20.0);
        s.window_margin = Margin::same(CARD_PAD);
        s.menu_margin = Margin::same(4);

        let v = &mut style.visuals;
        v.dark_mode = true;
        v.override_text_color = None;
        v.panel_fill = BG_PANEL;
        v.window_fill = BG_PANEL;
        v.window_shadow = egui::epaint::Shadow {
            offset: [0, 5],
            blur: 16,
            spread: 0,
            color: hex(0).gamma_multiply(0.40),
        };
        v.popup_shadow = egui::epaint::Shadow {
            offset: [0, 3],
            blur: 10,
            spread: 0,
            color: hex(0).gamma_multiply(0.32),
        };
        v.window_stroke = Stroke::new(1.0, GLASS_EDGE);
        v.window_corner_radius = CornerRadius::same(GLASS_RADIUS);
        v.menu_corner_radius = CornerRadius::same(RADIUS_FLOATING);
        v.extreme_bg_color = BG_INPUT;
        v.faint_bg_color = BG_RAISED;
        v.code_bg_color = BG_INPUT;
        v.hyperlink_color = ACCENT_SELECT;
        v.warn_fg_color = ACCENT_TIME;
        v.error_fg_color = STATE_DANGER;
        v.selection.bg_fill = ACCENT_SELECT_FILL;
        v.selection.stroke = Stroke::new(1.0, ON_ACCENT);
        v.widgets.noninteractive = widget(BG_PANEL, TEXT_PRIMARY, Stroke::new(1.0, LINE_SUBTLE));
        v.widgets.inactive = widget(BG_RAISED, TEXT_PRIMARY, Stroke::NONE);
        v.widgets.hovered = widget(BG_HOVER, TEXT_PRIMARY, Stroke::new(1.0, LINE_STRONG));
        v.widgets.active = widget(BG_HOVER, TEXT_PRIMARY, Stroke::new(1.0, ACCENT_SELECT));
        v.widgets.open = widget(BG_HOVER, TEXT_PRIMARY, Stroke::new(1.0, LINE_STRONG));
        // Sliders: the filled part shows the value at a glance, and the handle is a round knob.
        v.slider_trailing_fill = true;
        v.handle_shape = egui::style::HandleShape::Circle;
    });
}

#[cfg(feature = "dock")]
/// Dock styling (DESIGN §3.1): every view is a card on `bg.base` with
/// 6 px gutters. Tabs are flat, with the active one on a raised tone and the
/// focused view's tab outlined in cel blue (D3.2). No close or collapse
/// buttons (D3.4).
pub fn dock_style(style: &egui::Style) -> egui_dock::Style {
    use egui_dock::TabInteractionStyle;
    let tab_radius = CornerRadius::same(GLASS_RADIUS - GLASS_TAB_INSET as u8);
    let tab = |bg: Color32, text: Color32, outline: Color32| TabInteractionStyle {
        outline_color: outline,
        corner_radius: tab_radius,
        bg_fill: bg,
        text_color: text,
    };

    let mut d = egui_dock::Style::from_egui(style);
    d.dock_area_padding = Some(Margin::same(GUTTER as i8));
    d.main_surface_border_stroke = Stroke::NONE;
    d.main_surface_border_rounding = CornerRadius::ZERO;
    d.separator.width = GUTTER;
    d.separator.extra = 0.0;
    d.separator.color_idle = Color32::TRANSPARENT;
    d.separator.color_hovered = LINE_STRONG;
    d.separator.color_dragged = ACCENT_SELECT;
    d.tab_bar.bg_fill = Color32::TRANSPARENT;
    d.tab_bar.height = TAB_ROW;
    d.tab_bar.corner_radius = CornerRadius::ZERO;
    d.tab_bar.hline_color = Color32::TRANSPARENT;
    d.tab_bar.show_scroll_bar_on_overflow = false;
    d.tab_bar.inner_margin = Margin::same(GLASS_TAB_INSET);
    d.tab.active = tab(BG_RAISED, TEXT_PRIMARY, Color32::TRANSPARENT);
    d.tab.inactive = tab(Color32::TRANSPARENT, TEXT_SECONDARY, Color32::TRANSPARENT);
    d.tab.hovered = tab(BG_HOVER, TEXT_PRIMARY, Color32::TRANSPARENT);
    d.tab.focused = tab(BG_RAISED, TEXT_PRIMARY, ACCENT_SELECT);
    d.tab.active_with_kb_focus = tab(BG_RAISED, TEXT_PRIMARY, ACCENT_SELECT);
    d.tab.inactive_with_kb_focus = tab(Color32::TRANSPARENT, TEXT_PRIMARY, ACCENT_SELECT);
    d.tab.focused_with_kb_focus = tab(BG_RAISED, TEXT_PRIMARY, ACCENT_SELECT);
    d.tab.hline_below_active_tab_name = false;
    d.tab.tab_body.bg_fill = Color32::TRANSPARENT;
    d.tab.tab_body.stroke = Stroke::NONE;
    d.tab.tab_body.corner_radius = CornerRadius::ZERO;
    // Rectangular content stays inside the rounded outer corners.
    d.tab.tab_body.inner_margin = Margin {
        bottom: 4,
        left: 4,
        right: 4,
        top: 0,
    };
    d.overlay.selection_color = ACCENT_SELECT.gamma_multiply(0.25);
    d
}
