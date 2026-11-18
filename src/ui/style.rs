//! Shared Material styling for controls and the egui inputs used by the forms.
use crate::layout_config::ButtonConfig;
use egui::{Color32, FontId, RichText, Stroke, TextStyle};
use egui_material3::{MaterialThemeContext, ThemeMode, get_global_color, update_global_theme};

pub const CORNER_RADIUS: u8 = 6;

pub fn material_icon(name: &str) -> egui::Label {
    egui::Label::new(
        egui::RichText::new(egui_material3::material_symbol::material_symbol_text(name))
            .font(FontId::new(
                24.0,
                egui::FontFamily::Name(crate::fonts::MATERIAL_SYMBOLS.into()),
            ))
            .color(get_global_color("primary")),
    )
}

pub fn filled_button(text: impl Into<egui::WidgetText>) -> egui_material3::MaterialButton<'static> {
    egui_material3::MaterialButton::filled(text).corner_radius(CORNER_RADIUS)
}

pub fn outlined_button(
    text: impl Into<egui::WidgetText>,
) -> egui_material3::MaterialButton<'static> {
    egui_material3::MaterialButton::outlined(text).corner_radius(CORNER_RADIUS)
}

pub fn text_button(text: impl Into<egui::WidgetText>) -> egui_material3::MaterialButton<'static> {
    egui_material3::MaterialButton::text(text).corner_radius(CORNER_RADIUS)
}

pub(super) fn styled_button(
    button: &ButtonConfig,
    caption: String,
) -> egui_material3::MaterialButton<'static> {
    let (text, fill) = button_colors(button);
    filled_button(RichText::new(caption).size(15.0))
        .small()
        .text_color(text)
        .fill(fill)
}

pub(in crate::app) fn button_colors(button: &ButtonConfig) -> (Color32, Color32) {
    let (text, fill) = if button.command.trim().is_empty() {
        ("onSecondaryContainer", "surfaceVariant")
    } else {
        ("onPrimary", "primary")
    };
    let fill = button
        .bg_color
        .map(rgb)
        .unwrap_or_else(|| get_global_color(fill));
    let text = button.fg_color.map(rgb).unwrap_or_else(|| {
        if button.bg_color.is_some() {
            let linear = egui::Rgba::from(fill);
            let luminance = 0.2126 * linear.r() + 0.7152 * linear.g() + 0.0722 * linear.b();
            // Black or white gives at least 4.5:1 contrast against any opaque background.
            if luminance > 0.179 {
                Color32::BLACK
            } else {
                Color32::WHITE
            }
        } else {
            get_global_color(text)
        }
    });
    (text, fill)
}

pub fn text_input(value: &mut String) -> egui::TextEdit<'_> {
    egui::TextEdit::singleline(value)
        .margin(egui::Margin::symmetric(10, 10))
        .desired_width(f32::INFINITY)
}

/// Use Material buttons and a themed popup; MaterialSelect's selected fill uses invalid alpha.
pub fn font_picker(ui: &mut egui::Ui, font: &mut String, fonts: &[String]) -> bool {
    ui.label("Font");
    let response = ui
        .add_sized(
            [ui.available_width(), 40.0],
            outlined_button(font.as_str()).small(),
        )
        .on_hover_text("Choose a terminal font");
    let mut changed = false;
    egui::Popup::menu(&response)
        .width(response.rect.width())
        .show(|ui| {
            egui::ScrollArea::vertical()
                .max_height(220.0)
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    for name in fonts {
                        let mut button = text_button(name).small();
                        if name == font {
                            button = button
                                .fill(get_global_color("primaryContainer"))
                                .text_color(get_global_color("onPrimaryContainer"));
                        }
                        if ui.add_sized([ui.available_width(), 40.0], button).clicked() {
                            font.clone_from(name);
                            changed = true;
                            ui.close();
                        }
                    }
                });
        });
    changed
}

/// MaterialDialog hardcodes 28px corners. Use its theme with a configurable egui frame.
pub fn dialog(
    ctx: &egui::Context,
    id: impl Into<egui::Id>,
    title: impl Into<String>,
    width: std::ops::RangeInclusive<f32>,
    content: impl FnOnce(&mut egui::Ui),
) -> egui::ModalResponse<()> {
    let padding = 24;
    egui::Modal::new(id.into())
        .frame(
            egui::Frame::new()
                .inner_margin(egui::Margin::same(padding))
                .fill(get_global_color("surfaceContainerHigh"))
                .corner_radius(CORNER_RADIUS),
        )
        .show(ctx, |ui| {
            let margins = 2.0 * f32::from(padding);
            ui.set_min_width((*width.start() - margins).max(0.0));
            ui.set_max_width((*width.end() - margins).max(0.0));
            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
            ui.visuals_mut().panel_fill = get_global_color("surfaceContainerHigh");
            ui.vertical(|ui| {
                ui.label(
                    egui::RichText::new(title)
                        .size(24.0)
                        .color(get_global_color("onSurface")),
                );
                ui.add_space(16.0);
                ui.vertical(content);
            });
        })
}

