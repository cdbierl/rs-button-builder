//! Terminal pane geometry, divider interaction, and viewport composition.
use super::super::workspace::Split;
use crate::app::ButtonBuilder;
use egui::{Rect, Sense, Stroke, UiBuilder, Vec2};
use egui_material3::get_global_color;

impl ButtonBuilder {
    pub(in crate::app) fn terminal_workspace(&mut self, ui: &mut egui::Ui) {
        let Some(split) = self.workspace.split() else {
            self.terminal_panel(ui, 0);
            return;
        };
        let rect = ui.available_rect_before_wrap();
        let vertical_divider = split == Split::Right;
        let span = if vertical_divider {
            rect.width()
        } else {
            rect.height()
        };
        let usable = (span - 8.0).max(1.0);
        let minimum = (if vertical_divider { 180.0 } else { 150.0 } / usable).min(0.45);
        self.workspace.set_split_fraction(
            self.workspace
                .split_fraction()
                .clamp(minimum, 1.0 - minimum),
        );
        let divider_at = usable * self.workspace.split_fraction();
        let divider = if vertical_divider {
            Rect::from_min_size(
                rect.min + Vec2::new(divider_at, 0.0),
                Vec2::new(8.0, rect.height()),
            )
        } else {
            Rect::from_min_size(
                rect.min + Vec2::new(0.0, divider_at),
                Vec2::new(rect.width(), 8.0),
            )
        };
        let response = ui.interact(divider, ui.id().with("terminal_divider"), Sense::drag());
        if response.hovered() || response.dragged() {
            ui.ctx().set_cursor_icon(if vertical_divider {
                egui::CursorIcon::ResizeHorizontal
            } else {
                egui::CursorIcon::ResizeVertical
            });
        }
        if response.dragged()
            && let Some(pointer) = response.interact_pointer_pos()
        {
            let offset = if vertical_divider {
                pointer.x - rect.left()
            } else {
                pointer.y - rect.top()
            };
            self.workspace
                .set_split_fraction(((offset - 4.0) / usable).clamp(minimum, 1.0 - minimum));
            ui.ctx().request_repaint();
        }
        let regions = if vertical_divider {
            [
                Rect::from_min_max(rect.min, egui::pos2(divider.left(), rect.bottom())),
                Rect::from_min_max(egui::pos2(divider.right(), rect.top()), rect.max),
            ]
        } else {
            [
                Rect::from_min_max(rect.min, egui::pos2(rect.right(), divider.top())),
                Rect::from_min_max(egui::pos2(rect.left(), divider.bottom()), rect.max),
            ]
        };
        for (pane, region) in regions.into_iter().enumerate() {
            let mut child = ui.new_child(
                UiBuilder::new()
                    .id_salt(("terminal_region", pane))
                    .max_rect(region.shrink(8.0)),
            );
            child.set_clip_rect(ui.clip_rect().intersect(region));
            if child.rect_contains_pointer(region)
                && child.input(|input| input.pointer.any_pressed())
            {
                self.workspace.focus_pane(pane);
            }
            self.terminal_panel(&mut child, pane);
            if self.workspace.focused_pane() == pane {
                ui.painter().rect_stroke(
                    region,
                    super::CORNER_RADIUS,
                    Stroke::new(2.0_f32, get_global_color("primary")),
                    egui::StrokeKind::Inside,
                );
            }
        }
        let color = get_global_color(if response.hovered() || response.dragged() {
            "primary"
        } else {
            "outlineVariant"
        });
        ui.painter().line_segment(
            if vertical_divider {
                [divider.center_top(), divider.center_bottom()]
            } else {
                [divider.left_center(), divider.right_center()]
            },
            Stroke::new(2.0_f32, color),
        );
        ui.advance_cursor_after_rect(rect);
    }
}
