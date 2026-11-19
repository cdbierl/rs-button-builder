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

#[test]
fn selected_and_saved_layouts_are_remembered_and_failed_loads_preserve_settings() {
    let ctx = egui::Context::default();
    let directory = std::env::temp_dir().join(format!(
        "rs-button-builder-app-settings-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let settings = directory.join("rs_button_builder/settings.yaml");
    let mut app = ButtonBuilder::new(&ctx, None, Config::default())
        .with_settings(SettingsStore::open(settings.clone()));
    assert!(!settings.exists());
    let selected = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/system-monitor.yaml");
    app.load_config(selected.clone()).unwrap();
    assert_eq!(
        SettingsStore::open(settings.clone())
            .startup_layout(None)
            .unwrap()
            .0,
        Some(selected.clone())
    );
    assert!(app.load_config(directory.join("missing.yaml")).is_err());
    assert_eq!(
        SettingsStore::open(settings.clone())
            .startup_layout(None)
            .unwrap()
            .0,
        Some(selected.clone())
    );
    app.create_new_layout();
    assert_eq!(
        SettingsStore::open(settings.clone())
            .startup_layout(None)
            .unwrap()
            .0,
        Some(selected)
    );
    let saved = directory.join("New layout.yaml");
    app.config.save(&saved).unwrap();
    app.finish_save(app.config.clone(), saved.clone(), false);
    assert_eq!(
        SettingsStore::open(settings)
            .startup_layout(None)
            .unwrap()
            .0,
        Some(saved)
    );
}

#[test]
fn settings_write_failures_do_not_prevent_loading_or_saving_a_layout() {
    let ctx = egui::Context::default();
    let directory = std::env::temp_dir().join(format!(
        "rs-button-builder-blocked-settings-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&directory).unwrap();
    let blocker = directory.join("blocked");
    std::fs::write(&blocker, "not a directory").unwrap();
    let settings = SettingsStore::open(blocker.join("settings.yaml"));
    let mut app = ButtonBuilder::new(&ctx, None, Config::default()).with_settings(settings);
    let selected = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/system-monitor.yaml");
    app.load_config(selected.clone()).unwrap();
    assert_eq!(app.config_path, Some(selected));
    assert!(app.error.as_deref().unwrap().contains("app settings"));
    let saved = directory.join("Saved layout.yaml");
    app.config_path = Some(saved.clone());
    app.config.display_name = "Saved despite settings failure".into();
    app.dirty = true;
    app.save();
    assert_eq!(
        Config::load_existing(&saved).unwrap().display_name(),
        "Saved despite settings failure"
    );
    assert!(!app.dirty);
    assert!(app.error.as_deref().unwrap().contains("app settings"));
}

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

#[test]
fn terminal_split_right_preserves_sessions_and_routes_focus_input_and_tabs() {
    let ctx = egui::Context::default();
    let mut config = Config::default();
    for (index, label) in ["Alpha", "Beta", "Gamma"].into_iter().enumerate() {
        config.buttons[index] = interactive_button(label);
    }
    let mut app = ButtonBuilder::new(&ctx, None, config);
    app.launch(0);
    app.split_terminal(0, crate::ui::Split::Right);
    assert!(
        app.panes.split.is_none(),
        "A split needs two existing terminals"
    );
    app.launch(1);
    wait_for_tabs(&mut app, &ctx, |app| {
        app.tabs
            .iter()
            .all(|tab| tab.terminal.screen.text().contains("-ready"))
    });
    let sessions: Vec<_> = app
        .tabs
        .iter()
        .map(|tab| (tab.id, tab.terminal.status.clone()))
        .collect();
    app.select_tab(0);
    let menu = open_tab_menu(&mut app, &ctx, "2 · Beta");
    click(&mut app, &ctx, label_center(&menu, "Split right"));
    assert!(app.panes.split == Some(crate::ui::Split::Right));
    assert_eq!(app.pane_tab(0), Some(0));
    assert_eq!(app.pane_tab(1), Some(1));
    assert_eq!(app.panes.focused, 1);
    assert_eq!(
        sessions,
        app.tabs
            .iter()
            .map(|tab| (tab.id, tab.terminal.status.clone()))
            .collect::<Vec<_>>()
    );
    frame(&mut app, &ctx, vec![]);
    let output = frame(&mut app, &ctx, vec![]);
    let alpha = terminal_output_position(&output, "Alpha-ready");
    let beta = terminal_output_position(&output, "Beta-ready");
    assert!(alpha.x < beta.x && (alpha.y - beta.y).abs() < 1.0);
    assert_eq!(
        app.tabs[0].terminal.screen.cols,
        app.tabs[1].terminal.screen.cols
    );
    assert!(app.tabs[0].terminal.screen.cols < 80);
    click(&mut app, &ctx, alpha + Vec2::new(30.0, 30.0));
    assert_eq!(app.active_tab, Some(0));
    frame(
        &mut app,
        &ctx,
        vec![
            egui::Event::Text("only-alpha".into()),
            egui::Event::Key {
                key: egui::Key::Enter,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );
    wait_for_tabs(&mut app, &ctx, |app| {
        app.tabs[0]
            .terminal
            .screen
            .text()
            .contains("Alpha:only-alpha")
    });
    assert!(!app.tabs[1].terminal.screen.text().contains("only-alpha"));
    app.launch(2);
    assert_eq!(app.tabs[2].pane, 0, "New tabs belong to the focused pane");
    let menu = open_tab_menu(&mut app, &ctx, "3 · Gamma");
    click(&mut app, &ctx, label_center(&menu, "Move to other pane"));
    assert_eq!(app.tabs[2].pane, 1);
    assert_eq!(app.pane_tab(0), Some(0));
    assert_eq!(app.pane_tab(1), Some(2));
    app.launch(1);
    assert_eq!(
        app.tabs.len(),
        3,
        "Running buttons focus across panes without duplicating"
    );
    assert_eq!(app.active_tab, Some(1));
    assert_eq!(app.panes.focused, 1);
    frame(&mut app, &ctx, vec![]);
    let copied = frame_with(
        &mut app,
        &ctx,
        Vec2::new(1200.0, 760.0),
        egui::Modifiers {
            ctrl: true,
            shift: true,
            ..Default::default()
        },
        vec![egui::Event::Copy],
    );
    assert!(copied.platform_output.commands.iter().any(|command| matches!(command,
        egui::OutputCommand::CopyText(text) if text.contains("Beta-ready") && !text.contains("Alpha-ready"))));
    let menu = open_tab_menu(&mut app, &ctx, "2 · Beta");
    click(&mut app, &ctx, label_center(&menu, "Merge panes"));
    assert!(app.panes.split.is_none());
    assert!(
        app.tabs
            .iter()
            .all(|tab| tab.pane == 0 && tab.terminal.is_running())
    );
    assert_eq!(app.active_tab, Some(1));
}

#[test]
fn terminal_split_below_divider_resizes_each_pane_and_supports_minimum_window() {
    let ctx = egui::Context::default();
    let mut config = Config::default();
    config.buttons[0] = interactive_button("Alpha");
    config.buttons[1] = interactive_button("Beta");
    let mut app = ButtonBuilder::new(&ctx, None, config);
    app.launch(0);
    app.launch(1);
    wait_for_tabs(&mut app, &ctx, |app| {
        app.tabs
            .iter()
            .all(|tab| tab.terminal.screen.text().contains("-ready"))
    });
    let menu = open_tab_menu(&mut app, &ctx, "2 · Beta");
    click(&mut app, &ctx, label_center(&menu, "Split below"));
    frame(&mut app, &ctx, vec![]);
    let output = frame(&mut app, &ctx, vec![]);
    let alpha = terminal_output_position(&output, "Alpha-ready");
    let beta = terminal_output_position(&output, "Beta-ready");
    assert!(alpha.y < beta.y && (alpha.x - beta.x).abs() < 1.0);
    let rows = [
        app.tabs[0].terminal.screen.rows,
        app.tabs[1].terminal.screen.rows,
    ];
    let divider = output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::LineSegment { points, stroke }
                if (points[1].x - points[0].x) > 500.0
                    && points[0].y == points[1].y
                    && stroke.width == 2.0 =>
            {
                Some(points[0].lerp(points[1], 0.5))
            }
            _ => None,
        })
        .unwrap();
    frame(
        &mut app,
        &ctx,
        vec![
            egui::Event::PointerMoved(divider),
            egui::Event::PointerButton {
                pos: divider,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );
    let destination = divider + Vec2::new(0.0, 70.0);
    frame(&mut app, &ctx, vec![egui::Event::PointerMoved(destination)]);
    frame(
        &mut app,
        &ctx,
        vec![egui::Event::PointerButton {
            pos: destination,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    frame(&mut app, &ctx, vec![]);
    assert!(app.tabs[0].terminal.screen.rows > rows[0]);
    assert!(app.tabs[1].terminal.screen.rows < rows[1]);
    for direction in [crate::ui::Split::Right, crate::ui::Split::Below] {
        app.panes.split = Some(direction);
        let output = frame_with(
            &mut app,
            &ctx,
            Vec2::new(850.0, 500.0),
            egui::Modifiers::NONE,
            vec![],
        );
        assert!(
            app.tabs
                .iter()
                .all(|tab| tab.terminal.screen.cols >= 2 && tab.terminal.screen.rows >= 2)
        );
        assert!(
            output
                .shapes
                .iter()
                .filter_map(|shape| match &shape.shape {
                    egui::Shape::Text(text)
                        if text.galley.job.text.starts_with("Alpha-ready")
                            || text.galley.job.text.starts_with("Beta-ready") =>
                        Some((text.pos, shape.clip_rect)),
                    _ => None,
                })
                .all(|(pos, clip)| clip.contains(pos))
        );
    }
}

#[test]
fn terminal_panes_collapse_on_close_and_success_without_losing_remaining_sessions() {
    let ctx = egui::Context::default();
    let mut config = Config::default();
    for (index, label) in ["Alpha", "Beta", "Gamma"].into_iter().enumerate() {
        config.buttons[index] = interactive_button(label);
    }
    let mut app = ButtonBuilder::new(&ctx, None, config);
    for index in 0..3 {
        app.launch(index);
    }
    app.split_terminal(1, crate::ui::Split::Right);
    app.select_tab(0);
    app.close_tab(0);
    assert!(app.panes.split.is_some());
    assert_eq!(
        app.active().unwrap().label,
        "Gamma",
        "Closing selects another tab in the same pane"
    );
    let gamma_id = app.active().unwrap().id;
    app.close_tab(0); // Beta was the only tab in the other pane.
    assert!(app.panes.split.is_none());
    assert_eq!(app.active().unwrap().id, gamma_id);
    assert!(app.active().unwrap().terminal.is_running());
    app.launch(1);
    app.split_terminal(1, crate::ui::Split::Below);
    let button = ButtonConfig {
        command: "/usr/bin/printf".into(),
        arguments: "success".into(),
        close_on_success: true,
        ..Default::default()
    };
    app.tabs[1].terminal = Terminal::start(&button, 80, 24).unwrap();
    app.select_tab(0);
    wait_for_tabs(&mut app, &ctx, |app| app.tabs.len() == 1);
    assert!(app.panes.split.is_none());
    assert_eq!(app.active().unwrap().id, gamma_id);
    assert_eq!(app.active().unwrap().pane, 0);
    assert!(app.active().unwrap().terminal.is_running());
}

#[test]
fn footer_theme_picker_applies_persists_and_follows_system_appearance() {
    use crate::theme::{Accent, Appearance};
    let ctx = egui::Context::default();
    let path = std::env::temp_dir().join(format!(
        "button-builder-theme-{}-{}.yaml",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let config = Config::default();
    let original = serde_yaml_ng::to_string(&config).unwrap();
    let mut app =
        ButtonBuilder::new(&ctx, None, config).with_settings(SettingsStore::open(path.clone()));
    ctx.all_styles_mut(|style| style.animation_time = 0.0);
    frame(&mut app, &ctx, vec![]);
    let output = frame(&mut app, &ctx, vec![]);
    let palette = egui_material3::material_symbol::material_symbol_text("palette");
    let trigger = label_center(&output, &palette);
    assert!(trigger.x > 1100.0 && trigger.y > 700.0);
    click(&mut app, &ctx, trigger);
    let output = frame(&mut app, &ctx, vec![]);
    let light = label_center(&output, "Light");
    assert!(light.y < trigger.y, "The picker opens above the footer");
    assert!(trigger.y - light.y < 120.0, "The picker stays compact");
    click(&mut app, &ctx, light);
    let output = frame(&mut app, &ctx, vec![]);
    assert!(!ctx.style().visuals.dark_mode);
    label_center(&output, "System");
    let blue = output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Rect(rect) if rect.fill == rgb(Accent::Blue.colors(false)[0]) => {
                Some(rect.rect.center())
            }
            _ => None,
        })
        .unwrap();
    click(&mut app, &ctx, blue);
    frame(&mut app, &ctx, vec![]);
    assert_eq!(
        get_global_color("primary"),
        rgb(Accent::Blue.colors(false)[0])
    );
    assert_eq!(SettingsStore::open(path.clone()).theme(), app.theme);
    assert!(!app.dirty);
    assert_eq!(serde_yaml_ng::to_string(&app.config).unwrap(), original);
    let output = frame(&mut app, &ctx, vec![]);
    click(&mut app, &ctx, label_center(&output, "System"));
    for theme in [egui::Theme::Light, egui::Theme::Dark] {
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(850.0, 500.0),
                )),
                system_theme: Some(theme),
                ..Default::default()
            },
            |ctx| app.show(ctx),
        );
        assert_eq!(ctx.style().visuals.dark_mode, theme == egui::Theme::Dark);
    }
    assert_eq!(SettingsStore::open(path).theme().mode, Appearance::System);
    crate::ui::setup(&ctx);
}

#[test]
fn terminal_theme_recolors_scrollback_and_can_restore_automatic_colors() {
    use crate::terminal::colors::{Color, Palette};
    use crate::theme::Appearance;

    let ctx = egui::Context::default();
    let mut config = Config::default();
    config.buttons[0] = ButtonConfig {
        label: "Theme test".into(),
        command: "/usr/bin/printf".into(),
        arguments: "ready".into(),
        ..Default::default()
    };
    let mut app = ButtonBuilder::new(&ctx, None, config);
    app.launch(0);
    wait_for_tabs(&mut app, &ctx, |app| !app.tabs[0].terminal.needs_poll());
    app.tabs[0].terminal.screen.feed(b"\x1b[2J\x1b[H");
    for _ in 0..100 {
        app.tabs[0]
            .terminal
            .screen
            .feed(b"\x1b[32mansi\x1b[38;2;205;49;49mexact\x1b[0mplain\r\n");
    }
    app.tabs[0].scrollback = app.tabs[0].terminal.screen.history.len();
    let original = serde_yaml_ng::to_string(&app.config).unwrap();
    let scrollback = app.tabs[0].scrollback;
    let check_colors = |output: &egui::FullOutput, colors: &Palette| {
        for (label, expected) in [
            ("ansi", colors.resolve(Color::Indexed(2))),
            ("exact", [205, 49, 49]),
            ("plain", colors.foreground),
        ] {
            let color = output
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::Shape::Text(text) if text.galley.job.text.starts_with(label) => {
                        Some(text.galley.job.sections[0].format.color)
                    }
                    _ => None,
                })
                .unwrap_or_else(|| panic!("Missing terminal output {label}"));
            assert_eq!(color, rgb(expected), "{label}");
        }
        assert!(output.shapes.iter().any(|shape| matches!(&shape.shape,
            egui::Shape::Rect(rect) if rect.fill == rgb(colors.background) && rect.rect.width() > 300.0)));
    };
    for (mode, system, dark) in [
        (Appearance::Light, egui::Theme::Dark, false),
        (Appearance::Dark, egui::Theme::Light, true),
        (Appearance::System, egui::Theme::Light, false),
        (Appearance::System, egui::Theme::Dark, true),
    ] {
        app.theme.mode = mode;
        let output = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(1200.0, 760.0),
                )),
                system_theme: Some(system),
                ..Default::default()
            },
            |ctx| app.show(ctx),
        );
        check_colors(&output, &Palette::new(dark, None, None));
        assert_eq!(app.tabs[0].scrollback, scrollback);
    }
    assert!(!app.dirty);
    assert_eq!(serde_yaml_ng::to_string(&app.config).unwrap(), original);
    app.tabs[0].style.fg_color = Some([210, 220, 230]);
    app.tabs[0].style.bg_color = Some([5, 10, 15]);
    app.theme.mode = Appearance::Light;
    let output = frame(&mut app, &ctx, vec![]);
    check_colors(
        &output,
        &Palette::new(false, Some([210, 220, 230]), Some([5, 10, 15])),
    );

    let font_size = app.tabs[0].style.font_size;
    app.appearance_open = true;
    frame(&mut app, &ctx, vec![]);
    let output = frame(&mut app, &ctx, vec![]);
    click(&mut app, &ctx, label_center(&output, "Use theme colors"));
    assert_eq!(app.tabs[0].style.fg_color, None);
    assert_eq!(app.tabs[0].style.bg_color, None);
    assert_eq!(app.config.terminal.fg_color, None);
    assert_eq!(app.config.terminal.bg_color, None);
    assert_eq!(app.tabs[0].style.font_size, font_size);
    assert!(app.dirty);
    app.appearance_open = false;
    let output = frame(&mut app, &ctx, vec![]);
    check_colors(&output, &Palette::new(false, None, None));
    crate::ui::setup(&ctx);
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
    let track = output.shapes.iter().find_map(|shape| match &shape.shape {
        egui::Shape::Rect(rect)
            if rect.fill == get_global_color("surfaceContainerHighest")
                && (rect.rect.width() - (TERMINAL_SCROLLBAR_WIDTH - 8.0)).abs() < 0.1 =>
        {
            Some(rect.rect)
        }
        _ => None,
    })?;
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

