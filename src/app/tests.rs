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
            if track.contains_rect(rect.rect)
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
fn ctrl_click_tabs_route_input_and_toolbar_actions_to_the_selected_process() {
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
        1,
        "A normal click must not replace a running process"
    );
    let output = frame(&mut app, &ctx, vec![]);
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
    assert_eq!(app.tabs.len(), 2, "Normal clicks reuse a finished tab");
    assert_eq!(app.tabs[0].id, 1);
    assert!(app.tabs[0].terminal.is_running());
    let output = frame(&mut app, &ctx, vec![]);
    click(&mut app, &ctx, label_center(&output, "Stop"));
    assert!(!app.tabs[0].terminal.is_running());
    assert!(app.tabs[1].terminal.is_running());
    assert_eq!(app.tabs[0].terminal.status, "Stopped");

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
    assert_eq!(app.tabs.len(), 1);
    assert_eq!(app.active_tab, Some(0));
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
    assert!(app.tabs.is_empty() && app.active_tab.is_none());
    let output = frame(&mut app, &ctx, vec![]);
    click(&mut app, &ctx, label_center(&output, "Alpha"));
    assert_eq!(app.tabs[0].id, 3, "New tabs must not reuse old UI IDs");
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
    click_with_modifiers(
        &mut app,
        &ctx,
        label_center(&output, "Quiet"),
        egui::Modifiers::CTRL,
    );
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
    assert!(app.tabs[0].terminal.is_running());
    let pid: i32 = app.tabs[0]
        .terminal
        .status
        .split_whitespace()
        .last()
        .unwrap()
        .parse()
        .unwrap();
    let output = frame(&mut app, &ctx, vec![]);
    click(&mut app, &ctx, label_center(&output, "Close tab"));
    assert_eq!(app.tabs.len(), 1);
    assert_eq!(app.tabs[0].label, "Quiet");
    wait_for_tabs(&mut app, &ctx, |_| {
        // SAFETY: signal zero only observes this test's child.
        (unsafe { libc::kill(pid, 0) }) == -1
    });
    assert!(app.tabs[0].terminal.is_running());
}

#[test]
fn terminal_scrollbar_drags_history_and_preserves_tab_position_and_input_focus() {
    let ctx = egui::Context::default();
    let mut config = Config::default();
    config.buttons[0] = interactive_button("Alpha");
    config.buttons[1] = interactive_button("Beta");
    let mut app = ButtonBuilder::new(&ctx, Some("/tmp/scrollbar-tabs.yaml".into()), config);
    app.launch(0, false);
    app.launch(1, true);
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
        command: "/usr/bin/true".into(),
        ..Default::default()
    };
    let mut app = ButtonBuilder::new(&ctx, Some("/tmp/scrollbar-small.yaml".into()), config);
    app.launch(0, false);
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
    app.launch(0, false);
    app.launch(1, true);
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
    app.launch(1, true);
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
    let mut app = ButtonBuilder::new(
        &ctx,
        Some("/tmp/rs-button-builder-tab-appearance-test.yaml".into()),
        config,
    );
    app.launch(0, false);
    app.launch(0, true);
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
    app.tabs[0].style.bg_color = [10, 30, 40];
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
    app.launch(0, true);
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
    config.buttons[1].command = "/this/local/program/does/not/exist".into();
    let mut app = ButtonBuilder::new(
        &ctx,
        Some("/tmp/rs-button-builder-tab-load-test.yaml".into()),
        config,
    );
    app.launch(0, false);
    app.launch(0, true);
    app.tabs[0].style.bg_color = [10, 30, 40];
    app.tabs[1].terminal.screen.feed(b"Retain me");
    let next_id = app.next_tab_id;
    app.launch(1, true);
    assert!(app.error.is_some());
    assert_eq!(app.tabs.len(), 2);
    assert_eq!(app.active_tab, Some(1));
    assert_eq!(app.next_tab_id, next_id);
    app.tabs[1].terminal.stop();
    app.launch(1, false);
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
    assert_eq!(app.tabs[0].style.bg_color, [10, 30, 40]);
    assert!(app.tabs.iter().all(|tab| tab.button_index.is_none()));
    app.launch(0, true);
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
    config.buttons[0] = interactive_button("A long running command label");
    let mut app = ButtonBuilder::new(
        &ctx,
        Some("/tmp/rs-button-builder-many-tabs-test.yaml".into()),
        config,
    );
    for _ in 0..8 {
        app.launch(0, true);
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
    // Ctrl-click keeps configuring empty buttons and editing in edit mode.
    app.launch(1, true);
    assert_eq!(app.tabs.len(), 8);
    assert_eq!(app.editor.as_ref().unwrap().index, 1);
    app.editor = None;
    app.edit_mode = true;
    let output = frame(&mut app, &ctx, vec![]);
    click_with_modifiers(
        &mut app,
        &ctx,
        label_center(&output, "A long running command label"),
        egui::Modifiers::CTRL,
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
    click(&mut app, &ctx, label_center(&output, "Save layout *"));
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
    click(&mut app, &ctx, label_center(&output, "Save layout"));
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
fn button_logging_checkbox_cancels_and_saves_per_button() {
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
    click(&mut app, &ctx, label_center(&output, "Log output"));
    assert!(app.editor.as_ref().unwrap().button.log_output);
    let output = frame(&mut app, &ctx, vec![]);
    click(&mut app, &ctx, label_center(&output, "Cancel"));
    assert!(!app.config.buttons[0].log_output);

    app.edit(0);
    let output = frame(&mut app, &ctx, vec![]);
    click(&mut app, &ctx, label_center(&output, "Log output"));
    let output = frame(&mut app, &ctx, vec![]);
    click(&mut app, &ctx, label_center(&output, "Save button"));
    let saved = Config::load_existing(&path).unwrap();
    assert!(saved.buttons[0].log_output);
    assert!(!saved.buttons[1].log_output);

    app.edit(0);
    let output = frame(&mut app, &ctx, vec![]);
    assert!(app.editor.as_ref().unwrap().button.log_output);
    click(&mut app, &ctx, label_center(&output, "Log output"));
    let output = frame(&mut app, &ctx, vec![]);
    click(&mut app, &ctx, label_center(&output, "Save button"));
    assert!(!Config::load_existing(&path).unwrap().buttons[0].log_output);
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
    let positions: Vec<_> = (0..9)
        .map(|i| {
            label_center(
                &output,
                &if i == 0 {
                    "Button 1\n+ Configure".into()
                } else {
                    format!("Button {}\n+ Configure", i + 1)
                },
            )
        })
        .collect();
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
    let fourth = label_center(&output, "Button 4\n+ Configure");
    let fifth = label_center(&output, "Button 5\n+ Configure");
    let eighth = label_center(&output, "Button 8\n+ Configure");
    assert!(fourth.y < fifth.y);
    assert!(fifth.x < eighth.x);
    assert!((fifth.y - eighth.y).abs() < 1.0);
    assert!(!output.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Text(text) if text.galley.job.text == "Button 9\n+ Configure")));
    click(&mut app, &ctx, label_center(&output, "Save layout *"));
    let loaded = Config::load_existing(&path).unwrap();
    assert_eq!((loaded.rows, loaded.columns), (2, 4));
    assert_eq!(
        loaded.buttons.len(),
        9,
        "Hidden buttons must survive saving"
    );
}

