//! Terminal actions and process/log status.

use super::shortcuts::Action;
use super::{Button, CORNER_RADIUS, text_button};
use crate::app::ButtonBuilder;
use egui::{Color32, RichText, Vec2};
use egui_material3::get_global_color;

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
        egui::Frame::new()
            .fill(get_global_color("surfaceContainerLow"))
            .corner_radius(CORNER_RADIUS)
            .inner_margin(egui::Margin::symmetric(8, 4))
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.spacing_mut().item_spacing = Vec2::new(6.0, 4.0);
                ui.spacing_mut().interact_size.y = 32.0;
                ui.horizontal_wrapped(|ui| {
                    if ui
                        .add(toolbar_button("Appearance"))
                        .on_hover_text(
                            "Change this tab's font and colors; save as the default for new tabs\nCtrl+mouse wheel over the terminal changes its font size",
                        )
                        .clicked()
                    {
                        if let Some(index) = active {
                            self.workspace.select_tab(index);
                        }
                        self.appearance_open = true;
                    }
                    copy = ui
                        .add_enabled(active.is_some(), toolbar_button("Copy"))
                        .on_hover_text("Copy selected text, or all output (Ctrl+Shift+C)\nDrag over terminal text to select and copy automatically")
                        .clicked();
                    interrupt = ui
                        .add_enabled(running, toolbar_button("Ctrl+C"))
                        .on_hover_text("Send SIGINT to the foreground job; programs can ignore it")
                        .clicked();
                    stop = ui
                        .add_enabled(running, toolbar_button("Stop"))
                        .on_hover_text("Force termination with SIGKILL; discard pending output")
                        .clicked();
                    close = ui
                        .add_enabled(active.is_some(), toolbar_button("Close tab"))
                        .on_hover_text(format!("Close this tab and stop its process ({})", ui.ctx().format_shortcut(&Action::CloseTab.shortcut())))
                        .clicked();
                    if active.is_some_and(|index| self.workspace.tabs()[index].scrollback > 0) {
                        live = ui.add(toolbar_button("Jump to live")).clicked();
                    }
                });
            });
        if let Some(index) = active {
            if copy || interrupt || stop || live || close {
                self.workspace.select_tab(index);
            }
            let tab = &mut self.workspace.tabs_mut()[index];
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

fn toolbar_button(caption: &str) -> Button {
    text_button(RichText::new(caption).size(14.0))
        .small()
        .min_size(Vec2::new(0.0, 32.0))
}