#[test]
fn close_on_success_applies_to_auto_start_and_is_captured_at_launch() {
    let ctx = egui::Context::default();
    let mut config = Config::default();
    for (index, close_on_success, exit_code) in [(0, true, 0), (1, false, 0), (2, true, 1)] {
        config.buttons[index] = ButtonConfig {
            label: format!("Run {index}"),
            command: format!("printf 'result {index}'; exit {exit_code}"),
            shell: true,
            auto_start: true,
            close_on_success,
            ..Default::default()
        };
    }
    let mut app = ButtonBuilder::new(&ctx, Some("/tmp/close-on-success.yaml".into()), config);
    assert_eq!(app.tabs.len(), 3);
    // Editing button preferences doesn't change the policy for an existing run.
    app.config.buttons[0].close_on_success = false;
    app.config.buttons[1].close_on_success = true;
    wait_for_tabs(&mut app, &ctx, |app| {
        app.tabs.len() == 2 && app.tabs.iter().all(|tab| !tab.terminal.needs_poll())
    });
    assert_eq!(app.tabs[0].label, "Run 1");
    assert!(app.tabs[0].terminal.screen.text().contains("result 1"));
    assert_eq!(app.tabs[1].label, "Run 2");
    assert!(app.tabs[1].terminal.screen.text().contains("result 2"));
    assert_eq!(
        app.completion_notice.as_ref().unwrap().0,
        "Run 0 completed successfully"
    );
}

#[test]
fn silent_successes_close_without_disturbing_other_tabs_and_show_a_temporary_notice() {
    let ctx = egui::Context::default();
    let mut config = Config::default();
    for index in 0..2 {
        config.buttons[index] = ButtonConfig {
            label: format!("Silent {}", index + 1),
            command: "/usr/bin/true".into(),
            ..Default::default()
        };
    }
    config.buttons[2] = interactive_button("Keep open");
    let mut app = ButtonBuilder::new(&ctx, None, config);
    let baseline_notice = app.notice.clone();
    app.launch(2);
    wait_for_tabs(&mut app, &ctx, |app| {
        app.tabs[0]
            .terminal
            .screen
            .text()
            .contains("Keep open-ready")
    });
    let kept_id = app.tabs[0].id;
    app.launch(0);
    app.launch(1);
    app.select_tab(0);
    frame(&mut app, &ctx, vec![]);
    let focused = ctx.memory(|memory| memory.focused());
    wait_for_tabs(&mut app, &ctx, |app| app.tabs.len() == 1);
    assert_eq!(app.active_tab, Some(0));
    assert_eq!(app.tabs[0].id, kept_id);
    assert!(app.tabs[0].terminal.is_running());
    assert_eq!(ctx.memory(|memory| memory.focused()), focused);
    assert!(!app.dirty);
    assert_eq!(app.notice, baseline_notice);
    let notice = app.completion_notice.as_ref().unwrap().0.clone();
    assert!(notice.ends_with("completed successfully"));
    let output = frame(&mut app, &ctx, vec![]);
    label_center(&output, &notice);
    app.completion_notice.as_mut().unwrap().1 = Instant::now() - Duration::from_secs(1);
    let output = frame(&mut app, &ctx, vec![]);
    assert!(app.completion_notice.is_none());
    label_center(&output, &baseline_notice);
    app.close_tab(0);
    app.launch(0);
    wait_for_tabs(&mut app, &ctx, |app| app.tabs.is_empty());
    assert_eq!(app.active_tab, None);
    let output = frame(&mut app, &ctx, vec![]);
    label_center(&output, "No process output yet.");
    label_center(&output, "Silent 1 completed successfully");
    app.launch(2);
    assert!(app.completion_notice.is_none());
    wait_for_tabs(&mut app, &ctx, |app| {
        app.tabs[0]
            .terminal
            .screen
            .text()
            .contains("Keep open-ready")
    });
}

#[test]
fn silent_active_success_selects_a_remaining_tab_and_failures_and_output_stay_open() {
    let ctx = egui::Context::default();
    let mut config = Config::default();
    config.buttons[0] = interactive_button("Keep open");
    for (index, command) in [
        (1, "printf '\\r\\033[2K\\033[3D'"),
        (2, "printf 'failure details' >&2; exit 7"),
        (3, "printf '\\r\\033[2K\\033[3D'; exit 1"),
        (4, "printf 'successful output'"),
        (5, "printf 'erased output\\033[2J\\033[H'"),
    ] {
        config.buttons[index] = ButtonConfig {
            label: format!("Run {index}"),
            command: command.into(),
            shell: true,
            ..Default::default()
        };
    }
    config.buttons[6] = ButtonConfig {
        command: "/bin/sleep".into(),
        arguments: "60".into(),
        ..Default::default()
    };
    let mut app = ButtonBuilder::new(&ctx, None, config);
    app.launch(0);
    wait_for_tabs(&mut app, &ctx, |app| {
        app.tabs[0]
            .terminal
            .screen
            .text()
            .contains("Keep open-ready")
    });
    app.launch(1);
    assert_eq!(app.active_tab, Some(1));
    wait_for_tabs(&mut app, &ctx, |app| app.tabs.len() == 1);
    assert_eq!(app.active_tab, Some(0));
    for index in 2..=5 {
        app.launch(index);
    }
    wait_for_tabs(&mut app, &ctx, |app| {
        app.tabs
            .iter()
            .skip(1)
            .all(|tab| !tab.terminal.needs_poll())
    });
    assert_eq!(app.tabs.len(), 5);
    assert_eq!(
        app.tabs[1].terminal.status,
        "Process finished: exit status: 7"
    );
    assert!(
        app.tabs[1]
            .terminal
            .screen
            .text()
            .contains("failure details")
    );
    assert_eq!(
        app.tabs[2].terminal.status,
        "Process finished: exit status: 1"
    );
    assert!(app.tabs[2].terminal.screen.text().trim().is_empty());
    assert!(
        app.tabs[3]
            .terminal
            .screen
            .text()
            .contains("successful output")
    );
    assert!(app.tabs[4].terminal.screen.text().trim().is_empty());
    app.select_tab(2);
    let output = frame(&mut app, &ctx, vec![]);
    label_center(&output, "Process finished: exit status: 1");
    app.launch(6);
    app.tabs[5].terminal.stop();
    frame(&mut app, &ctx, vec![]);
    assert_eq!(app.tabs.len(), 6);
    assert_eq!(app.tabs[5].terminal.status, "Stopped");
}

