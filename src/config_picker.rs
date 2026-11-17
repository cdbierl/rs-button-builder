use crate::ui::{filled_button, outlined_button, text_button};
use egui_material3::{MaterialCheckbox, get_global_color};
use std::path::{Path, PathBuf};

struct Entry {
    path: PathBuf,
    name: String,
    directory: bool,
}

pub enum Action {
    None,
    Load(PathBuf),
    SaveCurrent,
    Cancel,
}

pub struct ConfigPicker {
    directory: PathBuf,
    pub path: String,
    entries: Vec<Entry>,
    pub error: Option<String>,
    pub replace_unsaved: bool,
}

impl ConfigPicker {
    pub fn new(current: &Path) -> Self {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let current = if current.is_absolute() {
            current.to_owned()
        } else {
            cwd.join(current)
        };
        let mut picker = Self {
            directory: current.parent().unwrap_or(&cwd).to_owned(),
            path: current.display().to_string(),
            entries: Vec::new(),
            error: None,
            replace_unsaved: false,
        };
        picker.refresh();
        picker
    }

    fn refresh(&mut self) {
        let result = (|| -> std::io::Result<Vec<Entry>> {
            let mut entries = Vec::new();
            for entry in std::fs::read_dir(&self.directory)? {
                let entry = entry?;
                let path = entry.path();
                let name = entry.file_name().to_string_lossy().into_owned();
                if name.starts_with('.') {
                    continue;
                }
                let directory = path.is_dir();
                let yaml = path.extension().is_some_and(|extension| {
                    extension.eq_ignore_ascii_case("yaml") || extension.eq_ignore_ascii_case("yml")
                });
                if directory || (path.is_file() && yaml) {
                    entries.push(Entry {
                        path,
                        name,
                        directory,
                    });
                }
            }
            entries.sort_by(|a, b| {
                b.directory
                    .cmp(&a.directory)
                    .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            });
            Ok(entries)
        })();
        match result {
            Ok(entries) => {
                self.entries = entries;
                self.error = None;
            }
            Err(error) => {
                self.entries.clear();
                self.error = Some(format!(
                    "Could not browse {}: {error}",
                    self.directory.display()
                ));
            }
        }
    }

    fn selected_path(&self) -> Option<PathBuf> {
        let text = self.path.trim();
        if text.is_empty() {
            return None;
        }
        let path = PathBuf::from(text);
        Some(if path.is_absolute() {
            path
        } else {
            self.directory.join(path)
        })
    }

    pub fn show(&mut self, ctx: &egui::Context, unsaved: bool, running: bool) -> Action {
        let mut action = Action::None;
        let mut browse = None;
        let mut refresh = false;
        let mut load = false;
        let dialog = crate::ui::dialog(
            ctx,
            "config_picker",
            "Load buttons config",
            500.0_f32.min(ctx.content_rect().width() - 64.0)..=640.0,
            |ui| {
                crate::ui::form(ui, ctx, "config_picker_form", |ui| {
                    ui.label("Config file (.yaml or .yml)");
                    let response = ui.add(
                        crate::ui::text_input(&mut self.path)
                            .hint_text("Select a file below or enter its path"),
                    );
                    if response.lost_focus()
                        && ui.input(|input| input.key_pressed(egui::Key::Enter))
                    {
                        load = true;
                    }
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        if ui
                            .add_enabled(
                                self.directory.parent().is_some(),
                                text_button("Parent folder").small(),
                            )
                            .clicked()
                        {
                            browse = self.directory.parent().map(Path::to_owned);
                        }
                        let examples = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples");
                        if ui
                            .add_enabled(examples.is_dir(), text_button("Examples").small())
                            .clicked()
                        {
                            browse = Some(examples);
                        }
                        refresh = ui.add(text_button("Refresh").small()).clicked();
                    });
                    ui.small(self.directory.display().to_string());
                    egui::ScrollArea::vertical()
                        .id_salt("config_files")
                        .max_height(230.0)
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                            if self.entries.is_empty() {
                                ui.label("No YAML files or folders here.");
                            }
                            for entry in &self.entries {
                                if entry.directory {
                                    if ui
                                        .add(text_button(format!("Folder: {}", entry.name)).small())
                                        .clicked()
                                    {
                                        browse = Some(entry.path.clone());
                                    }
                                } else {
                                    let selected =
                                        self.selected_path().as_ref() == Some(&entry.path);
                                    let mut button = text_button(&entry.name).small();
                                    if selected {
                                        button = button
                                            .fill(get_global_color("primaryContainer"))
                                            .text_color(get_global_color("onPrimaryContainer"));
                                    }
                                    let response =
                                        ui.add_sized([ui.available_width(), 36.0], button);
                                    if response.clicked() {
                                        self.path = entry.path.display().to_string();
                                    }
                                    if response.double_clicked() {
                                        load = true;
                                    }
                                }
                            }
                        });
                    ui.add_space(8.0);
                    if unsaved {
                        ui.label("There are unsaved layout changes or an open button edit.");
                        ui.add(MaterialCheckbox::new(
                            &mut self.replace_unsaved,
                            "Replace unsaved changes",
                        ));
                        if ui
                            .add(outlined_button("Save current layout first").small())
                            .clicked()
                        {
                            action = Action::SaveCurrent;
                        }
                    }
                    if running {
                        ui.small("The running process will continue after loading.");
                    }
                    if let Some(error) = &self.error {
                        ui.colored_label(egui::Color32::LIGHT_RED, error);
                    }
                });
                let can_load = (!unsaved || self.replace_unsaved) && self.selected_path().is_some();
                crate::ui::dialog_footer(ui, |ui| {
                    if ui.add(text_button("Cancel")).clicked() {
                        action = Action::Cancel;
                    }
                    load |= ui
                        .add_enabled(can_load, filled_button("Load selected config"))
                        .clicked();
                });
                if load && can_load {
                    action = Action::Load(self.selected_path().unwrap());
                }
            },
        );
        if let Some(directory) = browse {
            self.directory = directory;
            self.path.clear();
            self.refresh();
        } else if refresh {
            self.refresh();
        }
        if dialog.should_close() {
            action = Action::Cancel;
        }
        action
    }
}
