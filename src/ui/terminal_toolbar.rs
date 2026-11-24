//! Terminal actions and process/log status.

use super::shortcuts::Action;
use super::{Button, CORNER_RADIUS, text_button};
use crate::app::ButtonBuilder;
use egui::{Color32, FontId, RichText, Vec2};
use egui_material3::get_global_color;

const ICON_BUTTON_WIDTH: f32 = 30.0;

impl ButtonBuilder {
    pub(super) fn terminal_toolbar(&mut self, ui: &mut egui::Ui, pane: usize) {
        let active = self.workspace.pane_tab(pane);
        let running =
            active.is_some_and(|index| self.workspace.tabs()[index].terminal.is_running());
        let mut copy = false;
        let mut interrupt = false;
        let mut stop = false;
        let mut close = false;
        let mut live = false;
        let line_wrap = active.is_some_and(|index| self.workspace.tabs()[index].line_wrap);
        let display_paused =
            active.is_some_and(|index| self.workspace.tabs()[index].display_paused());
        let can_jump_to_live =
            active.is_some_and(|index| self.workspace.tabs()[index].scrollback > 0);
        let mut toggle_pause = false;
        let mut wrap_changed = false;
        ui.scope(|ui| {
            ui.set_width(super::clipped_available_width(ui));
            egui::Frame::new()
                .fill(get_global_color("surfaceContainerLow"))
                .corner_radius(CORNER_RADIUS)
                .inner_margin(egui::Margin::symmetric(8, 4))
                .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.spacing_mut().item_spacing = Vec2::new(6.0, 4.0);
                ui.spacing_mut().interact_size.y = 32.0;
                if let Some(font) = ui.style_mut().text_styles.get_mut(&egui::TextStyle::Body) {
                    font.size = 14.0;
                }
                ui.horizontal(|ui| {
                    ui.horizontal(|ui| {
                        if ui
                            .add(toolbar_button(symbol("tune")))
                            .on_hover_text(
                                "Appearance: change font and colors for this tab and new tabs\nCtrl+wheel to resize text",
                            )
                            .clicked()
                        {
                            if let Some(index) = active {
                                self.workspace.select_tab(index);
                            }
                            self.appearance_open = true;
                        }
                        interrupt = ui
                            .add_enabled(running, toolbar_button("Ctrl+C"))
                            .on_hover_text("Send SIGINT; the foreground job may ignore it")
                            .clicked();
                        stop = ui
                            .add_enabled(running, toolbar_button(symbol("stop_circle")))
                            .on_hover_text("Stop: send SIGKILL and discard unread output")
                            .clicked();
                        close = ui
                            .add_enabled(active.is_some(), toolbar_button(symbol("close")))
                            .on_hover_text(format!(
                                "Close tab and stop process ({})",
                                ui.ctx().format_shortcut(&Action::CloseTab.shortcut())
                            ))
                            .clicked();
                    });
                    let spacing = ui.spacing().item_spacing.x;
                    let available =
                        (ui.max_rect().right() - ui.next_widget_position().x).max(0.0);
                    let visible_controls =
                        (((available + spacing) / (ICON_BUTTON_WIDTH + spacing)) as usize).min(4);
                    let trailing_width = visible_controls as f32 * ICON_BUTTON_WIDTH
                        + visible_controls.saturating_sub(1) as f32 * spacing;
                    ui.add_space((available - trailing_width).max(0.0));
                    ui.horizontal(|ui| {
                        if visible_controls >= 3 {
                            live = ui
                                .add_enabled(
                                    can_jump_to_live,
                                    toolbar_button(symbol("vertical_align_bottom")),
                                )
                                .on_hover_text(if can_jump_to_live {
                                    "Jump to live output"
                                } else {
                                    "Already showing live output"
                                })
                                .clicked();
                        }
                        if visible_controls >= 1 {
                            toggle_pause = ui
                                .add_enabled(
                                    active.is_some(),
                                    toolbar_button(symbol(if display_paused {
                                        "play_arrow"
                                    } else {
                                        "pause"
                                    })),
                                )
                                .on_hover_text(if display_paused {
                                    "Resume updates to this terminal pane"
                                } else {
                                    "Pause pane updates while the process and logging continue"
                                })
                                .clicked();
                        }
                        if visible_controls >= 2 {
                            copy = ui
                                .add_enabled(
                                    active.is_some(),
                                    toolbar_button(symbol("content_copy")),
                                )
                                .on_hover_text("Copy selection or all output (Ctrl+Shift+C)")
                                .clicked();
                        }
                        if visible_controls >= 4 {
                            wrap_changed = ui
                                .add_enabled(
                                    active.is_some(),
                                    toggle_button(symbol("wrap_text"), line_wrap),
                                )
                                .on_hover_text(if line_wrap {
                                    "Line wrap is on; click to turn it off for this tab"
                                } else {
                                    "Line wrap is off; click to turn it on for this tab"
                                })
                                .clicked();
                        }
                    });
                });
                });
        });
        if let Some(index) = active {
            if copy || interrupt || stop || live || close || toggle_pause || wrap_changed {
                self.workspace.select_tab(index);
            }
            let tab = &mut self.workspace.tabs_mut()[index];
            if toggle_pause {
                tab.set_display_paused(!display_paused);
            }
            if wrap_changed {
                tab.set_line_wrap(!line_wrap);
            }
            if copy {
                ui.ctx().copy_text(tab.copy_text());
            }
            if interrupt {
                tab.terminal.interrupt();
            }
            if stop {
                tab.terminal.stop();
            }
            if live {
                tab.scrollback = 0;
                tab.scroll_fraction = 0.0;
            }
            if close {
                self.workspace.close_tab(index);
            }
        }
        let active = self.workspace.pane_tab(pane);
        let mut status = active
            .map(|index| self.workspace.tabs()[index].terminal.status.as_str())
            .unwrap_or("Ready · Click a button to open a terminal tab")
            .to_owned();
        let log_error = active.and_then(|index| self.workspace.tabs()[index].terminal.log_error());
        if let Some(error) = log_error {
            status.push_str(&format!(" · Log error: {error}"));
        }
        let mut tooltip = status.clone();
        if let Some(path) =
            active.and_then(|index| self.workspace.tabs()[index].terminal.log_path())
        {
            tooltip.push_str(&format!("\nLog: {}", path.display()));
        }
        let text = RichText::new(status).size(14.0);
        let text = if log_error.is_some() {
            text.color(Color32::LIGHT_RED)
        } else {
            text
        };
        ui.add(egui::Label::new(text).truncate())
            .on_hover_text(tooltip);
    }
}

fn toolbar_button(caption: impl Into<egui::WidgetText>) -> Button {
    text_button(caption).small().min_size(Vec2::new(0.0, 32.0))
}

fn toggle_button(caption: impl Into<egui::WidgetText>, selected: bool) -> Button {
    let button = toolbar_button(caption);
    if selected {
        button
            .fill(get_global_color("primaryContainer"))
            .text_color(get_global_color("onPrimaryContainer"))
    } else {
        button
    }
}

fn symbol(name: &str) -> RichText {
    RichText::new(egui_material3::material_symbol::material_symbol_text(name)).font(FontId::new(
        18.0,
        egui::FontFamily::Name(crate::fonts::MATERIAL_SYMBOLS.into()),
    ))
}