#[test]
fn button_clicks_route_input_and_toolbar_actions_to_the_selected_process() {
    let ctx = egui::Context::default();
    let mut config = Config::default();
    config.buttons[0] = interactive_button("Alpha");
    config.buttons[1] = interactive_button("Beta");
    let mut app = ButtonBuilder::new(
        &ctx,
        Some("/tmp/rs-button-builder-tabs-test.yaml".into()),
        config,
    );
    frame(&mut app, &ctx, vec![]);
    let output = frame(&mut app, &ctx, vec![]);
    click(&mut app, &ctx, label_center(&output, "Alpha"));
    assert_eq!(app.tabs.len(), 1);
    let output = frame(&mut app, &ctx, vec![]);
    click(&mut app, &ctx, label_center(&output, "Beta"));
    assert_eq!(
        app.tabs.len(),
        2,
        "Clicking a different button opens a tab without replacing a running process"
    );
    let output = frame(&mut app, &ctx, vec![]);
    click(&mut app, &ctx, label_center(&output, "Alpha"));
    assert_eq!(app.active_tab, Some(0));
    assert_eq!(
        app.tabs.len(),
        2,
        "Clicking a running button focuses its tab"
    );
    let output = frame(&mut app, &ctx, vec![]);
    // Ctrl no longer changes the click behavior.
    click_with_modifiers(
        &mut app,
        &ctx,
        label_center(&output, "Beta"),
        egui::Modifiers::CTRL,
    );
    assert_eq!(app.tabs.len(), 2);
    assert_eq!(app.active_tab, Some(1));
    assert!(app.tabs.iter().all(|tab| tab.terminal.is_running()));
    wait_for_tabs(&mut app, &ctx, |app| {
        app.tabs
            .iter()
            .all(|tab| tab.terminal.screen.text().contains("-ready"))
    });

    frame(
        &mut app,
        &ctx,
        vec![
            egui::Event::Text("only-beta".into()),
            egui::Event::Key {
                key: egui::Key::Enter,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );
    wait_for_tabs(&mut app, &ctx, |app| {
        app.tabs[1]
            .terminal
            .screen
            .text()
            .contains("Beta:only-beta")
    });
    assert!(!app.tabs[0].terminal.screen.text().contains("only-beta"));
    app.tabs[0].terminal.send(b"background\r");
    wait_for_tabs(&mut app, &ctx, |app| {
        app.tabs[0]
            .terminal
            .screen
            .text()
            .contains("Alpha:background")
    });
    assert_eq!(
        app.active_tab,
        Some(1),
        "Inactive terminals must keep polling"
    );

    let output = frame(&mut app, &ctx, vec![]);
    click(&mut app, &ctx, label_center(&output, "1 · Alpha"));
    assert_eq!(app.active_tab, Some(0));
    let output = frame(&mut app, &ctx, vec![]);
    let copy_pos = label_center(&output, "Copy");
    frame(
        &mut app,
        &ctx,
        vec![
            egui::Event::PointerMoved(copy_pos),
            egui::Event::PointerButton {
                pos: copy_pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );
    let copied = frame(
        &mut app,
        &ctx,
        vec![egui::Event::PointerButton {
            pos: copy_pos,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    assert!(copied.platform_output.commands.iter().any(|command| matches!(command,
        egui::OutputCommand::CopyText(text) if text.contains("Alpha:background") && !text.contains("Beta:only-beta")
    )));
    let output = frame(&mut app, &ctx, vec![]);
    click(&mut app, &ctx, label_center(&output, "Ctrl+C"));
    wait_for_tabs(&mut app, &ctx, |app| !app.tabs[0].terminal.is_running());
    assert!(app.tabs[1].terminal.is_running());

    let output = frame(&mut app, &ctx, vec![]);
    click(&mut app, &ctx, label_center(&output, "Alpha"));
    assert_eq!(app.tabs.len(), 3, "Rerunning preserves the finished tab");
    assert_eq!(app.tabs[0].id, 1);
    assert!(
        app.tabs[0]
            .terminal
            .screen
            .text()
            .contains("Alpha:background")
    );
    assert!(!app.tabs[0].terminal.is_running());
    assert_eq!(app.active_tab, Some(2));
    assert_eq!(app.tabs[2].id, 3);
    assert!(app.tabs[2].terminal.is_running());
    let output = frame(&mut app, &ctx, vec![]);
    click(&mut app, &ctx, label_center(&output, "Stop"));
    assert!(!app.tabs[0].terminal.is_running());
    assert!(app.tabs[1].terminal.is_running());
    assert_eq!(app.tabs[2].terminal.status, "Stopped");

    let output = frame(&mut app, &ctx, vec![]);
    click(&mut app, &ctx, label_center(&output, "2 · Beta"));
    let pid: i32 = app
        .active()
        .unwrap()
        .terminal
        .status
        .split_whitespace()
        .last()
        .unwrap()
        .parse()
        .unwrap();
    let output = frame(&mut app, &ctx, vec![]);
    click(&mut app, &ctx, label_center(&output, "Close tab"));
    assert_eq!(app.tabs.len(), 2);
    assert_eq!(app.active_tab, Some(1));
    // Reaping is asynchronous; wait for the worker while the UI keeps rendering.
    wait_for_tabs(&mut app, &ctx, |_| {
        // SAFETY: signal zero only checks whether this test's child still exists.
        (unsafe { libc::kill(pid, 0) }) == -1
    });
    // SAFETY: signal zero only checks whether this test's now-reaped child exists.
    assert_eq!(unsafe { libc::kill(pid, 0) }, -1);
    assert_eq!(
        std::io::Error::last_os_error().raw_os_error(),
        Some(libc::ESRCH)
    );
    let output = frame(&mut app, &ctx, vec![]);
    click(&mut app, &ctx, label_center(&output, "Close tab"));
    assert_eq!(app.tabs.len(), 1);
    let output = frame(&mut app, &ctx, vec![]);
    click(&mut app, &ctx, label_center(&output, "Close tab"));
    assert!(app.tabs.is_empty() && app.active_tab.is_none());
    let output = frame(&mut app, &ctx, vec![]);
    click(&mut app, &ctx, label_center(&output, "Alpha"));
    assert_eq!(app.tabs[0].id, 4, "New tabs must not reuse old UI IDs");
}

#[test]
fn noisy_processes_can_be_interrupted_stopped_closed_and_run_alongside_new_tabs() {
    let ctx = egui::Context::default();
    let mut config = Config::default();
    config.buttons[0] = ButtonConfig {
        label: "Flood".into(),
        command: "trap '' INT TERM; while :; do printf 'sample output line\\n'; done".into(),
        shell: true,
        ..Default::default()
    };
    config.buttons[1] = interactive_button("Quiet");
    let mut app = ButtonBuilder::new(
        &ctx,
        Some("/tmp/rs-button-builder-flood-test.yaml".into()),
        config,
    );
    frame(&mut app, &ctx, vec![]);
    let output = frame(&mut app, &ctx, vec![]);
    click(&mut app, &ctx, label_center(&output, "Flood"));
    wait_for_tabs(&mut app, &ctx, |app| {
        !app.tabs[0].terminal.screen.text().trim().is_empty()
    });
    let output = frame(&mut app, &ctx, vec![]);
    click(&mut app, &ctx, label_center(&output, "Quiet"));
    assert_eq!(app.tabs.len(), 2);
    wait_for_tabs(&mut app, &ctx, |app| {
        app.tabs[1].terminal.screen.text().contains("Quiet-ready")
    });
    for _ in 0..10 {
        let started = Instant::now();
        frame(&mut app, &ctx, vec![]);
        assert!(
            started.elapsed() < Duration::from_millis(500),
            "Output must not monopolize the UI"
        );
    }
    let output = frame(&mut app, &ctx, vec![]);
    click(&mut app, &ctx, label_center(&output, "1 · Flood"));
    let output = frame(&mut app, &ctx, vec![]);
    click(&mut app, &ctx, label_center(&output, "Ctrl+C"));
    assert!(
        app.tabs[0].terminal.is_running(),
        "The flood deliberately ignores SIGINT"
    );
    let output = frame(&mut app, &ctx, vec![]);
    click(&mut app, &ctx, label_center(&output, "Stop"));
    assert_eq!(app.tabs[0].terminal.status, "Stopped");
    assert!(!app.tabs[0].terminal.needs_poll());
    assert!(app.tabs[1].terminal.is_running());
    let output = frame(&mut app, &ctx, vec![]);
    click(&mut app, &ctx, label_center(&output, "Flood"));
    assert!(app.tabs[2].terminal.is_running());
    let pid: i32 = app.tabs[2]
        .terminal
        .status
        .split_whitespace()
        .last()
        .unwrap()
        .parse()
        .unwrap();
    let output = frame(&mut app, &ctx, vec![]);
    click(&mut app, &ctx, label_center(&output, "Close tab"));
    assert_eq!(app.tabs.len(), 2);
    assert_eq!(app.tabs[1].label, "Quiet");
    assert_eq!(app.tabs[0].terminal.status, "Stopped");
    wait_for_tabs(&mut app, &ctx, |_| {
        // SAFETY: signal zero only observes this test's child.
        (unsafe { libc::kill(pid, 0) }) == -1
    });
    assert!(app.tabs[1].terminal.is_running());
}

#[test]
fn terminal_drag_selects_highlights_and_copies_on_release() {
    let ctx = egui::Context::default();
    let mut config = Config::default();
    config.buttons[0] = interactive_button("Alpha");
    let mut app = ButtonBuilder::new(&ctx, None, config);
    app.launch(0);
    wait_for_tabs(&mut app, &ctx, |app| {
        app.tabs[0].terminal.screen.text().contains("Alpha-ready")
    });
    app.tabs[0]
        .terminal
        .screen
        .feed(b"\x1b[2J\x1b[Halpha bravo\r\ncharlie delta");
    let output = frame(&mut app, &ctx, vec![]);
    let origin = output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.job.text.starts_with("alpha bravo") => {
                Some(text.pos)
            }
            _ => None,
        })
        .unwrap();
    let font = egui::FontId::monospace(app.tabs[0].style.font_size);
    let (width, height) = ctx.fonts_mut(|fonts| {
        (
            fonts.glyph_width(&font, 'M'),
            fonts.row_height(&font) * app.tabs[0].style.line_spacing,
        )
    });
    let cell = |row: usize, col: usize| {
        origin + Vec2::new(col as f32 * width, (row as f32 + 0.5) * height)
    };
    let start = cell(0, 6);
    let end = cell(1, 7);
    let press = |pos, pressed| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    for (anchor, head) in [(start, end), (end, start)] {
        frame(
            &mut app,
            &ctx,
            vec![egui::Event::PointerMoved(anchor), press(anchor, true)],
        );
        let dragging = frame(&mut app, &ctx, vec![egui::Event::PointerMoved(head)]);
        assert!(
            dragging
                .platform_output
                .commands
                .iter()
                .all(|command| !matches!(command, egui::OutputCommand::CopyText(_))),
            "Wait until release before copying"
        );
        assert!(
            dragging.shapes.iter().any(|shape| matches!(&shape.shape,
                egui::Shape::Rect(rect) if rect.fill == ctx.style().visuals.selection.bg_fill
                    && (rect.rect.left() - start.x).abs() < 0.01
                && rect.rect.width() >= 5.0 * width - 0.01
            )),
            "Selected cells should have a visible background"
        );
        let released = frame(&mut app, &ctx, vec![press(head, false)]);
        assert!(
            released
                .platform_output
                .commands
                .iter()
                .any(|command| matches!(command,
                    egui::OutputCommand::CopyText(text) if text == "bravo\ncharlie"
                ))
        );
    }
    let copied = frame_with(
        &mut app,
        &ctx,
        Vec2::new(1200.0, 760.0),
        egui::Modifiers::CTRL | egui::Modifiers::SHIFT,
        vec![egui::Event::Copy],
    );
    assert!(
        copied
            .platform_output
            .commands
            .iter()
            .any(|command| matches!(command,
                egui::OutputCommand::CopyText(text) if text == "bravo\ncharlie"
            )),
        "Selection must retain terminal keyboard focus"
    );
    let copy_pos = label_center(&copied, "Copy");
    frame(
        &mut app,
        &ctx,
        vec![egui::Event::PointerMoved(copy_pos), press(copy_pos, true)],
    );
    let copied = frame(&mut app, &ctx, vec![press(copy_pos, false)]);
    assert!(
        copied
            .platform_output
            .commands
            .iter()
            .any(|command| matches!(command,
                egui::OutputCommand::CopyText(text) if text == "bravo\ncharlie"
            )),
        "The toolbar should copy selected text"
    );
    click(&mut app, &ctx, start);
    assert!(
        app.tabs[0]
            .selection
            .text(&app.tabs[0].terminal.screen)
            .is_none()
    );
    let copied = frame_with(
        &mut app,
        &ctx,
        Vec2::new(1200.0, 760.0),
        egui::Modifiers::CTRL | egui::Modifiers::SHIFT,
        vec![egui::Event::Copy],
    );
    assert!(
        copied
            .platform_output
            .commands
            .iter()
            .any(|command| matches!(command,
                egui::OutputCommand::CopyText(text) if text.contains("alpha bravo\ncharlie delta")
            )),
        "Copy without a selection should still copy all output"
    );
    frame(
        &mut app,
        &ctx,
        vec![egui::Event::PointerMoved(start), press(start, true)],
    );
    frame(&mut app, &ctx, vec![egui::Event::PointerMoved(end)]);
    frame(&mut app, &ctx, vec![press(end, false)]);
    frame(&mut app, &ctx, vec![egui::Event::Text("typed\n".into())]);
    assert!(
        app.tabs[0]
            .selection
            .text(&app.tabs[0].terminal.screen)
            .is_none()
    );
    wait_for_tabs(&mut app, &ctx, |app| {
        app.tabs[0].terminal.screen.text().contains("Alpha:typed")
    });
    frame(
        &mut app,
        &ctx,
        vec![egui::Event::PointerMoved(start), press(start, true)],
    );
    frame(&mut app, &ctx, vec![egui::Event::PointerMoved(end)]);
    frame(&mut app, &ctx, vec![press(end, false)]);
    let interrupted = frame_with(
        &mut app,
        &ctx,
        Vec2::new(1200.0, 760.0),
        egui::Modifiers::CTRL,
        vec![egui::Event::Copy],
    );
    assert!(
        interrupted
            .platform_output
            .commands
            .iter()
            .all(|command| !matches!(command, egui::OutputCommand::CopyText(_)))
    );
    wait_for_tabs(&mut app, &ctx, |app| !app.tabs[0].terminal.is_running());
}

#[test]
fn terminal_selection_copies_scrollback_survives_tab_switches_and_releases_outside_pane() {
    let ctx = egui::Context::default();
    let mut config = Config::default();
    config.buttons[0] = interactive_button("Alpha");
    config.buttons[1] = interactive_button("Beta");
    let mut app = ButtonBuilder::new(&ctx, None, config);
    app.launch(0);
    app.launch(1);
    wait_for_tabs(&mut app, &ctx, |app| {
        app.tabs
            .iter()
            .all(|tab| tab.terminal.screen.text().contains("-ready"))
    });
    app.select_tab(0);
    frame(&mut app, &ctx, vec![]);
    app.tabs[0].terminal.screen.feed(b"\x1b[2J\x1b[H");
    for i in 0..120 {
        app.tabs[0]
            .terminal
            .screen
            .feed(format!("history-{i}\r\n").as_bytes());
    }
    app.tabs[0].scrollback = app.tabs[0].terminal.screen.history.len();
    let output = frame(&mut app, &ctx, vec![]);
    let origin = output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.job.text.starts_with("history-0 ") => {
                Some(text.pos)
            }
            _ => None,
        })
        .unwrap();
    let font = egui::FontId::monospace(app.tabs[0].style.font_size);
    let (width, height) = ctx.fonts_mut(|fonts| {
        (
            fonts.glyph_width(&font, 'M'),
            fonts.row_height(&font) * app.tabs[0].style.line_spacing,
        )
    });
    let anchor = origin + Vec2::new(0.0, height / 2.0);
    let head = anchor + Vec2::new(9.0 * width, 0.0);
    let press = |pos, pressed| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    frame(
        &mut app,
        &ctx,
        vec![egui::Event::PointerMoved(anchor), press(anchor, true)],
    );
    frame(&mut app, &ctx, vec![egui::Event::PointerMoved(head)]);
    let copied = frame(&mut app, &ctx, vec![press(head, false)]);
    assert!(copied.platform_output.commands.iter().any(
        |command| matches!(command, egui::OutputCommand::CopyText(text) if text == "history-0")
    ));
    app.select_tab(1);
    frame(&mut app, &ctx, vec![]);
    assert!(
        app.tabs[1]
            .selection
            .text(&app.tabs[1].terminal.screen)
            .is_none()
    );
    app.select_tab(0);
    frame(&mut app, &ctx, vec![]);
    assert_eq!(
        app.tabs[0]
            .selection
            .text(&app.tabs[0].terminal.screen)
            .as_deref(),
        Some("history-0")
    );
    app.tabs[0].terminal.screen.feed(b"new output\r\n");
    let output = frame(&mut app, &ctx, vec![]);
    assert_eq!(
        app.tabs[0]
            .selection
            .text(&app.tabs[0].terminal.screen)
            .as_deref(),
        Some("history-0")
    );
    let (track, thumb) = scrollbar_rects(&output).unwrap();
    let grab = thumb.center();
    frame(
        &mut app,
        &ctx,
        vec![egui::Event::PointerMoved(grab), press(grab, true)],
    );
    frame(
        &mut app,
        &ctx,
        vec![egui::Event::PointerMoved(track.center())],
    );
    let released = frame(&mut app, &ctx, vec![press(track.center(), false)]);
    assert!(
        released
            .platform_output
            .commands
            .iter()
            .all(|command| !matches!(command, egui::OutputCommand::CopyText(_))),
        "Dragging the scrollbar must not copy text"
    );
    app.tabs[0].scrollback = app.tabs[0].terminal.screen.history.len();
    frame(&mut app, &ctx, vec![]);
    frame(
        &mut app,
        &ctx,
        vec![egui::Event::PointerMoved(anchor), press(anchor, true)],
    );
    frame(&mut app, &ctx, vec![egui::Event::PointerMoved(head)]);
    // Releasing beyond the left edge clamps to the beginning of the second row.
    let outside = origin + Vec2::new(-30.0, height * 1.5);
    let copied = frame(
        &mut app,
        &ctx,
        vec![egui::Event::PointerMoved(outside), press(outside, false)],
    );
    assert!(copied.platform_output.commands.iter().any(
        |command| matches!(command, egui::OutputCommand::CopyText(text) if text == "history-0\n")
    ));
}

#[test]
fn terminal_scrollbar_drags_history_and_preserves_tab_position_and_input_focus() {
    let ctx = egui::Context::default();
    let mut config = Config::default();
    config.buttons[0] = interactive_button("Alpha");
    config.buttons[1] = interactive_button("Beta");
    let mut app = ButtonBuilder::new(&ctx, Some("/tmp/scrollbar-tabs.yaml".into()), config);
    app.launch(0);
    app.launch(1);
    wait_for_tabs(&mut app, &ctx, |app| {
        app.tabs
            .iter()
            .all(|tab| tab.terminal.screen.text().contains("-ready"))
    });
    app.select_tab(0);
    frame(&mut app, &ctx, vec![]);
    for i in 0..120 {
        app.tabs[0]
            .terminal
            .screen
            .feed(format!("history-{i}\r\n").as_bytes());
    }
    let output = frame(&mut app, &ctx, vec![]);
    let (track, thumb) = scrollbar_rects(&output).unwrap();
    assert!((thumb.bottom() - track.bottom()).abs() < 0.01);
    let grab = egui::pos2(thumb.center().x, thumb.bottom() - 5.0);
    frame(
        &mut app,
        &ctx,
        vec![
            egui::Event::PointerMoved(grab),
            egui::Event::PointerButton {
                pos: grab,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );
    assert_eq!(
        app.tabs[0].scrollback, 0,
        "Grabbing the thumb must not jump"
    );
    frame(
        &mut app,
        &ctx,
        vec![egui::Event::PointerMoved(track.center())],
    );
    assert!(app.tabs[0].scrollback > 0);
    assert!(app.tabs[0].scrollback < app.tabs[0].terminal.screen.history.len());
    frame(
        &mut app,
        &ctx,
        vec![egui::Event::PointerButton {
            pos: track.center(),
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    let saved = app.tabs[0].scrollback;
    app.select_tab(1);
    let output = frame(&mut app, &ctx, vec![]);
    assert!(
        scrollbar_rects(&output).is_none(),
        "No scrollbar without history"
    );
    app.select_tab(0);
    frame(&mut app, &ctx, vec![]);
    assert_eq!(app.tabs[0].scrollback, saved);
    let anchor = app.tabs[0].terminal.screen.history.len() - saved;
    wheel(
        &mut app,
        &ctx,
        track.center(),
        egui::MouseWheelUnit::Line,
        Vec2::new(0.0, 1.0),
        egui::Modifiers::CTRL,
    );
    assert_eq!(
        app.tabs[0].style.font_size, 15.0,
        "Zoom also works over the scrollbar"
    );
    assert_eq!(
        app.tabs[0].terminal.screen.history.len() - app.tabs[0].scrollback,
        anchor
    );
    app.tabs[0].terminal.send(b"arrived\n");
    wait_for_tabs(&mut app, &ctx, |app| {
        app.tabs[0].terminal.screen.text().contains("Alpha:arrived")
    });
    assert_eq!(
        app.tabs[0].terminal.screen.history.len() - app.tabs[0].scrollback,
        anchor,
        "Incoming output must keep the viewed text anchored"
    );
    let output = frame(&mut app, &ctx, vec![]);
    let (track, _) = scrollbar_rects(&output).unwrap();
    click(&mut app, &ctx, track.center_top());
    assert_eq!(
        app.tabs[0].scrollback,
        app.tabs[0].terminal.screen.history.len()
    );
    click(&mut app, &ctx, track.center_bottom() - Vec2::new(0.0, 1.0));
    assert_eq!(app.tabs[0].scrollback, 0, "Bottom resumes live output");
    click(&mut app, &ctx, track.center_top());
    frame(&mut app, &ctx, vec![egui::Event::Text("hello\n".into())]);
    assert_eq!(app.tabs[0].scrollback, 0);
    wait_for_tabs(&mut app, &ctx, |app| {
        app.tabs[0].terminal.screen.text().contains("Alpha:hello")
    });
    assert!(!app.tabs[1].terminal.screen.text().contains("hello"));
}

#[test]
fn terminal_scrollbar_handles_full_buffer_and_alternate_screen_at_minimum_window_size() {
    let ctx = egui::Context::default();
    let mut config = Config::default();
    config.buttons[0] = ButtonConfig {
        label: "Local demo".into(),
        command: "/bin/sleep".into(),
        arguments: "60".into(),
        ..Default::default()
    };
    let mut app = ButtonBuilder::new(&ctx, Some("/tmp/scrollbar-small.yaml".into()), config);
    app.launch(0);
    let small_frame = |app: &mut ButtonBuilder, events| {
        frame_with(
            app,
            &ctx,
            Vec2::new(850.0, 500.0),
            egui::Modifiers::NONE,
            events,
        )
    };
    small_frame(&mut app, vec![]);
    for i in 0..2300 {
        app.tabs[0]
            .terminal
            .screen
            .feed(format!("line-{i}\r\n").as_bytes());
    }
    small_frame(&mut app, vec![]);
    let output = small_frame(&mut app, vec![]);
    let (track, thumb) = scrollbar_rects(&output).unwrap();
    assert!(
        egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(850.0, 500.0)).contains_rect(track)
    );
    assert!(track.contains_rect(thumb));
    assert_eq!(thumb.height(), 24.0);
    let grab = thumb.center();
    small_frame(
        &mut app,
        vec![
            egui::Event::PointerMoved(grab),
            egui::Event::PointerButton {
                pos: grab,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );
    let above = egui::pos2(grab.x, -100.0);
    small_frame(&mut app, vec![egui::Event::PointerMoved(above)]);
    assert_eq!(app.tabs[0].scrollback, 2000);
    let output = small_frame(&mut app, vec![]);
    let (track, thumb) = scrollbar_rects(&output).unwrap();
    assert_eq!(track.top(), thumb.top());
    let below = egui::pos2(grab.x, 600.0);
    small_frame(&mut app, vec![egui::Event::PointerMoved(below)]);
    assert_eq!(app.tabs[0].scrollback, 0);
    small_frame(
        &mut app,
        vec![egui::Event::PointerButton {
            pos: below,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    app.tabs[0].terminal.screen.feed(b"\x1b[?1049h");
    let output = small_frame(&mut app, vec![]);
    assert!(
        scrollbar_rects(&output).is_none(),
        "Full-screen programs use their alternate screen"
    );
    app.tabs[0].terminal.screen.feed(b"\x1b[?1049l");
    assert!(scrollbar_rects(&small_frame(&mut app, vec![])).is_some());
}

#[test]
fn control_wheel_zooms_only_active_tab_and_preserves_history_anchor() {
    let ctx = egui::Context::default();
    let mut config = Config::default();
    config.buttons[0] = interactive_button("Alpha");
    config.buttons[1] = interactive_button("Beta");
    let mut app = ButtonBuilder::new(
        &ctx,
        Some("/tmp/rs-button-builder-wheel-zoom-test.yaml".into()),
        config,
    );
    app.launch(0);
    app.launch(1);
    wait_for_tabs(&mut app, &ctx, |app| {
        app.tabs
            .iter()
            .all(|tab| tab.terminal.screen.text().contains("-ready"))
    });
    app.select_tab(0);
    frame(&mut app, &ctx, vec![]);
    for i in 0..100 {
        app.tabs[0]
            .terminal
            .screen
            .feed(format!("history-{i}\r\n").as_bytes());
    }
    app.tabs[0].scrollback = 20;
    app.tabs[0].scroll_fraction = 0.5;
    frame(&mut app, &ctx, vec![]);
    let anchor = app.tabs[0].terminal.screen.history.len() - app.tabs[0].scrollback;
    let first_size = app.terminal_size;
    let second_size = (
        app.tabs[1].terminal.screen.cols,
        app.tabs[1].terminal.screen.rows,
    );
    let pane = egui::pos2(700.0, 400.0);

    wheel(
        &mut app,
        &ctx,
        pane,
        egui::MouseWheelUnit::Line,
        Vec2::new(0.0, 2.0),
        egui::Modifiers::CTRL,
    );
    assert_eq!(app.tabs[0].style.font_size, 16.0);
    assert_eq!(app.tabs[1].style.font_size, 14.0);
    assert_eq!(app.config.terminal.font_size, 16.0);
    assert!(app.dirty);
    assert!(app.terminal_size.0 < first_size.0);
    assert!(app.terminal_size.1 < first_size.1);
    assert_eq!(
        app.terminal_size,
        (
            app.tabs[0].terminal.screen.cols,
            app.tabs[0].terminal.screen.rows
        )
    );
    assert_eq!(
        second_size,
        (
            app.tabs[1].terminal.screen.cols,
            app.tabs[1].terminal.screen.rows
        )
    );
    assert_eq!(
        app.tabs[0].terminal.screen.history.len() - app.tabs[0].scrollback,
        anchor
    );
    assert_eq!(app.tabs[0].scroll_fraction, 0.5);
    assert_eq!(ctx.zoom_factor(), 1.0);

    wheel(
        &mut app,
        &ctx,
        pane,
        egui::MouseWheelUnit::Line,
        Vec2::new(0.0, -1.0),
        egui::Modifiers::CTRL,
    );
    assert_eq!(app.tabs[0].style.font_size, 15.0);
    // egui's smoothed zoom input must not keep changing font size after the wheel stops.
    for _ in 0..12 {
        frame(&mut app, &ctx, vec![]);
    }
    assert_eq!(app.tabs[0].style.font_size, 15.0);
    assert_eq!(
        app.tabs[0].terminal.screen.history.len() - app.tabs[0].scrollback,
        anchor
    );
    let scrollback = app.tabs[0].scrollback;
    for _ in 0..5 {
        wheel(
            &mut app,
            &ctx,
            pane,
            egui::MouseWheelUnit::Point,
            Vec2::new(0.0, 7.0),
            egui::Modifiers::NONE,
        );
    }
    assert!(
        app.tabs[0].scrollback > scrollback,
        "Plain scrolling still moves through history"
    );
    assert_eq!(app.tabs[0].style.font_size, 15.0);
    app.save();
    assert_eq!(
        Config::load_existing(app.config_path.as_deref().unwrap())
            .unwrap()
            .terminal
            .font_size,
        15.0
    );
    app.config.buttons[2] = interactive_button("Gamma");
    app.launch(2);
    assert_eq!(app.tabs[2].style.font_size, 15.0);
    assert_eq!(app.tabs[1].style.font_size, 14.0);
}

#[test]
fn control_wheel_requires_hovering_terminal_and_respects_font_limits() {
    let ctx = egui::Context::default();
    let mut app = ButtonBuilder::new(
        &ctx,
        Some("/tmp/rs-button-builder-wheel-limits-test.yaml".into()),
        Config::default(),
    );
    frame(&mut app, &ctx, vec![]);
    let output = frame(&mut app, &ctx, vec![]);
    for pos in [
        egui::pos2(100.0, 300.0),
        label_center(&output, "Appearance"),
    ] {
        wheel(
            &mut app,
            &ctx,
            pos,
            egui::MouseWheelUnit::Line,
            Vec2::new(0.0, 1.0),
            egui::Modifiers::CTRL,
        );
        assert_eq!(app.config.terminal.font_size, 14.0);
        assert!(!app.dirty);
    }
    let pane = egui::pos2(700.0, 400.0);
    wheel(
        &mut app,
        &ctx,
        pane,
        egui::MouseWheelUnit::Point,
        Vec2::new(0.0, 10.0),
        egui::Modifiers::CTRL,
    );
    assert_eq!(app.config.terminal.font_size, 14.25);
    wheel(
        &mut app,
        &ctx,
        pane,
        egui::MouseWheelUnit::Line,
        Vec2::new(0.0, 200.0),
        egui::Modifiers::CTRL,
    );
    assert_eq!(app.config.terminal.font_size, 32.0);
    app.dirty = false;
    wheel(
        &mut app,
        &ctx,
        pane,
        egui::MouseWheelUnit::Line,
        Vec2::new(0.0, 1.0),
        egui::Modifiers::CTRL,
    );
    assert_eq!(app.config.terminal.font_size, 32.0);
    assert!(
        !app.dirty,
        "Scrolling past a limit must not mark the layout changed"
    );
    wheel(
        &mut app,
        &ctx,
        pane,
        egui::MouseWheelUnit::Line,
        Vec2::new(0.0, -200.0),
        egui::Modifiers::CTRL,
    );
    assert_eq!(app.config.terminal.font_size, 8.0);
    app.dirty = false;
    wheel(
        &mut app,
        &ctx,
        pane,
        egui::MouseWheelUnit::Line,
        Vec2::new(1.0, 0.0),
        egui::Modifiers::CTRL,
    );
    assert!(
        !app.dirty,
        "Horizontal scrolling does not zoom the terminal"
    );
    app.appearance_open = true;
    frame(&mut app, &ctx, vec![]);
    frame(&mut app, &ctx, vec![]);
    wheel(
        &mut app,
        &ctx,
        pane,
        egui::MouseWheelUnit::Line,
        Vec2::new(0.0, 1.0),
        egui::Modifiers::CTRL,
    );
    assert_eq!(
        app.config.terminal.font_size, 8.0,
        "A dialog covering the terminal blocks pane zoom"
    );
    assert_eq!(ctx.zoom_factor(), 1.0);
}

#[test]
fn tabs_preserve_appearance_history_and_resize_inactive_processes() {
    let ctx = egui::Context::default();
    let mut config = Config::default();
    config.buttons[0] = interactive_button("Alpha");
    config.buttons[1] = interactive_button("Beta");
    config.buttons[2] = interactive_button("Gamma");
    let mut app = ButtonBuilder::new(
        &ctx,
        Some("/tmp/rs-button-builder-tab-appearance-test.yaml".into()),
        config,
    );
    app.launch(0);
    app.launch(1);
    frame(&mut app, &ctx, vec![]);
    wait_for_tabs(&mut app, &ctx, |app| {
        app.tabs
            .iter()
            .all(|tab| tab.terminal.screen.text().contains("-ready"))
    });
    // Build history without depending on asynchronous process timing.
    for i in 0..100 {
        app.tabs[0]
            .terminal
            .screen
            .feed(format!("history-{i}\r\n").as_bytes());
    }
    app.tabs[0].scrollback = 10;
    app.tabs[0].scroll_fraction = 0.5;
    app.tabs[0].style.font_size = 22.0;
    app.tabs[0].style.bg_color = Some([10, 30, 40]);
    app.select_tab(0);
    let output = frame(&mut app, &ctx, vec![]);
    let history_anchor = app.tabs[0].terminal.screen.history.len() - app.tabs[0].scrollback;
    click(&mut app, &ctx, label_center(&output, "Appearance"));
    let output = frame(&mut app, &ctx, vec![]);
    let pos = label_center(&output, "22.0 pt");
    // The numeric field is editable through keyboard input.
    click(&mut app, &ctx, pos);
    frame_with(
        &mut app,
        &ctx,
        Vec2::new(1200.0, 760.0),
        egui::Modifiers::CTRL,
        vec![egui::Event::Key {
            key: egui::Key::A,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::CTRL,
        }],
    );
    frame(
        &mut app,
        &ctx,
        vec![
            egui::Event::Text("20".into()),
            egui::Event::Key {
                key: egui::Key::Enter,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );
    assert_eq!(app.tabs[0].style.font_size, 20.0);
    assert_eq!(app.tabs[1].style.font_size, 14.0);
    let output = frame(&mut app, &ctx, vec![]);
    click(&mut app, &ctx, label_center(&output, "Save appearance"));
    assert_eq!(
        Config::load_existing(app.config_path.as_deref().unwrap())
            .unwrap()
            .terminal
            .font_size,
        20.0
    );
    let output = frame(&mut app, &ctx, vec![]);
    click(&mut app, &ctx, label_center(&output, "Close"));
    let first_cols = app.tabs[0].terminal.screen.cols;
    let first_rows = app.tabs[0].terminal.screen.rows;
    app.select_tab(1);
    let second_output = frame(&mut app, &ctx, vec![]);
    assert!(!second_output.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Text(text) if text.galley.job.text == "Jump to live")));
    assert_eq!(
        app.tabs[0].terminal.screen.history.len() - app.tabs[0].scrollback,
        history_anchor
    );
    assert_eq!(app.tabs[0].scroll_fraction, 0.5);
    for _ in 0..3 {
        frame_with(
            &mut app,
            &ctx,
            Vec2::new(850.0, 500.0),
            egui::Modifiers::NONE,
            vec![],
        );
    }
    assert!(app.tabs[0].terminal.screen.cols < first_cols);
    assert!(app.tabs[0].terminal.screen.rows < first_rows);
    assert!(app.tabs[0].terminal.screen.cols < app.tabs[1].terminal.screen.cols);
    app.select_tab(0);
    let output = frame(&mut app, &ctx, vec![]);
    let output = frame(
        &mut app,
        &ctx,
        vec![egui::Event::PointerMoved(label_center(
            &output,
            "Jump to live",
        ))],
    );
    click(&mut app, &ctx, label_center(&output, "Jump to live"));
    assert_eq!(app.tabs[0].scrollback, 0);
    assert_eq!(app.tabs[0].scroll_fraction, 0.0);
    app.launch(2);
    assert_eq!(
        app.tabs[2].style.font_size, 20.0,
        "New tabs inherit the edited appearance default"
    );
    app.select_tab(1);
    app.appearance_open = true;
    frame(&mut app, &ctx, vec![]);
    let output = frame(&mut app, &ctx, vec![]);
    click(&mut app, &ctx, label_center(&output, "Save appearance"));
    assert_eq!(
        Config::load_existing(app.config_path.as_deref().unwrap())
            .unwrap()
            .terminal
            .font_size,
        14.0,
        "Saving an unchanged tab's appearance must make its settings the default"
    );
    assert_eq!(app.tabs[0].style.font_size, 20.0);
    assert_eq!(app.tabs[2].style.font_size, 20.0);
}

#[test]
fn failed_launch_and_config_loading_preserve_multiple_sessions() {
    let ctx = egui::Context::default();
    let mut config = Config::default();
    config.buttons[0] = interactive_button("Alpha");
    config.buttons[2] = interactive_button("Beta");
    config.buttons[1].command = "/this/local/program/does/not/exist".into();
    let mut app = ButtonBuilder::new(
        &ctx,
        Some("/tmp/rs-button-builder-tab-load-test.yaml".into()),
        config,
    );
    app.launch(0);
    app.launch(2);
    app.tabs[0].style.bg_color = Some([10, 30, 40]);
    app.tabs[1].terminal.screen.feed(b"Retain me");
    let next_id = app.next_tab_id;
    app.launch(1);
    assert!(app.error.is_some());
    assert_eq!(app.tabs.len(), 2);
    assert_eq!(app.active_tab, Some(1));
    assert_eq!(app.next_tab_id, next_id);
    app.tabs[1].terminal.stop();
    app.launch(1);
    assert!(app.error.is_some());
    assert!(app.tabs[1].terminal.screen.text().contains("Retain me"));
    assert_eq!(app.tabs[1].id, 2);
    app.load_config(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/terminal-demo.yaml"))
        .unwrap();
    assert_eq!(app.tabs.len(), 2);
    assert_eq!(app.active_tab, Some(1));
    assert!(
        app.running(),
        "A background process still counts as running"
    );
    assert!(app.tabs[0].terminal.is_running());
    assert_eq!(app.tabs[0].label, "Alpha");
    assert_eq!(app.tabs[0].style.bg_color, Some([10, 30, 40]));
    assert!(app.tabs.iter().all(|tab| tab.button_index.is_none()));
    app.launch(0);
    assert_eq!(app.tabs[2].label, app.config.buttons[0].label);
    assert_eq!(app.tabs[2].style.font_size, app.config.terminal.font_size);
    app.close_tab(0);
    assert_eq!(app.active_tab, Some(1));
    assert_eq!(app.active().unwrap().id, 3);
    app.close_tab(1);
    assert_eq!(app.active_tab, Some(0));
    assert_eq!(app.active().unwrap().id, 2);
}

#[test]
fn many_tabs_scroll_and_keep_toolbar_controls_visible_at_minimum_size() {
    let ctx = egui::Context::default();
    let mut config = Config::default();
    for button in &mut config.buttons[..8] {
        *button = interactive_button("A long running command label");
    }
    let mut app = ButtonBuilder::new(
        &ctx,
        Some("/tmp/rs-button-builder-many-tabs-test.yaml".into()),
        config,
    );
    for index in 0..8 {
        app.launch(index);
    }
    let size = Vec2::new(850.0, 500.0);
    let mut output = egui::FullOutput::default();
    for _ in 0..40 {
        output = frame_with(&mut app, &ctx, size, egui::Modifiers::NONE, vec![]);
    }
    let title = "8 · A long running command label";
    let label = output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.job.text == title => {
                Some((text, shape.clip_rect))
            }
            _ => None,
        })
        .unwrap();
    assert!(
        label
            .1
            .contains(label.0.pos + label.0.galley.rect.center().to_vec2()),
        "The newly selected tab must scroll into view"
    );
    for action in ["Appearance", "Copy", "Ctrl+C", "Stop", "Close tab"] {
        let pos = label_center(&output, action);
        assert!(pos.x < size.x && pos.y < size.y);
        assert!(output.shapes.iter().any(|shape| shape.clip_rect.contains(pos)
            && matches!(&shape.shape, egui::Shape::Text(text) if text.galley.job.text == action)));
    }
    let left = label_center(&output, "‹");
    let right = label_center(&output, "›");
    assert!(
        left.x < label.1.left() && right.x > label.1.right(),
        "Arrows must stay outside the scrolling tabs"
    );
    assert!(right.x < size.x && (left.y - right.y).abs() < 1.0);
    for glyph in ["‹", "›"] {
        assert_eq!(output.shapes.iter().filter(|shape|
            matches!(&shape.shape, egui::Shape::Text(text) if text.galley.job.text == glyph)
        ).count(), 1, "Use exactly one arrow at each end");
    }
    let click_arrow = |app: &mut ButtonBuilder, pos| {
        for pressed in [true, false] {
            frame_with(
                app,
                &ctx,
                size,
                egui::Modifiers::NONE,
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
        }
    };
    click_arrow(&mut app, right);
    assert_eq!(
        app.active_tab,
        Some(7),
        "The next arrow stops at the last tab"
    );
    for expected in (0..7).rev() {
        click_arrow(&mut app, left);
        assert_eq!(app.active_tab, Some(expected));
    }
    click_arrow(&mut app, left);
    assert_eq!(
        app.active_tab,
        Some(0),
        "The previous arrow stops at the first tab"
    );
    for _ in 0..40 {
        output = frame_with(&mut app, &ctx, size, egui::Modifiers::NONE, vec![]);
    }
    let first = label_center(&output, "1 · A long running command label");
    assert!(first.x > left.x && first.x < right.x);
    for expected in 1..8 {
        click_arrow(&mut app, right);
        assert_eq!(app.active_tab, Some(expected));
    }
    assert!(app.tabs.iter().all(|tab| tab.terminal.is_running()));
    // Empty slots and edit mode keep configuring buttons instead of launching.
    app.launch(8);
    assert_eq!(app.tabs.len(), 8);
    assert_eq!(app.editor.as_ref().unwrap().index, 8);
    app.editor = None;
    app.edit_mode = true;
    let output = frame(&mut app, &ctx, vec![]);
    click(
        &mut app,
        &ctx,
        label_center(&output, "A long running command label"),
    );
    assert_eq!(app.tabs.len(), 8);
    assert_eq!(app.editor.as_ref().unwrap().index, 0);
}

#[test]
fn layout_heading_edits_cancel_save_and_follow_the_loaded_config() {
    let ctx = egui::Context::default();
    let path = std::env::temp_dir().join(format!(
        "rs-button-builder-layout-name-{}.yaml",
        std::process::id()
    ));
    let mut app = ButtonBuilder::new(&ctx, Some(path.clone()), Config::default());
    let output = frame(&mut app, &ctx, vec![]);
    let assert_title = |output: &egui::FullOutput, expected: &str| {
        assert!(output.viewport_output[&egui::ViewportId::ROOT]
            .commands
            .iter()
            .any(|command| matches!(command, egui::ViewportCommand::Title(title) if title == expected)));
    };
    let assert_no_title_change = |output: &egui::FullOutput| {
        assert!(
            output.viewport_output[&egui::ViewportId::ROOT]
                .commands
                .iter()
                .all(|command| !matches!(command, egui::ViewportCommand::Title(_)))
        );
    };
    assert_title(&output, "Button Builder - Your buttons");
    let output = frame(&mut app, &ctx, vec![]);
    assert_no_title_change(&output);
    click(&mut app, &ctx, label_center(&output, DEFAULT_LAYOUT_NAME));
    assert!(app.layout_name_edit.is_some());
    frame(&mut app, &ctx, vec![]);
    let key = |key, modifiers| egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    };
    let replace_name = |app: &mut ButtonBuilder, name: &str| {
        let select_all = egui::Modifiers {
            command: true,
            ..egui::Modifiers::CTRL
        };
        frame_with(
            app,
            &ctx,
            Vec2::new(1200.0, 760.0),
            select_all,
            vec![key(egui::Key::A, select_all)],
        );
        frame(app, &ctx, vec![egui::Event::Text(name.into())]);
    };
    replace_name(&mut app, "  Workstation · Local  ");
    let output = frame(
        &mut app,
        &ctx,
        vec![key(egui::Key::Enter, egui::Modifiers::NONE)],
    );
    assert_title(&output, "Button Builder - Workstation · Local");
    assert_eq!(app.config.display_name, "Workstation · Local");
    assert!(app.dirty && app.layout_name_edit.is_none());
    assert!(app.tabs.is_empty(), "Renaming must not launch a process");
    let output = frame(&mut app, &ctx, vec![]);
    click_app_action(&mut app, &ctx, &output, "Save layout *");
    assert_eq!(
        Config::load_existing(&path).unwrap().display_name,
        "Workstation · Local"
    );

    let output = frame(&mut app, &ctx, vec![]);
    click(&mut app, &ctx, label_center(&output, "Workstation · Local"));
    frame(&mut app, &ctx, vec![]);
    replace_name(&mut app, "Cancel this change");
    let output = frame(
        &mut app,
        &ctx,
        vec![key(egui::Key::Escape, egui::Modifiers::NONE)],
    );
    assert_no_title_change(&output);
    assert_eq!(app.config.display_name, "Workstation · Local");
    assert!(!app.dirty && app.layout_name_edit.is_none());

    let output = frame(&mut app, &ctx, vec![]);
    click(&mut app, &ctx, label_center(&output, "Workstation · Local"));
    frame(&mut app, &ctx, vec![]);
    replace_name(&mut app, "Save while typing");
    let output = frame(&mut app, &ctx, vec![]);
    click_app_action(&mut app, &ctx, &output, "Save layout");
    assert_eq!(
        Config::load_existing(&path).unwrap().display_name,
        "Save while typing"
    );
    assert!(!app.dirty && app.layout_name_edit.is_none());

    let output = frame(&mut app, &ctx, vec![]);
    click(&mut app, &ctx, label_center(&output, "Save while typing"));
    frame(&mut app, &ctx, vec![]);
    replace_name(&mut app, "   ");
    let output = frame(&mut app, &ctx, vec![]);
    click(&mut app, &ctx, label_center(&output, "Edit buttons"));
    assert_eq!(app.config.display_name, DEFAULT_LAYOUT_NAME);
    assert!(app.dirty && app.layout_name_edit.is_none());

    app.load_config(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/json-streams.yaml"))
        .unwrap();
    let output = frame(&mut app, &ctx, vec![]);
    label_center(&output, "JSON Streams");
    assert_title(&output, "Button Builder - JSON Streams");
    assert!(!app.dirty && app.layout_name_edit.is_none());
    click(&mut app, &ctx, label_center(&output, "JSON Streams"));
    assert!(app.layout_name_edit.is_some());
    app.load_config(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/system-monitor.yaml"))
        .unwrap();
    assert!(app.layout_name_edit.is_none());
    assert_eq!(app.config.display_name, "System Monitor");
    let output = frame(&mut app, &ctx, vec![]);
    assert_title(&output, "Button Builder - System Monitor");
    app.create_new_layout();
    let output = frame(&mut app, &ctx, vec![]);
    assert_title(&output, "Button Builder - Your buttons");
}

#[test]
fn button_run_options_cancel_and_save_per_button() {
    let ctx = egui::Context::default();
    let path = std::env::temp_dir().join(format!(
        "rs-button-builder-log-checkbox-{}.yaml",
        std::process::id()
    ));
    let mut app = ButtonBuilder::new(&ctx, Some(path.clone()), Config::default());
    app.edit(0);
    frame(&mut app, &ctx, vec![]);
    let output = frame(&mut app, &ctx, vec![]);
    assert!(!app.editor.as_ref().unwrap().button.log_output);
    assert!(!app.editor.as_ref().unwrap().button.auto_start);
    assert!(!app.editor.as_ref().unwrap().button.close_on_success);
    click(&mut app, &ctx, label_center(&output, "Log output"));
    click(&mut app, &ctx, label_center(&output, "Auto start"));
    click(&mut app, &ctx, label_center(&output, "Close on success"));
    assert!(app.editor.as_ref().unwrap().button.log_output);
    assert!(app.editor.as_ref().unwrap().button.auto_start);
    assert!(app.editor.as_ref().unwrap().button.close_on_success);
    let output = frame(&mut app, &ctx, vec![]);
    click(&mut app, &ctx, label_center(&output, "Cancel"));
    assert!(!app.config.buttons[0].log_output);
    assert!(!app.config.buttons[0].auto_start);
    assert!(!app.config.buttons[0].close_on_success);

    app.edit(0);
    let output = frame(&mut app, &ctx, vec![]);
    click(&mut app, &ctx, label_center(&output, "Log output"));
    click(&mut app, &ctx, label_center(&output, "Auto start"));
    click(&mut app, &ctx, label_center(&output, "Close on success"));
    let output = frame(&mut app, &ctx, vec![]);
    click(&mut app, &ctx, label_center(&output, "Save button"));
    let saved = Config::load_existing(&path).unwrap();
    assert!(saved.buttons[0].log_output);
    assert!(saved.buttons[0].auto_start);
    assert!(saved.buttons[0].close_on_success);
    assert!(!saved.buttons[1].log_output);
    assert!(!saved.buttons[1].auto_start);
    assert!(!saved.buttons[1].close_on_success);

    app.edit(0);
    let output = frame(&mut app, &ctx, vec![]);
    assert!(app.editor.as_ref().unwrap().button.log_output);
    assert!(app.editor.as_ref().unwrap().button.auto_start);
    assert!(app.editor.as_ref().unwrap().button.close_on_success);
    click(&mut app, &ctx, label_center(&output, "Log output"));
    click(&mut app, &ctx, label_center(&output, "Auto start"));
    click(&mut app, &ctx, label_center(&output, "Close on success"));
    let output = frame(&mut app, &ctx, vec![]);
    click(&mut app, &ctx, label_center(&output, "Save button"));
    let saved = Config::load_existing(&path).unwrap();
    assert!(!saved.buttons[0].log_output);
    assert!(!saved.buttons[0].auto_start);
    assert!(!saved.buttons[0].close_on_success);
}

#[test]
fn loaded_layouts_auto_start_visible_buttons_in_separate_tabs() {
    let ctx = egui::Context::default();
    let directory = std::env::temp_dir();
    let startup_path = directory.join(format!(
        "rs-button-builder-auto-start-{}.yaml",
        std::process::id()
    ));
    let loaded_path = directory.join(format!(
        "rs-button-builder-auto-load-{}.yaml",
        std::process::id()
    ));
    let mut startup = Config {
        rows: 1,
        columns: 2,
        ..Default::default()
    };
    startup.buttons[0] = ButtonConfig {
        auto_start: true,
        ..interactive_button("Alpha")
    };
    startup.buttons[1] = ButtonConfig {
        auto_start: true,
        ..interactive_button("Beta")
    };
    startup.buttons[2] = ButtonConfig {
        auto_start: true,
        ..interactive_button("Hidden")
    };
    startup.save(&startup_path).unwrap();

    let mut app = ButtonBuilder::new(&ctx, Some(startup_path), startup);
    assert_eq!(app.tabs.len(), 2);
    assert_eq!(
        app.tabs
            .iter()
            .map(|tab| (tab.label.as_str(), tab.button_index))
            .collect::<Vec<_>>(),
        [("Alpha", Some(0)), ("Beta", Some(1))]
    );
    wait_for_tabs(&mut app, &ctx, |app| {
        app.tabs
            .iter()
            .all(|tab| tab.terminal.screen.text().contains("-ready"))
    });

    let mut loaded = Config {
        rows: 1,
        columns: 1,
        ..Default::default()
    };
    loaded.buttons[0] = ButtonConfig {
        auto_start: true,
        ..interactive_button("Gamma")
    };
    loaded.save(&loaded_path).unwrap();
    app.load_config(loaded_path).unwrap();

    assert_eq!(app.tabs.len(), 3);
    assert!(app.tabs[..2].iter().all(|tab| tab.button_index.is_none()));
    assert_eq!(app.tabs[2].label, "Gamma");
    assert_eq!(app.tabs[2].button_index, Some(0));
}

#[test]
fn button_context_menu_resets_one_slot_and_preserves_running_terminals() {
    let ctx = egui::Context::default();
    let path = std::env::temp_dir().join(format!(
        "rs-button-builder-reset-{}.yaml",
        std::process::id()
    ));
    let mut config = Config::default();
    config.buttons[1] = ButtonConfig {
        arguments: "'unused argument'".into(),
        working_directory: "/tmp".into(),
        log_output: true,
        auto_start: false,
        fg_color: Some([230, 240, 250]),
        bg_color: Some([20, 40, 60]),
        ..interactive_button("Alpha")
    };
    config.buttons[2] = interactive_button("Beta");
    let mut expected = config.clone();
    let mut app = ButtonBuilder::new(&ctx, Some(path.clone()), config);
    app.launch(1);
    app.launch(2);
    wait_for_tabs(&mut app, &ctx, |app| {
        app.tabs
            .iter()
            .all(|tab| tab.terminal.screen.text().contains("-ready"))
    });
    let output = frame(&mut app, &ctx, vec![]);
    let pos = label_center(&output, "Alpha");
    for pressed in [true, false] {
        frame(
            &mut app,
            &ctx,
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
    let output = frame(&mut app, &ctx, vec![]);
    label_center(&output, "Edit button…");
    let reset = label_center(&output, "Reset");
    assert!(
        !app.dirty && app.editor.is_none(),
        "Opening the menu must not change the button"
    );
    click(&mut app, &ctx, reset);
    expected.buttons[1] = Config::default().buttons[1].clone();
    assert_eq!(
        serde_yaml_ng::to_string(&app.config).unwrap(),
        serde_yaml_ng::to_string(&expected).unwrap()
    );
    assert!(app.dirty && app.editor.is_none());
    assert_eq!(app.tabs.len(), 2);
    assert!(app.tabs.iter().all(|tab| tab.terminal.is_running()));
    assert_eq!(app.tabs[0].button_index, None);
    assert_eq!(app.tabs[1].button_index, Some(2));
    assert!(app.tabs[0].terminal.screen.text().contains("Alpha-ready"));
    let output = frame(&mut app, &ctx, vec![]);
    let empty = empty_button_centers(&output)[1];
    click_app_action(&mut app, &ctx, &output, "Save layout *");
    assert!(!app.dirty);
    assert_eq!(
        serde_yaml_ng::to_string(&Config::load_existing(&path).unwrap()).unwrap(),
        serde_yaml_ng::to_string(&expected).unwrap()
    );
    click(&mut app, &ctx, empty);
    assert_eq!(app.editor.as_ref().unwrap().index, 1);
    assert!(app.editor.as_ref().unwrap().button.command.is_empty());
}

#[test]
fn button_drag_swaps_positions_without_running_and_saves_full_definitions() {
    let ctx = egui::Context::default();
    let path = std::env::temp_dir().join(format!(
        "rs-button-builder-reposition-{}.yaml",
        std::process::id()
    ));
    let mut config = Config::default();
    config.resize_grid(2, 3).unwrap();
    config.buttons[0] = ButtonConfig {
        label: "Alpha".into(),
        command: "/usr/bin/printf".into(),
        arguments: "'alpha output\\n'".into(),
        working_directory: "/tmp".into(),
        log_output: true,
        auto_start: false,
        fg_color: Some([230, 240, 250]),
        bg_color: Some([20, 40, 60]),
        ..Default::default()
    };
    config.buttons[4] = interactive_button("Beta");
    let mut expected = config.clone();
    let mut app = ButtonBuilder::new(&ctx, Some(path.clone()), config);
    frame(&mut app, &ctx, vec![]);
    let output = frame(&mut app, &ctx, vec![]);
    let alpha = label_center(&output, "Alpha");
    let beta = label_center(&output, "Beta");
    let dragging = start_button_drag(&mut app, &ctx, alpha, beta);
    assert_eq!(
        dragging.platform_output.cursor_icon,
        egui::CursorIcon::Grabbing
    );
    assert!(
        dragging.shapes.iter().any(|shape| matches!(&shape.shape,
            egui::Shape::Rect(rect) if rect.rect.contains(beta)
                && rect.stroke.width == 3.0 && rect.stroke.color == get_global_color("primary")
        )),
        "Show the destination slot while dragging"
    );
    assert!(!app.dirty, "Only commit a move when the mouse is released");
    frame(&mut app, &ctx, vec![primary_pointer(beta, false)]);
    expected.buttons.swap(0, 4);
    assert_eq!(
        serde_yaml_ng::to_string(&app.config).unwrap(),
        serde_yaml_ng::to_string(&expected).unwrap()
    );
    assert!(app.dirty && app.tabs.is_empty() && app.editor.is_none());
    let output = frame(&mut app, &ctx, vec![]);
    assert!(label_center(&output, "Alpha").distance(beta) < 1.0);
    assert!(label_center(&output, "Beta").distance(alpha) < 1.0);
    // Moving backwards in grid order also works, including an unassigned target.
    let empty = empty_button_centers(&output)[0];
    start_button_drag(&mut app, &ctx, beta, empty);
    frame(&mut app, &ctx, vec![primary_pointer(empty, false)]);
    expected.buttons.swap(4, 1);
    let output = frame(&mut app, &ctx, vec![]);
    click_app_action(&mut app, &ctx, &output, "Save layout *");
    assert!(!app.dirty && app.tabs.is_empty() && app.editor.is_none());
    assert_eq!(
        serde_yaml_ng::to_string(&Config::load_existing(&path).unwrap()).unwrap(),
        serde_yaml_ng::to_string(&expected).unwrap()
    );
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

#[test]
fn button_drag_cancellation_and_edit_mode_preserve_click_behavior() {
    let ctx = egui::Context::default();
    let mut config = Config::default();
    config.buttons[0] = interactive_button("Alpha");
    let mut app = ButtonBuilder::new(&ctx, None, config);
    frame(&mut app, &ctx, vec![]);
    let output = frame(&mut app, &ctx, vec![]);
    let source = label_center(&output, "Alpha");
    let target = empty_button_centers(&output)[0];
    let original = serde_yaml_ng::to_string(&app.config).unwrap();
    let outside = egui::pos2(800.0, 600.0);
    for destination in [outside, source] {
        start_button_drag(&mut app, &ctx, source, target);
        frame(
            &mut app,
            &ctx,
            vec![
                egui::Event::PointerMoved(destination),
                primary_pointer(destination, false),
            ],
        );
        assert_eq!(serde_yaml_ng::to_string(&app.config).unwrap(), original);
        assert!(!app.dirty && app.tabs.is_empty() && app.editor.is_none());
    }
    start_button_drag(&mut app, &ctx, source, target);
    frame(
        &mut app,
        &ctx,
        vec![egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    assert!(app.dragged_button.is_none());
    frame(&mut app, &ctx, vec![primary_pointer(target, false)]);
    assert_eq!(serde_yaml_ng::to_string(&app.config).unwrap(), original);
    assert!(!app.dirty && app.tabs.is_empty() && app.editor.is_none());
    let output = frame(&mut app, &ctx, vec![]);
    click(&mut app, &ctx, label_center(&output, "Edit buttons"));
    assert!(app.edit_mode);
    start_button_drag(&mut app, &ctx, source, target);
    frame(&mut app, &ctx, vec![primary_pointer(target, false)]);
    assert!(app.dirty && app.editor.is_none() && app.tabs.is_empty());
    assert_eq!(app.config.buttons[1].label, "Alpha");
    click(&mut app, &ctx, target);
    assert_eq!(app.editor.as_ref().unwrap().index, 1);
    let output = frame(&mut app, &ctx, vec![]);
    click(&mut app, &ctx, label_center(&output, "Cancel"));
    let output = frame(&mut app, &ctx, vec![]);
    click(&mut app, &ctx, label_center(&output, "Edit buttons"));
    assert!(!app.edit_mode);
    click(&mut app, &ctx, target);
    assert_eq!(app.tabs.len(), 1);
    assert_eq!(app.tabs[0].button_index, Some(1));
    assert_eq!(app.tabs[0].label, "Alpha");
}

#[test]
fn repositioning_buttons_preserves_tab_associations_and_editor_drafts() {
    let ctx = egui::Context::default();
    let mut config = Config::default();
    config.buttons[0] = interactive_button("Alpha");
    config.buttons[1] = interactive_button("Beta");
    let mut app = ButtonBuilder::new(&ctx, None, config);
    app.launch(0);
    app.tabs[0].button_index = None; // A tab retained from a previous layout.
    app.launch(0);
    app.launch(1);
    app.edit(0);
    app.editor.as_mut().unwrap().button.label = "Alpha draft".into();
    app.reposition_button(0, 1);
    assert_eq!(
        app.tabs
            .iter()
            .map(|tab| tab.button_index)
            .collect::<Vec<_>>(),
        [None, Some(1), Some(0)]
    );
    assert!(app.tabs.iter().all(|tab| tab.terminal.is_running()));
    assert_eq!(app.active_tab, Some(2));
    app.launch(1);
    assert_eq!(app.tabs.len(), 3);
    assert_eq!(
        app.active_tab,
        Some(1),
        "Moved buttons still focus their running tab"
    );
    assert_eq!(app.editor.as_ref().unwrap().index, 1);
    assert_eq!(app.editor.as_ref().unwrap().button.label, "Alpha draft");
    assert_eq!(app.config.buttons[1].label, "Alpha");
    assert_eq!(app.config.buttons[0].label, "Beta");
    app.dirty = false;
    app.reposition_button(1, 1);
    app.reposition_button(0, usize::MAX);
    app.config.resize_grid(1, 1).unwrap();
    app.reposition_button(0, 1);
    assert!(
        !app.dirty,
        "No-op and hidden-slot moves must not change the layout"
    );
    app.dragged_button = Some(0);
    app.create_new_layout();
    assert!(
        app.dragged_button.is_none(),
        "Replacing a layout cancels its drag"
    );
}

#[test]
fn grid_editor_save_and_launch_work_through_the_ui() {
    let ctx = egui::Context::default();
    let path = std::env::temp_dir().join(format!(
        "rs-button-builder-ui-test-{}.yaml",
        std::process::id()
    ));
    let mut app = ButtonBuilder::new(&ctx, Some(path.clone()), Config::default());
    frame(&mut app, &ctx, vec![]);
    let output = frame(&mut app, &ctx, vec![]);
    let positions = empty_button_centers(&output);
    assert_eq!(positions.len(), 9);
    for row in 0..3 {
        assert!((positions[row * 3].y - positions[row * 3 + 1].y).abs() < 15.0);
        assert!(positions[row * 3].x < positions[row * 3 + 1].x);
        assert!(positions[row * 3 + 1].x < positions[row * 3 + 2].x);
    }
    assert!(positions[0].y < positions[3].y && positions[3].y < positions[6].y);
    click(&mut app, &ctx, positions[1]);
    assert_eq!(app.editor.as_ref().unwrap().index, 1);
    let editor = app.editor.as_mut().unwrap();
    editor.button.label = "Local output".into();
    editor.button.command = "/usr/bin/printf".into();
    editor.button.arguments = "'hello from a button\\n'".into();
    editor.button.fg_color = Some([250, 230, 210]);
    editor.button.bg_color = Some([20, 50, 80]);
    let output = frame(&mut app, &ctx, vec![]);
    click(&mut app, &ctx, label_center(&output, "Save button"));
    assert!(app.editor.is_none());
    assert!(app.tabs.is_empty(), "Saving must not execute the command");
    assert_eq!(
        Config::load_existing(&path).unwrap().buttons[1].label,
        "Local output"
    );
    assert_eq!(
        Config::load_existing(&path).unwrap().buttons[1].fg_color,
        Some([250, 230, 210])
    );
    assert_eq!(
        Config::load_existing(&path).unwrap().buttons[1].bg_color,
        Some([20, 50, 80])
    );
    let output = frame(&mut app, &ctx, vec![]);
    assert!(output.shapes.iter().any(|shape| matches!(
        &shape.shape,
        egui::Shape::Rect(rect) if rect.fill == rgb([20, 50, 80])
    )));
    assert!(output.shapes.iter().any(|shape| matches!(
        &shape.shape,
        egui::Shape::Text(text) if text.galley.job.text == "Local output"
            && text.fallback_color == rgb([250, 230, 210])
            && text.galley.job.sections.iter().all(|section|
                section.format.color == Color32::PLACEHOLDER
                    || section.format.color == rgb([250, 230, 210]))
    )));
    click(&mut app, &ctx, label_center(&output, "Local output"));
    assert!(!app.tabs.is_empty());
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while app.running() {
        assert!(std::time::Instant::now() < deadline);
        frame(&mut app, &ctx, vec![]);
        std::thread::sleep(Duration::from_millis(10));
    }
    frame(&mut app, &ctx, vec![]);
    assert!(
        app.active()
            .unwrap()
            .terminal
            .screen
            .text()
            .contains("hello from a button")
    );
}

#[test]
fn material_font_selection_and_validation_keep_the_dialog_usable() {
    let ctx = egui::Context::default();
    let path = std::env::temp_dir().join(format!(
        "rs-button-builder-material-test-{}.yaml",
        std::process::id()
    ));
    let mut app = ButtonBuilder::new(&ctx, Some(path), Config::default());
    frame(&mut app, &ctx, vec![]);
    let mut definitions = ctx.fonts(|fonts| fonts.definitions().clone());
    definitions.families.insert(
        FontFamily::Name("Test font".into()),
        definitions.families[&FontFamily::Monospace].clone(),
    );
    ctx.set_fonts(definitions);
    app.fonts = vec!["Monospace".into(), "Test font".into()];
    app.appearance_open = true;
    frame(&mut app, &ctx, vec![]);
    let output = frame(&mut app, &ctx, vec![]);
    assert!(
        label_center(&output, "Terminal appearance").y + 12.0 < label_center(&output, "Font").y,
        "The dialog title must stay above the font field"
    );
    click(&mut app, &ctx, label_center(&output, "Monospace"));
    let output = frame(&mut app, &ctx, vec![]);
    click(&mut app, &ctx, label_center(&output, "Test font"));
    assert_eq!(app.config.terminal.font, "Test font");
    assert!(app.appearance_open && app.dirty);
    let output = frame(&mut app, &ctx, vec![]);
    click(&mut app, &ctx, label_center(&output, "Save appearance"));
    assert!(!app.dirty);
    assert_eq!(
        Config::load_existing(app.config_path.as_deref().unwrap())
            .unwrap()
            .terminal
            .font,
        "Test font"
    );
    let output = frame(&mut app, &ctx, vec![]);
    click(&mut app, &ctx, label_center(&output, "Close"));
    assert!(!app.appearance_open);

    app.edit(0);
    app.editor.as_mut().unwrap().button.label.clear();
    frame(&mut app, &ctx, vec![]);
    let output = frame(&mut app, &ctx, vec![]);
    assert!(
        label_center(&output, "Edit button 1").y + 12.0 < label_center(&output, "Label").y,
        "The dialog title must stay above the editor fields"
    );
    click(&mut app, &ctx, label_center(&output, "Save button"));
    assert!(app.editor.as_ref().unwrap().error.is_some());
    let output = frame(&mut app, &ctx, vec![]);
    assert!(output.shapes.iter().any(|shape| matches!(
        &shape.shape,
        egui::Shape::Text(text) if text.galley.job.text == "Enter a label for the button."
    )));
    click(&mut app, &ctx, label_center(&output, "Cancel"));
    assert!(app.editor.is_none());
    assert_eq!(app.config.buttons[0].label, "Button 1");

    app.edit(0);
    let mut output = egui::FullOutput::default();
    for _ in 0..3 {
        output = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(850.0, 500.0),
                )),
                ..Default::default()
            },
            |ctx| app.show(ctx),
        );
    }
    let save = label_center(&output, "Save button");
    assert!(
        save.y < 500.0,
        "Save must remain accessible in the minimum window size"
    );
    assert!(output.shapes.iter().any(|shape| shape.clip_rect.contains(save)
        && matches!(&shape.shape, egui::Shape::Text(text) if text.galley.job.text == "Save button")));

    app.editor = None;
    for appearance in [true, false] {
        app.appearance_open = appearance;
        app.config_picker = if appearance {
            None
        } else {
            app.dirty = true;
            Some(ConfigPicker::new(app.config_path.as_deref()))
        };
        for _ in 0..3 {
            output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        Vec2::new(850.0, 500.0),
                    )),
                    ..Default::default()
                },
                |ctx| app.show(ctx),
            );
        }
        let actions: &[&str] = if appearance {
            &["Save appearance", "Reset appearance", "Close"]
        } else {
            &["Load selected config", "Cancel"]
        };
        for &action in actions {
            let position = label_center(&output, action);
            assert!(position.y < 500.0, "{action} must stay inside the window");
            assert!(output.shapes.iter().any(|shape| shape.clip_rect.contains(position)
                && matches!(&shape.shape, egui::Shape::Text(text) if text.galley.job.text == action)),
                "{action} must remain visible without scrolling");
        }
    }
}

#[test]
fn dialog_footers_stay_compact_in_tall_windows() {
    fn filled_rect_at(shape: &egui::Shape, fill: Color32, pos: egui::Pos2) -> Option<egui::Rect> {
        match shape {
            egui::Shape::Rect(rect) if rect.fill == fill && rect.rect.contains(pos) => {
                Some(rect.rect)
            }
            egui::Shape::Vec(shapes) => shapes
                .iter()
                .find_map(|shape| filled_rect_at(shape, fill, pos)),
            _ => None,
        }
    }
    let ctx = egui::Context::default();
    let path = std::env::temp_dir().join("rs-button-builder-footer-preview.yaml");
    let mut app = ButtonBuilder::new(&ctx, Some(path), Config::default());
    let mut time = 0.0;
    for (title, action) in [
        ("Edit button 1", "Save button"),
        ("Terminal appearance", "Save appearance"),
        ("Load buttons config", "Load selected config"),
        ("Save buttons config", "Save config"),
        ("Create a new layout?", "Save and create"),
    ] {
        app.editor = None;
        app.new_layout_requested = title == "Create a new layout?";
        app.save_as = if title == "Save buttons config" {
            let mut picker = ConfigPicker::save(None);
            picker.path = std::env::temp_dir()
                .join(format!(
                    "rs-button-builder-footer-save-{}.yaml",
                    std::process::id()
                ))
                .display()
                .to_string();
            Some(SaveAs {
                picker,
                config: app.config.clone(),
                close_editor: false,
                create_new_layout: false,
            })
        } else {
            None
        };
        app.appearance_open = title == "Terminal appearance";
        app.config_picker = if title == "Load buttons config" {
            Some(ConfigPicker::new(app.config_path.as_deref()))
        } else {
            None
        };
        if title == "Edit button 1" {
            app.edit(0);
        }
        let mut output = egui::FullOutput::default();
        for _ in 0..3 {
            time += 0.25;
            output = ctx.run(
                egui::RawInput {
                    time: Some(time),
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        Vec2::new(1600.0, 1200.0),
                    )),
                    ..Default::default()
                },
                |ctx| app.show(ctx),
            );
        }
        let title_pos = label_center(&output, title);
        let action_pos = label_center(&output, action);
        let dialog = output
            .shapes
            .iter()
            .find_map(|shape| {
                filled_rect_at(
                    &shape.shape,
                    get_global_color("surfaceContainerHigh"),
                    title_pos,
                )
            })
            .unwrap();
        let button = output
            .shapes
            .iter()
            .find_map(|shape| filled_rect_at(&shape.shape, get_global_color("primary"), action_pos))
            .unwrap();
        let bottom_gap = dialog.bottom() - button.bottom();
        assert!(
            bottom_gap <= 26.0,
            "{title} adds {bottom_gap} pixels below its footer in a tall window"
        );
    }
}

#[test]
fn automatic_button_text_contrasts_with_custom_backgrounds() {
    let luminance = |color: Color32| {
        let linear = egui::Rgba::from(color);
        0.2126 * linear.r() + 0.7152 * linear.g() + 0.0722 * linear.b()
    };
    let mut button = ButtonConfig::default();
    for background in [
        [0, 0, 0],
        [255, 255, 255],
        [128, 128, 128],
        [31, 106, 72],
        [35, 79, 128],
        [120, 73, 20],
        [75, 55, 120],
    ] {
        button.bg_color = Some(background);
        let (text, fill) = button_colors(&button);
        let (text, fill) = (luminance(text), luminance(fill));
        let contrast = (text.max(fill) + 0.05) / (text.min(fill) + 0.05);
        assert!(
            contrast >= 4.5,
            "Low contrast on {background:?}: {contrast}"
        );
    }
    button.fg_color = Some([220, 180, 140]);
    assert_eq!(button_colors(&button).0, rgb([220, 180, 140]));
}

#[test]
fn terminal_keys_include_control_codes_and_application_arrows() {
    assert_eq!(key_bytes(egui::Key::C, egui::Modifiers::CTRL, false), [3]);
    assert_eq!(
        key_bytes(egui::Key::ArrowUp, egui::Modifiers::NONE, true),
        b"\x1bOA"
    );
    assert_eq!(
        key_bytes(egui::Key::ArrowUp, egui::Modifiers::NONE, false),
        b"\x1b[A"
    );
    assert_eq!(
        key_bytes(egui::Key::Enter, egui::Modifiers::NONE, false),
        b"\r"
    );
}

#[test]
fn resized_grid_renders_new_rows_and_columns_and_saves_them() {
    let ctx = egui::Context::default();
    let path = std::env::temp_dir().join(format!(
        "rs-button-builder-grid-ui-test-{}.yaml",
        std::process::id()
    ));
    let mut app = ButtonBuilder::new(&ctx, Some(path.clone()), Config::default());
    app.config.resize_grid(2, 4).unwrap();
    app.dirty = true;
    frame(&mut app, &ctx, vec![]);
    let output = frame(&mut app, &ctx, vec![]);
    let positions = empty_button_centers(&output);
    assert_eq!(positions.len(), 8);
    let (fourth, fifth, eighth) = (positions[3], positions[4], positions[7]);
    assert!(fourth.y < fifth.y);
    assert!(fifth.x < eighth.x);
    assert!((fifth.y - eighth.y).abs() < 1.0);
    click_app_action(&mut app, &ctx, &output, "Save layout *");
    let loaded = Config::load_existing(&path).unwrap();
    assert_eq!((loaded.rows, loaded.columns), (2, 4));
    assert_eq!(
        loaded.buttons.len(),
        9,
        "Hidden buttons must survive saving"
    );
}

#[test]
fn footer_save_as_preserves_the_original_and_changes_destination_only_after_saving() {
    let ctx = egui::Context::default();
    let directory = std::env::temp_dir().join(format!(
        "rs-button-builder-footer-save-as-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&directory).unwrap();
    let original = directory.join("original.yaml");
    let copy = directory.join("copy.yaml");
    let config = Config::default();
    config.save(&original).unwrap();
    let mut app = ButtonBuilder::new(&ctx, Some(original.clone()), config)
        .with_settings(SettingsStore::open(directory.join("settings.yaml")));
    app.config.display_name = "Changed layout".into();
    app.dirty = true;
    frame(&mut app, &ctx, vec![]);
    let output = frame(&mut app, &ctx, vec![]);
    click_app_action(&mut app, &ctx, &output, "Save layout as…");
    assert!(app.save_as.is_some());
    let output = frame(&mut app, &ctx, vec![]);
    click(&mut app, &ctx, label_center(&output, "Cancel"));
    assert!(app.save_as.is_none() && app.dirty);
    assert_eq!(app.config_path.as_ref(), Some(&original));
    assert_eq!(
        Config::load_existing(&original).unwrap().display_name(),
        DEFAULT_LAYOUT_NAME
    );

    let output = frame(&mut app, &ctx, vec![]);
    click_app_action(&mut app, &ctx, &output, "Save layout as…");
    app.save_as.as_mut().unwrap().picker.path = copy.display().to_string();
    let output = frame(&mut app, &ctx, vec![]);
    click(&mut app, &ctx, label_center(&output, "Save config"));
    assert!(app.save_as.is_none() && !app.dirty);
    assert_eq!(app.config_path.as_ref(), Some(&copy));
    assert_eq!(
        Config::load_existing(&copy).unwrap().display_name(),
        "Changed layout"
    );
    assert_eq!(
        Config::load_existing(&original).unwrap().display_name(),
        DEFAULT_LAYOUT_NAME
    );
}

#[test]
fn footer_app_menu_clears_the_layout_and_preserves_running_terminals() {
    let ctx = egui::Context::default();
    let path = std::env::temp_dir().join(format!(
        "rs-button-builder-new-running-{}.yaml",
        std::process::id()
    ));
    let mut config = Config {
        display_name: "Original layout".into(),
        ..Default::default()
    };
    config.resize_grid(2, 4).unwrap();
    config.buttons[0] = interactive_button("Alpha");
    config.buttons[1] = interactive_button("Beta");
    config.terminal.font_size = 18.0;
    config.terminal.bg_color = Some([10, 20, 30]);
    config.save(&path).unwrap();
    let mut app = ButtonBuilder::new(&ctx, Some(path.clone()), config);
    app.launch(0);
    app.launch(1);
    wait_for_tabs(&mut app, &ctx, |app| {
        app.tabs
            .iter()
            .all(|tab| tab.terminal.screen.text().contains("-ready"))
    });
    for size in [Vec2::new(1200.0, 760.0), Vec2::new(850.0, 500.0)] {
        frame_with(&mut app, &ctx, size, egui::Modifiers::NONE, vec![]);
        let output = frame_with(&mut app, &ctx, size, egui::Modifiers::NONE, vec![]);
        let trigger = label_center(&output, "Button Builder");
        assert!(trigger.y > size.y - 40.0);
        let terminal_sizes: Vec<_> = app
            .tabs
            .iter()
            .map(|tab| (tab.terminal.screen.cols, tab.terminal.screen.rows))
            .collect();
        click(&mut app, &ctx, trigger);
        let output = frame_with(&mut app, &ctx, size, egui::Modifiers::NONE, vec![]);
        let labels = [
            "New layout",
            "Load config…",
            "Save layout",
            "Save layout as…",
            "Exit",
        ];
        let positions: Vec<_> = labels
            .iter()
            .map(|label| label_center(&output, label))
            .collect();
        assert_eq!(
            terminal_sizes,
            app.tabs
                .iter()
                .map(|tab| (tab.terminal.screen.cols, tab.terminal.screen.rows))
                .collect::<Vec<_>>()
        );
        assert!(positions.windows(2).all(|pair| pair[0].y < pair[1].y));
        for (label, pos) in labels.iter().zip(positions) {
            assert!(pos.x < size.x && pos.y < trigger.y);
            assert!(output.shapes.iter().any(|shape| shape.clip_rect.contains(pos)
                && matches!(&shape.shape, egui::Shape::Text(text) if text.galley.job.text == *label)));
        }
        frame_with(
            &mut app,
            &ctx,
            size,
            egui::Modifiers::NONE,
            vec![egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        assert!(!egui::Popup::is_any_open(&ctx));
        let output = frame_with(&mut app, &ctx, size, egui::Modifiers::NONE, vec![]);
        click(&mut app, &ctx, label_center(&output, "Button Builder"));
        assert!(egui::Popup::is_any_open(&ctx));
        click(&mut app, &ctx, egui::pos2(size.x - 40.0, 200.0));
        assert!(!egui::Popup::is_any_open(&ctx));
    }
    frame(&mut app, &ctx, vec![]);
    let output = frame(&mut app, &ctx, vec![]);
    let active_id = app.active().unwrap().id;
    let text = app.tabs[0].terminal.screen.text();
    click_app_action(&mut app, &ctx, &output, "New layout");
    assert!(app.config_path.is_none() && !app.dirty && !app.new_layout_requested);
    assert_eq!(
        (
            app.config.rows,
            app.config.columns,
            app.config.buttons.len()
        ),
        (3, 3, 9)
    );
    assert_eq!(app.config.display_name(), DEFAULT_LAYOUT_NAME);
    assert!(
        app.config
            .buttons
            .iter()
            .all(|button| button.command.is_empty())
    );
    assert_eq!(app.config.terminal.font_size, 14.0);
    assert_eq!(app.tabs.len(), 2);
    assert_eq!(app.active().unwrap().id, active_id);
    assert_eq!(app.tabs[0].terminal.screen.text(), text);
    assert!(
        app.tabs
            .iter()
            .all(|tab| tab.terminal.is_running() && tab.button_index.is_none())
    );
    assert!(
        app.tabs
            .iter()
            .all(|tab| tab.style.font_size == 18.0 && tab.style.bg_color == Some([10, 20, 30]))
    );
    assert_eq!(
        Config::load_existing(&path).unwrap().display_name,
        "Original layout"
    );
    app.save();
    assert!(
        app.save_as.as_ref().unwrap().picker.path.is_empty(),
        "New layouts need their own save destination"
    );
}

#[test]
fn new_layout_confirmation_cancels_saves_or_discards_changes() {
    let ctx = egui::Context::default();
    let path = std::env::temp_dir().join(format!(
        "rs-button-builder-new-confirmation-{}.yaml",
        std::process::id()
    ));
    let config = Config {
        display_name: "Original".into(),
        ..Default::default()
    };
    config.save(&path).unwrap();
    let mut app = ButtonBuilder::new(&ctx, Some(path.clone()), config);
    app.config.display_name = "Changed layout".into();
    app.config.resize_grid(1, 2).unwrap();
    app.dirty = true;
    frame(&mut app, &ctx, vec![]);
    let output = frame(&mut app, &ctx, vec![]);
    click_app_action(&mut app, &ctx, &output, "New layout");
    assert!(app.new_layout_requested && app.dirty);
    let output = frame(&mut app, &ctx, vec![]);
    click(&mut app, &ctx, label_center(&output, "Cancel"));
    assert!(!app.new_layout_requested && app.dirty);
    assert_eq!(app.config.display_name, "Changed layout");
    assert_eq!(app.config_path.as_ref(), Some(&path));
    assert_eq!(
        Config::load_existing(&path).unwrap().display_name,
        "Original"
    );
    app.layout_name_edit = Some("Save this title".into());
    let output = frame(&mut app, &ctx, vec![]);
    click_app_action(&mut app, &ctx, &output, "New layout");
    let output = frame(&mut app, &ctx, vec![]);
    click(&mut app, &ctx, label_center(&output, "Save and create"));
    assert!(app.config_path.is_none() && !app.dirty && !app.new_layout_requested);
    let saved = Config::load_existing(&path).unwrap();
    assert_eq!(saved.display_name, "Save this title");
    assert_eq!((saved.rows, saved.columns), (1, 2));
    assert_eq!((app.config.rows, app.config.columns), (3, 3));

    app.load_config(path.clone()).unwrap();
    app.config.display_name = "Discard this title".into();
    app.dirty = true;
    let output = frame(&mut app, &ctx, vec![]);
    click_app_action(&mut app, &ctx, &output, "New layout");
    let output = frame(&mut app, &ctx, vec![]);
    click(
        &mut app,
        &ctx,
        label_center(&output, "Create without saving"),
    );
    assert!(app.config_path.is_none() && !app.dirty && app.save_as.is_none());
    assert_eq!(app.config.display_name(), DEFAULT_LAYOUT_NAME);
    assert_eq!(
        Config::load_existing(&path).unwrap().display_name,
        "Save this title"
    );
}

#[test]
fn new_layout_waits_for_a_successful_first_save_and_preserves_button_edits() {
    let ctx = egui::Context::default();
    let directory = std::env::temp_dir().join(format!(
        "rs-button-builder-new-save-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&directory).unwrap();
    let saved_path = directory.join("previous-layout.yaml");
    let mut app = ButtonBuilder::new(&ctx, None, Config::default());
    app.config.display_name = "Save this layout".into();
    app.dirty = true;
    app.edit(0);
    app.editor.as_mut().unwrap().button.label.clear();
    app.request_new_layout();
    frame(&mut app, &ctx, vec![]);
    let output = frame(&mut app, &ctx, vec![]);
    click(&mut app, &ctx, label_center(&output, "Save and create"));
    assert_eq!(
        app.new_layout_error.as_deref(),
        Some("Enter a label for the button.")
    );
    assert!(app.new_layout_requested && app.editor.is_some() && app.save_as.is_none());
    let output = frame(&mut app, &ctx, vec![]);
    click(&mut app, &ctx, label_center(&output, "Cancel"));
    let editor = app.editor.as_mut().unwrap();
    editor.button.label = "Draft button".into();
    editor.button.command = "/usr/bin/true".into();
    app.request_new_layout();
    let output = frame(&mut app, &ctx, vec![]);
    click(&mut app, &ctx, label_center(&output, "Save and create"));
    assert!(app.save_as.as_ref().unwrap().create_new_layout);
    assert!(app.save_as.as_ref().unwrap().picker.path.is_empty());
    let output = frame(&mut app, &ctx, vec![]);
    click(&mut app, &ctx, label_center(&output, "Cancel"));
    assert!(app.new_layout_requested && app.config_path.is_none() && app.dirty);
    assert_eq!(app.editor.as_ref().unwrap().button.label, "Draft button");
    let output = frame(&mut app, &ctx, vec![]);
    click(&mut app, &ctx, label_center(&output, "Save and create"));
    app.save_as.as_mut().unwrap().picker.path =
        directory.join("missing/layout.yaml").display().to_string();
    let output = frame(&mut app, &ctx, vec![]);
    click(&mut app, &ctx, label_center(&output, "Save config"));
    assert!(app.save_as.as_ref().unwrap().picker.error.is_some());
    assert_eq!(app.config.display_name, "Save this layout");
    assert!(app.config_path.is_none() && app.editor.is_some() && app.dirty);
    app.save_as.as_mut().unwrap().picker.path = saved_path.display().to_string();
    let output = frame(&mut app, &ctx, vec![]);
    click(&mut app, &ctx, label_center(&output, "Save config"));
    assert!(app.save_as.is_none() && app.editor.is_none() && !app.new_layout_requested);
    assert!(app.config_path.is_none() && !app.dirty);
    assert!(
        app.config
            .buttons
            .iter()
            .all(|button| button.command.is_empty())
    );
    let saved = Config::load_existing(&saved_path).unwrap();
    assert_eq!(saved.display_name, "Save this layout");
    assert_eq!(saved.buttons[0].label, "Draft button");
    assert_eq!(saved.buttons[0].command, "/usr/bin/true");
}

#[test]
fn unsaved_layout_requires_a_filename_and_then_reuses_the_selected_file() {
    let ctx = egui::Context::default();
    let path = std::env::temp_dir().join(format!(
        "rs-button-builder-first-save-{}-{}.yaml",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let mut app = ButtonBuilder::new(&ctx, None, Config::default());
    assert!(app.config_path.is_none());
    assert!(
        app.config
            .buttons
            .iter()
            .all(|button| button.command.is_empty())
    );
    assert!(app.tabs.is_empty());
    app.config.resize_grid(2, 2).unwrap();
    app.dirty = true;
    frame(&mut app, &ctx, vec![]);
    let output = frame(&mut app, &ctx, vec![]);
    click_app_action(&mut app, &ctx, &output, "Save layout…");
    assert!(app.save_as.as_ref().unwrap().picker.path.is_empty());
    let output = frame(&mut app, &ctx, vec![]);
    click(&mut app, &ctx, label_center(&output, "Save config"));
    assert!(app.save_as.is_some() && app.config_path.is_none());
    for _ in 0..3 {
        let small = frame_with(
            &mut app,
            &ctx,
            Vec2::new(850.0, 500.0),
            egui::Modifiers::NONE,
            vec![],
        );
        for label in ["Save config", "Cancel"] {
            let pos = label_center(&small, label);
            assert!(pos.y < 500.0);
            assert!(small.shapes.iter().any(|shape| shape.clip_rect.contains(pos)
                && matches!(&shape.shape, egui::Shape::Text(text) if text.galley.job.text == label)));
        }
    }
    let output = frame(&mut app, &ctx, vec![]);
    click(&mut app, &ctx, label_center(&output, "Cancel"));
    assert!(app.save_as.is_none() && app.config_path.is_none() && app.dirty);
    assert!(!path.exists());

    let output = frame(&mut app, &ctx, vec![]);
    click_app_action(&mut app, &ctx, &output, "Save layout…");
    let output = frame(&mut app, &ctx, vec![]);
    click(
        &mut app,
        &ctx,
        label_center(&output, "Enter a filename or path"),
    );
    frame(
        &mut app,
        &ctx,
        vec![egui::Event::Text(
            path.with_extension("").display().to_string(),
        )],
    );
    let output = frame(&mut app, &ctx, vec![]);
    click(&mut app, &ctx, label_center(&output, "Save config"));
    assert!(app.save_as.is_none() && !app.dirty);
    assert_eq!(app.config_path.as_ref(), Some(&path));
    assert!(!path.with_extension("").exists());
    let saved = Config::load_existing(&path).unwrap();
    assert_eq!((saved.rows, saved.columns), (2, 2));
    assert!(saved.buttons.iter().all(|button| button.command.is_empty()));
    app.config.terminal.font_size = 20.0;
    app.dirty = true;
    let output = frame(&mut app, &ctx, vec![]);
    click_app_action(&mut app, &ctx, &output, "Save layout *");
    assert!(app.save_as.is_none() && !app.dirty);
    assert_eq!(
        Config::load_existing(&path).unwrap().terminal.font_size,
        20.0
    );
}

#[test]
fn first_button_save_preserves_its_draft_on_cancel_and_error_and_confirms_overwrite() {
    let ctx = egui::Context::default();
    let directory = std::env::temp_dir().join(format!(
        "rs-button-builder-first-button-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&directory).unwrap();
    let existing = directory.join("existing.yaml");
    Config {
        display_name: "Original".into(),
        ..Default::default()
    }
    .save(&existing)
    .unwrap();
    let mut app = ButtonBuilder::new(&ctx, None, Config::default());
    frame(&mut app, &ctx, vec![]);
    let output = frame(&mut app, &ctx, vec![]);
    click(&mut app, &ctx, empty_button_centers(&output)[0]);
    let editor = app.editor.as_mut().unwrap();
    editor.button.label = "Local output".into();
    editor.button.command = "/usr/bin/printf".into();
    editor.button.arguments = "'first button works\\n'".into();
    let output = frame(&mut app, &ctx, vec![]);
    click(&mut app, &ctx, label_center(&output, "Save button"));
    assert_eq!(
        app.save_as.as_ref().unwrap().config.buttons[0].label,
        "Local output"
    );
    let output = frame(&mut app, &ctx, vec![]);
    click(&mut app, &ctx, label_center(&output, "Cancel"));
    assert_eq!(app.editor.as_ref().unwrap().button.label, "Local output");
    assert!(app.config.buttons[0].command.is_empty() && app.config_path.is_none());
    let output = frame(&mut app, &ctx, vec![]);
    click(&mut app, &ctx, label_center(&output, "Save button"));
    app.save_as.as_mut().unwrap().picker.path =
        directory.join("missing/layout.yaml").display().to_string();
    let output = frame(&mut app, &ctx, vec![]);
    click(&mut app, &ctx, label_center(&output, "Save config"));
    assert!(
        app.save_as
            .as_ref()
            .unwrap()
            .picker
            .error
            .as_ref()
            .unwrap()
            .contains("Could not save")
    );
    assert!(app.config_path.is_none() && app.editor.is_some());
    assert!(app.config.buttons[0].command.is_empty());
    app.save_as.as_mut().unwrap().picker.path = existing.with_extension("").display().to_string();
    let output = frame(&mut app, &ctx, vec![]);
    click(&mut app, &ctx, label_center(&output, "Save config"));
    assert!(app.save_as.is_some());
    assert_eq!(
        Config::load_existing(&existing).unwrap().display_name,
        "Original"
    );
    let output = frame(&mut app, &ctx, vec![]);
    click(
        &mut app,
        &ctx,
        label_center(&output, "Overwrite existing file"),
    );
    let output = frame(&mut app, &ctx, vec![]);
    click(&mut app, &ctx, label_center(&output, "Save config"));
    assert!(app.save_as.is_none() && app.editor.is_none() && !app.dirty);
    assert_eq!(app.config_path.as_ref(), Some(&existing));
    assert_eq!(
        Config::load_existing(&existing).unwrap().buttons[0].label,
        "Local output"
    );
    assert!(app.tabs.is_empty(), "Saving a config must not run commands");
    app.launch(0);
    wait_for_tabs(&mut app, &ctx, |app| {
        app.tabs[0]
            .terminal
            .screen
            .text()
            .contains("first button works")
    });
}

#[test]
fn loading_without_a_config_can_save_the_unsaved_layout_before_replacing_it() {
    let ctx = egui::Context::default();
    let directory = std::env::temp_dir().join(format!(
        "rs-button-builder-save-before-load-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&directory).unwrap();
    let saved = directory.join("created.yaml");
    let loaded = directory.join("selected.yaml");
    Config {
        display_name: "Selected layout".into(),
        ..Default::default()
    }
    .save(&loaded)
    .unwrap();
    let mut app = ButtonBuilder::new(&ctx, None, Config::default());
    frame(&mut app, &ctx, vec![]);
    let output = frame(&mut app, &ctx, vec![]);
    click_app_action(&mut app, &ctx, &output, "Load config…");
    assert!(app.config_picker.as_ref().unwrap().path.is_empty());
    let output = frame(&mut app, &ctx, vec![]);
    click(&mut app, &ctx, label_center(&output, "Cancel"));
    assert!(app.config_picker.is_none() && app.config_path.is_none());
    app.config.resize_grid(1, 2).unwrap();
    app.dirty = true;
    let output = frame(&mut app, &ctx, vec![]);
    click_app_action(&mut app, &ctx, &output, "Load config…");
    app.config_picker.as_mut().unwrap().path = loaded.display().to_string();
    let output = frame(&mut app, &ctx, vec![]);
    click(
        &mut app,
        &ctx,
        label_center(&output, "Save current layout first"),
    );
    assert!(app.save_as.is_some() && app.config_picker.is_some());
    let output = frame(&mut app, &ctx, vec![]);
    click(&mut app, &ctx, label_center(&output, "Cancel"));
    assert!(app.save_as.is_none() && app.dirty && app.config_path.is_none());
    assert_eq!(
        app.config_picker.as_ref().unwrap().path,
        loaded.display().to_string()
    );
    let output = frame(&mut app, &ctx, vec![]);
    click(
        &mut app,
        &ctx,
        label_center(&output, "Save current layout first"),
    );
    app.save_as.as_mut().unwrap().picker.path = saved.display().to_string();
    let output = frame(&mut app, &ctx, vec![]);
    click(&mut app, &ctx, label_center(&output, "Save config"));
    assert_eq!(app.config_path.as_ref(), Some(&saved));
    assert!(!app.dirty && app.config_picker.is_some());
    let output = frame(&mut app, &ctx, vec![]);
    click(
        &mut app,
        &ctx,
        label_center(&output, "Load selected config"),
    );
    assert!(app.config_picker.is_none() && !app.dirty);
    assert_eq!(app.config_path.as_ref(), Some(&loaded));
    assert_eq!(app.config.display_name, "Selected layout");
    assert_eq!(
        (
            Config::load_existing(&saved).unwrap().rows,
            Config::load_existing(&saved).unwrap().columns
        ),
        (1, 2)
    );
}

#[test]
fn chooser_loads_an_example_only_after_unsaved_changes_are_replaced() {
    let ctx = egui::Context::default();
    let current = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("buttons.yaml");
    let mut app = ButtonBuilder::new(&ctx, Some(current.clone()), Config::default());
    app.dirty = true;
    frame(&mut app, &ctx, vec![]);
    let output = frame(&mut app, &ctx, vec![]);
    click_app_action(&mut app, &ctx, &output, "Load config…");
    assert!(app.config_picker.is_some());
    frame(&mut app, &ctx, vec![]);
    let output = frame(&mut app, &ctx, vec![]);
    click(&mut app, &ctx, label_center(&output, "Examples"));
    assert!(app.config_picker.as_ref().unwrap().path.is_empty());
    frame(&mut app, &ctx, vec![]);
    let output = frame(&mut app, &ctx, vec![]);
    click(&mut app, &ctx, label_center(&output, "json-streams.yaml"));
    let output = frame(&mut app, &ctx, vec![]);
    click(
        &mut app,
        &ctx,
        label_center(&output, "Load selected config"),
    );
    assert_eq!(app.config_path.as_ref(), Some(&current));
    assert!(app.dirty);
    let output = frame(&mut app, &ctx, vec![]);
    click(
        &mut app,
        &ctx,
        label_center(&output, "Replace unsaved changes"),
    );
    let output = frame(&mut app, &ctx, vec![]);
    click(
        &mut app,
        &ctx,
        label_center(&output, "Load selected config"),
    );
    assert!(app.config_picker.is_none());
    assert!(!app.dirty);
    assert_eq!((app.config.rows, app.config.columns), (2, 3));
    assert_eq!(app.config.buttons[0].label, "Pretty JSON");
    assert_eq!(app.config.buttons[0].bg_color, Some([75, 55, 120]));
    assert_eq!(app.config.terminal.font_size, 15.0);
    assert_eq!(
        app.config_path.as_ref().unwrap().file_name().unwrap(),
        "json-streams.yaml"
    );
    assert!(app.tabs.is_empty(), "Loading must not execute a command");
}

#[test]
fn failed_load_keeps_layout_and_successful_load_keeps_running_process() {
    let ctx = egui::Context::default();
    let directory = std::env::temp_dir().join(format!(
        "rs-button-builder-load-test-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&directory).unwrap();
    let current = directory.join("current.yaml");
    let malformed = directory.join("invalid.yaml");
    std::fs::write(&malformed, "rows: [invalid\n").unwrap();
    let mut app = ButtonBuilder::new(&ctx, Some(current.clone()), Config::default());
    app.config.buttons[0].label = "Keep this label".into();
    app.dirty = true;
    app.edit(0);
    for path in [directory.join("missing.yaml"), malformed] {
        assert!(app.load_config(path).is_err());
        assert_eq!(app.config_path.as_ref(), Some(&current));
        assert_eq!(app.config.buttons[0].label, "Keep this label");
        assert!(app.dirty && app.editor.is_some());
    }
    let button = ButtonConfig {
        command: "/bin/sleep".into(),
        arguments: "60".into(),
        ..Default::default()
    };
    let mut terminal = Terminal::start(&button, 80, 24).unwrap();
    terminal.screen.feed(b"Retained output");
    let status = terminal.status.clone();
    app.config.buttons[0] = button;
    app.launch(0);
    app.tabs[0].terminal = terminal;
    app.load_config(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/terminal-demo.yaml"))
        .unwrap();
    assert!(app.running());
    assert_eq!(app.active().unwrap().terminal.status, status);
    assert!(
        app.active()
            .unwrap()
            .terminal
            .screen
            .text()
            .contains("Retained output")
    );
    assert!(app.active().unwrap().button_index.is_none() && app.editor.is_none());
    assert_eq!((app.config.rows, app.config.columns), (2, 2));
}
