//! Hover feedback for transparent Material buttons until the upstream alpha blend is fixed.
use egui::{Color32, Response, Shape, Ui, Vec2, Widget};
use egui_material3::{MaterialButton, get_global_color};

use super::CORNER_RADIUS;

pub(crate) struct Button {
    inner: MaterialButton<'static>,
    transparent: bool,
    text_color: Option<Color32>,
}

impl Button {
    pub(super) fn new(inner: MaterialButton<'static>) -> Self {
        Self {
            inner,
            transparent: true,
            text_color: None,
        }
    }

    pub fn small(mut self) -> Self {
        self.inner = self.inner.small();
        self
    }

    pub fn min_size(mut self, size: Vec2) -> Self {
        self.inner = self.inner.min_size(size);
        self
    }

    pub fn truncate(mut self) -> Self {
        self.inner = self.inner.truncate();
        self
    }

    pub fn fill(mut self, color: Color32) -> Self {
        self.transparent = color.a() == 0;
        self.inner = self.inner.fill(color);
        self
    }

    pub fn text_color(mut self, color: Color32) -> Self {
        self.text_color = Some(color);
        self.inner = self.inner.text_color(color);
        self
    }
}

impl Widget for Button {
    fn ui(self, ui: &mut Ui) -> Response {
        // Reserve a paint slot below the label and outline, using the widget's real hit area.
        let background = self.transparent.then(|| ui.painter().add(Shape::Noop));
        let response = self.inner.ui(ui);
        if let Some(background) = background
            && response.enabled()
            && (response.hovered() || response.is_pointer_button_down_on())
        {
            let color = self
                .text_color
                .unwrap_or_else(|| get_global_color("onBackground"));
            let alpha = if response.is_pointer_button_down_on() {
                31
            } else {
                20
            };
            ui.painter().set(
                background,
                Shape::rect_filled(
                    response.rect,
                    CORNER_RADIUS,
                    Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), alpha),
                ),
            );
        }
        response
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::ui::{outlined_button, style::apply_theme, text_button};
    use crate::theme::ThemeSettings;

    #[test]
    fn transparent_buttons_show_hover_and_press_feedback_in_both_themes() {
        for dark in [true, false] {
            for outlined in [false, true] {
                let ctx = egui::Context::default();
                apply_theme(&ctx, ThemeSettings::default(), dark);
                let render = |events, enabled, fill| {
                    let mut response = None;
                    let output = ctx.run(
                        egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                Vec2::new(300.0, 150.0),
                            )),
                            events,
                            ..Default::default()
                        },
                        |ctx| {
                            egui::CentralPanel::default().show(ctx, |ui| {
                                let mut button = if outlined {
                                    outlined_button("Close")
                                } else {
                                    text_button("Close")
                                };
                                if let Some(fill) = fill {
                                    button = button.fill(fill);
                                }
                                response = Some(ui.add_enabled(enabled, button));
                            });
                        },
                    );
                    (output, response.unwrap())
                };
                let layers = |output: &egui::FullOutput, response: &Response| {
                    output
                        .shapes
                        .iter()
                        .filter_map(|shape| match &shape.shape {
                            Shape::Rect(rect)
                                if rect.rect == response.rect
                                    && rect.fill.a() > 0
                                    && rect.fill.a() < 255 =>
                            {
                                Some(rect.fill)
                            }
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                };
                let (idle, response) = render(vec![], true, None);
                assert!(layers(&idle, &response).is_empty());
                let pos = response.rect.center();
                let (hover, response) = render(vec![egui::Event::PointerMoved(pos)], true, None);
                let color = get_global_color("onBackground");
                assert_eq!(
                    layers(&hover, &response),
                    [Color32::from_rgba_unmultiplied(
                        color.r(),
                        color.g(),
                        color.b(),
                        20
                    )]
                );
                let pointer = |pressed| egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                };
                let (pressed, response) = render(vec![pointer(true)], true, None);
                assert_eq!(
                    layers(&pressed, &response),
                    [Color32::from_rgba_unmultiplied(
                        color.r(),
                        color.g(),
                        color.b(),
                        31
                    )]
                );
                assert!(
                    pressed
                        .shapes
                        .iter()
                        .position(
                            |shape| matches!(&shape.shape, Shape::Rect(rect) if rect.fill.a() == 31)
                        )
                        .unwrap()
                        < pressed
                            .shapes
                            .iter()
                            .position(|shape| matches!(&shape.shape, Shape::Text(_)))
                            .unwrap(),
                    "Feedback must paint beneath the label"
                );
                let (_, response) = render(vec![pointer(false)], true, None);
                assert!(response.clicked());
                let (disabled, response) = render(vec![], false, None);
                assert!(layers(&disabled, &response).is_empty());
                let (filled, response) = render(vec![], true, Some(Color32::from_rgb(35, 105, 75)));
                assert!(
                    layers(&filled, &response).is_empty(),
                    "Filled buttons use the existing Material feedback"
                );
                let (idle, response) = render(
                    vec![egui::Event::PointerMoved(egui::pos2(290.0, 140.0))],
                    true,
                    None,
                );
                assert!(layers(&idle, &response).is_empty());
            }
        }
        crate::app::ui::setup(&egui::Context::default());
    }
}
