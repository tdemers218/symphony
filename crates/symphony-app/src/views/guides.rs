//! Stage-space reference overlays shared by Camera and Drawing.
use eframe::egui::{self, Pos2, Stroke, Vec2};
use symphony_core::math::Affine;
use symphony_ui::{theme, widgets};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Perspective {
    #[default]
    Off,
    OnePoint,
    TwoPoint,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Guides {
    pub perspective: Perspective,
    horizon: f32,
    centre: f32,
    spread: f32,
    axes: [bool; 2],
}

impl Default for Guides {
    fn default() -> Self {
        Self {
            perspective: Perspective::Off,
            horizon: 0.0,
            centre: 0.0,
            spread: 0.7,
            axes: [false; 2],
        }
    }
}

impl Guides {
    pub(super) fn vanishing_points(&self) -> impl Iterator<Item = [f32; 2]> {
        let x = self.centre;
        let y = self.horizon;
        match self.perspective {
            Perspective::Off => [None, None],
            Perspective::OnePoint => [Some([x, y]), None],
            Perspective::TwoPoint => [Some([x - self.spread, y]), Some([x + self.spread, y])],
        }
        .into_iter()
        .flatten()
    }

    pub fn menu(&mut self, ui: &mut egui::Ui) {
        let active = self.perspective != Perspective::Off || self.axes.contains(&true);
        ui.menu_button(if active { "Guides: on" } else { "Guides" }, |ui| {
            ui.set_min_width(176.0);
            ui.label("Perspective");
            widgets::segmented(
                ui,
                "guide-perspective",
                &mut self.perspective,
                &[
                    (Perspective::Off, "Off"),
                    (Perspective::OnePoint, "One point"),
                    (Perspective::TwoPoint, "Two points"),
                ],
                false,
            );
            if self.perspective != Perspective::Off {
                ui.add(egui::Slider::new(&mut self.horizon, -0.5..=0.5).text("Horizon"));
                ui.add(egui::Slider::new(&mut self.centre, -1.0..=1.0).text("Centre"));
                if self.perspective == Perspective::TwoPoint {
                    ui.add(egui::Slider::new(&mut self.spread, 0.15..=2.0).text("Spread"));
                }
            }
            ui.add_space(8.0);
            ui.label("Centre lines");
            widgets::checkbox(ui, &mut self.axes[0], "Horizontal");
            widgets::checkbox(ui, &mut self.axes[1], "Vertical");
            if ui.button("Reset guide settings").clicked() {
                let perspective = self.perspective;
                let axes = self.axes;
                *self = Self {
                    perspective,
                    axes,
                    ..Self::default()
                };
            }
        });
    }

    pub fn paint(&self, painter: &egui::Painter, view: Affine, width: f32, height: f32) {
        let screen = |p: [f32; 2]| {
            let (x, y) = view.apply(p[0] * width, p[1] * height);
            Pos2::new(x, y)
        };
        self.paint_projected(painter, screen);
    }

    pub(super) fn paint_projected(
        &self,
        painter: &egui::Painter,
        screen: impl Fn([f32; 2]) -> Pos2,
    ) {
        let line = |a, b, ink| painter.line_segment([screen(a), screen(b)], ink);
        let strong = Stroke::new(1.0, theme::ACCENT_SELECT.gamma_multiply(0.8));
        let faint = Stroke::new(1.0, theme::ACCENT_SELECT.gamma_multiply(0.38));
        if self.axes[0] {
            line([-0.5, 0.0], [0.5, 0.0], strong);
        }
        if self.axes[1] {
            line([0.0, -0.5], [0.0, 0.5], strong);
        }
        if self.perspective == Perspective::Off {
            return;
        }
        line([-0.5, self.horizon], [0.5, self.horizon], strong);
        for point in self.vanishing_points() {
            for index in 0..=8 {
                let x = index as f32 / 8.0 - 0.5;
                line(point, [x, -0.5], faint);
                line(point, [x, 0.5], faint);
            }
            let centre = screen(point);
            painter.rect_filled(
                egui::Rect::from_center_size(centre, Vec2::splat(6.0)),
                2.0,
                theme::ACCENT_SELECT,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn perspective_points_share_the_horizon_and_can_extend_offstage() {
        let guide = Guides {
            perspective: Perspective::TwoPoint,
            horizon: -0.2,
            centre: 0.1,
            spread: 0.8,
            ..Default::default()
        };
        let points: Vec<_> = guide.vanishing_points().collect();
        assert_eq!(points.len(), 2);
        assert!(points[0][0] < -0.5 && points[1][0] > 0.5);
        assert!(points.iter().all(|point| point[1] == guide.horizon));
    }
}
