//! Button configuration dialog and preview.

use super::{
    button_colors, dialog, dialog_footer, filled_button, form, styled_button, text_button,
    text_input,
};
use crate::app::{ButtonBuilder, validate_button};
use egui::Color32;
use egui_material3::MaterialCheckbox;

impl ButtonBuilder {
    pub(in crate::app) fn editor_window(&mut self, ctx: &egui::Context) {
        let Some(editor) = &mut self.editor else {
            return;
        };
        let mut save = false;
        let mut cancel = false;
        let dialog = dialog(
            ctx,
            "button_editor",
            format!("Edit button {}", editor.index + 1),
            480.0_f32.min(ctx.content_rect().width() - 64.0)..=560.0,
            |ui| {
                form(ui, ctx, "button_editor_form", |ui| {
                    ui.label("Label");
                    ui.add(text_input(&mut editor.button.label));
                    ui.add_space(8.0);
                    ui.horizontal_wrapped(|ui| {
                        ui.add(MaterialCheckbox::new(
                            &mut editor.button.shell,
                            "Shell script (/bin/sh)",
                        ));
                        ui.add(MaterialCheckbox::new(&mut editor.button.log_output, "Log output"))
                            .on_hover_text("Save this button's output to a file. Requires BUTTON_BUILDER_LOG_DIR in .env; applies to new runs.");
                        ui.add(MaterialCheckbox::new(&mut editor.button.auto_start, "Auto start"))
                            .on_hover_text("Open this button in a terminal tab when its layout is loaded.");
                        ui.add(MaterialCheckbox::new(&mut editor.button.close_on_success, "Close on success"))
                            .on_hover_text("Close after a successful exit, even with output. Interrupted runs and terminal or logging errors stay open; applies to new runs.");
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
                        ui.add(text_input(&mut editor.button.command).hint_text("/usr/bin/top"));
                        ui.label("Arguments (optional)");
                        ui.add(text_input(&mut editor.button.arguments).hint_text("-d 1"));
                        ui.small("Quote arguments containing spaces: --name \"two words\"");
                    }
                    ui.add_space(8.0);
                    ui.label("Working directory (optional)");
                    ui.add(
                        text_input(&mut editor.button.working_directory)
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
                dialog_footer(ui, |ui| {
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
}
