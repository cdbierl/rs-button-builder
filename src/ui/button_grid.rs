//! Layout heading, grid sizing, and command buttons.

use super::{CORNER_RADIUS, styled_button, text_button};
use crate::app::ButtonBuilder;
use crate::layout_config::{ButtonConfig, DEFAULT_LAYOUT_NAME, MAX_GRID_COLUMNS, MAX_GRID_ROWS};
use egui::{PointerButton, RichText, Stroke, Vec2};
use egui_material3::{MaterialCheckbox, get_global_color};

// The bundled Material Symbols font includes `new_window`; the crate's name map predates it.
const EMPTY_BUTTON_ICON: char = '\u{f710}';

fn empty_button_icon_font() -> egui::FontId {
    egui::FontId::new(
        32.0,
        egui::FontFamily::Name(crate::fonts::MATERIAL_SYMBOLS.into()),
    )
}

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
            self.dragged_button = None;
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
        let mut reset = None;
        let mut slots = Vec::new();
        if self.dragged_button.is_some()
            && ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
        {
            self.dragged_button = None;
        }
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
                                String::new()
                            };
                            let mut widget = styled_button(button, caption)
                                .wrap()
                                .sense(egui::Sense::click_and_drag());
                            if selected {
                                widget = widget
                                    .stroke(Stroke::new(2.0_f32, get_global_color("primary")));
                            }
                            let response = ui.add_sized([width, 88.0], widget);
                            if !configured {
                                ui.painter().text(
                                    response.rect.center(),
                                    egui::Align2::CENTER_CENTER,
                                    EMPTY_BUTTON_ICON,
                                    empty_button_icon_font(),
                                    get_global_color("outline"),
                                );
                                response.widget_info(|| {
                                    egui::WidgetInfo::labeled(
                                        egui::WidgetType::Button,
                                        ui.is_enabled(),
                                        format!("Configure button {}", index + 1),
                                    )
                                });
                            }
                            if response.drag_started_by(PointerButton::Primary) {
                                self.dragged_button = Some(index);
                            }
                            if response.clicked() {
                                clicked = Some(index);
                            }
                            response.context_menu(|ui| {
                                if ui.add(text_button("Edit button").small()).clicked() {
                                    edit = Some(index);
                                    ui.close();
                                }
                                if ui
                                    .add(text_button("Reset button").small())
                                    .on_hover_text("Clear this slot's label, command, and custom settings")
                                    .clicked()
                                {
                                    reset = Some(index);
                                    ui.close();
                                }
                            });
                            slots.push((index, response.clone(), ui.painter().clone()));
                            response.on_hover_text(if configured {
                                format!("{}\nCtrl-click to open a new terminal tab\nDrag onto another button to swap positions", button.command)
                            } else {
                                "Click to assign a label and command\nDrag onto another button to swap positions".into()
                            });
                            if (index + 1) % columns == 0 {
                                ui.end_row();
                            }
                        }
                    });
            });
        if let Some(source) = self.dragged_button {
            let target = slots
                .iter()
                .find(|(_, response, _)| response.contains_pointer());
            if ui.input(|input| input.pointer.button_released(PointerButton::Primary)) {
                if let Some((target, _, _)) = target {
                    self.reposition_button(source, *target);
                }
                self.dragged_button = None;
                ui.ctx().request_repaint();
            } else if !ui.input(|input| input.pointer.primary_down()) {
                self.dragged_button = None;
            } else {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
                for (index, response, painter) in &slots {
                    if *index == source || target.is_some_and(|(target, _, _)| index == target) {
                        painter.rect_stroke(
                            response.rect.shrink(2.0),
                            CORNER_RADIUS,
                            Stroke::new(
                                3.0_f32,
                                get_global_color(if *index == source {
                                    "outline"
                                } else {
                                    "primary"
                                }),
                            ),
                            egui::StrokeKind::Inside,
                        );
                    }
                }
                if let Some(pointer) = ui.input(|input| input.pointer.hover_pos()) {
                    let painter = ui.ctx().layer_painter(egui::LayerId::new(
                        egui::Order::Tooltip,
                        ui.id().with("button_drag_preview"),
                    ));
                    let button = &self.config.buttons[source];
                    let (caption, font, color) = if button.command.trim().is_empty() {
                        (
                            EMPTY_BUTTON_ICON.to_string(),
                            empty_button_icon_font(),
                            get_global_color("outline"),
                        )
                    } else {
                        (
                            button.label.clone(),
                            egui::FontId::proportional(15.0),
                            get_global_color("onPrimaryContainer"),
                        )
                    };
                    let text = painter.layout_no_wrap(caption, font, color);
                    let rect = egui::Rect::from_min_size(
                        pointer + Vec2::splat(16.0),
                        text.size() + Vec2::new(20.0, 12.0),
                    );
                    painter.rect_filled(rect, CORNER_RADIUS, get_global_color("primaryContainer"));
                    painter.galley(rect.min + Vec2::new(10.0, 6.0), text, color);
                }
            }
        }
        if let Some(index) = reset {
            self.reset_button(index);
            ui.ctx().request_repaint();
        }
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

    fn reset_button(&mut self, index: usize) {
        self.config.buttons[index] = ButtonConfig {
            label: format!("Button {}", index + 1),
            ..Default::default()
        };
        for tab in &mut self.tabs {
            if tab.button_index == Some(index) {
                tab.button_index = None;
            }
        }
        if self
            .editor
            .as_ref()
            .is_some_and(|editor| editor.index == index)
        {
            self.editor = None;
        }
        if self.dragged_button == Some(index) {
            self.dragged_button = None;
        }
        self.dirty = true;
    }

    pub(in crate::app) fn reposition_button(&mut self, source: usize, target: usize) {
        if source == target
            || source >= self.config.visible_buttons()
            || target >= self.config.visible_buttons()
        {
            return;
        }
        self.config.buttons.swap(source, target);
        let moved_index = |index| {
            if index == source {
                target
            } else if index == target {
                source
            } else {
                index
            }
        };
        for tab in &mut self.tabs {
            tab.button_index = tab.button_index.map(moved_index);
        }
        if let Some(editor) = &mut self.editor {
            editor.index = moved_index(editor.index);
        }
        self.dirty = true;
    }
}
