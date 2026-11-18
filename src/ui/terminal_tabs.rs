//! Scrollable terminal tab strip and previous/next controls.

use super::{outlined_button, text_button};
use crate::app::ButtonBuilder;
use egui::{Color32, FontId, RichText, Vec2};
use egui_material3::{MaterialButton, get_global_color};

impl ButtonBuilder {
    pub(super) fn terminal_tabs(&mut self, ui: &mut egui::Ui) {
        let mut selected = None;
        let bar_height = 40.0;
        ui.allocate_ui_with_layout(
            Vec2::new(ui.available_width(), bar_height),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.set_min_height(bar_height);
                ui.strong("Terminal");
                ui.add_space(6.0);
                let previous = self.active_tab.and_then(|index| index.checked_sub(1));
                if ui
                    .add_enabled(previous.is_some(), tab_arrow("‹"))
                    .on_hover_text("Previous terminal tab")
                    .clicked()
                {
                    selected = previous;
                }
                let strip_width =
                    (ui.available_width() - 32.0 - ui.spacing().item_spacing.x).max(1.0);
                ui.allocate_ui_with_layout(
                    Vec2::new(strip_width, bar_height),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        let tabs_rect = ui.max_rect();
                        egui::ScrollArea::horizontal()
                            .id_salt("terminal_tabs")
                            .scroll_bar_visibility(
                                egui::scroll_area::ScrollBarVisibility::AlwaysHidden,
                            )
                            .auto_shrink([false, true])
                            .show(ui, |ui| {
                                ui.set_clip_rect(ui.clip_rect().intersect(tabs_rect));
                                ui.spacing_mut().item_spacing.x = 6.0;
                                ui.spacing_mut().interact_size.y = bar_height;
                                ui.horizontal(|ui| {
                                    for (index, tab) in self.tabs.iter().enumerate() {
                                        let active = self.active_tab == Some(index);
                                        let running = tab.terminal.is_running();
                                        let title = format!("{} · {}", tab.id, tab.label);
                                        let width = (ui
                                            .painter()
                                            .layout_no_wrap(
                                                title.clone(),
                                                FontId::proportional(14.0),
                                                Color32::WHITE,
                                            )
                                            .size()
                                            .x
                                            + 24.0)
                                            .clamp(100.0, 180.0);
                                        let button = text_button(RichText::new(&title).size(14.0))
                                            .small()
                                            .truncate()
                                            .min_size(Vec2::new(0.0, 32.0))
                                            .fill(get_global_color(if active {
                                                "primary"
                                            } else {
                                                "surfaceContainerLow"
                                            }))
                                            .text_color(get_global_color(if active {
                                                "onPrimary"
                                            } else {
                                                "onSurfaceVariant"
                                            }));
                                        let response = ui
                                            .push_id(tab.id, |ui| {
                                                ui.add_sized([width, 32.0], button)
                                            })
                                            .inner;
                                        let dot = egui::pos2(
                                            response.rect.left() + 7.0,
                                            response.rect.top() + 6.0,
                                        );
                                        ui.painter().circle_filled(
                                            dot,
                                            2.5,
                                            get_global_color(if running {
                                                if active { "onPrimary" } else { "primary" }
                                            } else {
                                                "outline"
                                            }),
                                        );
                                        if active && self.reveal_active_tab {
                                            response.scroll_to_me(Some(egui::Align::Center));
                                        }
                                        if response
                                            .on_hover_text(format!(
                                                "{title}\n{}\n{}",
                                                tab.command, tab.terminal.status
                                            ))
                                            .clicked()
                                        {
                                            selected = Some(index);
                                        }
                                    }
                                });
                            });
                    },
                );
                let next = self
                    .active_tab
                    .map(|index| index + 1)
                    .filter(|&index| index < self.tabs.len());
                if ui
                    .add_enabled(next.is_some(), tab_arrow("›"))
                    .on_hover_text("Next terminal tab")
                    .clicked()
                {
                    selected = next;
                }
            },
        );
        self.reveal_active_tab = false;
        if let Some(index) = selected {
            self.select_tab(index);
        }
    }
}

fn tab_arrow(caption: &str) -> MaterialButton<'static> {
    outlined_button(RichText::new(caption).size(18.0))
        .small()
        .min_size(Vec2::splat(32.0))
}