#[test]
fn new_layout_toolbar_clears_the_layout_and_preserves_running_terminals() {
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
    config.terminal.bg_color = [10, 20, 30];
    config.save(&path).unwrap();
    let mut app = ButtonBuilder::new(&ctx, Some(path.clone()), config);
    app.launch(0, false);
    app.launch(1, true);
    wait_for_tabs(&mut app, &ctx, |app| {
        app.tabs
            .iter()
            .all(|tab| tab.terminal.screen.text().contains("-ready"))
    });
    for size in [Vec2::new(1200.0, 760.0), Vec2::new(850.0, 500.0)] {
        frame_with(&mut app, &ctx, size, egui::Modifiers::NONE, vec![]);
        let output = frame_with(&mut app, &ctx, size, egui::Modifiers::NONE, vec![]);
        let positions: Vec<_> = ["New layout", "Load config…", "Save layout", "Exit"]
            .iter()
            .map(|label| label_center(&output, label))
            .collect();
        assert!(positions.windows(2).all(|pair| pair[0].x < pair[1].x));
        for (label, pos) in ["New layout", "Load config…", "Save layout", "Exit"]
            .iter()
            .zip(positions)
        {
            assert!(pos.x < size.x && pos.y < 64.0);
            assert!(output.shapes.iter().any(|shape| shape.clip_rect.contains(pos)
                && matches!(&shape.shape, egui::Shape::Text(text) if text.galley.job.text == *label)));
        }
    }
    frame(&mut app, &ctx, vec![]);
    let output = frame(&mut app, &ctx, vec![]);
    let active_id = app.active().unwrap().id;
    let text = app.tabs[0].terminal.screen.text();
    click(&mut app, &ctx, label_center(&output, "New layout"));
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
            .all(|tab| tab.style.font_size == 18.0 && tab.style.bg_color == [10, 20, 30])
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
    click(&mut app, &ctx, label_center(&output, "New layout"));
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
    click(&mut app, &ctx, label_center(&output, "New layout"));
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
    click(&mut app, &ctx, label_center(&output, "New layout"));
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
    click(&mut app, &ctx, label_center(&output, "Save layout…"));
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
    click(&mut app, &ctx, label_center(&output, "Save layout…"));
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
    click(&mut app, &ctx, label_center(&output, "Save layout *"));
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
    click(
        &mut app,
        &ctx,
        label_center(&output, "Button 1\n+ Configure"),
    );
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
    app.launch(0, false);
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
    click(&mut app, &ctx, label_center(&output, "Load config…"));
    assert!(app.config_picker.as_ref().unwrap().path.is_empty());
    let output = frame(&mut app, &ctx, vec![]);
    click(&mut app, &ctx, label_center(&output, "Cancel"));
    assert!(app.config_picker.is_none() && app.config_path.is_none());
    app.config.resize_grid(1, 2).unwrap();
    app.dirty = true;
    let output = frame(&mut app, &ctx, vec![]);
    click(&mut app, &ctx, label_center(&output, "Load config…"));
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
    click(&mut app, &ctx, label_center(&output, "Load config…"));
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
    app.launch(0, false);
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
