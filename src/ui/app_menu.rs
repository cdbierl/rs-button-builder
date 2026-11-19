//! Footer application menu and layout actions.

use super::text_button;
use crate::app::ButtonBuilder;
use crate::config_picker::ConfigPicker;
use egui::{FontId, RichText, Vec2};
use egui_material3::get_global_color;

impl ButtonBuilder {
    pub(in crate::app) fn app_menu(&mut self, ui: &mut egui::Ui) {
        let response = ui
            .add_sized([164.0, 28.0], text_button("Button Builder").small())
            .on_hover_text("Layout actions and application menu");
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
                        RichText::new(self.config.display_name())
                            .small()
                            .color(get_global_color("onSurfaceVariant")),
                    )
                    .wrap(),
                );
                ui.separator();
                if ui.button("New layout").clicked() {
                    self.request_new_layout();
                    ui.close();
                }
                if ui.button("Load config…").clicked() {
                    self.config_picker = Some(ConfigPicker::new(self.config_path.as_deref()));
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
                    .button(save_label)
                    .on_hover_text(self.config_path.as_ref().map_or_else(
                        || "Choose a file to save this layout".into(),
                        |path| format!("Save to {}", path.display()),
                    ))
                    .clicked()
                {
                    self.save();
                    ui.close();
                }
                if ui.button("Save layout as…").clicked() {
                    self.apply_layout_name();
                    self.save_as = Some(crate::app::SaveAs {
                        picker: ConfigPicker::save(self.config_path.as_deref()),
                        config: self.config.clone(),
                        close_editor: false,
                        create_new_layout: false,
                    });
                    ui.close();
                }
                ui.separator();
                if ui.button("Exit").clicked() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                    ui.close();
                }
            });
    }
}
