//! Shared helpers for application interaction tests.

use super::{ButtonBuilder, SaveAs};
use crate::config_picker::ConfigPicker;
use crate::layout_config::{ButtonConfig, Config, DEFAULT_LAYOUT_NAME};
use crate::settings::SettingsStore;
use crate::terminal::Terminal;
use crate::ui::{TERMINAL_SCROLLBAR_WIDTH, button_colors, key_bytes, rgb};
use egui::{Color32, FontFamily, Vec2};
use egui_material3::get_global_color;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use super::workspace::Split;

mod buttons;
mod layouts;
mod panes;
mod terminals;
mod themes;

fn frame(
    app: &mut ButtonBuilder,
    ctx: &egui::Context,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    frame_with(
        app,
        ctx,
        Vec2::new(1200.0, 760.0),
        egui::Modifiers::NONE,
        events,
    )
}

fn frame_with(
    app: &mut ButtonBuilder,
    ctx: &egui::Context,
    size: Vec2,
    modifiers: egui::Modifiers,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            modifiers,
            events,
            ..Default::default()
        },
        |ctx| app.show(ctx),
    )
}

fn hotkey(
    app: &mut ButtonBuilder,
    ctx: &egui::Context,
    key: egui::Key,
    shift: bool,
    repeat: bool,
) -> egui::FullOutput {
    let modifiers = egui::Modifiers {
        ctrl: true,
        command: true,
        shift,
        ..egui::Modifiers::NONE
    };
    let mut events = Vec::new();
    if !repeat {
        events.push(egui::Event::Key {
            key,
            physical_key: None,
            pressed: false,
            repeat: false,
            modifiers,
        });
    }
    events.push(egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat,
        modifiers,
    });
    frame_with(app, ctx, Vec2::new(1200.0, 760.0), modifiers, events)
}

fn label_center(output: &egui::FullOutput, label: &str) -> egui::Pos2 {
    output
        .shapes
        .iter()
        .find_map(|shape| {
            if let egui::Shape::Text(text) = &shape.shape
                && text.galley.job.text == label
            {
                Some(text.pos + text.galley.rect.center().to_vec2())
            } else {
                None
            }
        })
        .unwrap_or_else(|| panic!("Missing UI label: {label}"))
}

fn click_app_action(
    app: &mut ButtonBuilder,
    ctx: &egui::Context,
    output: &egui::FullOutput,
    label: &str,
) {
    click(app, ctx, label_center(output, "Button Builder"));
    let menu = frame(app, ctx, vec![]);
    let label = if matches!(label, "Save layout" | "Save layout *" | "Save layout…") {
        if app.config_path.is_none() {
            "Save layout…"
        } else if app.dirty {
            "Save layout *"
        } else {
            "Save layout"
        }
    } else {
        label
    };
    click(app, ctx, label_center(&menu, label));
}

fn terminal_output_position(output: &egui::FullOutput, prefix: &str) -> egui::Pos2 {
    output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.job.text.starts_with(prefix) => Some(text.pos),
            _ => None,
        })
        .unwrap_or_else(|| panic!("Missing terminal text: {prefix}"))
}

fn open_tab_menu(app: &mut ButtonBuilder, ctx: &egui::Context, title: &str) -> egui::FullOutput {
    frame(app, ctx, vec![]);
    let output = frame(app, ctx, vec![]);
    let pos = label_center(&output, title);
    for pressed in [true, false] {
        frame(
            app,
            ctx,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Secondary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
    }
    frame(app, ctx, vec![])
}

fn empty_button_centers(output: &egui::FullOutput) -> Vec<egui::Pos2> {
    output
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.job.text == "\u{f710}" => {
                Some(text.pos + text.galley.rect.center().to_vec2())
            }
            _ => None,
        })
        .collect()
}

fn wheel(
    app: &mut ButtonBuilder,
    ctx: &egui::Context,
    pos: egui::Pos2,
    unit: egui::MouseWheelUnit,
    delta: Vec2,
    modifiers: egui::Modifiers,
) {
    frame_with(
        app,
        ctx,
        Vec2::new(1200.0, 760.0),
        modifiers,
        vec![
            egui::Event::PointerMoved(pos),
            egui::Event::MouseWheel {
                unit,
                delta,
                modifiers,
            },
        ],
    );
}

fn click(app: &mut ButtonBuilder, ctx: &egui::Context, pos: egui::Pos2) {
    click_with_modifiers(app, ctx, pos, egui::Modifiers::NONE);
}

fn click_with_modifiers(
    app: &mut ButtonBuilder,
    ctx: &egui::Context,
    pos: egui::Pos2,
    modifiers: egui::Modifiers,
) {
    for pressed in [true, false] {
        frame_with(
            app,
            ctx,
            Vec2::new(1200.0, 760.0),
            modifiers,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers,
                },
            ],
        );
    }
}

fn interactive_button(label: &str) -> ButtonConfig {
    ButtonConfig {
        label: label.into(),
        command: format!(
            "printf '{label}-ready\\n'; while IFS= read -r line; do printf '{label}:%s\\n' \"$line\"; done"
        ),
        shell: true,
        ..Default::default()
    }
}

fn scrollbar_rects(output: &egui::FullOutput) -> Option<(egui::Rect, egui::Rect)> {
    let track = scrollbar_track(output, false)?;
    let thumb = output.shapes.iter().find_map(|shape| match &shape.shape {
        egui::Shape::Rect(rect)
            if track.expand(0.1).contains_rect(rect.rect)
                && (rect.fill == get_global_color("outline")
                    || rect.fill == get_global_color("primary")) =>
        {
            Some(rect.rect)
        }
        _ => None,
    })?;
    Some((track, thumb))
}

fn scrollbar_track(output: &egui::FullOutput, horizontal: bool) -> Option<egui::Rect> {
    output.shapes.iter().find_map(|shape| match &shape.shape {
        egui::Shape::Rect(rect)
            if rect.fill == get_global_color("surfaceContainerHighest")
                && ((if horizontal {
                    rect.rect.height()
                } else {
                    rect.rect.width()
                }) - (TERMINAL_SCROLLBAR_WIDTH - 8.0))
                    .abs()
                    < 0.1 =>
        {
            Some(rect.rect)
        }
        _ => None,
    })
}

fn wait_for_tabs(
    app: &mut ButtonBuilder,
    ctx: &egui::Context,
    predicate: impl Fn(&ButtonBuilder) -> bool,
) {
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        frame(app, ctx, vec![]);
        if predicate(app) {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "Timed out waiting for terminal tabs"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn primary_pointer(pos: egui::Pos2, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    }
}

fn start_button_drag(
    app: &mut ButtonBuilder,
    ctx: &egui::Context,
    source: egui::Pos2,
    target: egui::Pos2,
) -> egui::FullOutput {
    frame(
        app,
        ctx,
        vec![
            egui::Event::PointerMoved(source),
            primary_pointer(source, true),
        ],
    );
    frame(app, ctx, vec![egui::Event::PointerMoved(target)])
}
