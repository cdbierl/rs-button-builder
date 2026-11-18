//! Terminal font, spacing, and color settings.

use super::{dialog, dialog_footer, filled_button, font_picker, form, text_button};
use crate::app::ButtonBuilder;
use crate::layout_config::TerminalStyle;
use crate::terminal::colors::Palette;
use egui::Vec2;
use egui_material3::MaterialSlider;

impl ButtonBuilder {
    pub(in crate::app) fn appearance_window(&mut self, ctx: &egui::Context) {
        if !self.appearance_open {
            return;
        }
        let mut save = false;
        let mut refresh_fonts = false;
        let mut close = false;
        let mut changed = false;
        let dialog = dialog(
            ctx,
            "terminal_appearance",
            "Terminal appearance",
            400.0_f32.min(ctx.content_rect().width() - 64.0)..=480.0,
            |ui| {
                form(ui, ctx, "appearance_form", |ui| {
                    let style = match self.active_tab {
                        Some(index) => &mut self.tabs[index].style,
                        None => &mut self.config.terminal,
                    };
                    changed |= font_picker(ui, &mut style.font, &self.fonts);
                    refresh_fonts = ui.add(text_button("Refresh fonts").small()).clicked();
                    if !self.fonts.contains(&style.font) {
                        ui.small("The selected font is unavailable; using built-in Monospace.");
                    }
                    ui.label("Font size");
                    ui.horizontal(|ui| {
                        changed |= ui
                            .add(
                                MaterialSlider::new(&mut style.font_size, 8.0..=32.0)
                                    .show_value(false)
                                    .width((ui.available_width() - 100.0).max(100.0)),
                            )
                            .changed();
                        changed |= ui
                            .add(
                                egui::DragValue::new(&mut style.font_size)
                                    .range(8.0..=32.0)
                                    .speed(0.25)
                                    .fixed_decimals(1)
                                    .suffix(" pt"),
                            )
                            .changed();
                    });
                    ui.label("Line spacing");
                    ui.horizontal(|ui| {
                        changed |= ui
                            .add(
                                MaterialSlider::new(&mut style.line_spacing, 1.0..=1.8)
                                    .show_value(false)
                                    .width((ui.available_width() - 100.0).max(100.0)),
                            )
                            .changed();
                        changed |= ui
                            .add(
                                egui::DragValue::new(&mut style.line_spacing)
                                    .range(1.0..=1.8)
                                    .speed(0.01)
                                    .fixed_decimals(2)
                                    .suffix("×"),
                            )
                            .changed();
                    });
                    ui.horizontal(|ui| {
                        let colors = Palette::new(
                            self.theme.is_dark(ctx.system_theme()),
                            style.fg_color,
                            style.bg_color,
                        );
                        let mut foreground = colors.foreground;
                        let mut background = colors.background;
                        ui.label("Text");
                        if ui.color_edit_button_srgb(&mut foreground).changed() {
                            style.fg_color = Some(foreground);
                            changed = true;
                        }
                        ui.label("Background");
                        if ui.color_edit_button_srgb(&mut background).changed() {
                            style.bg_color = Some(background);
                            changed = true;
                        }
                        if ui
                            .add(text_button("Use theme colors").small())
                            .on_hover_text("Follow the footer's Light, Dark, or System setting")
                            .clicked()
                        {
                            changed |= style.fg_color.is_some() || style.bg_color.is_some();
                            style.fg_color = None;
                            style.bg_color = None;
                        }
                    });
                });
                dialog_footer(ui, |ui| {
                    close = ui
                        .add(text_button("Close").small().min_size(Vec2::new(0.0, 40.0)))
                        .clicked();
                    if ui
                        .add(
                            text_button("Reset appearance")
                                .small()
                                .min_size(Vec2::new(0.0, 40.0)),
                        )
                        .clicked()
                    {
                        self.config.terminal = TerminalStyle::default();
                        if let Some(index) = self.active_tab {
                            self.tabs[index].style = TerminalStyle::default();
                        }
                        self.dirty = true;
                    }
                    save = ui
                        .add(
                            filled_button("Save appearance")
                                .small()
                                .min_size(Vec2::new(0.0, 40.0)),
                        )
                        .clicked();
                });
            },
        );
        if changed {
            if let Some(tab) = self.active() {
                self.config.terminal = tab.style.clone();
            }
            self.dirty = true;
        }
        if dialog.should_close() || close {
            self.appearance_open = false;
        }
        if refresh_fonts {
            self.fonts = crate::fonts::load(ctx);
            self.notice = format!(
                "Found {} installed monospace font families",
                self.fonts.len() - 1
            );
        }
        if save {
            if let Some(tab) = self.active() {
                self.config.terminal = tab.style.clone();
            }
            self.save();
        }
    }
}
