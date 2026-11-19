//! Shared proportional scrollbars for terminal history and wide output.

use super::CORNER_RADIUS;
use egui::{Id, Rect, Vec2};
use egui_material3::get_global_color;

pub(in crate::app) const TERMINAL_SCROLLBAR_WIDTH: f32 = 20.0;

pub(super) struct Scrollbar {
    pub id: Id,
    pub rect: Rect,
    pub horizontal: bool,
    pub visible: f32,
    pub total: f32,
}

impl Scrollbar {
    pub fn show(&self, ui: &mut egui::Ui, offset: &mut f32) -> bool {
        let grab_id = self.id.with("grab_offset");
        let max = (self.total - self.visible).max(0.0);
        *offset = offset.clamp(0.0, max);
        let track = self.rect.shrink2(if self.horizontal {
            Vec2::new(0.0, 4.0)
        } else {
            Vec2::new(4.0, 0.0)
        });
        let length = if self.horizontal {
            track.width()
        } else {
            track.height()
        };
        if max == 0.0 || length <= 0.0 {
            ui.ctx().data_mut(|data| data.remove::<f32>(grab_id));
            return false;
        }
        let thumb_length = (length * self.visible / self.total).max(24.0).min(length);
        let travel = length - thumb_length;
        let position = |point: egui::Pos2| if self.horizontal { point.x } else { point.y };
        let thumb = |offset: f32| {
            Rect::from_min_size(
                track.min
                    + if self.horizontal {
                        Vec2::new(travel * offset / max, 0.0)
                    } else {
                        Vec2::new(0.0, travel * offset / max)
                    },
                if self.horizontal {
                    Vec2::new(thumb_length, track.height())
                } else {
                    Vec2::new(track.width(), thumb_length)
                },
            )
        };
        let response = ui.interact(self.rect, self.id, egui::Sense::click_and_drag());
        let interacting =
            response.is_pointer_button_down_on() || response.dragged() || response.clicked();
        if interacting && let Some(pointer) = response.interact_pointer_pos() {
            let current = thumb(*offset);
            let grab = ui.ctx().data_mut(|data| {
                *data.get_temp_mut_or_insert_with(grab_id, || {
                    if current.contains(pointer) {
                        position(pointer) - position(current.min)
                    } else {
                        thumb_length / 2.0
                    }
                })
            });
            if travel > 0.0 {
                *offset = ((position(pointer) - grab - position(track.min)) / travel)
                    .clamp(0.0, 1.0)
                    * max;
            }
        }
        if !ui.input(|input| input.pointer.any_down()) {
            ui.ctx().data_mut(|data| data.remove::<f32>(grab_id));
        }
        if response.hovered() || interacting {
            ui.ctx().set_cursor_icon(if interacting {
                egui::CursorIcon::Grabbing
            } else {
                egui::CursorIcon::Grab
            });
        }
        ui.painter().rect_filled(
            track,
            CORNER_RADIUS,
            get_global_color("surfaceContainerHighest"),
        );
        ui.painter().rect_filled(
            thumb(*offset),
            CORNER_RADIUS,
            get_global_color(if response.hovered() || interacting {
                "primary"
            } else {
                "outline"
            }),
        );
        interacting
    }
}
