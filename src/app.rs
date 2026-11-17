use crate::config::{ButtonConfig, Config, MAX_GRID_COLUMNS, MAX_GRID_ROWS, TerminalStyle};
use crate::config_picker::{Action as PickerAction, ConfigPicker};
use crate::terminal::{Terminal, screen::Cell};
use egui::{Color32, FontFamily, FontId, RichText, Stroke, Vec2};
use std::path::PathBuf;
use std::time::Duration;

struct Editor {
    index: usize,
    button: ButtonConfig,
    error: Option<String>,
}

pub struct ButtonBuilder {
    config: Config,
    config_path: PathBuf,
    config_picker: Option<ConfigPicker>,
    terminal: Option<Terminal>,
    active_button: Option<usize>,
    editor: Option<Editor>,
    edit_mode: bool,
    appearance_open: bool,
    fonts: Vec<String>,
    dirty: bool,
    notice: String,
    error: Option<String>,
    terminal_size: (usize, usize),
    focus_terminal: bool,
    scrollback: usize,
    scroll_fraction: f32,
}

impl ButtonBuilder {
    pub fn new(ctx: &egui::Context, config_path: PathBuf, config: Config) -> Self {
        ctx.set_visuals(egui::Visuals::dark());
        ctx.all_styles_mut(|style| {
            style
                .text_styles
                .insert(egui::TextStyle::Small, FontId::proportional(14.0));
        });
        let fonts = crate::fonts::load(ctx);
        Self {
            config,
            config_path,
            config_picker: None,
            terminal: None,
            active_button: None,
            editor: None,
            edit_mode: false,
            appearance_open: false,
            fonts,
            dirty: false,
            notice: "Ready".into(),
            error: None,
            terminal_size: (80, 24),
            focus_terminal: false,
            scrollback: 0,
            scroll_fraction: 0.0,
        }
    }

    fn running(&self) -> bool {
        self.terminal.as_ref().is_some_and(Terminal::is_running)
    }

    fn save(&mut self) {
        match self.config.save(&self.config_path) {
            Ok(()) => {
                self.dirty = false;
                self.error = None;
                self.notice = format!("Saved {}", self.config_path.display());
            }
            Err(error) => self.error = Some(error),
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
        self.config = config;
        self.config_path = path;
        self.editor = None;
        self.appearance_open = false;
        self.edit_mode = false;
        self.active_button = None;
        self.dirty = false;
        self.error = None;
        self.notice = format!("Loaded {}", self.config_path.display());
        Ok(())
    }

    fn config_picker_window(&mut self, ctx: &egui::Context) {
        let running = self.running();
        let unsaved = self.dirty || self.editor.is_some();
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
            PickerAction::None => {}
        }
    }

    fn launch(&mut self, index: usize) {
        if self.config.buttons[index].command.trim().is_empty() {
            self.edit(index);
            return;
        }
        if self.running() {
            return;
        }
        match Terminal::start(
            &self.config.buttons[index],
            self.terminal_size.0,
            self.terminal_size.1,
        ) {
            Ok(terminal) => {
                self.terminal = Some(terminal);
                self.active_button = Some(index);
                self.scrollback = 0;
                self.focus_terminal = true;
                self.error = None;
            }
            Err(error) => self.error = Some(error),
        }
    }

