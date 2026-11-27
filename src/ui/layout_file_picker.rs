//! Layout file picker.

use super::{Checkbox, filled_button, outlined_button, text_button};
use egui_material3::get_global_color;
use std::path::{Path, PathBuf};

struct Entry {
    path: PathBuf,
    name: String,
    directory: bool,
}

pub enum Action {
    None,
    Load(PathBuf),
    Save(PathBuf),
    SaveCurrent,
    Cancel,
}

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Load,
    Save,
}

pub struct LayoutFilePicker {
    mode: Mode,
    directory: PathBuf,
    pub path: String,
    entries: Vec<Entry>,
    pub error: Option<String>,
    pub replace_unsaved: bool,
    overwrite_existing: bool,
}

impl LayoutFilePicker {
    pub fn new(current: Option<&Path>) -> Self {
        Self::with_mode(current, Mode::Load)
    }

    pub fn save(current: Option<&Path>) -> Self {
        Self::with_mode(current, Mode::Save)
    }

    fn with_mode(current: Option<&Path>, mode: Mode) -> Self {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let current = current.map(|path| {
            if path.is_absolute() {
                path.to_owned()
            } else {
                cwd.join(path)
            }
        });
        let mut picker = Self {
            mode,
            directory: current
                .as_ref()
                .and_then(|path| path.parent())
                .unwrap_or(&cwd)
                .to_owned(),
            path: current
                .map(|path| path.display().to_string())
                .unwrap_or_default(),
            entries: Vec::new(),
            error: None,
            replace_unsaved: false,
            overwrite_existing: false,
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
        let mut path = if path.is_absolute() {
            path
        } else {
            self.directory.join(path)
        };
        if self.mode == Mode::Save
            && !path.is_dir()
            && path
                .extension()
                .is_none_or(|extension| extension.is_empty())
        {
            path.set_extension("yaml");
        }
        Some(path)
    }

    pub fn show(&mut self, ctx: &egui::Context, unsaved: bool, running: bool) -> Action {
        let mut action = Action::None;
        let mut browse = None;
        let mut refresh = false;
        let mut submit = false;
        let saving = self.mode == Mode::Save;
        let dialog = crate::ui::dialog(
            ctx,
            if saving {
                "layout_file_save_picker"
            } else {
                "layout_file_picker"
            },
            if saving {
                "Save buttons config"
            } else {
                "Load buttons config"
            },
            500.0_f32.min(ctx.content_rect().width() - 64.0)..=640.0,
            |ui| {
                crate::ui::form(ui, ctx, "layout_file_picker_form", |ui| {
                    ui.label("Config file (.yaml or .yml)");
                    let response = crate::ui::text_edit_mouse_actions(
                        ui.add(crate::ui::text_input(&mut self.path).hint_text(if saving {
                            "Enter a filename or path"
                        } else {
                            "Select a file below or enter its path"
                        })),
                        &self.path,
                    );
                    if response.changed() {
                        self.overwrite_existing = false;
                    }
                    if response.lost_focus()
                        && ui.input(|input| input.key_pressed(egui::Key::Enter))
                    {
                        submit = true;
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
                                        self.overwrite_existing = false;
                                    }
                                    if response.double_clicked() {
                                        submit = true;
                                    }
                                }
                            }
                        });
                    ui.add_space(8.0);
                    if saving && self.selected_path().is_some_and(|path| path.is_file()) {
                        ui.add(Checkbox::new(
                            &mut self.overwrite_existing,
                            "Overwrite existing file",
                        ));
                    }
                    if !saving && unsaved {
                        ui.label("There are unsaved layout changes or an open button edit.");
                        ui.add(Checkbox::new(
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
                    if !saving && running {
                        ui.small("Running processes will continue after loading.");
                    }
                    if let Some(error) = &self.error {
                        ui.colored_label(egui::Color32::LIGHT_RED, error);
                    }
                });
                let selected = self.selected_path();
                let can_submit = selected.as_ref().is_some_and(|path| {
                    if saving {
                        !path.is_dir() && (!path.exists() || self.overwrite_existing)
                    } else {
                        !unsaved || self.replace_unsaved
                    }
                });
                crate::ui::dialog_footer(ui, |ui| {
                    if ui.add(text_button("Cancel")).clicked() {
                        action = Action::Cancel;
                    }
                    submit |= ui
                        .add_enabled(
                            can_submit,
                            filled_button(if saving {
                                "Save config"
                            } else {
                                "Load selected config"
                            }),
                        )
                        .clicked();
                });
                if submit && can_submit {
                    let path = selected.unwrap();
                    action = if saving {
                        Action::Save(path)
                    } else {
                        Action::Load(path)
                    };
                }
            },
        );
        if let Some(directory) = browse {
            self.directory = directory;
            self.path.clear();
            self.overwrite_existing = false;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_paths_add_yaml_only_when_needed_and_load_paths_stay_exact() {
        let directory = std::env::temp_dir().join(format!(
            "rs-button-builder-config-extension-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&directory).unwrap();
        let current = directory.join("selected.yaml");
        let mut save = LayoutFilePicker::save(Some(&current));
        let mut load = LayoutFilePicker::new(Some(&current));
        for (input, expected) in [
            ("targeting", "targeting.yaml"),
            ("nested/My buttons", "nested/My buttons.yaml"),
            ("layout.yaml", "layout.yaml"),
            ("layout.yml", "layout.yml"),
            ("layout.YAML", "layout.YAML"),
            ("layout.custom", "layout.custom"),
            (".layout", ".layout.yaml"),
            ("layout.", "layout.yaml"),
        ] {
            save.path = input.into();
            load.path = input.into();
            assert_eq!(save.selected_path(), Some(directory.join(expected)));
            assert_eq!(load.selected_path(), Some(directory.join(input)));
        }
        save.path = directory.join("Absolute path").display().to_string();
        assert_eq!(
            save.selected_path(),
            Some(directory.join("Absolute path.yaml"))
        );
        save.path = directory.display().to_string();
        assert_eq!(save.selected_path(), Some(directory));
        save.path = "   ".into();
        assert!(save.selected_path().is_none());
    }
}