/// Keep form scrolling inside a dialog that shrinks to its content.
pub fn form(ui: &mut egui::Ui, ctx: &egui::Context, id: &str, content: impl FnOnce(&mut egui::Ui)) {
    let height = (ctx.content_rect().height() * 0.9 - 160.0).max(120.0);
    ui.set_max_height(height);
    egui::ScrollArea::vertical()
        .id_salt(id)
        .max_height(height)
        .auto_shrink([false, true])
        .show(ui, content);
}

/// Keep dialog actions in a compact row at the right edge; declare buttons from right to left.
pub fn dialog_footer(ui: &mut egui::Ui, actions: impl FnOnce(&mut egui::Ui)) {
    ui.add_space(8.0);
    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), 40.0),
        egui::Layout::right_to_left(egui::Align::Center).with_main_wrap(true),
        actions,
    );
}

pub fn setup(ctx: &egui::Context) {
    // Use the crate's built-in theme and local fonts. Asset downloads are unnecessary.
    let mut theme = MaterialThemeContext {
        theme_mode: ThemeMode::Dark,
        ..Default::default()
    };
    for (role, color) in [
        ("primary", [255, 183, 123]),
        ("onPrimary", [79, 37, 0]),
        ("primaryContainer", [112, 55, 0]),
        ("onPrimaryContainer", [255, 220, 193]),
        ("surfaceTint", [255, 183, 123]),
        ("secondary", [222, 193, 167]),
        ("onSecondary", [63, 45, 30]),
        ("secondaryContainer", [85, 67, 52]),
        ("onSecondaryContainer", [251, 221, 194]),
        ("tertiary", [193, 203, 147]),
        ("onTertiary", [43, 51, 11]),
        ("tertiaryContainer", [65, 74, 31]),
        ("onTertiaryContainer", [221, 231, 173]),
        ("background", [25, 18, 13]),
        ("onBackground", [240, 223, 211]),
        ("surface", [25, 18, 13]),
        ("onSurface", [240, 223, 211]),
        ("surfaceVariant", [81, 68, 58]),
        ("onSurfaceVariant", [214, 195, 180]),
        ("surfaceContainerLowest", [17, 13, 9]),
        ("surfaceContainerLow", [33, 27, 21]),
        ("surfaceContainer", [37, 31, 25]),
        ("surfaceContainerHigh", [48, 41, 34]),
        ("surfaceContainerHighest", [59, 52, 44]),
        ("outline", [159, 141, 128]),
        ("outlineVariant", [81, 68, 58]),
        ("inverseSurface", [240, 223, 211]),
        ("inverseOnSurface", [55, 46, 39]),
        ("inversePrimary", [143, 76, 0]),
    ] {
        theme
            .selected_colors
            .insert(role.into(), Color32::from_rgb(color[0], color[1], color[2]));
    }
    update_global_theme(theme);
    ctx.set_visuals(egui::Visuals::dark());
    ctx.all_styles_mut(|style| {
        for (text_style, size) in [
            (TextStyle::Heading, 22.0),
            (TextStyle::Body, 15.0),
            (TextStyle::Button, 15.0),
            (TextStyle::Small, 14.0),
        ] {
            style
                .text_styles
                .insert(text_style, FontId::proportional(size));
        }
        style.spacing.item_spacing = egui::vec2(10.0, 8.0);
        style.spacing.button_padding = egui::vec2(12.0, 8.0);
        style.spacing.interact_size.y = 36.0;
        style.visuals.panel_fill = get_global_color("surface");
        style.visuals.window_fill = get_global_color("surfaceContainerHigh");
        style.visuals.extreme_bg_color = get_global_color("surfaceContainerLowest");
        // Material buttons choose text colors for each variant and custom button color.
        style.visuals.override_text_color = None;
        style.visuals.window_corner_radius = egui::CornerRadius::same(CORNER_RADIUS);
        style.visuals.menu_corner_radius = egui::CornerRadius::same(CORNER_RADIUS);
        style.visuals.window_stroke = Stroke::new(1.0_f32, get_global_color("outlineVariant"));
        style.visuals.selection.bg_fill = get_global_color("primaryContainer");
        style.visuals.selection.stroke =
            Stroke::new(1.0_f32, get_global_color("onPrimaryContainer"));
        for (widget, fill) in [
            (
                &mut style.visuals.widgets.noninteractive,
                "surfaceContainer",
            ),
            (&mut style.visuals.widgets.inactive, "surfaceContainerHigh"),
            (
                &mut style.visuals.widgets.hovered,
                "surfaceContainerHighest",
            ),
            (&mut style.visuals.widgets.active, "primaryContainer"),
            (&mut style.visuals.widgets.open, "surfaceContainerHighest"),
        ] {
            widget.bg_fill = get_global_color(fill);
            widget.weak_bg_fill = widget.bg_fill;
            widget.bg_stroke = Stroke::new(1.0_f32, get_global_color("outlineVariant"));
            widget.fg_stroke = Stroke::new(1.0_f32, get_global_color("onSurface"));
            widget.corner_radius = egui::CornerRadius::same(CORNER_RADIUS);
        }
    });
}

pub(in crate::app) fn rgb(color: [u8; 3]) -> Color32 {
    Color32::from_rgb(color[0], color[1], color[2])
}