    fn buttons_panel(&mut self, ui: &mut egui::Ui) {
        ui.add_space(10.0);
        ui.heading("Your buttons");
        ui.add_space(4.0);
        ui.label(format!(
            "{} shortcuts. One terminal.",
            self.config.visible_buttons()
        ));
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
        ui.checkbox(&mut self.edit_mode, "Edit buttons");
        ui.add_space(12.0);
        let running = self.running();
        let columns = self.config.columns;
        let width =
            ((ui.available_width() - 8.0 * (columns - 1) as f32) / columns as f32).max(90.0);
        let mut clicked = None;
        let mut edit = None;
        egui::ScrollArea::both()
            .id_salt("grid_scroll")
            .auto_shrink([false, true])
            .max_height((ui.available_height() - 150.0).max(100.0))
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
                            let selected = self.active_button == Some(index) && running;
                            let caption = if configured {
                                button.label.clone()
                            } else {
                                format!("{}\n+ Configure", button.label)
                            };
                            let response = ui
                                .add_enabled_ui(self.edit_mode || !running || !configured, |ui| {
                                    ui.add_sized(
                                        [width, 88.0],
                                        styled_button(button, caption).wrap().selected(selected),
                                    )
                                })
                                .inner;
                            if response.clicked() {
                                clicked = Some(index);
                            }
                            response.context_menu(|ui| {
                                if ui.button("Edit button").clicked() {
                                    edit = Some(index);
                                    ui.close();
                                }
                            });
                            response.on_hover_text(if configured {
                                button.command.as_str()
                            } else {
                                "Click to assign a label and command"
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
                self.launch(index);
            }
        }
        ui.add_space(16.0);
        ui.small("Right-click any button to edit it.");
        let hidden = self.config.buttons.len() - self.config.visible_buttons();
        if hidden > 0 {
            ui.small(format!(
                "{hidden} hidden buttons kept. Expand the grid to restore them."
            ));
        }
        if running {
            ui.small("Stop or quit the current process to run another button.");
        }
        ui.add_space(12.0);
        ui.separator();
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui
                .button(if self.dirty {
                    "Save layout *"
                } else {
                    "Save layout"
                })
                .clicked()
            {
                self.save();
            }
            if ui.button("Load config…").clicked() {
                self.config_picker = Some(ConfigPicker::new(&self.config_path));
            }
        });
        ui.small(self.config_path.display().to_string());
        ui.add_space(8.0);
        ui.small(
            "Buttons and terminal appearance are saved together. Commands run only when clicked.",
        );
    }

    fn editor_window(&mut self, ctx: &egui::Context) {
        let Some(editor) = &mut self.editor else {
            return;
        };
        let mut open = true;
        let mut save = false;
        let mut cancel = false;
        egui::Window::new(format!("Edit button {}", editor.index + 1))
            .id(egui::Id::new("button_editor"))
            .open(&mut open)
            .collapsible(false)
            .default_width(480.0)
            .vscroll(true)
            .show(ctx, |ui| {
                ui.label("Label");
                ui.add(
                    egui::TextEdit::singleline(&mut editor.button.label)
                        .desired_width(f32::INFINITY),
                );
                ui.add_space(8.0);
                ui.checkbox(&mut editor.button.shell, "Shell pipeline (/bin/sh)");
                if editor.button.shell {
                    ui.label("Command or pipeline");
                    ui.add(
                        egui::TextEdit::multiline(&mut editor.button.command)
                            .font(egui::TextStyle::Monospace)
                            .desired_rows(3)
                            .desired_width(f32::INFINITY),
                    );
                    ui.small("Supports pipes, redirects, and shell quoting.");
                } else {
                    ui.label("Program (full path or a name on PATH)");
                    ui.add(
                        egui::TextEdit::singleline(&mut editor.button.command)
                            .hint_text("/usr/bin/top")
                            .desired_width(f32::INFINITY),
                    );
                    ui.label("Arguments (optional)");
                    ui.add(
                        egui::TextEdit::singleline(&mut editor.button.arguments)
                            .hint_text("-d 1")
                            .desired_width(f32::INFINITY),
                    );
                    ui.small("Quote arguments containing spaces: --name \"two words\"");
                }
                ui.add_space(8.0);
                ui.label("Working directory (optional)");
                ui.add(
                    egui::TextEdit::singleline(&mut editor.button.working_directory)
                        .hint_text("Defaults to the app's working directory")
                        .desired_width(f32::INFINITY),
                );
                ui.small("Leave the command empty to make an unassigned button.");
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    let text = ui.visuals().text_color();
                    let fill = ui.visuals().widgets.inactive.bg_fill;
                    let mut foreground =
                        editor
                            .button
                            .foreground
                            .unwrap_or([text.r(), text.g(), text.b()]);
                    let mut background =
                        editor
                            .button
                            .background
                            .unwrap_or([fill.r(), fill.g(), fill.b()]);
                    ui.label("Text color");
                    if ui.color_edit_button_srgb(&mut foreground).changed() {
                        editor.button.foreground = Some(foreground);
                    }
                    ui.label("Background");
                    if ui.color_edit_button_srgb(&mut background).changed() {
                        editor.button.background = Some(background);
                    }
                    if ui.button("Reset colors").clicked() {
                        editor.button.foreground = None;
                        editor.button.background = None;
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
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    save = ui.button("Save button").clicked();
                    cancel = ui.button("Cancel").clicked();
                });
            });
        if save {
            let validation = if editor.button.label.trim().is_empty() {
                Err("Enter a label for the button.".into())
            } else if !editor.button.command.trim().is_empty() {
                editor.button.args().map(|_| ())
            } else {
                Ok(())
            };
            match validation {
                Err(error) => editor.error = Some(error),
                Ok(()) => {
                    let mut config = self.config.clone();
                    config.buttons[editor.index] = editor.button.clone();
                    match config.save(&self.config_path) {
                        Ok(()) => {
                            self.config = config;
                            self.dirty = false;
                            self.notice = "Button saved".into();
                            self.error = None;
                            self.editor = None;
                        }
                        Err(error) => editor.error = Some(error),
                    }
                }
            }
        } else if !open || cancel {
            self.editor = None;
        }
    }

    fn appearance_window(&mut self, ctx: &egui::Context) {
        let mut save = false;
        let mut refresh_fonts = false;
        egui::Window::new("Terminal appearance")
            .open(&mut self.appearance_open)
            .collapsible(false)
            .default_width(350.0)
            .show(ctx, |ui| {
                let style = &mut self.config.terminal;
                egui::ComboBox::from_label("Font")
                    .selected_text(&style.font)
                    .show_ui(ui, |ui| {
                        for name in &self.fonts {
                            self.dirty |= ui
                                .selectable_value(&mut style.font, name.clone(), name)
                                .changed();
                        }
                    });
                refresh_fonts = ui.button("Refresh fonts").clicked();
                if !self.fonts.contains(&style.font) {
                    ui.small("The selected font is unavailable; using built-in Monospace.");
                }
                self.dirty |= ui
                    .add(egui::Slider::new(&mut style.font_size, 8.0..=32.0).text("Font size"))
                    .changed();
                self.dirty |= ui
                    .add(egui::Slider::new(&mut style.line_spacing, 1.0..=1.8).text("Line spacing"))
                    .changed();
                ui.horizontal(|ui| {
                    ui.label("Text");
                    self.dirty |= ui.color_edit_button_srgb(&mut style.foreground).changed();
                    ui.label("Background");
                    self.dirty |= ui.color_edit_button_srgb(&mut style.background).changed();
                });
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    save = ui.button("Save appearance").clicked();
                    if ui.button("Reset appearance").clicked() {
                        *style = TerminalStyle::default();
                        self.dirty = true;
                    }
                });
            });
        if refresh_fonts {
            self.fonts = crate::fonts::load(ctx);
            self.notice = format!(
                "Found {} installed monospace font families",
                self.fonts.len() - 1
            );
        }
        if save {
            self.save();
        }
    }

    fn terminal_panel(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.heading("Terminal");
            if ui.button("Appearance").clicked() {
                self.appearance_open = true;
            }
            if ui
                .add_enabled(self.terminal.is_some(), egui::Button::new("Copy output"))
                .clicked()
                && let Some(terminal) = &self.terminal
            {
                ui.ctx().copy_text(terminal.screen.text());
            }
            if ui
                .add_enabled(self.running(), egui::Button::new("Ctrl+C"))
                .clicked()
                && let Some(terminal) = &mut self.terminal
            {
                terminal.send(b"\x03");
            }
            if ui
                .add_enabled(self.running(), egui::Button::new("Stop"))
                .clicked()
                && let Some(terminal) = &mut self.terminal
            {
                terminal.stop();
            }
        });
        ui.horizontal(|ui| {
            let status = self
                .terminal
                .as_ref()
                .map(|t| t.status.as_str())
                .unwrap_or("Choose a button to start a process");
            ui.label(status);
        });
        ui.horizontal_wrapped(|ui| {
            ui.small(
                "Click the terminal to type · Scroll for history · Ctrl+Shift+C copies output",
            );
            if self.scrollback > 0 && ui.small_button("Jump to live").clicked() {
                self.scrollback = 0;
            }
        });
        ui.add_space(4.0);
        let style = &self.config.terminal;
        let family = if style.font == "Monospace" || !self.fonts.contains(&style.font) {
            FontFamily::Monospace
        } else {
            FontFamily::Name(style.font.clone().into())
        };
        let font = FontId::new(style.font_size, family);
        let (char_width, line_height) = ui.fonts_mut(|fonts| {
            (
                fonts.glyph_width(&font, 'M'),
                fonts.row_height(&font) * style.line_spacing,
            )
        });
        let size = ui.available_size().max(Vec2::splat(1.0));
        let cols = (((size.x - 16.0) / char_width) as usize).clamp(2, 500);
        let rows = (((size.y - 16.0) / line_height) as usize).clamp(2, 200);
        self.terminal_size = (cols, rows);
        let (response, painter) = ui.allocate_painter(size, egui::Sense::click());
        let rect = response.rect;
        painter.rect_filled(rect, 5.0, rgb(style.background));
        if response.clicked() || self.focus_terminal {
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
                5.0,
                Stroke::new(1.0_f32, Color32::from_rgb(76, 142, 205)),
                egui::StrokeKind::Inside,
            );
        }
        let origin = rect.min + Vec2::splat(8.0);
        let Some(terminal) = &mut self.terminal else {
            painter.text(
                origin,
                egui::Align2::LEFT_TOP,
                "Your process output will appear here.\n\nTry Run Top, or configure a button.",
                font,
                rgb(style.foreground),
            );
            return;
        };
        terminal.resize(cols, rows);
        let screen = &terminal.screen;
        let history_len = if screen.is_alternate() {
            0
        } else {
            screen.history.len()
        };
        if response.hovered() {
            let delta = ui.input(|input| input.smooth_scroll_delta.y);
            self.scroll_fraction += delta / line_height;
            let lines = self.scroll_fraction.trunc() as isize;
            self.scroll_fraction -= lines as f32;
            self.scrollback = self.scrollback.saturating_add_signed(lines);
        }
        self.scrollback = self.scrollback.min(history_len);
        let start = history_len.saturating_sub(self.scrollback);
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
        if self.scrollback == 0
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
                Stroke::new(1.0_f32, rgb(style.foreground)),
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
                    terminal.send(&bytes);
                    self.scrollback = 0;
                }
            }
        }
    }

    fn show(&mut self, ctx: &egui::Context) {
        if let Some(terminal) = &mut self.terminal {
            let previous = terminal.screen.scrolled_lines;
            terminal.poll();
            if self.scrollback > 0 {
                self.scrollback = self
                    .scrollback
                    .saturating_add(terminal.screen.scrolled_lines.saturating_sub(previous));
            }
        }
        egui::TopBottomPanel::top("header").show(ctx, |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.heading("Button Builder");
                ui.separator();
                ui.label("A home for your commands");
            });
            ui.add_space(6.0);
        });
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
        self.editor_window(ctx);
        self.appearance_window(ctx);
        self.config_picker_window(ctx);
        if self.terminal.as_ref().is_some_and(Terminal::needs_poll) {
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

fn styled_button(button: &ButtonConfig, caption: String) -> egui::Button<'static> {
    let mut text = RichText::new(caption).size(15.0);
    if let Some(foreground) = button.foreground {
        text = text.color(rgb(foreground));
    }
    let mut widget = egui::Button::new(text);
    if let Some(background) = button.background {
        widget = widget.fill(rgb(background));
    }
    widget
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
        let mut foreground = rgb(style.foreground.unwrap_or(terminal_style.foreground));
        let mut background = rgb(style.background.unwrap_or(terminal_style.background));
        if style.inverse {
            std::mem::swap(&mut foreground, &mut background);
        }
        let position = origin + Vec2::new(start as f32 * char_width, 0.0);
        let width = (end - start) as f32 * char_width;
        let rect = egui::Rect::from_min_size(position, Vec2::new(width, line_height));
        if background != rgb(terminal_style.background) {
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
        ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(1200.0, 760.0),
                )),
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

    fn click(app: &mut ButtonBuilder, ctx: &egui::Context, pos: egui::Pos2) {
        for pressed in [true, false] {
            frame(
                app,
                ctx,
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
    }

    #[test]
    fn grid_editor_save_and_launch_work_through_the_ui() {
        let ctx = egui::Context::default();
        let path = std::env::temp_dir().join(format!(
            "rs-button-builder-ui-test-{}.yaml",
            std::process::id()
        ));
        let mut app = ButtonBuilder::new(&ctx, path.clone(), Config::default());
        frame(&mut app, &ctx, vec![]);
        let output = frame(&mut app, &ctx, vec![]);
        let positions: Vec<_> = (0..9)
            .map(|i| {
                label_center(
                    &output,
                    &if i == 0 {
                        "Run Top".into()
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
        editor.button.foreground = Some([250, 230, 210]);
        editor.button.background = Some([20, 50, 80]);
        let output = frame(&mut app, &ctx, vec![]);
        click(&mut app, &ctx, label_center(&output, "Save button"));
        assert!(app.editor.is_none());
        assert!(
            app.terminal.is_none(),
            "Saving must not execute the command"
        );
        assert_eq!(
            Config::load(&path).unwrap().buttons[1].label,
            "Local output"
        );
        assert_eq!(
            Config::load(&path).unwrap().buttons[1].foreground,
            Some([250, 230, 210])
        );
        assert_eq!(
            Config::load(&path).unwrap().buttons[1].background,
            Some([20, 50, 80])
        );
        let output = frame(&mut app, &ctx, vec![]);
        click(&mut app, &ctx, label_center(&output, "Local output"));
        assert!(app.terminal.is_some());
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while app.running() {
            assert!(std::time::Instant::now() < deadline);
            frame(&mut app, &ctx, vec![]);
            std::thread::sleep(Duration::from_millis(10));
        }
        frame(&mut app, &ctx, vec![]);
        assert!(
            app.terminal
                .as_ref()
                .unwrap()
                .screen
                .text()
                .contains("hello from a button")
        );
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
        let mut app = ButtonBuilder::new(&ctx, path.clone(), Config::default());
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
        let loaded = Config::load(&path).unwrap();
        assert_eq!((loaded.rows, loaded.columns), (2, 4));
        assert_eq!(
            loaded.buttons.len(),
            9,
            "Hidden buttons must survive saving"
        );
    }

    #[test]
    fn chooser_loads_an_example_only_after_unsaved_changes_are_replaced() {
        let ctx = egui::Context::default();
        let current = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("buttons.yaml");
        let mut app = ButtonBuilder::new(&ctx, current.clone(), Config::default());
        app.dirty = true;
        frame(&mut app, &ctx, vec![]);
        let output = frame(&mut app, &ctx, vec![]);
        click(&mut app, &ctx, label_center(&output, "Load config…"));
        assert!(app.config_picker.is_some());
        let output = frame(&mut app, &ctx, vec![]);
        click(&mut app, &ctx, label_center(&output, "Examples"));
        let output = frame(&mut app, &ctx, vec![]);
        click(&mut app, &ctx, label_center(&output, "json-streams.yaml"));
        let output = frame(&mut app, &ctx, vec![]);
        click(
            &mut app,
            &ctx,
            label_center(&output, "Load selected config"),
        );
        assert_eq!(app.config_path, current);
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
        assert_eq!(app.config.buttons[0].background, Some([75, 55, 120]));
        assert_eq!(app.config.terminal.font_size, 15.0);
        assert_eq!(app.config_path.file_name().unwrap(), "json-streams.yaml");
        assert!(app.terminal.is_none(), "Loading must not execute a command");
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
        let mut app = ButtonBuilder::new(&ctx, current.clone(), Config::default());
        app.config.buttons[0].label = "Keep this label".into();
        app.dirty = true;
        app.edit(0);
        for path in [directory.join("missing.yaml"), malformed] {
            assert!(app.load_config(path).is_err());
            assert_eq!(app.config_path, current);
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
        app.terminal = Some(terminal);
        app.active_button = Some(0);
        app.load_config(
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/terminal-demo.yaml"),
        )
        .unwrap();
        assert!(app.running());
        assert_eq!(app.terminal.as_ref().unwrap().status, status);
        assert!(
            app.terminal
                .as_ref()
                .unwrap()
                .screen
                .text()
                .contains("Retained output")
        );
        assert!(app.active_button.is_none() && app.editor.is_none());
        assert_eq!((app.config.rows, app.config.columns), (2, 2));
    }
}
