use crate::config::{
    ButtonConfig, Config, DEFAULT_LAYOUT_NAME, MAX_GRID_COLUMNS, MAX_GRID_ROWS, TerminalStyle,
};
use crate::config_picker::{Action as PickerAction, ConfigPicker};
use crate::terminal::{Terminal, screen::Cell};
use crate::ui::{CORNER_RADIUS, filled_button, outlined_button, text_button};
use egui::{Color32, FontFamily, FontId, RichText, Stroke, Vec2};
use egui_material3::{
    MaterialButton, MaterialCheckbox, MaterialSlider, MaterialToolbar, get_global_color,
};
use std::path::PathBuf;
use std::time::{Duration, Instant};

// Reserve the gutter even without history so the PTY width stays stable as output arrives.
const TERMINAL_SCROLLBAR_WIDTH: f32 = 20.0;
const UNSAVED_LAYOUT_NOTICE: &str = "Unsaved layout · Load a config or save a new layout";

struct Editor {
    index: usize,
    button: ButtonConfig,
    error: Option<String>,
}

struct SaveAs {
    picker: ConfigPicker,
    config: Config,
    close_editor: bool,
    create_new_layout: bool,
}

struct TerminalTab {
    id: u64,
    label: String,
    command: String,
    button_index: Option<usize>,
    terminal: Terminal,
    style: TerminalStyle,
    scrollback: usize,
    scroll_fraction: f32,
}

impl TerminalTab {
    fn poll(&mut self) {
        let previous = self.terminal.screen.scrolled_lines;
        self.terminal.poll();
        if self.scrollback > 0 {
            self.scrollback = self
                .scrollback
                .saturating_add(self.terminal.screen.scrolled_lines.saturating_sub(previous));
        }
    }
}

pub struct ButtonBuilder {
    config: Config,
    config_path: Option<PathBuf>,
    config_picker: Option<ConfigPicker>,
    save_as: Option<SaveAs>,
    new_layout_requested: bool,
    new_layout_error: Option<String>,
    layout_name_edit: Option<String>,
    focus_layout_name: bool,
    tabs: Vec<TerminalTab>,
    active_tab: Option<usize>,
    next_tab_id: u64,
    next_poll_tab: usize,
    reveal_active_tab: bool,
    editor: Option<Editor>,
    edit_mode: bool,
    appearance_open: bool,
    fonts: Vec<String>,
    dirty: bool,
    notice: String,
    error: Option<String>,
    terminal_size: (usize, usize),
    focus_terminal: bool,
}

impl ButtonBuilder {
    pub fn new(ctx: &egui::Context, config_path: Option<PathBuf>, config: Config) -> Self {
        crate::ui::setup(ctx);
        let fonts = crate::fonts::load(ctx);
        let notice = match &config_path {
            Some(path) => format!("Loaded {}", path.display()),
            None => UNSAVED_LAYOUT_NOTICE.into(),
        };
        Self {
            config,
            config_path,
            config_picker: None,
            save_as: None,
            new_layout_requested: false,
            new_layout_error: None,
            layout_name_edit: None,
            focus_layout_name: false,
            tabs: Vec::new(),
            active_tab: None,
            next_tab_id: 1,
            next_poll_tab: 0,
            reveal_active_tab: false,
            editor: None,
            edit_mode: false,
            appearance_open: false,
            fonts,
            dirty: false,
            notice,
            error: None,
            terminal_size: (80, 24),
            focus_terminal: false,
        }
    }

    fn running(&self) -> bool {
        self.tabs.iter().any(|tab| tab.terminal.is_running())
    }

    fn active(&self) -> Option<&TerminalTab> {
        self.active_tab.and_then(|index| self.tabs.get(index))
    }

    fn active_running(&self) -> bool {
        self.active().is_some_and(|tab| tab.terminal.is_running())
    }

    fn select_tab(&mut self, index: usize) {
        self.active_tab = Some(index);
        self.reveal_active_tab = true;
        self.focus_terminal = true;
    }

    fn close_tab(&mut self, index: usize) {
        // Dropping a terminal requests termination; reaping happens off the UI thread.
        self.tabs.remove(index);
        self.active_tab = match self.active_tab {
            _ if self.tabs.is_empty() => None,
            Some(active) if active > index => Some(active - 1),
            Some(active) if active == index => Some(index.min(self.tabs.len() - 1)),
            active => active,
        };
        self.focus_terminal = true;
        self.reveal_active_tab = true;
    }

    fn save(&mut self) {
        self.apply_layout_name();
        if let Err(error) = self.save_config(self.config.clone(), false) {
            self.error = Some(error);
        }
    }

    fn save_config(&mut self, config: Config, close_editor: bool) -> Result<(), String> {
        if let Some(path) = &self.config_path {
            config.save(path)?;
            self.finish_save(config, path.clone(), close_editor);
        } else {
            self.save_as = Some(SaveAs {
                picker: ConfigPicker::save(None),
                config,
                close_editor,
                create_new_layout: false,
            });
        }
        Ok(())
    }

    fn finish_save(&mut self, config: Config, path: PathBuf, close_editor: bool) {
        self.config = config;
        self.notice = format!("Saved {}", path.display());
        self.config_path = Some(path);
        self.dirty = false;
        self.error = None;
        if close_editor {
            self.editor = None;
        }
        if let Some(picker) = &mut self.config_picker {
            picker.error = None;
        }
    }

    fn save_as_window(&mut self, ctx: &egui::Context) {
        let Some(mut save_as) = self.save_as.take() else {
            return;
        };
        match save_as.picker.show(ctx, false, false) {
            PickerAction::Save(path) => match save_as.config.save(&path) {
                Ok(()) => {
                    self.finish_save(save_as.config, path, save_as.close_editor);
                    if save_as.create_new_layout {
                        self.create_new_layout();
                    }
                }
                Err(error) => {
                    save_as.picker.error = Some(error);
                    self.save_as = Some(save_as);
                }
            },
            PickerAction::Cancel => {}
            _ => self.save_as = Some(save_as),
        }
    }

    fn apply_layout_name(&mut self) {
        let Some(draft) = self.layout_name_edit.take() else {
            return;
        };
        let name = draft.trim();
        let name = if name.is_empty() {
            DEFAULT_LAYOUT_NAME
        } else {
            name
        };
        if self.config.display_name != name {
            self.config.display_name = name.to_owned();
            self.dirty = true;
        }
        self.focus_layout_name = false;
    }

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

    fn edit(&mut self, index: usize) {
        self.editor = Some(Editor {
            index,
            button: self.config.buttons[index].clone(),
            error: None,
        });
    }

    fn load_config(&mut self, path: PathBuf) -> Result<(), String> {
        let config = Config::load_existing(&path)?;
        self.replace_layout(config, Some(path));
        Ok(())
    }

    fn replace_layout(&mut self, config: Config, path: Option<PathBuf>) {
        self.config = config;
        self.notice = match &path {
            Some(path) => format!("Loaded {}", path.display()),
            None => UNSAVED_LAYOUT_NOTICE.into(),
        };
        self.config_path = path;
        self.config_picker = None;
        self.save_as = None;
        self.new_layout_requested = false;
        self.new_layout_error = None;
        self.layout_name_edit = None;
        self.focus_layout_name = false;
        self.editor = None;
        self.appearance_open = false;
        self.edit_mode = false;
        for tab in &mut self.tabs {
            tab.button_index = None;
        }
        self.dirty = false;
        self.error = None;
    }

    fn has_unsaved_changes(&self) -> bool {
        self.dirty || self.editor.is_some() || self.layout_name_edit.is_some()
    }

    fn request_new_layout(&mut self) {
        self.new_layout_error = None;
        if self.has_unsaved_changes() {
            self.new_layout_requested = true;
        } else {
            self.create_new_layout();
        }
    }

    fn create_new_layout(&mut self) {
        self.replace_layout(Config::default(), None);
    }

    fn save_before_new_layout(&mut self) {
        self.apply_layout_name();
        let mut config = self.config.clone();
        if let Some(editor) = &self.editor {
            if let Err(error) = validate_button(&editor.button) {
                self.new_layout_error = Some(error);
                return;
            }
            config.buttons[editor.index] = editor.button.clone();
        }
        match self.save_config(config, true) {
            Ok(()) => {
                if let Some(save_as) = &mut self.save_as {
                    save_as.create_new_layout = true;
                } else {
                    self.create_new_layout();
                }
            }
            Err(error) => self.new_layout_error = Some(error),
        }
    }

    fn new_layout_window(&mut self, ctx: &egui::Context) {
        if !self.new_layout_requested {
            return;
        }
        let mut save = false;
        let mut discard = false;
        let mut cancel = false;
        let dialog = crate::ui::dialog(
            ctx,
            "new_layout",
            "Create a new layout?",
            480.0..=560.0,
            |ui| {
                ui.label("Save your changes before starting a new layout?");
                if self.running() {
                    ui.small("Running terminals will stay open.");
                }
                if let Some(error) = &self.new_layout_error {
                    ui.colored_label(Color32::LIGHT_RED, error);
                }
                crate::ui::dialog_footer(ui, |ui| {
                    cancel = ui.add(text_button("Cancel")).clicked();
                    discard = ui.add(outlined_button("Create without saving")).clicked();
                    save = ui.add(filled_button("Save and create")).clicked();
                });
            },
        );
        if save {
            self.save_before_new_layout();
        } else if discard {
            self.create_new_layout();
        } else if cancel || dialog.should_close() {
            self.new_layout_requested = false;
            self.new_layout_error = None;
        }
    }

    fn config_picker_window(&mut self, ctx: &egui::Context) {
        let running = self.running();
        let unsaved = self.has_unsaved_changes();
        let Some(picker) = &mut self.config_picker else {
            return;
        };
        match picker.show(ctx, unsaved, running) {
            PickerAction::Load(path) => match self.load_config(path) {
                Ok(()) => self.config_picker = None,
                Err(error) => {
                    if let Some(picker) = &mut self.config_picker {
                        picker.error = Some(error);
                    }
                }
            },
            PickerAction::SaveCurrent => {
                self.save();
                if let Some(picker) = &mut self.config_picker {
                    picker.error = self.error.clone();
                }
            }
            PickerAction::Cancel => self.config_picker = None,
            PickerAction::None | PickerAction::Save(_) => {}
        }
    }

    fn launch(&mut self, index: usize, new_tab: bool) {
        if self.config.buttons[index].command.trim().is_empty() {
            self.edit(index);
            return;
        }
        if !new_tab && self.active_running() {
            return;
        }
        match Terminal::start(
            &self.config.buttons[index],
            self.terminal_size.0,
            self.terminal_size.1,
        ) {
            Ok(terminal) => {
                let replace = self.active_tab.filter(|_| !new_tab);
                let id = replace.map_or(self.next_tab_id, |index| self.tabs[index].id);
                let style = replace
                    .map(|index| self.tabs[index].style.clone())
                    .unwrap_or_else(|| self.config.terminal.clone());
                let tab = TerminalTab {
                    id,
                    label: self.config.buttons[index].label.clone(),
                    command: self.config.buttons[index].command.clone(),
                    button_index: Some(index),
                    terminal,
                    style,
                    scrollback: 0,
                    scroll_fraction: 0.0,
                };
                if let Some(index) = replace {
                    self.tabs[index] = tab;
                } else {
                    self.next_tab_id += 1;
                    self.tabs.push(tab);
                    self.active_tab = Some(self.tabs.len() - 1);
                }
                self.focus_terminal = true;
                self.reveal_active_tab = true;
                self.error = None;
            }
            Err(error) => self.error = Some(error),
        }
    }

