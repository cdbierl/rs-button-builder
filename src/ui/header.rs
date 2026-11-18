//! Application toolbar and layout actions.

use super::{filled_button, material_icon, outlined_button};
use crate::app::ButtonBuilder;
use crate::config_picker::ConfigPicker;
use egui::Vec2;
use egui_material3::MaterialToolbar;

impl ButtonBuilder {
    pub(in crate::app) fn header(&mut self, ui: &mut egui::Ui) {
        ui.add(
            MaterialToolbar::new()
                .top(true)
                .min_height(64.0)
                .padding(Vec2::new(20.0, 12.0))
                .item_fn(|ui| ui.add(material_icon("apps")))
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
}
