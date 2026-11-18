//! Layout heading, grid sizing, and command buttons.

use super::{styled_button, text_button};
use crate::app::ButtonBuilder;
use crate::layout_config::{DEFAULT_LAYOUT_NAME, MAX_GRID_COLUMNS, MAX_GRID_ROWS};
use egui::{RichText, Stroke};
use egui_material3::{MaterialCheckbox, get_global_color};

impl ButtonBuilder {
    fn layout_heading(&mut self, ui: &mut egui::Ui) {
        let Some(draft) = &mut self.layout_name_edit else {
            if ui
                .add(
                    egui::Label::new(RichText::new(self.config.display_name()).heading())
                        .sense(egui::Sense::click()),
                )
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text("Click to rename this layout")
                .clicked()
            {
                self.layout_name_edit = Some(self.config.display_name().to_owned());
                self.focus_layout_name = true;
            }
            return;
        };
        let id = ui.make_persistent_id("layout_name");
        if self.focus_layout_name {
            ui.memory_mut(|memory| memory.request_focus(id));
            let mut state = egui::TextEdit::load_state(ui.ctx(), id).unwrap_or_default();
            state
                .cursor
                .set_char_range(Some(egui::text::CCursorRange::two(
                    egui::text::CCursor::new(0),
                    egui::text::CCursor::new(draft.chars().count()),
                )));
            state.store(ui.ctx(), id);
            self.focus_layout_name = false;
        }
        let response = ui
            .add(
                egui::TextEdit::singleline(draft)
                    .id(id)
                    .font(egui::TextStyle::Heading)
                    .margin(egui::Margin::symmetric(6, 4))
                    .desired_width(ui.available_width())
                    .hint_text(DEFAULT_LAYOUT_NAME),
            )
            .on_hover_text("Enter to apply, Escape to cancel");
        if ui.input(|input| input.key_pressed(egui::Key::Escape)) {
            self.layout_name_edit = None;
        } else if response.lost_focus() || ui.input(|input| input.key_pressed(egui::Key::Enter)) {
            self.apply_layout_name();
        }
    }

    pub(in crate::app) fn buttons_panel(&mut self, ui: &mut egui::Ui) {
        ui.add_space(10.0);
        self.layout_heading(ui);
        ui.add_space(8.0);
        let (mut rows, mut columns) = (self.config.rows, self.config.columns);
        ui.horizontal(|ui| {
            ui.label("Rows");
            ui.add(
                egui::DragValue::new(&mut rows)
                    .range(1..=MAX_GRID_ROWS)
                    .speed(0.1),
            );
            ui.label("Columns");
            ui.add(
                egui::DragValue::new(&mut columns)
                    .range(1..=MAX_GRID_COLUMNS)
                    .speed(0.1),
            );
        });
        if (rows, columns) != (self.config.rows, self.config.columns) {
            match self.config.resize_grid(rows, columns) {
                Ok(()) => self.dirty = true,
                Err(error) => self.error = Some(error),
            }
        }
        ui.add_space(12.0);
        ui.add(MaterialCheckbox::new(&mut self.edit_mode, "Edit buttons"))
            .on_hover_text(
                "Click buttons to edit instead of running them. Right-click also edits.",
            );
        ui.add_space(12.0);
        let new_tab = ui.input(|input| input.modifiers.ctrl);
        let columns = self.config.columns;
        let width =
            ((ui.available_width() - 8.0 * (columns - 1) as f32) / columns as f32).max(90.0);
        let mut clicked = None;
        let mut edit = None;
        egui::ScrollArea::both()
            .id_salt("grid_scroll")
            .auto_shrink([false, false])
            .max_height(ui.available_height())
            .show(ui, |ui| {
                egui::Grid::new(("button_grid", columns))
                    .num_columns(columns)
                    .spacing([8.0, 8.0])
                    .show(ui, |ui| {
                        for (index, button) in self
                            .config
                            .buttons
                            .iter()
                            .take(self.config.visible_buttons())
                            .enumerate()
                        {
                            let configured = !button.command.trim().is_empty();
                            let selected = self.active().is_some_and(|tab| {
                                tab.button_index == Some(index) && tab.terminal.is_running()
                            });
                            let caption = if configured {
                                button.label.clone()
                            } else {
                                format!("{}\n+ Configure", button.label)
                            };
                            let mut widget = styled_button(button, caption).wrap();
                            if selected {
                                widget = widget
                                    .stroke(Stroke::new(2.0_f32, get_global_color("primary")));
                            }
                            let response = ui.add_sized([width, 88.0], widget);
                            if response.clicked() {
                                clicked = Some(index);
                            }
                            response.context_menu(|ui| {
                                if ui.add(text_button("Edit button").small()).clicked() {
                                    edit = Some(index);
                                    ui.close();
                                }
                            });
                            response.on_hover_text(if configured {
                                format!("{}\nCtrl-click to open a new terminal tab", button.command)
                            } else {
                                "Click to assign a label and command".into()
                            });
                            if (index + 1) % columns == 0 {
                                ui.end_row();
                            }
                        }
                    });
            });
        if let Some(index) = edit {
            self.edit(index);
        }
        if let Some(index) = clicked {
            if self.edit_mode {
                self.edit(index);
            } else {
                self.launch(index, new_tab);
            }
        }
    }
}
