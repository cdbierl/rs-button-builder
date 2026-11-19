//! Status messages and a compact application theme picker.
use super::{rgb, style::apply_theme, text_button};
use crate::app::ButtonBuilder;
use crate::theme::{Accent, Appearance};
use egui::{Align, FontId, RichText, Vec2};
use egui_material3::get_global_color;

fn symbol(name: &str) -> RichText {
    RichText::new(egui_material3::material_symbol::material_symbol_text(name)).font(FontId::new(
        18.0,
        egui::FontFamily::Name(crate::fonts::MATERIAL_SYMBOLS.into()),
    ))
}

impl ButtonBuilder {
    pub(in crate::app) fn sync_theme(&mut self, ctx: &egui::Context) {
        let dark = self.theme.is_dark(ctx.system_theme());
        if self.applied_theme != Some((self.theme, dark)) {
            apply_theme(ctx, self.theme, dark);
            self.applied_theme = Some((self.theme, dark));
            ctx.request_repaint();
        }
    }

    pub(in crate::app) fn footer(&mut self, ui: &mut egui::Ui) {
        ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
            self.theme_picker(ui);
            ui.with_layout(egui::Layout::left_to_right(Align::Center), |ui| {
                self.app_menu(ui);
                ui.separator();
                let (message, color) = if let Some(error) = &self.error {
                    (error.as_str(), get_global_color("error"))
                } else {
                    (
                        self.completion_notice
                            .as_ref()
                            .map_or(self.notice.as_str(), |(text, _)| text),
                        get_global_color("onSurfaceVariant"),
                    )
                };
                ui.add(egui::Label::new(RichText::new(message).small().color(color)).truncate())
                    .on_hover_text(message);
            });
        });
    }

    fn theme_picker(&mut self, ui: &mut egui::Ui) {
        let before = self.theme;
        let response = ui
            .add_sized([76.0, 28.0], text_button(symbol("palette")).small())
            .on_hover_text(format!("Choose theme ({})", self.theme.accent.name()));
        response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Theme"));
        ui.painter().circle_filled(
            response.rect.left_center() + Vec2::new(12.0, 0.0),
            5.0,
            rgb(self.theme.accent.colors(false)[0]),
        );
        ui.painter().text(
            response.rect.right_center() - Vec2::new(12.0, 0.0),
            egui::Align2::CENTER_CENTER,
            egui_material3::material_symbol::material_symbol_text("expand_more"),
            FontId::new(
                16.0,
                egui::FontFamily::Name(crate::fonts::MATERIAL_SYMBOLS.into()),
            ),
            get_global_color("onSurfaceVariant"),
        );
        egui::Popup::menu(&response)
            .align(egui::RectAlign::TOP_END)
            .layout(egui::Layout::top_down(Align::Min))
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .show(|ui| {
                ui.horizontal(|ui| {
                    for (mode, label) in [
                        (Appearance::Light, "Light"),
                        (Appearance::Dark, "Dark"),
                        (Appearance::System, "System"),
                    ] {
                        if ui
                            .selectable_label(self.theme.mode == mode, label)
                            .clicked()
                        {
                            self.theme.mode = mode;
                        }
                    }
                });
                ui.separator();
                ui.horizontal(|ui| {
                    for accent in Accent::ALL {
                        let [fill, text, ..] = accent.colors(false);
                        let caption = if self.theme.accent == accent {
                            symbol("check")
                        } else {
                            RichText::new("")
                        };
                        let swatch = ui
                            .add_sized(
                                [32.0, 32.0],
                                text_button(caption)
                                    .small()
                                    .fill(rgb(fill))
                                    .text_color(rgb(text)),
                            )
                            .on_hover_text(accent.name());
                        swatch.widget_info(|| {
                            egui::WidgetInfo::selected(
                                egui::WidgetType::Button,
                                true,
                                self.theme.accent == accent,
                                accent.name(),
                            )
                        });
                        if swatch.clicked() {
                            self.theme.accent = accent;
                        }
                    }
                });
            });
        if self.theme != before {
            if let Err(error) = self.settings.remember_theme(self.theme) {
                self.error = Some(error);
            }
            ui.ctx().request_repaint();
        }
    }
}
