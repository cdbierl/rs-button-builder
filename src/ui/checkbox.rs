//! Material checkbox with correctly blended interaction overlays.

use egui::{Color32, Pos2, Rect, Response, Sense, Stroke, Vec2, Widget};
use egui_material3::get_global_color;

pub(crate) struct Checkbox<'a> {
    checked: &'a mut bool,
    text: &'a str,
}

impl<'a> Checkbox<'a> {
    pub(crate) fn new(checked: &'a mut bool, text: &'a str) -> Self {
        Self { checked, text }
    }
}

impl Widget for Checkbox<'_> {
    fn ui(self, ui: &mut egui::Ui) -> Response {
        const BOX_SIZE: f32 = 18.0;
        const SPACING: f32 = 4.0;

        let enabled = ui.is_enabled();
        let on_surface = get_global_color("onSurface");
        let disabled = on_surface.gamma_multiply(0.38);
        let text_color = if enabled { on_surface } else { disabled };
        let text = ui
            .painter()
            .layout_no_wrap(self.text.to_owned(), body_font(ui), text_color);
        let (rect, mut response) = ui.allocate_exact_size(
            Vec2::new(BOX_SIZE + SPACING + text.size().x, 24.0),
            Sense::click(),
        );
        if enabled && response.clicked() {
            *self.checked = !*self.checked;
            response.mark_changed();
        }

        let primary = get_global_color("primary");
        let box_rect = Rect::from_min_size(
            Pos2::new(rect.min.x, rect.center().y - BOX_SIZE / 2.0),
            Vec2::splat(BOX_SIZE),
        );
        let (fill, border, check) = if !enabled {
            if *self.checked {
                (disabled, Color32::TRANSPARENT, disabled)
            } else {
                (Color32::TRANSPARENT, disabled, disabled)
            }
        } else if *self.checked {
            (primary, Color32::TRANSPARENT, get_global_color("onPrimary"))
        } else {
            (
                Color32::TRANSPARENT,
                get_global_color(if response.hovered() {
                    "onSurface"
                } else {
                    "onSurfaceVariant"
                }),
                on_surface,
            )
        };

        ui.painter().rect_filled(box_rect, 2.0, fill);
        if !*self.checked {
            ui.painter().rect_stroke(
                box_rect,
                2.0,
                Stroke::new(2.0_f32, border),
                egui::StrokeKind::Outside,
            );
        } else {
            let center = box_rect.center();
            let size = BOX_SIZE * 0.6;
            let middle = Pos2::new(center.x - size * 0.1, center.y + size * 0.2);
            ui.painter().line_segment(
                [Pos2::new(center.x - size * 0.3, center.y), middle],
                Stroke::new(2.0_f32, check),
            );
            ui.painter().line_segment(
                [
                    middle,
                    Pos2::new(center.x + size * 0.3, center.y - size * 0.2),
                ],
                Stroke::new(2.0_f32, check),
            );
        }
        ui.painter().galley(
            Pos2::new(
                box_rect.max.x + SPACING,
                rect.center().y - text.size().y / 2.0,
            ),
            text,
            text_color,
        );

        if enabled
            && (response.hovered() || response.has_focus() || response.is_pointer_button_down_on())
        {
            let color = if *self.checked { primary } else { on_surface };
            let alpha = if response.has_focus() || response.is_pointer_button_down_on() {
                25
            } else {
                20
            };
            ui.painter()
                .circle_filled(box_rect.center(), 20.0, overlay(color, alpha));
        }

        response
    }
}

fn body_font(ui: &egui::Ui) -> egui::FontId {
    ui.style()
        .text_styles
        .get(&egui::TextStyle::Body)
        .cloned()
        .unwrap_or_default()
}

fn overlay(color: Color32, alpha: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), alpha)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interaction_overlay_uses_valid_alpha_blending() {
        let color = Color32::from_rgb(250, 245, 240);
        assert_ne!(
            overlay(color, 20),
            Color32::from_rgba_premultiplied(color.r(), color.g(), color.b(), 20)
        );
    }
}