    fn header(&mut self, ui: &mut egui::Ui) {
        ui.add(
            MaterialToolbar::new()
                .top(true)
                .min_height(64.0)
                .padding(Vec2::new(20.0, 12.0))
                .item_fn(|ui| ui.add(crate::ui::material_icon("apps")))
                .item_fn(|ui| ui.heading("Button Builder"))
                .item_fn(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add(
                                outlined_button("Exit")
                                    .small()
                                    .min_size(Vec2::new(0.0, 40.0)),
                            )
                            .on_hover_text("Close Button Builder")
                            .clicked()
                        {
                            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                        if ui
                            .add(
                                filled_button(if self.config_path.is_none() {
                                    "Save layout…"
                                } else if self.dirty {
                                    "Save layout *"
                                } else {
                                    "Save layout"
                                })
                                .small()
                                .min_size(Vec2::new(0.0, 40.0)),
                            )
                            .on_hover_text(self.config_path.as_ref().map_or_else(
                                || "Choose a file to save this layout".into(),
                                |path| format!("Save to {}", path.display()),
                            ))
                            .clicked()
                        {
                            self.save();
                        }
                        let response = ui.add(
                            outlined_button("Load config…")
                                .small()
                                .min_size(Vec2::new(0.0, 40.0)),
                        );
                        if response.clicked() {
                            self.config_picker =
                                Some(ConfigPicker::new(self.config_path.as_deref()));
                        }
                        if ui
                            .add(
                                outlined_button("New layout")
                                    .small()
                                    .min_size(Vec2::new(0.0, 40.0)),
                            )
                            .on_hover_text("Create an empty, unsaved layout")
                            .clicked()
                        {
                            self.request_new_layout();
                        }
                        response
                    })
                    .inner
                }),
        );
    }

    fn buttons_panel(&mut self, ui: &mut egui::Ui) {
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

    fn editor_window(&mut self, ctx: &egui::Context) {
        let Some(editor) = &mut self.editor else {
            return;
        };
        let mut save = false;
        let mut cancel = false;
        let dialog = crate::ui::dialog(
            ctx,
            "button_editor",
            format!("Edit button {}", editor.index + 1),
            480.0_f32.min(ctx.content_rect().width() - 64.0)..=560.0,
            |ui| {
                crate::ui::form(ui, ctx, "button_editor_form", |ui| {
                    ui.label("Label");
                    ui.add(crate::ui::text_input(&mut editor.button.label));
                    ui.add_space(8.0);
                    ui.horizontal_wrapped(|ui| {
                        ui.add(MaterialCheckbox::new(
                            &mut editor.button.shell,
                            "Shell pipeline (/bin/sh)",
                        ));
                        ui.add(MaterialCheckbox::new(&mut editor.button.log_output, "Log output"))
                            .on_hover_text("Save this button's output to a file. Requires BUTTON_BUILDER_LOG_DIR in .env; applies to new runs.");
                    });
                    if editor.button.shell {
                        ui.label("Command or pipeline");
                        ui.add(
                            egui::TextEdit::multiline(&mut editor.button.command)
                                .margin(egui::Margin::symmetric(10, 8))
                                .font(egui::TextStyle::Monospace)
                                .desired_rows(3)
                                .desired_width(f32::INFINITY),
                        );
                        ui.small("Supports pipes, redirects, and shell quoting.");
                    } else {
                        ui.label("Program (full path or a name on PATH)");
                        ui.add(
                            crate::ui::text_input(&mut editor.button.command)
                                .hint_text("/usr/bin/top"),
                        );
                        ui.label("Arguments (optional)");
                        ui.add(
                            crate::ui::text_input(&mut editor.button.arguments).hint_text("-d 1"),
                        );
                        ui.small("Quote arguments containing spaces: --name \"two words\"");
                    }
                    ui.add_space(8.0);
                    ui.label("Working directory (optional)");
                    ui.add(
                        crate::ui::text_input(&mut editor.button.working_directory)
                            .hint_text("Defaults to the app's working directory"),
                    );
                    ui.small("Leave the command empty to make an unassigned button.");
                    ui.add_space(10.0);
                    ui.horizontal_wrapped(|ui| {
                        let (text, fill) = button_colors(&editor.button);
                        let mut foreground = [text.r(), text.g(), text.b()];
                        let mut background = [fill.r(), fill.g(), fill.b()];
                        ui.label("Text color");
                        if ui.color_edit_button_srgb(&mut foreground).changed() {
                            editor.button.fg_color = Some(foreground);
                        }
                        ui.label("Background");
                        if ui.color_edit_button_srgb(&mut background).changed() {
                            editor.button.bg_color = Some(background);
                        }
                        if ui.add(text_button("Reset colors").small()).clicked() {
                            editor.button.fg_color = None;
                            editor.button.bg_color = None;
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.label("Preview");
                        ui.add_sized(
                            [180.0, 50.0],
                            styled_button(&editor.button, editor.button.label.clone()).wrap(),
                        );
                    });
                    if let Some(error) = &editor.error {
                        ui.colored_label(Color32::LIGHT_RED, error);
                    }
                });
                crate::ui::dialog_footer(ui, |ui| {
                    cancel = ui.add(text_button("Cancel")).clicked();
                    save = ui.add(filled_button("Save button")).clicked();
                });
            },
        );
        if save {
            match validate_button(&editor.button) {
                Err(error) => editor.error = Some(error),
                Ok(()) => {
                    let mut config = self.config.clone();
                    config.buttons[editor.index] = editor.button.clone();
                    if let Err(error) = self.save_config(config, true)
                        && let Some(editor) = &mut self.editor
                    {
                        editor.error = Some(error);
                    }
                }
            }
        } else if dialog.should_close() || cancel {
            self.editor = None;
        }
    }

    fn appearance_window(&mut self, ctx: &egui::Context) {
        if !self.appearance_open {
            return;
        }
        let mut save = false;
        let mut refresh_fonts = false;
        let mut close = false;
        let mut changed = false;
        let dialog = crate::ui::dialog(
            ctx,
            "terminal_appearance",
            "Terminal appearance",
            400.0_f32.min(ctx.content_rect().width() - 64.0)..=480.0,
            |ui| {
                crate::ui::form(ui, ctx, "appearance_form", |ui| {
                    let style = match self.active_tab {
                        Some(index) => &mut self.tabs[index].style,
                        None => &mut self.config.terminal,
                    };
                    changed |= crate::ui::font_picker(ui, &mut style.font, &self.fonts);
                    refresh_fonts = ui.add(text_button("Refresh fonts").small()).clicked();
                    if !self.fonts.contains(&style.font) {
                        ui.small("The selected font is unavailable; using built-in Monospace.");
                    }
                    ui.label("Font size");
                    ui.horizontal(|ui| {
                        changed |= ui
                            .add(
                                MaterialSlider::new(&mut style.font_size, 8.0..=32.0)
                                    .show_value(false)
                                    .width((ui.available_width() - 100.0).max(100.0)),
                            )
                            .changed();
                        changed |= ui
                            .add(
                                egui::DragValue::new(&mut style.font_size)
                                    .range(8.0..=32.0)
                                    .speed(0.25)
                                    .fixed_decimals(1)
                                    .suffix(" pt"),
                            )
                            .changed();
                    });
                    ui.label("Line spacing");
                    ui.horizontal(|ui| {
                        changed |= ui
                            .add(
                                MaterialSlider::new(&mut style.line_spacing, 1.0..=1.8)
                                    .show_value(false)
                                    .width((ui.available_width() - 100.0).max(100.0)),
                            )
                            .changed();
                        changed |= ui
                            .add(
                                egui::DragValue::new(&mut style.line_spacing)
                                    .range(1.0..=1.8)
                                    .speed(0.01)
                                    .fixed_decimals(2)
                                    .suffix("×"),
                            )
                            .changed();
                    });
                    ui.horizontal(|ui| {
                        ui.label("Text");
                        changed |= ui.color_edit_button_srgb(&mut style.fg_color).changed();
                        ui.label("Background");
                        changed |= ui.color_edit_button_srgb(&mut style.bg_color).changed();
                    });
                });
                crate::ui::dialog_footer(ui, |ui| {
                    close = ui
                        .add(text_button("Close").small().min_size(Vec2::new(0.0, 40.0)))
                        .clicked();
                    if ui
                        .add(
                            text_button("Reset appearance")
                                .small()
                                .min_size(Vec2::new(0.0, 40.0)),
                        )
                        .clicked()
                    {
                        self.config.terminal = TerminalStyle::default();
                        if let Some(index) = self.active_tab {
                            self.tabs[index].style = TerminalStyle::default();
                        }
                        self.dirty = true;
                    }
                    save = ui
                        .add(
                            filled_button("Save appearance")
                                .small()
                                .min_size(Vec2::new(0.0, 40.0)),
                        )
                        .clicked();
                });
            },
        );
        if changed {
            if let Some(tab) = self.active() {
                self.config.terminal = tab.style.clone();
            }
            self.dirty = true;
        }
        if dialog.should_close() || close {
            self.appearance_open = false;
        }
        if refresh_fonts {
            self.fonts = crate::fonts::load(ctx);
            self.notice = format!(
                "Found {} installed monospace font families",
                self.fonts.len() - 1
            );
        }
        if save {
            if let Some(tab) = self.active() {
                self.config.terminal = tab.style.clone();
            }
            self.save();
        }
    }

    fn terminal_tabs(&mut self, ui: &mut egui::Ui) {
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
                                                "primaryContainer"
                                            } else {
                                                "surfaceContainerLow"
                                            }))
                                            .text_color(get_global_color(if active {
                                                "onPrimaryContainer"
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
                                                "primary"
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

    fn terminal_toolbar(&mut self, ui: &mut egui::Ui) {
        let active = self.active_tab;
        let running = self.active_running();
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
                        self.appearance_open = true;
                    }
                    copy = ui
                        .add_enabled(active.is_some(), toolbar_button("Copy"))
                        .on_hover_text("Copy this tab's output (Ctrl+Shift+C)")
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
                        .on_hover_text("Close this tab and stop its process")
                        .clicked();
                    if self.active().is_some_and(|tab| tab.scrollback > 0) {
                        live = ui.add(toolbar_button("Jump to live")).clicked();
                    }
                });
            });
        if let Some(index) = active {
            let tab = &mut self.tabs[index];
            if copy {
                ui.ctx().copy_text(tab.terminal.screen.text());
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
                self.close_tab(index);
            }
        }
        let mut status = self
            .active()
            .map(|tab| tab.terminal.status.as_str())
            .unwrap_or("Ready · Ctrl-click a button to open another terminal tab")
            .to_owned();
        let log_error = self.active().and_then(|tab| tab.terminal.log_error());
        if let Some(error) = log_error {
            status.push_str(&format!(" · Log error: {error}"));
        }
        let mut tooltip = status.clone();
        if let Some(path) = self.active().and_then(|tab| tab.terminal.log_path()) {
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

    fn terminal_panel(&mut self, ui: &mut egui::Ui) {
        self.terminal_tabs(ui);
        self.terminal_toolbar(ui);
        ui.add_space(4.0);
        let size = ui.available_size().max(Vec2::splat(1.0));
        let id = self.active().map(|tab| tab.id);
        let (response, painter) = ui
            .push_id(("terminal_pane", id), |ui| {
                ui.allocate_painter(size, egui::Sense::click())
            })
            .inner;
        let control_scroll = ui.input(|input| {
            input.modifiers.ctrl
                || input.events.iter().any(|event| {
                    matches!(event, egui::Event::MouseWheel { modifiers, .. } if modifiers.ctrl)
                })
        });
        let pane_hovered = ui.rect_contains_pointer(response.rect);
        if pane_hovered && control_scroll {
            // Read wheel events directly: egui turns Ctrl+wheel into smoothed zoom input.
            // One wheel notch changes the font by 1 pt; pixel scrolling scales continuously.
            let delta: f32 = ui.input(|input| {
                input
                    .events
                    .iter()
                    .filter_map(|event| match event {
                        egui::Event::MouseWheel {
                            unit,
                            delta,
                            modifiers,
                        } if modifiers.ctrl => Some(match unit {
                            egui::MouseWheelUnit::Point => delta.y / 40.0,
                            egui::MouseWheelUnit::Line | egui::MouseWheelUnit::Page => delta.y,
                        }),
                        _ => None,
                    })
                    .sum()
            });
            let style = match self.active_tab {
                Some(index) => &mut self.tabs[index].style,
                None => &mut self.config.terminal,
            };
            let font_size = (style.font_size + delta).clamp(8.0, 32.0);
            if font_size != style.font_size {
                style.font_size = font_size;
                self.config.terminal = style.clone();
                self.dirty = true;
            }
        }
        let style = match self.active_tab {
            Some(index) => &self.tabs[index].style,
            None => &self.config.terminal,
        };
        let font = terminal_font(style, &self.fonts);
        let foreground = rgb(style.fg_color);
        let (char_width, line_height) = ui.fonts_mut(|fonts| {
            (
                fonts.glyph_width(&font, 'M'),
                fonts.row_height(&font) * style.line_spacing,
            )
        });
        let content_width = (size.x - 16.0 - TERMINAL_SCROLLBAR_WIDTH).max(0.0);
        let cols = ((content_width / char_width) as usize).clamp(2, 500);
        let rows = (((size.y - 16.0) / line_height) as usize).clamp(2, 200);
        self.terminal_size = (cols, rows);
        let rect = response.rect;
        painter.rect_filled(rect, CORNER_RADIUS, rgb(style.bg_color));
        // Inactive processes also receive viewport changes, using their own font metrics.
        for tab in &mut self.tabs {
            let tab_font = terminal_font(&tab.style, &self.fonts);
            let (width, height) = ui.fonts_mut(|fonts| {
                (
                    fonts.glyph_width(&tab_font, 'M'),
                    fonts.row_height(&tab_font) * tab.style.line_spacing,
                )
            });
            let previous = tab.terminal.screen.scrolled_lines;
            tab.terminal.resize(
                ((content_width / width) as usize).clamp(2, 500),
                (((size.y - 16.0) / height) as usize).clamp(2, 200),
            );
            // Shrinking the viewport can move rows into history. Keep the viewed text anchored.
            if tab.scrollback > 0 {
                tab.scrollback = tab
                    .scrollback
                    .saturating_add(tab.terminal.screen.scrolled_lines.saturating_sub(previous));
            }
        }
        let mut scrollbar_interacted = false;
        if let Some(index) = self.active_tab {
            let tab = &mut self.tabs[index];
            let screen = &tab.terminal.screen;
            let history_len = if screen.is_alternate() {
                0
            } else {
                screen.history.len()
            };
            if pane_hovered && !control_scroll {
                let delta = ui.input(|input| input.smooth_scroll_delta.y);
                tab.scroll_fraction += delta / line_height;
                let lines = tab.scroll_fraction.trunc() as isize;
                tab.scroll_fraction -= lines as f32;
                tab.scrollback = tab.scrollback.saturating_add_signed(lines);
            }
            tab.scrollback = tab.scrollback.min(history_len);
            scrollbar_interacted =
                terminal_scrollbar(ui, &response, rows, history_len, &mut tab.scrollback);
            if scrollbar_interacted {
                tab.scroll_fraction = 0.0;
            }
        }
        if response.clicked() || scrollbar_interacted || self.focus_terminal {
            response.request_focus();
            self.focus_terminal = false;
        }
        if response.has_focus() {
            ui.memory_mut(|memory| {
                memory.set_focus_lock_filter(
                    response.id,
                    egui::EventFilter {
                        tab: true,
                        horizontal_arrows: true,
                        vertical_arrows: true,
                        escape: true,
                    },
                )
            });
            painter.rect_stroke(
                rect,
                CORNER_RADIUS,
                Stroke::new(1.0_f32, Color32::from_rgb(76, 142, 205)),
                egui::StrokeKind::Inside,
            );
        }
        let origin = rect.min + Vec2::splat(8.0);
        let painter = painter.with_clip_rect(egui::Rect::from_min_max(
            origin,
            egui::pos2(
                (rect.right() - TERMINAL_SCROLLBAR_WIDTH - 8.0).max(origin.x),
                rect.bottom() - 8.0,
            ),
        ));
        let Some(index) = self.active_tab else {
            painter.text(
                origin,
                egui::Align2::LEFT_TOP,
                "No process output yet.",
                font,
                foreground,
            );
            return;
        };
        let tab = &mut self.tabs[index];
        let style = &tab.style;
        let terminal = &mut tab.terminal;
        let screen = &terminal.screen;
        let history_len = if screen.is_alternate() {
            0
        } else {
            screen.history.len()
        };
        let start = history_len.saturating_sub(tab.scrollback);
        for visible_row in 0..rows {
            let index = start + visible_row;
            let line = if index < history_len {
                &screen.history[index]
            } else {
                screen.line(index - history_len)
            };
            paint_line(
                &painter,
                origin + Vec2::new(0.0, visible_row as f32 * line_height),
                line,
                &font,
                char_width,
                line_height,
                cols,
                style,
            );
        }
        if tab.scrollback == 0
            && screen.cursor_visible
            && response.has_focus()
            && terminal.is_running()
        {
            let (x, y) = screen.cursor();
            let cursor = egui::Rect::from_min_size(
                origin + Vec2::new(x as f32 * char_width, y as f32 * line_height),
                Vec2::new(char_width, line_height),
            );
            painter.rect_stroke(
                cursor,
                0.0,
                Stroke::new(1.0_f32, rgb(style.fg_color)),
                egui::StrokeKind::Inside,
            );
        }
        if response.has_focus() {
            let events = ui.input(|input| input.events.clone());
            let copy_output = ui.input(|input| input.modifiers.ctrl && input.modifiers.shift)
                && events.iter().any(|event| {
                    matches!(
                        event,
                        egui::Event::Copy
                            | egui::Event::Key {
                                key: egui::Key::C,
                                pressed: true,
                                ..
                            }
                    )
                });
            if copy_output {
                ui.ctx().copy_text(terminal.screen.text());
            }
            let copied = events
                .iter()
                .any(|event| matches!(event, egui::Event::Copy));
            let cut = events.iter().any(|event| matches!(event, egui::Event::Cut));
            for event in events {
                let bytes = match event {
                    egui::Event::Copy if !copy_output => vec![3],
                    egui::Event::Cut => vec![24],
                    egui::Event::Text(text) => text.into_bytes(),
                    egui::Event::Paste(text) => {
                        text.replace("\r\n", "\n").replace('\n', "\r").into_bytes()
                    }
                    egui::Event::Key {
                        key,
                        pressed: true,
                        modifiers,
                        ..
                    } => {
                        if (key == egui::Key::C && (copied || copy_output))
                            || (key == egui::Key::X && cut)
                        {
                            continue;
                        }
                        key_bytes(key, modifiers, terminal.screen.application_cursor)
                    }
                    _ => Vec::new(),
                };
                if !bytes.is_empty() {
                    if bytes == [3] {
                        terminal.interrupt();
                    } else {
                        terminal.send(&bytes);
                    }
                    tab.scrollback = 0;
                }
            }
        }
    }

    fn show(&mut self, ctx: &egui::Context) {
        // Share a frame budget and rotate the starting tab so noisy tabs cannot starve others.
        let deadline = Instant::now() + Duration::from_millis(4);
        let count = self.tabs.len();
        for _ in 0..count {
            if Instant::now() >= deadline {
                break;
            }
            let index = self.next_poll_tab % count;
            self.tabs[index].poll();
            self.next_poll_tab = (index + 1) % count;
        }
        egui::TopBottomPanel::top("header")
            .frame(egui::Frame::NONE)
            .show(ctx, |ui| self.header(ui));
        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            if let Some(error) = &self.error {
                ui.colored_label(Color32::LIGHT_RED, error);
            } else {
                ui.small(&self.notice);
            }
        });
        egui::SidePanel::left("buttons")
            .resizable(true)
            .default_width(330.0)
            .width_range(290.0..=500.0)
            .show(ctx, |ui| {
                self.buttons_panel(ui);
            });
        egui::CentralPanel::default().show(ctx, |ui| self.terminal_panel(ui));
        if self.save_as.is_none() && !self.new_layout_requested {
            self.editor_window(ctx);
        }
        if self.save_as.is_none() && !self.new_layout_requested {
            self.appearance_window(ctx);
        }
        if self.save_as.is_none() && !self.new_layout_requested {
            self.config_picker_window(ctx);
        }
        if self.save_as.is_none() {
            self.new_layout_window(ctx);
        }
        self.save_as_window(ctx);
        if self.tabs.iter().any(|tab| tab.terminal.needs_poll()) {
            ctx.request_repaint_after(Duration::from_millis(33));
        }
    }
}

