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
        let mut open = true;
        let mut browse = None;
        let mut refresh = false;
        let mut load = false;
        egui::Window::new("Load buttons config")
            .id(egui::Id::new("config_picker"))
            .open(&mut open)
            .collapsible(false)
            .default_width(560.0)
            .vscroll(true)
            .show(ctx, |ui| {
                ui.label("Config file (.yaml or .yml)");
                let response = ui.add(
                    egui::TextEdit::singleline(&mut self.path)
                        .hint_text("Select a file below or enter its path")
                        .desired_width(f32::INFINITY),
                );
                if response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)) {
                    load = true;
                }
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(
                            self.directory.parent().is_some(),
                            egui::Button::new("Parent folder"),
                        )
                        .clicked()
                    {
                        browse = self.directory.parent().map(Path::to_owned);
                    }
                    let examples = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples");
                    if ui
                        .add_enabled(examples.is_dir(), egui::Button::new("Examples"))
                        .clicked()
                    {
                        browse = Some(examples);
                    }
                    refresh = ui.button("Refresh").clicked();
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
                                if ui.button(format!("Folder: {}", entry.name)).clicked() {
                                    browse = Some(entry.path.clone());
                                }
                            } else {
                                let selected = self.selected_path().as_ref() == Some(&entry.path);
                                let response = ui.selectable_label(selected, &entry.name);
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
                    ui.checkbox(&mut self.replace_unsaved, "Replace unsaved changes");
                    if ui.button("Save current layout first").clicked() {
                        action = Action::SaveCurrent;
                    }
                }
                if running {
                    ui.small("The running process will continue after loading.");
                }
                if let Some(error) = &self.error {
                    ui.colored_label(egui::Color32::LIGHT_RED, error);
                }
                let can_load = (!unsaved || self.replace_unsaved) && self.selected_path().is_some();
                ui.horizontal(|ui| {
                    load |= ui
                        .add_enabled(can_load, egui::Button::new("Load selected config"))
                        .clicked();
                    if ui.button("Cancel").clicked() {
                        action = Action::Cancel;
                    }
                });
                if load && can_load {
                    action = Action::Load(self.selected_path().unwrap());
                }
            });
        if let Some(directory) = browse {
            self.directory = directory;
            self.path.clear();
            self.refresh();
        } else if refresh {
            self.refresh();
        }
        if !open {
            action = Action::Cancel;
        }
        action
    }
}
