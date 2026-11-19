//! Scrollable terminal tab strip and previous/next controls.

use super::super::workspace::{PaneAction, Split};
use super::shortcuts::Action;
use super::style::{contrasting_text, rgb};
use super::terminal_workspace::{DIVIDER_WIDTH, MIN_PANE_SIZE, PANE_PADDING};
use super::{Button, outlined_button, text_button};
use crate::app::ButtonBuilder;
use egui::{Color32, FontId, RichText, Vec2};
use egui_material3::get_global_color;

impl ButtonBuilder {
    pub(super) fn terminal_tabs(&mut self, ui: &mut egui::Ui, pane: usize) {
        let mut selected = None;
        let mut pane_action = None;
        let pane_size = ui.available_size()
            + if self.workspace.split().is_some() {
                Vec2::splat(2.0 * PANE_PADDING)
            } else {
                Vec2::ZERO
            };
        let active = self.workspace.pane_tab(pane);
        let indices: Vec<_> = self
            .workspace
            .tabs()
            .iter()
            .enumerate()
            .filter_map(|(index, tab)| (tab.pane() == pane).then_some(index))
            .collect();
        let bar_height = 40.0;
        ui.allocate_ui_with_layout(
            Vec2::new(ui.available_width(), bar_height),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.set_min_height(bar_height);
                ui.strong("Terminal");
                ui.add_space(6.0);
                let previous = active.and_then(|index| {
                    indices
                        .iter()
                        .copied()
                        .take_while(|&other| other < index)
                        .last()
                });
                if ui
                    .add_enabled(previous.is_some(), tab_arrow("‹"))
                    .on_hover_text(format!(
                        "Previous tab ({})",
                        ui.ctx().format_shortcut(&Action::PreviousTab.shortcut())
                    ))
                    .clicked()
                {
                    selected = previous;
                }
                let strip_width =
                    (ui.available_width() - 32.0 - ui.spacing().item_spacing.x).max(1.0);
                let resized = ui.data_mut(|data| {
                    let id = ui.id().with("terminal_tabs_width");
                    let previous = data.get_temp::<f32>(id);
                    data.insert_temp(id, strip_width);
                    previous != Some(strip_width)
                });
                ui.allocate_ui_with_layout(
                    Vec2::new(strip_width, bar_height),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        let tabs_rect = ui.max_rect();
                        egui::ScrollArea::horizontal()
                            .animated(false)
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
                                    for (index, tab) in self.workspace.tabs().iter().enumerate() {
                                        if tab.pane() != pane {
                                            continue;
                                        }
                                        let active = active == Some(index);
                                        let running = tab.terminal.is_running();
                                        let (text, fill) = if active {
                                            let custom_fill = tab
                                                .button_index
                                                .and_then(|index| self.config.buttons.get(index))
                                                .and_then(|button| button.bg_color)
                                                .map(rgb);
                                            if let Some(fill) = custom_fill {
                                                (contrasting_text(fill), fill)
                                            } else {
                                                (
                                                    get_global_color("onPrimary"),
                                                    get_global_color("primary"),
                                                )
                                            }
                                        } else {
                                            (
                                                get_global_color("onSurfaceVariant"),
                                                get_global_color("surfaceContainerLow"),
                                            )
                                        };
                                        let title = format!("{} · {}", tab.id(), tab.label);
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
                                            .fill(fill)
                                            .text_color(text);
                                        let response = ui
                                            .push_id(tab.id(), |ui| {
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
                                            if running {
                                                if active {
                                                    text
                                                } else {
                                                    get_global_color("primary")
                                                }
                                            } else {
                                                get_global_color("outline")
                                            },
                                        );
                                        if active
                                            && (resized || self.workspace.reveal_requested(pane))
                                        {
                                            response.scroll_to_me(Some(egui::Align::Center));
                                        }
                                        egui::Popup::context_menu(&response)
                                            .close_behavior(
                                                egui::PopupCloseBehavior::CloseOnClickOutside,
                                            )
                                            .show(|ui| {
                                                if let Some(action) =
                                                    self.pane_menu(ui, index, pane_size)
                                                {
                                                    pane_action = Some(action);
                                                    ui.close();
                                                }
                                            });
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
                let next =
                    active.and_then(|index| indices.iter().copied().find(|&other| other > index));
                if ui
                    .add_enabled(next.is_some(), tab_arrow("›"))
                    .on_hover_text(format!(
                        "Next tab ({})",
                        ui.ctx().format_shortcut(&Action::NextTab.shortcut())
                    ))
                    .clicked()
                {
                    selected = next;
                }
            },
        );
        self.workspace.acknowledge_tab_reveal(pane);
        if let Some(index) = selected {
            self.workspace.select_tab(index);
        }
        if let Some(action) = pane_action {
            self.workspace.queue_pane_action(action);
        }
    }

    fn pane_menu(&self, ui: &mut egui::Ui, index: usize, size: Vec2) -> Option<PaneAction> {
        let tab = &self.workspace.tabs()[index];
        let mut action = None;
        for (label, direction) in [("Split right", Split::Right), ("Split below", Split::Below)] {
            let fits = match direction {
                Split::Right => {
                    size.x >= 2.0 * MIN_PANE_SIZE.x + DIVIDER_WIDTH && size.y >= MIN_PANE_SIZE.y
                }
                Split::Below => {
                    size.y >= 2.0 * MIN_PANE_SIZE.y + DIVIDER_WIDTH && size.x >= MIN_PANE_SIZE.x
                }
            };
            if ui
                .add_enabled(
                    self.workspace.can_split(index) && fits,
                    text_button(label).small(),
                )
                .on_disabled_hover_text("Requires another tab in this pane and room for both panes")
                .clicked()
            {
                action = Some(PaneAction::Split {
                    tab: tab.id(),
                    direction,
                });
            }
        }
        if self.workspace.split().is_some() {
            ui.separator();
            ui.menu_button("Move to pane", |ui| {
                for (position, destination) in self.workspace.pane_ids().into_iter().enumerate() {
                    if destination == tab.pane() {
                        continue;
                    }
                    let label = self
                        .workspace
                        .pane_tab(destination)
                        .map(|index| self.workspace.tabs()[index].label.as_str())
                        .unwrap_or("Empty");
                    if ui
                        .add(text_button(format!("Pane {} · {label}", position + 1)).small())
                        .clicked()
                    {
                        action = Some(PaneAction::Move {
                            tab: tab.id(),
                            pane: destination,
                        });
                        ui.close();
                    }
                }
            });
            if ui.add(text_button("Merge all panes").small()).clicked() {
                action = Some(PaneAction::Merge);
            }
        }
        action
    }
}

fn tab_arrow(caption: &str) -> Button {
    outlined_button(RichText::new(caption).size(18.0))
        .small()
        .min_size(Vec2::splat(32.0))
}