impl eframe::App for ButtonBuilder {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.show(ctx);
    }
}

fn rgb(color: [u8; 3]) -> Color32 {
    Color32::from_rgb(color[0], color[1], color[2])
}

fn validate_button(button: &ButtonConfig) -> Result<(), String> {
    if button.label.trim().is_empty() {
        Err("Enter a label for the button.".into())
    } else if !button.command.trim().is_empty() {
        button.args().map(|_| ())
    } else {
        Ok(())
    }
}

fn toolbar_button(caption: &str) -> MaterialButton<'static> {
    text_button(RichText::new(caption).size(14.0))
        .small()
        .min_size(Vec2::new(0.0, 32.0))
}

fn tab_arrow(caption: &str) -> MaterialButton<'static> {
    outlined_button(RichText::new(caption).size(18.0))
        .small()
        .min_size(Vec2::splat(32.0))
}

fn terminal_font(style: &TerminalStyle, fonts: &[String]) -> FontId {
    let family = if style.font == "Monospace" || !fonts.contains(&style.font) {
        FontFamily::Monospace
    } else {
        FontFamily::Name(style.font.clone().into())
    };
    FontId::new(style.font_size, family)
}

/// Map the full scrollback range onto a proportional thumb, keeping zero at live output.
fn terminal_scrollbar(
    ui: &mut egui::Ui,
    pane: &egui::Response,
    rows: usize,
    history_len: usize,
    scrollback: &mut usize,
) -> bool {
    let id = pane.id.with("scrollbar");
    let grab_id = id.with("grab_offset");
    if history_len == 0
        || pane.rect.height() <= 16.0
        || pane.rect.width() <= TERMINAL_SCROLLBAR_WIDTH
    {
        ui.ctx().data_mut(|data| data.remove::<f32>(grab_id));
        return false;
    }
    let gutter = egui::Rect::from_min_max(
        egui::pos2(
            pane.rect.right() - TERMINAL_SCROLLBAR_WIDTH,
            pane.rect.top() + 8.0,
        ),
        egui::pos2(pane.rect.right(), pane.rect.bottom() - 8.0),
    );
    let track = gutter.shrink2(Vec2::new(4.0, 0.0));
    let thumb_height = (track.height() * rows as f32 / (history_len + rows) as f32)
        .max(24.0)
        .min(track.height());
    let travel = track.height() - thumb_height;
    let thumb_rect = |scrollback: usize| {
        egui::Rect::from_min_size(
            track.min
                + Vec2::new(
                    0.0,
                    travel * (history_len - scrollback) as f32 / history_len as f32,
                ),
            Vec2::new(track.width(), thumb_height),
        )
    };
    let response = ui.interact(gutter, id, egui::Sense::click_and_drag());
    let interacting =
        response.is_pointer_button_down_on() || response.dragged() || response.clicked();
    if interacting && let Some(pointer) = response.interact_pointer_pos() {
        let thumb = thumb_rect(*scrollback);
        // Preserve where the thumb was grabbed; clicking the track centers it at the pointer.
        let grab_offset = ui.ctx().data_mut(|data| {
            *data.get_temp_mut_or_insert_with(grab_id, || {
                if thumb.contains(pointer) {
                    pointer.y - thumb.top()
                } else {
                    thumb_height / 2.0
                }
            })
        });
        if travel > 0.0 {
            let fraction = ((pointer.y - grab_offset - track.top()) / travel).clamp(0.0, 1.0);
            *scrollback = history_len - (fraction * history_len as f32).round() as usize;
        }
    }
    if !ui.input(|input| input.pointer.any_down()) {
        ui.ctx().data_mut(|data| data.remove::<f32>(grab_id));
    }
    let color = if response.hovered() || interacting {
        get_global_color("primary")
    } else {
        get_global_color("outline")
    };
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
    ui.painter()
        .rect_filled(thumb_rect(*scrollback), CORNER_RADIUS, color);
    interacting
}

fn styled_button(button: &ButtonConfig, caption: String) -> MaterialButton<'static> {
    let (text, fill) = button_colors(button);
    filled_button(RichText::new(caption).size(15.0))
        .small()
        .text_color(text)
        .fill(fill)
}

fn button_colors(button: &ButtonConfig) -> (Color32, Color32) {
    let (text, fill) = if button.command.trim().is_empty() {
        ("onSecondaryContainer", "surfaceVariant")
    } else {
        ("onPrimary", "primary")
    };
    let fill = button
        .bg_color
        .map(rgb)
        .unwrap_or_else(|| get_global_color(fill));
    let text = button.fg_color.map(rgb).unwrap_or_else(|| {
        if button.bg_color.is_some() {
            let linear = egui::Rgba::from(fill);
            let luminance = 0.2126 * linear.r() + 0.7152 * linear.g() + 0.0722 * linear.b();
            // Black or white gives at least 4.5:1 contrast against any opaque background.
            if luminance > 0.179 {
                Color32::BLACK
            } else {
                Color32::WHITE
            }
        } else {
            get_global_color(text)
        }
    });
    (text, fill)
}

#[allow(clippy::too_many_arguments)]
fn paint_line(
    painter: &egui::Painter,
    origin: egui::Pos2,
    line: &[Cell],
    font: &FontId,
    char_width: f32,
    line_height: f32,
    cols: usize,
    terminal_style: &TerminalStyle,
) {
    let mut start = 0;
    while start < line.len().min(cols) {
        let style = line[start].style;
        let mut end = start + 1;
        while end < line.len().min(cols) && line[end].style == style {
            end += 1;
        }
        let mut foreground = rgb(style.fg_color.unwrap_or(terminal_style.fg_color));
        let mut background = rgb(style.bg_color.unwrap_or(terminal_style.bg_color));
        if style.inverse {
            std::mem::swap(&mut foreground, &mut background);
        }
        let position = origin + Vec2::new(start as f32 * char_width, 0.0);
        let width = (end - start) as f32 * char_width;
        let rect = egui::Rect::from_min_size(position, Vec2::new(width, line_height));
        if background != rgb(terminal_style.bg_color) {
            painter.rect_filled(rect, 0.0, background);
        }
        let text: String = line[start..end]
            .iter()
            .filter(|cell| !cell.continuation)
            .map(|cell| cell.text.as_str())
            .collect();
        painter.text(
            position,
            egui::Align2::LEFT_TOP,
            &text,
            font.clone(),
            foreground,
        );
        if style.bold {
            painter.text(
                position + Vec2::new(0.5, 0.0),
                egui::Align2::LEFT_TOP,
                &text,
                font.clone(),
                foreground,
            );
        }
        if style.underline {
            painter.line_segment(
                [rect.left_bottom(), rect.right_bottom()],
                Stroke::new(1.0_f32, foreground),
            );
        }
        start = end;
    }
}

