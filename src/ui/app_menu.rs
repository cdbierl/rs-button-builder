//! Footer application menu and layout actions.

use super::shortcuts::Action;
use super::text_button;
use crate::app::ButtonBuilder;
use egui::{FontId, RichText, Vec2};
use egui_material3::get_global_color;

impl ButtonBuilder {
    pub(in crate::app) fn app_menu(&mut self, ui: &mut egui::Ui) {
        let active_layout = self
            .config_path
            .as_deref()
            .and_then(|path| std::path::absolute(path).ok());
        let recent_layouts: Vec<_> = self
            .settings
            .recent_layouts()
            .iter()
            .filter(|path| Some(*path) != active_layout.as_ref() && path.is_file())
            .cloned()
            .collect();
        let has_recent_layouts = !self.settings.recent_layouts().is_empty();
        let mut recent_selection = None;
        let mut clear_recent_layouts = false;
        let response = ui
            .add_sized([164.0, 28.0], text_button("Button Builder").small())
            .on_hover_text("Layout and app actions");
        ui.painter().text(
            response.rect.right_center() - Vec2::new(12.0, 0.0),
            egui::Align2::CENTER_CENTER,
            egui_material3::material_symbol::material_symbol_text("expand_less"),
            FontId::new(
                16.0,
                egui::FontFamily::Name(crate::fonts::MATERIAL_SYMBOLS.into()),
            ),
            get_global_color("onSurfaceVariant"),
        );
        egui::Popup::menu(&response)
            .align(egui::RectAlign::TOP_START)
            .show(|ui| {
                ui.set_min_width(200.0);
                ui.add(
                    egui::Label::new(
                        RichText::new(self.layout_display_label())
                            .small()
                            .color(get_global_color("onSurfaceVariant")),
                    )
                    .wrap(),
                );
                ui.separator();
                if menu_action(ui, "New layout", Action::NewLayout) {
                    self.perform_action(ui.ctx(), Action::NewLayout);
                    ui.close();
                }
                if menu_action(ui, "Load config…", Action::LoadConfig) {
                    self.perform_action(ui.ctx(), Action::LoadConfig);
                    ui.close();
                }
                if has_recent_layouts {
                    ui.menu_button("Recent layouts", |ui| {
                        if recent_layouts.is_empty() {
                            ui.add_enabled(false, egui::Button::new("No recent layouts"));
                        }
                        for path in &recent_layouts {
                            let label = path.file_name().map_or_else(
                                || path.display().to_string(),
                                |name| name.to_string_lossy().into_owned(),
                            );
                            if ui
                                .add(egui::Button::new(label).truncate())
                                .on_hover_text(path.display().to_string())
                                .clicked()
                            {
                                recent_selection = Some(path.clone());
                                ui.close();
                            }
                        }
                        ui.separator();
                        if ui.button("Clear recent layouts").clicked() {
                            clear_recent_layouts = true;
                            ui.close();
                        }
                    });
                }
                if ui
                    .add_enabled(
                        !self.edit_mode,
                        egui::Button::new("Edit layout").shortcut_text(
                            ui.ctx().format_shortcut(&Action::EditLayout.shortcut()),
                        ),
                    )
                    .clicked()
                {
                    self.perform_action(ui.ctx(), Action::EditLayout);
                    ui.close();
                }
                let save_label = if self.config_path.is_none() {
                    "Save layout…"
                } else if self.dirty {
                    "Save layout *"
                } else {
                    "Save layout"
                };
                if ui
                    .add(
                        egui::Button::new(save_label).shortcut_text(
                            ui.ctx().format_shortcut(&Action::SaveLayout.shortcut()),
                        ),
                    )
                    .on_hover_text(self.config_path.as_ref().map_or_else(
                        || "Choose a layout file".into(),
                        |path| format!("Save to {}", path.display()),
                    ))
                    .clicked()
                {
                    self.perform_action(ui.ctx(), Action::SaveLayout);
                    ui.close();
                }
                if menu_action(ui, "Save layout as…", Action::SaveLayoutAs) {
                    self.perform_action(ui.ctx(), Action::SaveLayoutAs);
                    ui.close();
                }
                ui.separator();
                if menu_action(ui, "Exit", Action::Exit) {
                    self.perform_action(ui.ctx(), Action::Exit);
                    ui.close();
                }
            });
        if clear_recent_layouts {
            if let Err(error) = self.settings.clear_recent_layouts() {
                log::warn!("{error}");
                self.error = Some(error);
            }
        } else if let Some(path) = recent_selection {
            self.request_load_config(path);
        }
    }
}

fn menu_action(ui: &mut egui::Ui, label: &str, action: Action) -> bool {
    ui.add(egui::Button::new(label).shortcut_text(ui.ctx().format_shortcut(&action.shortcut())))
        .clicked()
}
