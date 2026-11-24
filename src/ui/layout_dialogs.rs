//! Layout loading, saving, and unsaved-change confirmation dialogs.

use super::{
    LayoutFileAction as FileAction, dialog, dialog_footer, filled_button, outlined_button,
    text_button,
};
use crate::app::ButtonBuilder;
use egui::Color32;

impl ButtonBuilder {
    pub(in crate::app) fn save_as_window(&mut self, ctx: &egui::Context) {
        let Some(mut save_as) = self.save_as.take() else {
            return;
        };
        match save_as.file_picker.show(ctx, false, false) {
            FileAction::Save(path) => match save_as.config.save(&path) {
                Ok(()) => {
                    self.finish_save(save_as.config, path, save_as.close_editor);
                    if save_as.create_new_layout {
                        self.create_new_layout();
                    }
                }
                Err(error) => {
                    save_as.file_picker.error = Some(error);
                    self.save_as = Some(save_as);
                }
            },
            FileAction::Cancel => {}
            _ => self.save_as = Some(save_as),
        }
    }

    pub(in crate::app) fn new_layout_window(&mut self, ctx: &egui::Context) {
        if !self.new_layout_requested {
            return;
        }
        let mut save = false;
        let mut discard = false;
        let mut cancel = false;
        let dialog = dialog(
            ctx,
            "new_layout",
            "Create a new layout?",
            480.0..=560.0,
            |ui| {
                ui.label("Save your changes before starting a new layout?");
                if self.workspace.running() {
                    ui.small("Running terminals will stay open.");
                }
                if let Some(error) = &self.new_layout_error {
                    ui.colored_label(Color32::LIGHT_RED, error);
                }
                dialog_footer(ui, |ui| {
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

    pub(in crate::app) fn layout_file_picker_window(&mut self, ctx: &egui::Context) {
        let running = self.workspace.running();
        let unsaved = self.has_unsaved_changes();
        let Some(picker) = &mut self.layout_file_picker else {
            return;
        };
        match picker.show(ctx, unsaved, running) {
            FileAction::Load(path) => match self.load_config(path) {
                Ok(()) => self.layout_file_picker = None,
                Err(error) => {
                    if let Some(picker) = &mut self.layout_file_picker {
                        picker.error = Some(error);
                    }
                }
            },
            FileAction::SaveCurrent => {
                self.save();
                if let Some(picker) = &mut self.layout_file_picker {
                    picker.error = self.error.clone();
                }
            }
            FileAction::Cancel => self.layout_file_picker = None,
            FileAction::None | FileAction::Save(_) => {}
        }
    }
}