fn key_bytes(key: egui::Key, modifiers: egui::Modifiers, application_cursor: bool) -> Vec<u8> {
    use egui::Key;
    if modifiers.ctrl {
        let letter = match key {
            Key::A => 1,
            Key::B => 2,
            Key::C => 3,
            Key::D => 4,
            Key::E => 5,
            Key::F => 6,
            Key::G => 7,
            Key::H => 8,
            Key::I => 9,
            Key::J => 10,
            Key::K => 11,
            Key::L => 12,
            Key::M => 13,
            Key::N => 14,
            Key::O => 15,
            Key::P => 16,
            Key::Q => 17,
            Key::R => 18,
            Key::S => 19,
            Key::T => 20,
            Key::U => 21,
            Key::V => return Vec::new(),
            Key::W => 23,
            Key::X => 24,
            Key::Y => 25,
            Key::Z => 26,
            Key::Space => 0,
            Key::OpenBracket => 27,
            Key::Backslash => 28,
            Key::CloseBracket => 29,
            _ => return Vec::new(),
        };
        return vec![letter];
    }
    let arrow_prefix = if application_cursor { "\x1bO" } else { "\x1b[" };
    match key {
        Key::ArrowUp => format!("{arrow_prefix}A").into_bytes(),
        Key::ArrowDown => format!("{arrow_prefix}B").into_bytes(),
        Key::ArrowRight => format!("{arrow_prefix}C").into_bytes(),
        Key::ArrowLeft => format!("{arrow_prefix}D").into_bytes(),
        Key::Enter => vec![b'\r'],
        Key::Tab => vec![b'\t'],
        Key::Backspace => vec![0x7f],
        Key::Escape => vec![0x1b],
        Key::Home => b"\x1b[H".to_vec(),
        Key::End => b"\x1b[F".to_vec(),
        Key::Insert => b"\x1b[2~".to_vec(),
        Key::Delete => b"\x1b[3~".to_vec(),
        Key::PageUp => b"\x1b[5~".to_vec(),
        Key::PageDown => b"\x1b[6~".to_vec(),
        Key::F1 => b"\x1bOP".to_vec(),
        Key::F2 => b"\x1bOQ".to_vec(),
        Key::F3 => b"\x1bOR".to_vec(),
        Key::F4 => b"\x1bOS".to_vec(),
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(
        app: &mut ButtonBuilder,
        ctx: &egui::Context,
        events: Vec<egui::Event>,
    ) -> egui::FullOutput {
        frame_with(
            app,
            ctx,
            Vec2::new(1200.0, 760.0),
            egui::Modifiers::NONE,
            events,
        )
    }

    fn frame_with(
        app: &mut ButtonBuilder,
        ctx: &egui::Context,
        size: Vec2,
        modifiers: egui::Modifiers,
        events: Vec<egui::Event>,
    ) -> egui::FullOutput {
        ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                modifiers,
                events,
                ..Default::default()
            },
            |ctx| app.show(ctx),
        )
    }

    fn label_center(output: &egui::FullOutput, label: &str) -> egui::Pos2 {
        output
            .shapes
            .iter()
            .find_map(|shape| {
                if let egui::Shape::Text(text) = &shape.shape
                    && text.galley.job.text == label
                {
                    Some(text.pos + text.galley.rect.center().to_vec2())
                } else {
                    None
                }
            })
            .unwrap_or_else(|| panic!("Missing UI label: {label}"))
    }

    fn wheel(
        app: &mut ButtonBuilder,
        ctx: &egui::Context,
        pos: egui::Pos2,
        unit: egui::MouseWheelUnit,
        delta: Vec2,
        modifiers: egui::Modifiers,
    ) {
        frame_with(
            app,
            ctx,
            Vec2::new(1200.0, 760.0),
            modifiers,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::MouseWheel {
                    unit,
                    delta,
                    modifiers,
                },
            ],
        );
    }

    fn click(app: &mut ButtonBuilder, ctx: &egui::Context, pos: egui::Pos2) {
        click_with_modifiers(app, ctx, pos, egui::Modifiers::NONE);
    }

    fn click_with_modifiers(
        app: &mut ButtonBuilder,
        ctx: &egui::Context,
        pos: egui::Pos2,
        modifiers: egui::Modifiers,
    ) {
        for pressed in [true, false] {
            frame_with(
                app,
                ctx,
                Vec2::new(1200.0, 760.0),
                modifiers,
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers,
                    },
                ],
            );
        }
    }

    fn interactive_button(label: &str) -> ButtonConfig {
        ButtonConfig {
            label: label.into(),
            command: format!(
                "printf '{label}-ready\\n'; while IFS= read -r line; do printf '{label}:%s\\n' \"$line\"; done"
            ),
            shell: true,
            ..Default::default()
        }
    }

    fn scrollbar_rects(output: &egui::FullOutput) -> Option<(egui::Rect, egui::Rect)> {
        let track = output.shapes.iter().find_map(|shape| match &shape.shape {
            egui::Shape::Rect(rect)
                if rect.fill == get_global_color("surfaceContainerHighest")
                    && (rect.rect.width() - (TERMINAL_SCROLLBAR_WIDTH - 8.0)).abs() < 0.1 =>
            {
                Some(rect.rect)
            }
            _ => None,
        })?;
        let thumb = output.shapes.iter().find_map(|shape| match &shape.shape {
            egui::Shape::Rect(rect)
                if track.contains_rect(rect.rect)
                    && (rect.fill == get_global_color("outline")
                        || rect.fill == get_global_color("primary")) =>
            {
                Some(rect.rect)
            }
            _ => None,
        })?;
        Some((track, thumb))
    }

    fn wait_for_tabs(
        app: &mut ButtonBuilder,
        ctx: &egui::Context,
        predicate: impl Fn(&ButtonBuilder) -> bool,
    ) {
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            frame(app, ctx, vec![]);
            if predicate(app) {
                return;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "Timed out waiting for terminal tabs"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn ctrl_click_tabs_route_input_and_toolbar_actions_to_the_selected_process() {
        let ctx = egui::Context::default();
        let mut config = Config::default();
        config.buttons[0] = interactive_button("Alpha");
        config.buttons[1] = interactive_button("Beta");
        let mut app = ButtonBuilder::new(
            &ctx,
            Some("/tmp/rs-button-builder-tabs-test.yaml".into()),
            config,
        );
        frame(&mut app, &ctx, vec![]);
        let output = frame(&mut app, &ctx, vec![]);
        click(&mut app, &ctx, label_center(&output, "Alpha"));
        assert_eq!(app.tabs.len(), 1);
        let output = frame(&mut app, &ctx, vec![]);
        click(&mut app, &ctx, label_center(&output, "Beta"));
        assert_eq!(
            app.tabs.len(),
            1,
            "A normal click must not replace a running process"
        );
        let output = frame(&mut app, &ctx, vec![]);
        click_with_modifiers(
            &mut app,
            &ctx,
            label_center(&output, "Beta"),
            egui::Modifiers::CTRL,
        );
        assert_eq!(app.tabs.len(), 2);
        assert_eq!(app.active_tab, Some(1));
        assert!(app.tabs.iter().all(|tab| tab.terminal.is_running()));
        wait_for_tabs(&mut app, &ctx, |app| {
            app.tabs
                .iter()
                .all(|tab| tab.terminal.screen.text().contains("-ready"))
        });

        frame(
            &mut app,
            &ctx,
            vec![
                egui::Event::Text("only-beta".into()),
                egui::Event::Key {
                    key: egui::Key::Enter,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        wait_for_tabs(&mut app, &ctx, |app| {
            app.tabs[1]
                .terminal
                .screen
                .text()
                .contains("Beta:only-beta")
        });
        assert!(!app.tabs[0].terminal.screen.text().contains("only-beta"));
        app.tabs[0].terminal.send(b"background\r");
        wait_for_tabs(&mut app, &ctx, |app| {
            app.tabs[0]
                .terminal
                .screen
                .text()
                .contains("Alpha:background")
        });
        assert_eq!(
            app.active_tab,
            Some(1),
            "Inactive terminals must keep polling"
        );

        let output = frame(&mut app, &ctx, vec![]);
        click(&mut app, &ctx, label_center(&output, "1 · Alpha"));
        assert_eq!(app.active_tab, Some(0));
        let output = frame(&mut app, &ctx, vec![]);
        let copy_pos = label_center(&output, "Copy");
        frame(
            &mut app,
            &ctx,
            vec![
                egui::Event::PointerMoved(copy_pos),
                egui::Event::PointerButton {
                    pos: copy_pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        let copied = frame(
            &mut app,
            &ctx,
            vec![egui::Event::PointerButton {
                pos: copy_pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        assert!(copied.platform_output.commands.iter().any(|command| matches!(command,
            egui::OutputCommand::CopyText(text) if text.contains("Alpha:background") && !text.contains("Beta:only-beta")
        )));
        let output = frame(&mut app, &ctx, vec![]);
        click(&mut app, &ctx, label_center(&output, "Ctrl+C"));
        wait_for_tabs(&mut app, &ctx, |app| !app.tabs[0].terminal.is_running());
        assert!(app.tabs[1].terminal.is_running());

        let output = frame(&mut app, &ctx, vec![]);
        click(&mut app, &ctx, label_center(&output, "Alpha"));
        assert_eq!(app.tabs.len(), 2, "Normal clicks reuse a finished tab");
        assert_eq!(app.tabs[0].id, 1);
        assert!(app.tabs[0].terminal.is_running());
        let output = frame(&mut app, &ctx, vec![]);
        click(&mut app, &ctx, label_center(&output, "Stop"));
        assert!(!app.tabs[0].terminal.is_running());
        assert!(app.tabs[1].terminal.is_running());
        assert_eq!(app.tabs[0].terminal.status, "Stopped");

        let output = frame(&mut app, &ctx, vec![]);
        click(&mut app, &ctx, label_center(&output, "2 · Beta"));
        let pid: i32 = app
            .active()
            .unwrap()
            .terminal
            .status
            .split_whitespace()
            .last()
            .unwrap()
            .parse()
            .unwrap();
        let output = frame(&mut app, &ctx, vec![]);
        click(&mut app, &ctx, label_center(&output, "Close tab"));
        assert_eq!(app.tabs.len(), 1);
        assert_eq!(app.active_tab, Some(0));
        // Reaping is asynchronous; wait for the worker while the UI keeps rendering.
        wait_for_tabs(&mut app, &ctx, |_| {
            // SAFETY: signal zero only checks whether this test's child still exists.
            (unsafe { libc::kill(pid, 0) }) == -1
        });
        // SAFETY: signal zero only checks whether this test's now-reaped child exists.
        assert_eq!(unsafe { libc::kill(pid, 0) }, -1);
        assert_eq!(
            std::io::Error::last_os_error().raw_os_error(),
            Some(libc::ESRCH)
        );
        let output = frame(&mut app, &ctx, vec![]);
        click(&mut app, &ctx, label_center(&output, "Close tab"));
        assert!(app.tabs.is_empty() && app.active_tab.is_none());
        let output = frame(&mut app, &ctx, vec![]);
        click(&mut app, &ctx, label_center(&output, "Alpha"));
        assert_eq!(app.tabs[0].id, 3, "New tabs must not reuse old UI IDs");
    }

    #[test]
    fn noisy_processes_can_be_interrupted_stopped_closed_and_run_alongside_new_tabs() {
        let ctx = egui::Context::default();
        let mut config = Config::default();
        config.buttons[0] = ButtonConfig {
            label: "Flood".into(),
            command: "trap '' INT TERM; while :; do printf 'sample output line\\n'; done".into(),
            shell: true,
            ..Default::default()
        };
        config.buttons[1] = interactive_button("Quiet");
        let mut app = ButtonBuilder::new(
            &ctx,
            Some("/tmp/rs-button-builder-flood-test.yaml".into()),
            config,
        );
        frame(&mut app, &ctx, vec![]);
        let output = frame(&mut app, &ctx, vec![]);
        click(&mut app, &ctx, label_center(&output, "Flood"));
        wait_for_tabs(&mut app, &ctx, |app| {
            !app.tabs[0].terminal.screen.text().trim().is_empty()
        });
        let output = frame(&mut app, &ctx, vec![]);
        click_with_modifiers(
            &mut app,
            &ctx,
            label_center(&output, "Quiet"),
            egui::Modifiers::CTRL,
        );
        assert_eq!(app.tabs.len(), 2);
        wait_for_tabs(&mut app, &ctx, |app| {
            app.tabs[1].terminal.screen.text().contains("Quiet-ready")
        });
        for _ in 0..10 {
            let started = Instant::now();
            frame(&mut app, &ctx, vec![]);
            assert!(
                started.elapsed() < Duration::from_millis(500),
                "Output must not monopolize the UI"
            );
        }
        let output = frame(&mut app, &ctx, vec![]);
        click(&mut app, &ctx, label_center(&output, "1 · Flood"));
        let output = frame(&mut app, &ctx, vec![]);
        click(&mut app, &ctx, label_center(&output, "Ctrl+C"));
        assert!(
            app.tabs[0].terminal.is_running(),
            "The flood deliberately ignores SIGINT"
        );
        let output = frame(&mut app, &ctx, vec![]);
        click(&mut app, &ctx, label_center(&output, "Stop"));
        assert_eq!(app.tabs[0].terminal.status, "Stopped");
        assert!(!app.tabs[0].terminal.needs_poll());
        assert!(app.tabs[1].terminal.is_running());
        let output = frame(&mut app, &ctx, vec![]);
        click(&mut app, &ctx, label_center(&output, "Flood"));
        assert!(app.tabs[0].terminal.is_running());
        let pid: i32 = app.tabs[0]
            .terminal
            .status
            .split_whitespace()
            .last()
            .unwrap()
            .parse()
            .unwrap();
        let output = frame(&mut app, &ctx, vec![]);
        click(&mut app, &ctx, label_center(&output, "Close tab"));
        assert_eq!(app.tabs.len(), 1);
        assert_eq!(app.tabs[0].label, "Quiet");
        wait_for_tabs(&mut app, &ctx, |_| {
            // SAFETY: signal zero only observes this test's child.
            (unsafe { libc::kill(pid, 0) }) == -1
        });
        assert!(app.tabs[0].terminal.is_running());
    }

    #[test]
    fn terminal_scrollbar_drags_history_and_preserves_tab_position_and_input_focus() {
        let ctx = egui::Context::default();
        let mut config = Config::default();
        config.buttons[0] = interactive_button("Alpha");
        config.buttons[1] = interactive_button("Beta");
        let mut app = ButtonBuilder::new(&ctx, Some("/tmp/scrollbar-tabs.yaml".into()), config);
        app.launch(0, false);
        app.launch(1, true);
        wait_for_tabs(&mut app, &ctx, |app| {
            app.tabs
                .iter()
                .all(|tab| tab.terminal.screen.text().contains("-ready"))
        });
        app.select_tab(0);
        frame(&mut app, &ctx, vec![]);
        for i in 0..120 {
            app.tabs[0]
                .terminal
                .screen
                .feed(format!("history-{i}\r\n").as_bytes());
        }
        let output = frame(&mut app, &ctx, vec![]);
        let (track, thumb) = scrollbar_rects(&output).unwrap();
        assert!((thumb.bottom() - track.bottom()).abs() < 0.01);
        let grab = egui::pos2(thumb.center().x, thumb.bottom() - 5.0);
        frame(
            &mut app,
            &ctx,
            vec![
                egui::Event::PointerMoved(grab),
                egui::Event::PointerButton {
                    pos: grab,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        assert_eq!(
            app.tabs[0].scrollback, 0,
            "Grabbing the thumb must not jump"
        );
        frame(
            &mut app,
            &ctx,
            vec![egui::Event::PointerMoved(track.center())],
        );
        assert!(app.tabs[0].scrollback > 0);
        assert!(app.tabs[0].scrollback < app.tabs[0].terminal.screen.history.len());
        frame(
            &mut app,
            &ctx,
            vec![egui::Event::PointerButton {
                pos: track.center(),
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        let saved = app.tabs[0].scrollback;
        app.select_tab(1);
        let output = frame(&mut app, &ctx, vec![]);
        assert!(
            scrollbar_rects(&output).is_none(),
            "No scrollbar without history"
        );
        app.select_tab(0);
        frame(&mut app, &ctx, vec![]);
        assert_eq!(app.tabs[0].scrollback, saved);
        let anchor = app.tabs[0].terminal.screen.history.len() - saved;
        wheel(
            &mut app,
            &ctx,
            track.center(),
            egui::MouseWheelUnit::Line,
            Vec2::new(0.0, 1.0),
            egui::Modifiers::CTRL,
        );
        assert_eq!(
            app.tabs[0].style.font_size, 15.0,
            "Zoom also works over the scrollbar"
        );
        assert_eq!(
            app.tabs[0].terminal.screen.history.len() - app.tabs[0].scrollback,
            anchor
        );
        app.tabs[0].terminal.send(b"arrived\n");
        wait_for_tabs(&mut app, &ctx, |app| {
            app.tabs[0].terminal.screen.text().contains("Alpha:arrived")
        });
        assert_eq!(
            app.tabs[0].terminal.screen.history.len() - app.tabs[0].scrollback,
            anchor,
            "Incoming output must keep the viewed text anchored"
        );
        let output = frame(&mut app, &ctx, vec![]);
        let (track, _) = scrollbar_rects(&output).unwrap();
        click(&mut app, &ctx, track.center_top());
        assert_eq!(
            app.tabs[0].scrollback,
            app.tabs[0].terminal.screen.history.len()
        );
        click(&mut app, &ctx, track.center_bottom() - Vec2::new(0.0, 1.0));
        assert_eq!(app.tabs[0].scrollback, 0, "Bottom resumes live output");
        click(&mut app, &ctx, track.center_top());
        frame(&mut app, &ctx, vec![egui::Event::Text("hello\n".into())]);
        assert_eq!(app.tabs[0].scrollback, 0);
        wait_for_tabs(&mut app, &ctx, |app| {
            app.tabs[0].terminal.screen.text().contains("Alpha:hello")
        });
        assert!(!app.tabs[1].terminal.screen.text().contains("hello"));
    }

    #[test]
    fn terminal_scrollbar_handles_full_buffer_and_alternate_screen_at_minimum_window_size() {
        let ctx = egui::Context::default();
        let mut config = Config::default();
        config.buttons[0] = ButtonConfig {
            label: "Local demo".into(),
            command: "/usr/bin/true".into(),
            ..Default::default()
        };
        let mut app = ButtonBuilder::new(&ctx, Some("/tmp/scrollbar-small.yaml".into()), config);
        app.launch(0, false);
        let small_frame = |app: &mut ButtonBuilder, events| {
            frame_with(
                app,
                &ctx,
                Vec2::new(850.0, 500.0),
                egui::Modifiers::NONE,
                events,
            )
        };
        small_frame(&mut app, vec![]);
        for i in 0..2300 {
            app.tabs[0]
                .terminal
                .screen
                .feed(format!("line-{i}\r\n").as_bytes());
        }
        small_frame(&mut app, vec![]);
        let output = small_frame(&mut app, vec![]);
        let (track, thumb) = scrollbar_rects(&output).unwrap();
        assert!(
            egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(850.0, 500.0))
                .contains_rect(track)
        );
        assert!(track.contains_rect(thumb));
        assert_eq!(thumb.height(), 24.0);
        let grab = thumb.center();
        small_frame(
            &mut app,
            vec![
                egui::Event::PointerMoved(grab),
                egui::Event::PointerButton {
                    pos: grab,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        let above = egui::pos2(grab.x, -100.0);
        small_frame(&mut app, vec![egui::Event::PointerMoved(above)]);
        assert_eq!(app.tabs[0].scrollback, 2000);
        let output = small_frame(&mut app, vec![]);
        let (track, thumb) = scrollbar_rects(&output).unwrap();
        assert_eq!(track.top(), thumb.top());
        let below = egui::pos2(grab.x, 600.0);
        small_frame(&mut app, vec![egui::Event::PointerMoved(below)]);
        assert_eq!(app.tabs[0].scrollback, 0);
        small_frame(
            &mut app,
            vec![egui::Event::PointerButton {
                pos: below,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        app.tabs[0].terminal.screen.feed(b"\x1b[?1049h");
        let output = small_frame(&mut app, vec![]);
        assert!(
            scrollbar_rects(&output).is_none(),
            "Full-screen programs use their alternate screen"
        );
        app.tabs[0].terminal.screen.feed(b"\x1b[?1049l");
        assert!(scrollbar_rects(&small_frame(&mut app, vec![])).is_some());
    }

    #[test]
    fn control_wheel_zooms_only_active_tab_and_preserves_history_anchor() {
        let ctx = egui::Context::default();
        let mut config = Config::default();
        config.buttons[0] = interactive_button("Alpha");
        config.buttons[1] = interactive_button("Beta");
        let mut app = ButtonBuilder::new(
            &ctx,
            Some("/tmp/rs-button-builder-wheel-zoom-test.yaml".into()),
            config,
        );
        app.launch(0, false);
        app.launch(1, true);
        wait_for_tabs(&mut app, &ctx, |app| {
            app.tabs
                .iter()
                .all(|tab| tab.terminal.screen.text().contains("-ready"))
        });
        app.select_tab(0);
        frame(&mut app, &ctx, vec![]);
        for i in 0..100 {
            app.tabs[0]
                .terminal
                .screen
                .feed(format!("history-{i}\r\n").as_bytes());
        }
        app.tabs[0].scrollback = 20;
        app.tabs[0].scroll_fraction = 0.5;
        frame(&mut app, &ctx, vec![]);
        let anchor = app.tabs[0].terminal.screen.history.len() - app.tabs[0].scrollback;
        let first_size = app.terminal_size;
        let second_size = (
            app.tabs[1].terminal.screen.cols,
            app.tabs[1].terminal.screen.rows,
        );
        let pane = egui::pos2(700.0, 400.0);

        wheel(
            &mut app,
            &ctx,
            pane,
            egui::MouseWheelUnit::Line,
            Vec2::new(0.0, 2.0),
            egui::Modifiers::CTRL,
        );
        assert_eq!(app.tabs[0].style.font_size, 16.0);
        assert_eq!(app.tabs[1].style.font_size, 14.0);
        assert_eq!(app.config.terminal.font_size, 16.0);
        assert!(app.dirty);
        assert!(app.terminal_size.0 < first_size.0);
        assert!(app.terminal_size.1 < first_size.1);
        assert_eq!(
            app.terminal_size,
            (
                app.tabs[0].terminal.screen.cols,
                app.tabs[0].terminal.screen.rows
            )
        );
        assert_eq!(
            second_size,
            (
                app.tabs[1].terminal.screen.cols,
                app.tabs[1].terminal.screen.rows
            )
        );
        assert_eq!(
            app.tabs[0].terminal.screen.history.len() - app.tabs[0].scrollback,
            anchor
        );
        assert_eq!(app.tabs[0].scroll_fraction, 0.5);
        assert_eq!(ctx.zoom_factor(), 1.0);

        wheel(
            &mut app,
            &ctx,
            pane,
            egui::MouseWheelUnit::Line,
            Vec2::new(0.0, -1.0),
            egui::Modifiers::CTRL,
        );
        assert_eq!(app.tabs[0].style.font_size, 15.0);
        // egui's smoothed zoom input must not keep changing font size after the wheel stops.
        for _ in 0..12 {
            frame(&mut app, &ctx, vec![]);
        }
        assert_eq!(app.tabs[0].style.font_size, 15.0);
        assert_eq!(
            app.tabs[0].terminal.screen.history.len() - app.tabs[0].scrollback,
            anchor
        );
        let scrollback = app.tabs[0].scrollback;
        for _ in 0..5 {
            wheel(
                &mut app,
                &ctx,
                pane,
                egui::MouseWheelUnit::Point,
                Vec2::new(0.0, 7.0),
                egui::Modifiers::NONE,
            );
        }
        assert!(
            app.tabs[0].scrollback > scrollback,
            "Plain scrolling still moves through history"
        );
        assert_eq!(app.tabs[0].style.font_size, 15.0);
        app.save();
        assert_eq!(
            Config::load_existing(app.config_path.as_deref().unwrap())
                .unwrap()
                .terminal
                .font_size,
            15.0
        );
        app.launch(1, true);
        assert_eq!(app.tabs[2].style.font_size, 15.0);
        assert_eq!(app.tabs[1].style.font_size, 14.0);
    }

    #[test]
    fn control_wheel_requires_hovering_terminal_and_respects_font_limits() {
        let ctx = egui::Context::default();
        let mut app = ButtonBuilder::new(
            &ctx,
            Some("/tmp/rs-button-builder-wheel-limits-test.yaml".into()),
            Config::default(),
        );
        frame(&mut app, &ctx, vec![]);
        let output = frame(&mut app, &ctx, vec![]);
        for pos in [
            egui::pos2(100.0, 300.0),
            label_center(&output, "Appearance"),
        ] {
            wheel(
                &mut app,
                &ctx,
                pos,
                egui::MouseWheelUnit::Line,
                Vec2::new(0.0, 1.0),
                egui::Modifiers::CTRL,
            );
            assert_eq!(app.config.terminal.font_size, 14.0);
            assert!(!app.dirty);
        }
        let pane = egui::pos2(700.0, 400.0);
        wheel(
            &mut app,
            &ctx,
            pane,
            egui::MouseWheelUnit::Point,
            Vec2::new(0.0, 10.0),
            egui::Modifiers::CTRL,
        );
        assert_eq!(app.config.terminal.font_size, 14.25);
        wheel(
            &mut app,
            &ctx,
            pane,
            egui::MouseWheelUnit::Line,
            Vec2::new(0.0, 200.0),
            egui::Modifiers::CTRL,
        );
        assert_eq!(app.config.terminal.font_size, 32.0);
        app.dirty = false;
        wheel(
            &mut app,
            &ctx,
            pane,
            egui::MouseWheelUnit::Line,
            Vec2::new(0.0, 1.0),
            egui::Modifiers::CTRL,
        );
        assert_eq!(app.config.terminal.font_size, 32.0);
        assert!(
            !app.dirty,
            "Scrolling past a limit must not mark the layout changed"
        );
        wheel(
            &mut app,
            &ctx,
            pane,
            egui::MouseWheelUnit::Line,
            Vec2::new(0.0, -200.0),
            egui::Modifiers::CTRL,
        );
        assert_eq!(app.config.terminal.font_size, 8.0);
        app.dirty = false;
        wheel(
            &mut app,
            &ctx,
            pane,
            egui::MouseWheelUnit::Line,
            Vec2::new(1.0, 0.0),
            egui::Modifiers::CTRL,
        );
        assert!(
            !app.dirty,
            "Horizontal scrolling does not zoom the terminal"
        );
        app.appearance_open = true;
        frame(&mut app, &ctx, vec![]);
        frame(&mut app, &ctx, vec![]);
        wheel(
            &mut app,
            &ctx,
            pane,
            egui::MouseWheelUnit::Line,
            Vec2::new(0.0, 1.0),
            egui::Modifiers::CTRL,
        );
        assert_eq!(
            app.config.terminal.font_size, 8.0,
            "A dialog covering the terminal blocks pane zoom"
        );
        assert_eq!(ctx.zoom_factor(), 1.0);
    }

    #[test]
    fn tabs_preserve_appearance_history_and_resize_inactive_processes() {
        let ctx = egui::Context::default();
        let mut config = Config::default();
        config.buttons[0] = interactive_button("Alpha");
        let mut app = ButtonBuilder::new(
            &ctx,
            Some("/tmp/rs-button-builder-tab-appearance-test.yaml".into()),
            config,
        );
        app.launch(0, false);
        app.launch(0, true);
        frame(&mut app, &ctx, vec![]);
        wait_for_tabs(&mut app, &ctx, |app| {
            app.tabs
                .iter()
                .all(|tab| tab.terminal.screen.text().contains("-ready"))
        });
        // Build history without depending on asynchronous process timing.
        for i in 0..100 {
            app.tabs[0]
                .terminal
                .screen
                .feed(format!("history-{i}\r\n").as_bytes());
        }
        app.tabs[0].scrollback = 10;
        app.tabs[0].scroll_fraction = 0.5;
        app.tabs[0].style.font_size = 22.0;
        app.tabs[0].style.bg_color = [10, 30, 40];
        app.select_tab(0);
        let output = frame(&mut app, &ctx, vec![]);
        let history_anchor = app.tabs[0].terminal.screen.history.len() - app.tabs[0].scrollback;
        click(&mut app, &ctx, label_center(&output, "Appearance"));
        let output = frame(&mut app, &ctx, vec![]);
        let pos = label_center(&output, "22.0 pt");
        // The numeric field is editable through keyboard input.
        click(&mut app, &ctx, pos);
        frame_with(
            &mut app,
            &ctx,
            Vec2::new(1200.0, 760.0),
            egui::Modifiers::CTRL,
            vec![egui::Event::Key {
                key: egui::Key::A,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::CTRL,
            }],
        );
        frame(
            &mut app,
            &ctx,
            vec![
                egui::Event::Text("20".into()),
                egui::Event::Key {
                    key: egui::Key::Enter,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        assert_eq!(app.tabs[0].style.font_size, 20.0);
        assert_eq!(app.tabs[1].style.font_size, 14.0);
        let output = frame(&mut app, &ctx, vec![]);
        click(&mut app, &ctx, label_center(&output, "Save appearance"));
        assert_eq!(
            Config::load_existing(app.config_path.as_deref().unwrap())
                .unwrap()
                .terminal
                .font_size,
            20.0
        );
        let output = frame(&mut app, &ctx, vec![]);
        click(&mut app, &ctx, label_center(&output, "Close"));
        let first_cols = app.tabs[0].terminal.screen.cols;
        let first_rows = app.tabs[0].terminal.screen.rows;
        app.select_tab(1);
        let second_output = frame(&mut app, &ctx, vec![]);
        assert!(!second_output.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Text(text) if text.galley.job.text == "Jump to live")));
        assert_eq!(
            app.tabs[0].terminal.screen.history.len() - app.tabs[0].scrollback,
            history_anchor
        );
        assert_eq!(app.tabs[0].scroll_fraction, 0.5);
        for _ in 0..3 {
            frame_with(
                &mut app,
                &ctx,
                Vec2::new(850.0, 500.0),
                egui::Modifiers::NONE,
                vec![],
            );
        }
        assert!(app.tabs[0].terminal.screen.cols < first_cols);
        assert!(app.tabs[0].terminal.screen.rows < first_rows);
        assert!(app.tabs[0].terminal.screen.cols < app.tabs[1].terminal.screen.cols);
        app.select_tab(0);
        let output = frame(&mut app, &ctx, vec![]);
        let output = frame(
            &mut app,
            &ctx,
            vec![egui::Event::PointerMoved(label_center(
                &output,
                "Jump to live",
            ))],
        );
        click(&mut app, &ctx, label_center(&output, "Jump to live"));
        assert_eq!(app.tabs[0].scrollback, 0);
        assert_eq!(app.tabs[0].scroll_fraction, 0.0);
        app.launch(0, true);
        assert_eq!(
            app.tabs[2].style.font_size, 20.0,
            "New tabs inherit the edited appearance default"
        );
        app.select_tab(1);
        app.appearance_open = true;
        frame(&mut app, &ctx, vec![]);
        let output = frame(&mut app, &ctx, vec![]);
        click(&mut app, &ctx, label_center(&output, "Save appearance"));
        assert_eq!(
            Config::load_existing(app.config_path.as_deref().unwrap())
                .unwrap()
                .terminal
                .font_size,
            14.0,
            "Saving an unchanged tab's appearance must make its settings the default"
        );
        assert_eq!(app.tabs[0].style.font_size, 20.0);
        assert_eq!(app.tabs[2].style.font_size, 20.0);
    }

    #[test]
    fn failed_launch_and_config_loading_preserve_multiple_sessions() {
        let ctx = egui::Context::default();
        let mut config = Config::default();
        config.buttons[0] = interactive_button("Alpha");
        config.buttons[1].command = "/this/local/program/does/not/exist".into();
        let mut app = ButtonBuilder::new(
            &ctx,
            Some("/tmp/rs-button-builder-tab-load-test.yaml".into()),
            config,
        );
        app.launch(0, false);
        app.launch(0, true);
        app.tabs[0].style.bg_color = [10, 30, 40];
        app.tabs[1].terminal.screen.feed(b"Retain me");
        let next_id = app.next_tab_id;
        app.launch(1, true);
        assert!(app.error.is_some());
        assert_eq!(app.tabs.len(), 2);
        assert_eq!(app.active_tab, Some(1));
        assert_eq!(app.next_tab_id, next_id);
        app.tabs[1].terminal.stop();
        app.launch(1, false);
        assert!(app.error.is_some());
        assert!(app.tabs[1].terminal.screen.text().contains("Retain me"));
        assert_eq!(app.tabs[1].id, 2);
        app.load_config(
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/terminal-demo.yaml"),
        )
        .unwrap();
        assert_eq!(app.tabs.len(), 2);
        assert_eq!(app.active_tab, Some(1));
        assert!(
            app.running(),
            "A background process still counts as running"
        );
        assert!(app.tabs[0].terminal.is_running());
        assert_eq!(app.tabs[0].label, "Alpha");
        assert_eq!(app.tabs[0].style.bg_color, [10, 30, 40]);
        assert!(app.tabs.iter().all(|tab| tab.button_index.is_none()));
        app.launch(0, true);
        assert_eq!(app.tabs[2].label, app.config.buttons[0].label);
        assert_eq!(app.tabs[2].style.font_size, app.config.terminal.font_size);
        app.close_tab(0);
        assert_eq!(app.active_tab, Some(1));
        assert_eq!(app.active().unwrap().id, 3);
        app.close_tab(1);
        assert_eq!(app.active_tab, Some(0));
        assert_eq!(app.active().unwrap().id, 2);
    }

    #[test]
    fn many_tabs_scroll_and_keep_toolbar_controls_visible_at_minimum_size() {
        let ctx = egui::Context::default();
        let mut config = Config::default();
        config.buttons[0] = interactive_button("A long running command label");
        let mut app = ButtonBuilder::new(
            &ctx,
            Some("/tmp/rs-button-builder-many-tabs-test.yaml".into()),
            config,
        );
        for _ in 0..8 {
            app.launch(0, true);
        }
        let size = Vec2::new(850.0, 500.0);
        let mut output = egui::FullOutput::default();
        for _ in 0..40 {
            output = frame_with(&mut app, &ctx, size, egui::Modifiers::NONE, vec![]);
        }
        let title = "8 · A long running command label";
        let label = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.job.text == title => {
                    Some((text, shape.clip_rect))
                }
                _ => None,
            })
            .unwrap();
        assert!(
            label
                .1
                .contains(label.0.pos + label.0.galley.rect.center().to_vec2()),
            "The newly selected tab must scroll into view"
        );
        for action in ["Appearance", "Copy", "Ctrl+C", "Stop", "Close tab"] {
            let pos = label_center(&output, action);
            assert!(pos.x < size.x && pos.y < size.y);
            assert!(output.shapes.iter().any(|shape| shape.clip_rect.contains(pos)
                && matches!(&shape.shape, egui::Shape::Text(text) if text.galley.job.text == action)));
        }
        let left = label_center(&output, "‹");
        let right = label_center(&output, "›");
        assert!(
            left.x < label.1.left() && right.x > label.1.right(),
            "Arrows must stay outside the scrolling tabs"
        );
        assert!(right.x < size.x && (left.y - right.y).abs() < 1.0);
        for glyph in ["‹", "›"] {
            assert_eq!(output.shapes.iter().filter(|shape|
                matches!(&shape.shape, egui::Shape::Text(text) if text.galley.job.text == glyph)
            ).count(), 1, "Use exactly one arrow at each end");
        }
        let click_arrow = |app: &mut ButtonBuilder, pos| {
            for pressed in [true, false] {
                frame_with(
                    app,
                    &ctx,
                    size,
                    egui::Modifiers::NONE,
                    vec![
                        egui::Event::PointerMoved(pos),
                        egui::Event::PointerButton {
                            pos,
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ],
                );
            }
        };
        click_arrow(&mut app, right);
        assert_eq!(
            app.active_tab,
            Some(7),
            "The next arrow stops at the last tab"
        );
        for expected in (0..7).rev() {
            click_arrow(&mut app, left);
            assert_eq!(app.active_tab, Some(expected));
        }
        click_arrow(&mut app, left);
        assert_eq!(
            app.active_tab,
            Some(0),
            "The previous arrow stops at the first tab"
        );
        for _ in 0..40 {
            output = frame_with(&mut app, &ctx, size, egui::Modifiers::NONE, vec![]);
        }
        let first = label_center(&output, "1 · A long running command label");
        assert!(first.x > left.x && first.x < right.x);
        for expected in 1..8 {
            click_arrow(&mut app, right);
            assert_eq!(app.active_tab, Some(expected));
        }
        assert!(app.tabs.iter().all(|tab| tab.terminal.is_running()));
        // Ctrl-click keeps configuring empty buttons and editing in edit mode.
        app.launch(1, true);
        assert_eq!(app.tabs.len(), 8);
        assert_eq!(app.editor.as_ref().unwrap().index, 1);
        app.editor = None;
        app.edit_mode = true;
        let output = frame(&mut app, &ctx, vec![]);
        click_with_modifiers(
            &mut app,
            &ctx,
            label_center(&output, "A long running command label"),
            egui::Modifiers::CTRL,
        );
        assert_eq!(app.tabs.len(), 8);
        assert_eq!(app.editor.as_ref().unwrap().index, 0);
    }

    #[test]
    fn layout_heading_edits_cancel_save_and_follow_the_loaded_config() {
        let ctx = egui::Context::default();
        let path = std::env::temp_dir().join(format!(
            "rs-button-builder-layout-name-{}.yaml",
            std::process::id()
        ));
        let mut app = ButtonBuilder::new(&ctx, Some(path.clone()), Config::default());
        frame(&mut app, &ctx, vec![]);
        let output = frame(&mut app, &ctx, vec![]);
        click(&mut app, &ctx, label_center(&output, DEFAULT_LAYOUT_NAME));
        assert!(app.layout_name_edit.is_some());
        frame(&mut app, &ctx, vec![]);
        let key = |key, modifiers| egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        };
        let replace_name = |app: &mut ButtonBuilder, name: &str| {
            let select_all = egui::Modifiers {
                command: true,
                ..egui::Modifiers::CTRL
            };
            frame_with(
                app,
                &ctx,
                Vec2::new(1200.0, 760.0),
                select_all,
                vec![key(egui::Key::A, select_all)],
            );
            frame(app, &ctx, vec![egui::Event::Text(name.into())]);
        };
        replace_name(&mut app, "  Workstation · Local  ");
        frame(
            &mut app,
            &ctx,
            vec![key(egui::Key::Enter, egui::Modifiers::NONE)],
        );
        assert_eq!(app.config.display_name, "Workstation · Local");
        assert!(app.dirty && app.layout_name_edit.is_none());
        assert!(app.tabs.is_empty(), "Renaming must not launch a process");
        let output = frame(&mut app, &ctx, vec![]);
        click(&mut app, &ctx, label_center(&output, "Save layout *"));
        assert_eq!(
            Config::load_existing(&path).unwrap().display_name,
            "Workstation · Local"
        );

        let output = frame(&mut app, &ctx, vec![]);
        click(&mut app, &ctx, label_center(&output, "Workstation · Local"));
        frame(&mut app, &ctx, vec![]);
        replace_name(&mut app, "Cancel this change");
        frame(
            &mut app,
            &ctx,
            vec![key(egui::Key::Escape, egui::Modifiers::NONE)],
        );
        assert_eq!(app.config.display_name, "Workstation · Local");
        assert!(!app.dirty && app.layout_name_edit.is_none());

        let output = frame(&mut app, &ctx, vec![]);
        click(&mut app, &ctx, label_center(&output, "Workstation · Local"));
        frame(&mut app, &ctx, vec![]);
        replace_name(&mut app, "Save while typing");
        let output = frame(&mut app, &ctx, vec![]);
        click(&mut app, &ctx, label_center(&output, "Save layout"));
        assert_eq!(
            Config::load_existing(&path).unwrap().display_name,
            "Save while typing"
        );
        assert!(!app.dirty && app.layout_name_edit.is_none());

        let output = frame(&mut app, &ctx, vec![]);
        click(&mut app, &ctx, label_center(&output, "Save while typing"));
        frame(&mut app, &ctx, vec![]);
        replace_name(&mut app, "   ");
        let output = frame(&mut app, &ctx, vec![]);
        click(&mut app, &ctx, label_center(&output, "Edit buttons"));
        assert_eq!(app.config.display_name, DEFAULT_LAYOUT_NAME);
        assert!(app.dirty && app.layout_name_edit.is_none());

        app.load_config(
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/json-streams.yaml"),
        )
        .unwrap();
        let output = frame(&mut app, &ctx, vec![]);
        label_center(&output, "JSON Streams");
        assert!(!app.dirty && app.layout_name_edit.is_none());
        click(&mut app, &ctx, label_center(&output, "JSON Streams"));
        assert!(app.layout_name_edit.is_some());
        app.load_config(
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/system-monitor.yaml"),
        )
        .unwrap();
        assert!(app.layout_name_edit.is_none());
        assert_eq!(app.config.display_name, "System Monitor");
    }

    #[test]
    fn button_logging_checkbox_cancels_and_saves_per_button() {
        let ctx = egui::Context::default();
        let path = std::env::temp_dir().join(format!(
            "rs-button-builder-log-checkbox-{}.yaml",
            std::process::id()
        ));
        let mut app = ButtonBuilder::new(&ctx, Some(path.clone()), Config::default());
        app.edit(0);
        frame(&mut app, &ctx, vec![]);
        let output = frame(&mut app, &ctx, vec![]);
        assert!(!app.editor.as_ref().unwrap().button.log_output);
        click(&mut app, &ctx, label_center(&output, "Log output"));
        assert!(app.editor.as_ref().unwrap().button.log_output);
        let output = frame(&mut app, &ctx, vec![]);
        click(&mut app, &ctx, label_center(&output, "Cancel"));
        assert!(!app.config.buttons[0].log_output);

        app.edit(0);
        let output = frame(&mut app, &ctx, vec![]);
        click(&mut app, &ctx, label_center(&output, "Log output"));
        let output = frame(&mut app, &ctx, vec![]);
        click(&mut app, &ctx, label_center(&output, "Save button"));
        let saved = Config::load_existing(&path).unwrap();
        assert!(saved.buttons[0].log_output);
        assert!(!saved.buttons[1].log_output);

        app.edit(0);
        let output = frame(&mut app, &ctx, vec![]);
        assert!(app.editor.as_ref().unwrap().button.log_output);
        click(&mut app, &ctx, label_center(&output, "Log output"));
        let output = frame(&mut app, &ctx, vec![]);
        click(&mut app, &ctx, label_center(&output, "Save button"));
        assert!(!Config::load_existing(&path).unwrap().buttons[0].log_output);
    }

    #[test]
    fn grid_editor_save_and_launch_work_through_the_ui() {
        let ctx = egui::Context::default();
        let path = std::env::temp_dir().join(format!(
            "rs-button-builder-ui-test-{}.yaml",
            std::process::id()
        ));
        let mut app = ButtonBuilder::new(&ctx, Some(path.clone()), Config::default());
        frame(&mut app, &ctx, vec![]);
        let output = frame(&mut app, &ctx, vec![]);
        let positions: Vec<_> = (0..9)
            .map(|i| {
                label_center(
                    &output,
                    &if i == 0 {
                        "Button 1\n+ Configure".into()
                    } else {
                        format!("Button {}\n+ Configure", i + 1)
                    },
                )
            })
            .collect();
        for row in 0..3 {
            assert!((positions[row * 3].y - positions[row * 3 + 1].y).abs() < 15.0);
            assert!(positions[row * 3].x < positions[row * 3 + 1].x);
            assert!(positions[row * 3 + 1].x < positions[row * 3 + 2].x);
        }
        assert!(positions[0].y < positions[3].y && positions[3].y < positions[6].y);
        click(&mut app, &ctx, positions[1]);
        assert_eq!(app.editor.as_ref().unwrap().index, 1);
        let editor = app.editor.as_mut().unwrap();
        editor.button.label = "Local output".into();
        editor.button.command = "/usr/bin/printf".into();
        editor.button.arguments = "'hello from a button\\n'".into();
        editor.button.fg_color = Some([250, 230, 210]);
        editor.button.bg_color = Some([20, 50, 80]);
        let output = frame(&mut app, &ctx, vec![]);
        click(&mut app, &ctx, label_center(&output, "Save button"));
        assert!(app.editor.is_none());
        assert!(app.tabs.is_empty(), "Saving must not execute the command");
        assert_eq!(
            Config::load_existing(&path).unwrap().buttons[1].label,
            "Local output"
        );
        assert_eq!(
            Config::load_existing(&path).unwrap().buttons[1].fg_color,
            Some([250, 230, 210])
        );
        assert_eq!(
            Config::load_existing(&path).unwrap().buttons[1].bg_color,
            Some([20, 50, 80])
        );
        let output = frame(&mut app, &ctx, vec![]);
        assert!(output.shapes.iter().any(|shape| matches!(
            &shape.shape,
            egui::Shape::Rect(rect) if rect.fill == rgb([20, 50, 80])
        )));
        assert!(output.shapes.iter().any(|shape| matches!(
            &shape.shape,
            egui::Shape::Text(text) if text.galley.job.text == "Local output"
                && text.fallback_color == rgb([250, 230, 210])
                && text.galley.job.sections.iter().all(|section|
                    section.format.color == Color32::PLACEHOLDER
                        || section.format.color == rgb([250, 230, 210]))
        )));
        click(&mut app, &ctx, label_center(&output, "Local output"));
        assert!(!app.tabs.is_empty());
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while app.running() {
            assert!(std::time::Instant::now() < deadline);
            frame(&mut app, &ctx, vec![]);
            std::thread::sleep(Duration::from_millis(10));
        }
        frame(&mut app, &ctx, vec![]);
        assert!(
            app.active()
                .unwrap()
                .terminal
                .screen
                .text()
                .contains("hello from a button")
        );
    }

    #[test]
    fn material_font_selection_and_validation_keep_the_dialog_usable() {
        let ctx = egui::Context::default();
        let path = std::env::temp_dir().join(format!(
            "rs-button-builder-material-test-{}.yaml",
            std::process::id()
        ));
        let mut app = ButtonBuilder::new(&ctx, Some(path), Config::default());
        frame(&mut app, &ctx, vec![]);
        let mut definitions = ctx.fonts(|fonts| fonts.definitions().clone());
        definitions.families.insert(
            FontFamily::Name("Test font".into()),
            definitions.families[&FontFamily::Monospace].clone(),
        );
        ctx.set_fonts(definitions);
        app.fonts = vec!["Monospace".into(), "Test font".into()];
        app.appearance_open = true;
        frame(&mut app, &ctx, vec![]);
        let output = frame(&mut app, &ctx, vec![]);
        assert!(
            label_center(&output, "Terminal appearance").y + 12.0 < label_center(&output, "Font").y,
            "The dialog title must stay above the font field"
        );
        click(&mut app, &ctx, label_center(&output, "Monospace"));
        let output = frame(&mut app, &ctx, vec![]);
        click(&mut app, &ctx, label_center(&output, "Test font"));
        assert_eq!(app.config.terminal.font, "Test font");
        assert!(app.appearance_open && app.dirty);
        let output = frame(&mut app, &ctx, vec![]);
        click(&mut app, &ctx, label_center(&output, "Save appearance"));
        assert!(!app.dirty);
        assert_eq!(
            Config::load_existing(app.config_path.as_deref().unwrap())
                .unwrap()
                .terminal
                .font,
            "Test font"
        );
        let output = frame(&mut app, &ctx, vec![]);
        click(&mut app, &ctx, label_center(&output, "Close"));
        assert!(!app.appearance_open);

        app.edit(0);
        app.editor.as_mut().unwrap().button.label.clear();
        frame(&mut app, &ctx, vec![]);
        let output = frame(&mut app, &ctx, vec![]);
        assert!(
            label_center(&output, "Edit button 1").y + 12.0 < label_center(&output, "Label").y,
            "The dialog title must stay above the editor fields"
        );
        click(&mut app, &ctx, label_center(&output, "Save button"));
        assert!(app.editor.as_ref().unwrap().error.is_some());
        let output = frame(&mut app, &ctx, vec![]);
        assert!(output.shapes.iter().any(|shape| matches!(
            &shape.shape,
            egui::Shape::Text(text) if text.galley.job.text == "Enter a label for the button."
        )));
        click(&mut app, &ctx, label_center(&output, "Cancel"));
        assert!(app.editor.is_none());
        assert_eq!(app.config.buttons[0].label, "Button 1");

        app.edit(0);
        let mut output = egui::FullOutput::default();
        for _ in 0..3 {
            output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        Vec2::new(850.0, 500.0),
                    )),
                    ..Default::default()
                },
                |ctx| app.show(ctx),
            );
        }
        let save = label_center(&output, "Save button");
        assert!(
            save.y < 500.0,
            "Save must remain accessible in the minimum window size"
        );
        assert!(output.shapes.iter().any(|shape| shape.clip_rect.contains(save)
            && matches!(&shape.shape, egui::Shape::Text(text) if text.galley.job.text == "Save button")));

        app.editor = None;
        for appearance in [true, false] {
            app.appearance_open = appearance;
            app.config_picker = if appearance {
                None
            } else {
                app.dirty = true;
                Some(ConfigPicker::new(app.config_path.as_deref()))
            };
            for _ in 0..3 {
                output = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            Vec2::new(850.0, 500.0),
                        )),
                        ..Default::default()
                    },
                    |ctx| app.show(ctx),
                );
            }
            let actions: &[&str] = if appearance {
                &["Save appearance", "Reset appearance", "Close"]
            } else {
                &["Load selected config", "Cancel"]
            };
            for &action in actions {
                let position = label_center(&output, action);
                assert!(position.y < 500.0, "{action} must stay inside the window");
                assert!(output.shapes.iter().any(|shape| shape.clip_rect.contains(position)
                    && matches!(&shape.shape, egui::Shape::Text(text) if text.galley.job.text == action)),
                    "{action} must remain visible without scrolling");
            }
        }
    }

    #[test]
    fn dialog_footers_stay_compact_in_tall_windows() {
        fn filled_rect_at(
            shape: &egui::Shape,
            fill: Color32,
            pos: egui::Pos2,
        ) -> Option<egui::Rect> {
            match shape {
                egui::Shape::Rect(rect) if rect.fill == fill && rect.rect.contains(pos) => {
                    Some(rect.rect)
                }
                egui::Shape::Vec(shapes) => shapes
                    .iter()
                    .find_map(|shape| filled_rect_at(shape, fill, pos)),
                _ => None,
            }
        }
        let ctx = egui::Context::default();
        let path = std::env::temp_dir().join("rs-button-builder-footer-preview.yaml");
        let mut app = ButtonBuilder::new(&ctx, Some(path), Config::default());
        let mut time = 0.0;
        for (title, action) in [
            ("Edit button 1", "Save button"),
            ("Terminal appearance", "Save appearance"),
            ("Load buttons config", "Load selected config"),
            ("Save buttons config", "Save config"),
            ("Create a new layout?", "Save and create"),
        ] {
            app.editor = None;
            app.new_layout_requested = title == "Create a new layout?";
            app.save_as = if title == "Save buttons config" {
                let mut picker = ConfigPicker::save(None);
                picker.path = std::env::temp_dir()
                    .join(format!(
                        "rs-button-builder-footer-save-{}.yaml",
                        std::process::id()
                    ))
                    .display()
                    .to_string();
                Some(SaveAs {
                    picker,
                    config: app.config.clone(),
                    close_editor: false,
                    create_new_layout: false,
                })
            } else {
                None
            };
            app.appearance_open = title == "Terminal appearance";
            app.config_picker = if title == "Load buttons config" {
                Some(ConfigPicker::new(app.config_path.as_deref()))
            } else {
                None
            };
            if title == "Edit button 1" {
                app.edit(0);
            }
            let mut output = egui::FullOutput::default();
            for _ in 0..3 {
                time += 0.25;
                output = ctx.run(
                    egui::RawInput {
                        time: Some(time),
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            Vec2::new(1600.0, 1200.0),
                        )),
                        ..Default::default()
                    },
                    |ctx| app.show(ctx),
                );
            }
            let title_pos = label_center(&output, title);
            let action_pos = label_center(&output, action);
            let dialog = output
                .shapes
                .iter()
                .find_map(|shape| {
                    filled_rect_at(
                        &shape.shape,
                        get_global_color("surfaceContainerHigh"),
                        title_pos,
                    )
                })
                .unwrap();
            let button = output
                .shapes
                .iter()
                .find_map(|shape| {
                    filled_rect_at(&shape.shape, get_global_color("primary"), action_pos)
                })
                .unwrap();
            let bottom_gap = dialog.bottom() - button.bottom();
            assert!(
                bottom_gap <= 26.0,
                "{title} adds {bottom_gap} pixels below its footer in a tall window"
            );
        }
    }

    #[test]
    fn automatic_button_text_contrasts_with_custom_backgrounds() {
        let luminance = |color: Color32| {
            let linear = egui::Rgba::from(color);
            0.2126 * linear.r() + 0.7152 * linear.g() + 0.0722 * linear.b()
        };
        let mut button = ButtonConfig::default();
        for background in [
            [0, 0, 0],
            [255, 255, 255],
            [128, 128, 128],
            [31, 106, 72],
            [35, 79, 128],
            [120, 73, 20],
            [75, 55, 120],
        ] {
            button.bg_color = Some(background);
            let (text, fill) = button_colors(&button);
            let (text, fill) = (luminance(text), luminance(fill));
            let contrast = (text.max(fill) + 0.05) / (text.min(fill) + 0.05);
            assert!(
                contrast >= 4.5,
                "Low contrast on {background:?}: {contrast}"
            );
        }
        button.fg_color = Some([220, 180, 140]);
        assert_eq!(button_colors(&button).0, rgb([220, 180, 140]));
    }

    #[test]
    fn terminal_keys_include_control_codes_and_application_arrows() {
        assert_eq!(key_bytes(egui::Key::C, egui::Modifiers::CTRL, false), [3]);
        assert_eq!(
            key_bytes(egui::Key::ArrowUp, egui::Modifiers::NONE, true),
            b"\x1bOA"
        );
        assert_eq!(
            key_bytes(egui::Key::ArrowUp, egui::Modifiers::NONE, false),
            b"\x1b[A"
        );
        assert_eq!(
            key_bytes(egui::Key::Enter, egui::Modifiers::NONE, false),
            b"\r"
        );
    }

    #[test]
    fn resized_grid_renders_new_rows_and_columns_and_saves_them() {
        let ctx = egui::Context::default();
        let path = std::env::temp_dir().join(format!(
            "rs-button-builder-grid-ui-test-{}.yaml",
            std::process::id()
        ));
        let mut app = ButtonBuilder::new(&ctx, Some(path.clone()), Config::default());
        app.config.resize_grid(2, 4).unwrap();
        app.dirty = true;
        frame(&mut app, &ctx, vec![]);
        let output = frame(&mut app, &ctx, vec![]);
        let fourth = label_center(&output, "Button 4\n+ Configure");
        let fifth = label_center(&output, "Button 5\n+ Configure");
        let eighth = label_center(&output, "Button 8\n+ Configure");
        assert!(fourth.y < fifth.y);
        assert!(fifth.x < eighth.x);
        assert!((fifth.y - eighth.y).abs() < 1.0);
        assert!(!output.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Text(text) if text.galley.job.text == "Button 9\n+ Configure")));
        click(&mut app, &ctx, label_center(&output, "Save layout *"));
        let loaded = Config::load_existing(&path).unwrap();
        assert_eq!((loaded.rows, loaded.columns), (2, 4));
        assert_eq!(
            loaded.buttons.len(),
            9,
            "Hidden buttons must survive saving"
        );
    }

    #[test]
    fn new_layout_toolbar_clears_the_layout_and_preserves_running_terminals() {
        let ctx = egui::Context::default();
        let path = std::env::temp_dir().join(format!(
            "rs-button-builder-new-running-{}.yaml",
            std::process::id()
        ));
        let mut config = Config {
            display_name: "Original layout".into(),
            ..Default::default()
        };
        config.resize_grid(2, 4).unwrap();
        config.buttons[0] = interactive_button("Alpha");
        config.buttons[1] = interactive_button("Beta");
        config.terminal.font_size = 18.0;
        config.terminal.bg_color = [10, 20, 30];
        config.save(&path).unwrap();
        let mut app = ButtonBuilder::new(&ctx, Some(path.clone()), config);
        app.launch(0, false);
        app.launch(1, true);
        wait_for_tabs(&mut app, &ctx, |app| {
            app.tabs
                .iter()
                .all(|tab| tab.terminal.screen.text().contains("-ready"))
        });
        for size in [Vec2::new(1200.0, 760.0), Vec2::new(850.0, 500.0)] {
            frame_with(&mut app, &ctx, size, egui::Modifiers::NONE, vec![]);
            let output = frame_with(&mut app, &ctx, size, egui::Modifiers::NONE, vec![]);
            let positions: Vec<_> = ["New layout", "Load config…", "Save layout", "Exit"]
                .iter()
                .map(|label| label_center(&output, label))
                .collect();
            assert!(positions.windows(2).all(|pair| pair[0].x < pair[1].x));
            for (label, pos) in ["New layout", "Load config…", "Save layout", "Exit"]
                .iter()
                .zip(positions)
            {
                assert!(pos.x < size.x && pos.y < 64.0);
                assert!(output.shapes.iter().any(|shape| shape.clip_rect.contains(pos)
                    && matches!(&shape.shape, egui::Shape::Text(text) if text.galley.job.text == *label)));
            }
        }
        frame(&mut app, &ctx, vec![]);
        let output = frame(&mut app, &ctx, vec![]);
        let active_id = app.active().unwrap().id;
        let text = app.tabs[0].terminal.screen.text();
        click(&mut app, &ctx, label_center(&output, "New layout"));
        assert!(app.config_path.is_none() && !app.dirty && !app.new_layout_requested);
        assert_eq!(
            (
                app.config.rows,
                app.config.columns,
                app.config.buttons.len()
            ),
            (3, 3, 9)
        );
        assert_eq!(app.config.display_name(), DEFAULT_LAYOUT_NAME);
        assert!(
            app.config
                .buttons
                .iter()
                .all(|button| button.command.is_empty())
        );
        assert_eq!(app.config.terminal.font_size, 14.0);
        assert_eq!(app.tabs.len(), 2);
        assert_eq!(app.active().unwrap().id, active_id);
        assert_eq!(app.tabs[0].terminal.screen.text(), text);
        assert!(
            app.tabs
                .iter()
                .all(|tab| tab.terminal.is_running() && tab.button_index.is_none())
        );
        assert!(
            app.tabs
                .iter()
                .all(|tab| tab.style.font_size == 18.0 && tab.style.bg_color == [10, 20, 30])
        );
        assert_eq!(
            Config::load_existing(&path).unwrap().display_name,
            "Original layout"
        );
        app.save();
        assert!(
            app.save_as.as_ref().unwrap().picker.path.is_empty(),
            "New layouts need their own save destination"
        );
    }

    #[test]
    fn new_layout_confirmation_cancels_saves_or_discards_changes() {
        let ctx = egui::Context::default();
        let path = std::env::temp_dir().join(format!(
            "rs-button-builder-new-confirmation-{}.yaml",
            std::process::id()
        ));
        let config = Config {
            display_name: "Original".into(),
            ..Default::default()
        };
        config.save(&path).unwrap();
        let mut app = ButtonBuilder::new(&ctx, Some(path.clone()), config);
        app.config.display_name = "Changed layout".into();
        app.config.resize_grid(1, 2).unwrap();
        app.dirty = true;
        frame(&mut app, &ctx, vec![]);
        let output = frame(&mut app, &ctx, vec![]);
        click(&mut app, &ctx, label_center(&output, "New layout"));
        assert!(app.new_layout_requested && app.dirty);
        let output = frame(&mut app, &ctx, vec![]);
        click(&mut app, &ctx, label_center(&output, "Cancel"));
        assert!(!app.new_layout_requested && app.dirty);
        assert_eq!(app.config.display_name, "Changed layout");
        assert_eq!(app.config_path.as_ref(), Some(&path));
        assert_eq!(
            Config::load_existing(&path).unwrap().display_name,
            "Original"
        );
        app.layout_name_edit = Some("Save this title".into());
        let output = frame(&mut app, &ctx, vec![]);
        click(&mut app, &ctx, label_center(&output, "New layout"));
        let output = frame(&mut app, &ctx, vec![]);
        click(&mut app, &ctx, label_center(&output, "Save and create"));
        assert!(app.config_path.is_none() && !app.dirty && !app.new_layout_requested);
        let saved = Config::load_existing(&path).unwrap();
        assert_eq!(saved.display_name, "Save this title");
        assert_eq!((saved.rows, saved.columns), (1, 2));
        assert_eq!((app.config.rows, app.config.columns), (3, 3));

        app.load_config(path.clone()).unwrap();
        app.config.display_name = "Discard this title".into();
        app.dirty = true;
        let output = frame(&mut app, &ctx, vec![]);
        click(&mut app, &ctx, label_center(&output, "New layout"));
        let output = frame(&mut app, &ctx, vec![]);
        click(
            &mut app,
            &ctx,
            label_center(&output, "Create without saving"),
        );
        assert!(app.config_path.is_none() && !app.dirty && app.save_as.is_none());
        assert_eq!(app.config.display_name(), DEFAULT_LAYOUT_NAME);
        assert_eq!(
            Config::load_existing(&path).unwrap().display_name,
            "Save this title"
        );
    }

    #[test]
    fn new_layout_waits_for_a_successful_first_save_and_preserves_button_edits() {
        let ctx = egui::Context::default();
        let directory = std::env::temp_dir().join(format!(
            "rs-button-builder-new-save-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&directory).unwrap();
        let saved_path = directory.join("previous-layout.yaml");
        let mut app = ButtonBuilder::new(&ctx, None, Config::default());
        app.config.display_name = "Save this layout".into();
        app.dirty = true;
        app.edit(0);
        app.editor.as_mut().unwrap().button.label.clear();
        app.request_new_layout();
        frame(&mut app, &ctx, vec![]);
        let output = frame(&mut app, &ctx, vec![]);
        click(&mut app, &ctx, label_center(&output, "Save and create"));
        assert_eq!(
            app.new_layout_error.as_deref(),
            Some("Enter a label for the button.")
        );
        assert!(app.new_layout_requested && app.editor.is_some() && app.save_as.is_none());
        let output = frame(&mut app, &ctx, vec![]);
        click(&mut app, &ctx, label_center(&output, "Cancel"));
        let editor = app.editor.as_mut().unwrap();
        editor.button.label = "Draft button".into();
        editor.button.command = "/usr/bin/true".into();
        app.request_new_layout();
        let output = frame(&mut app, &ctx, vec![]);
        click(&mut app, &ctx, label_center(&output, "Save and create"));
        assert!(app.save_as.as_ref().unwrap().create_new_layout);
        assert!(app.save_as.as_ref().unwrap().picker.path.is_empty());
        let output = frame(&mut app, &ctx, vec![]);
        click(&mut app, &ctx, label_center(&output, "Cancel"));
        assert!(app.new_layout_requested && app.config_path.is_none() && app.dirty);
        assert_eq!(app.editor.as_ref().unwrap().button.label, "Draft button");
        let output = frame(&mut app, &ctx, vec![]);
        click(&mut app, &ctx, label_center(&output, "Save and create"));
        app.save_as.as_mut().unwrap().picker.path =
            directory.join("missing/layout.yaml").display().to_string();
        let output = frame(&mut app, &ctx, vec![]);
        click(&mut app, &ctx, label_center(&output, "Save config"));
        assert!(app.save_as.as_ref().unwrap().picker.error.is_some());
        assert_eq!(app.config.display_name, "Save this layout");
        assert!(app.config_path.is_none() && app.editor.is_some() && app.dirty);
        app.save_as.as_mut().unwrap().picker.path = saved_path.display().to_string();
        let output = frame(&mut app, &ctx, vec![]);
        click(&mut app, &ctx, label_center(&output, "Save config"));
        assert!(app.save_as.is_none() && app.editor.is_none() && !app.new_layout_requested);
        assert!(app.config_path.is_none() && !app.dirty);
        assert!(
            app.config
                .buttons
                .iter()
                .all(|button| button.command.is_empty())
        );
        let saved = Config::load_existing(&saved_path).unwrap();
        assert_eq!(saved.display_name, "Save this layout");
        assert_eq!(saved.buttons[0].label, "Draft button");
        assert_eq!(saved.buttons[0].command, "/usr/bin/true");
    }

    #[test]
    fn unsaved_layout_requires_a_filename_and_then_reuses_the_selected_file() {
        let ctx = egui::Context::default();
        let path = std::env::temp_dir().join(format!(
            "rs-button-builder-first-save-{}-{}.yaml",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut app = ButtonBuilder::new(&ctx, None, Config::default());
        assert!(app.config_path.is_none());
        assert!(
            app.config
                .buttons
                .iter()
                .all(|button| button.command.is_empty())
        );
        assert!(app.tabs.is_empty());
        app.config.resize_grid(2, 2).unwrap();
        app.dirty = true;
        frame(&mut app, &ctx, vec![]);
        let output = frame(&mut app, &ctx, vec![]);
        click(&mut app, &ctx, label_center(&output, "Save layout…"));
        assert!(app.save_as.as_ref().unwrap().picker.path.is_empty());
        let output = frame(&mut app, &ctx, vec![]);
        click(&mut app, &ctx, label_center(&output, "Save config"));
        assert!(app.save_as.is_some() && app.config_path.is_none());
        for _ in 0..3 {
            let small = frame_with(
                &mut app,
                &ctx,
                Vec2::new(850.0, 500.0),
                egui::Modifiers::NONE,
                vec![],
            );
            for label in ["Save config", "Cancel"] {
                let pos = label_center(&small, label);
                assert!(pos.y < 500.0);
                assert!(small.shapes.iter().any(|shape| shape.clip_rect.contains(pos)
                    && matches!(&shape.shape, egui::Shape::Text(text) if text.galley.job.text == label)));
            }
        }
        let output = frame(&mut app, &ctx, vec![]);
        click(&mut app, &ctx, label_center(&output, "Cancel"));
        assert!(app.save_as.is_none() && app.config_path.is_none() && app.dirty);
        assert!(!path.exists());

        let output = frame(&mut app, &ctx, vec![]);
        click(&mut app, &ctx, label_center(&output, "Save layout…"));
        let output = frame(&mut app, &ctx, vec![]);
        click(
            &mut app,
            &ctx,
            label_center(&output, "Enter a filename or path"),
        );
        frame(
            &mut app,
            &ctx,
            vec![egui::Event::Text(
                path.with_extension("").display().to_string(),
            )],
        );
        let output = frame(&mut app, &ctx, vec![]);
        click(&mut app, &ctx, label_center(&output, "Save config"));
        assert!(app.save_as.is_none() && !app.dirty);
        assert_eq!(app.config_path.as_ref(), Some(&path));
        assert!(!path.with_extension("").exists());
        let saved = Config::load_existing(&path).unwrap();
        assert_eq!((saved.rows, saved.columns), (2, 2));
        assert!(saved.buttons.iter().all(|button| button.command.is_empty()));
        app.config.terminal.font_size = 20.0;
        app.dirty = true;
        let output = frame(&mut app, &ctx, vec![]);
        click(&mut app, &ctx, label_center(&output, "Save layout *"));
        assert!(app.save_as.is_none() && !app.dirty);
        assert_eq!(
            Config::load_existing(&path).unwrap().terminal.font_size,
            20.0
        );
    }

    #[test]
    fn first_button_save_preserves_its_draft_on_cancel_and_error_and_confirms_overwrite() {
        let ctx = egui::Context::default();
        let directory = std::env::temp_dir().join(format!(
            "rs-button-builder-first-button-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&directory).unwrap();
        let existing = directory.join("existing.yaml");
        Config {
            display_name: "Original".into(),
            ..Default::default()
        }
        .save(&existing)
        .unwrap();
        let mut app = ButtonBuilder::new(&ctx, None, Config::default());
        frame(&mut app, &ctx, vec![]);
        let output = frame(&mut app, &ctx, vec![]);
        click(
            &mut app,
            &ctx,
            label_center(&output, "Button 1\n+ Configure"),
        );
        let editor = app.editor.as_mut().unwrap();
        editor.button.label = "Local output".into();
        editor.button.command = "/usr/bin/printf".into();
        editor.button.arguments = "'first button works\\n'".into();
        let output = frame(&mut app, &ctx, vec![]);
        click(&mut app, &ctx, label_center(&output, "Save button"));
        assert_eq!(
            app.save_as.as_ref().unwrap().config.buttons[0].label,
            "Local output"
        );
        let output = frame(&mut app, &ctx, vec![]);
        click(&mut app, &ctx, label_center(&output, "Cancel"));
        assert_eq!(app.editor.as_ref().unwrap().button.label, "Local output");
        assert!(app.config.buttons[0].command.is_empty() && app.config_path.is_none());
        let output = frame(&mut app, &ctx, vec![]);
        click(&mut app, &ctx, label_center(&output, "Save button"));
        app.save_as.as_mut().unwrap().picker.path =
            directory.join("missing/layout.yaml").display().to_string();
        let output = frame(&mut app, &ctx, vec![]);
        click(&mut app, &ctx, label_center(&output, "Save config"));
        assert!(
            app.save_as
                .as_ref()
                .unwrap()
                .picker
                .error
                .as_ref()
                .unwrap()
                .contains("Could not save")
        );
        assert!(app.config_path.is_none() && app.editor.is_some());
        assert!(app.config.buttons[0].command.is_empty());
        app.save_as.as_mut().unwrap().picker.path =
            existing.with_extension("").display().to_string();
        let output = frame(&mut app, &ctx, vec![]);
        click(&mut app, &ctx, label_center(&output, "Save config"));
        assert!(app.save_as.is_some());
        assert_eq!(
            Config::load_existing(&existing).unwrap().display_name,
            "Original"
        );
        let output = frame(&mut app, &ctx, vec![]);
        click(
            &mut app,
            &ctx,
            label_center(&output, "Overwrite existing file"),
        );
        let output = frame(&mut app, &ctx, vec![]);
        click(&mut app, &ctx, label_center(&output, "Save config"));
        assert!(app.save_as.is_none() && app.editor.is_none() && !app.dirty);
        assert_eq!(app.config_path.as_ref(), Some(&existing));
        assert_eq!(
            Config::load_existing(&existing).unwrap().buttons[0].label,
            "Local output"
        );
        assert!(app.tabs.is_empty(), "Saving a config must not run commands");
        app.launch(0, false);
        wait_for_tabs(&mut app, &ctx, |app| {
            app.tabs[0]
                .terminal
                .screen
                .text()
                .contains("first button works")
        });
    }

    #[test]
    fn loading_without_a_config_can_save_the_unsaved_layout_before_replacing_it() {
        let ctx = egui::Context::default();
        let directory = std::env::temp_dir().join(format!(
            "rs-button-builder-save-before-load-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&directory).unwrap();
        let saved = directory.join("created.yaml");
        let loaded = directory.join("selected.yaml");
        Config {
            display_name: "Selected layout".into(),
            ..Default::default()
        }
        .save(&loaded)
        .unwrap();
        let mut app = ButtonBuilder::new(&ctx, None, Config::default());
        frame(&mut app, &ctx, vec![]);
        let output = frame(&mut app, &ctx, vec![]);
        click(&mut app, &ctx, label_center(&output, "Load config…"));
        assert!(app.config_picker.as_ref().unwrap().path.is_empty());
        let output = frame(&mut app, &ctx, vec![]);
        click(&mut app, &ctx, label_center(&output, "Cancel"));
        assert!(app.config_picker.is_none() && app.config_path.is_none());
        app.config.resize_grid(1, 2).unwrap();
        app.dirty = true;
        let output = frame(&mut app, &ctx, vec![]);
        click(&mut app, &ctx, label_center(&output, "Load config…"));
        app.config_picker.as_mut().unwrap().path = loaded.display().to_string();
        let output = frame(&mut app, &ctx, vec![]);
        click(
            &mut app,
            &ctx,
            label_center(&output, "Save current layout first"),
        );
        assert!(app.save_as.is_some() && app.config_picker.is_some());
        let output = frame(&mut app, &ctx, vec![]);
        click(&mut app, &ctx, label_center(&output, "Cancel"));
        assert!(app.save_as.is_none() && app.dirty && app.config_path.is_none());
        assert_eq!(
            app.config_picker.as_ref().unwrap().path,
            loaded.display().to_string()
        );
        let output = frame(&mut app, &ctx, vec![]);
        click(
            &mut app,
            &ctx,
            label_center(&output, "Save current layout first"),
        );
        app.save_as.as_mut().unwrap().picker.path = saved.display().to_string();
        let output = frame(&mut app, &ctx, vec![]);
        click(&mut app, &ctx, label_center(&output, "Save config"));
        assert_eq!(app.config_path.as_ref(), Some(&saved));
        assert!(!app.dirty && app.config_picker.is_some());
        let output = frame(&mut app, &ctx, vec![]);
        click(
            &mut app,
            &ctx,
            label_center(&output, "Load selected config"),
        );
        assert!(app.config_picker.is_none() && !app.dirty);
        assert_eq!(app.config_path.as_ref(), Some(&loaded));
        assert_eq!(app.config.display_name, "Selected layout");
        assert_eq!(
            (
                Config::load_existing(&saved).unwrap().rows,
                Config::load_existing(&saved).unwrap().columns
            ),
            (1, 2)
        );
    }

    #[test]
    fn chooser_loads_an_example_only_after_unsaved_changes_are_replaced() {
        let ctx = egui::Context::default();
        let current = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("buttons.yaml");
        let mut app = ButtonBuilder::new(&ctx, Some(current.clone()), Config::default());
        app.dirty = true;
        frame(&mut app, &ctx, vec![]);
        let output = frame(&mut app, &ctx, vec![]);
        click(&mut app, &ctx, label_center(&output, "Load config…"));
        assert!(app.config_picker.is_some());
        frame(&mut app, &ctx, vec![]);
        let output = frame(&mut app, &ctx, vec![]);
        click(&mut app, &ctx, label_center(&output, "Examples"));
        assert!(app.config_picker.as_ref().unwrap().path.is_empty());
        frame(&mut app, &ctx, vec![]);
        let output = frame(&mut app, &ctx, vec![]);
        click(&mut app, &ctx, label_center(&output, "json-streams.yaml"));
        let output = frame(&mut app, &ctx, vec![]);
        click(
            &mut app,
            &ctx,
            label_center(&output, "Load selected config"),
        );
        assert_eq!(app.config_path.as_ref(), Some(&current));
        assert!(app.dirty);
        let output = frame(&mut app, &ctx, vec![]);
        click(
            &mut app,
            &ctx,
            label_center(&output, "Replace unsaved changes"),
        );
        let output = frame(&mut app, &ctx, vec![]);
        click(
            &mut app,
            &ctx,
            label_center(&output, "Load selected config"),
        );
        assert!(app.config_picker.is_none());
        assert!(!app.dirty);
        assert_eq!((app.config.rows, app.config.columns), (2, 3));
        assert_eq!(app.config.buttons[0].label, "Pretty JSON");
        assert_eq!(app.config.buttons[0].bg_color, Some([75, 55, 120]));
        assert_eq!(app.config.terminal.font_size, 15.0);
        assert_eq!(
            app.config_path.as_ref().unwrap().file_name().unwrap(),
            "json-streams.yaml"
        );
        assert!(app.tabs.is_empty(), "Loading must not execute a command");
    }

    #[test]
    fn failed_load_keeps_layout_and_successful_load_keeps_running_process() {
        let ctx = egui::Context::default();
        let directory = std::env::temp_dir().join(format!(
            "rs-button-builder-load-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&directory).unwrap();
        let current = directory.join("current.yaml");
        let malformed = directory.join("invalid.yaml");
        std::fs::write(&malformed, "rows: [invalid\n").unwrap();
        let mut app = ButtonBuilder::new(&ctx, Some(current.clone()), Config::default());
        app.config.buttons[0].label = "Keep this label".into();
        app.dirty = true;
        app.edit(0);
        for path in [directory.join("missing.yaml"), malformed] {
            assert!(app.load_config(path).is_err());
            assert_eq!(app.config_path.as_ref(), Some(&current));
            assert_eq!(app.config.buttons[0].label, "Keep this label");
            assert!(app.dirty && app.editor.is_some());
        }
        let button = ButtonConfig {
            command: "/bin/sleep".into(),
            arguments: "60".into(),
            ..Default::default()
        };
        let mut terminal = Terminal::start(&button, 80, 24).unwrap();
        terminal.screen.feed(b"Retained output");
        let status = terminal.status.clone();
        app.config.buttons[0] = button;
        app.launch(0, false);
        app.tabs[0].terminal = terminal;
        app.load_config(
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/terminal-demo.yaml"),
        )
        .unwrap();
        assert!(app.running());
        assert_eq!(app.active().unwrap().terminal.status, status);
        assert!(
            app.active()
                .unwrap()
                .terminal
                .screen
                .text()
                .contains("Retained output")
        );
        assert!(app.active().unwrap().button_index.is_none() && app.editor.is_none());
        assert_eq!((app.config.rows, app.config.columns), (2, 2));
    }
}
