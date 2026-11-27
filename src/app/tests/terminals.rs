//! Application terminals tests.

use super::*;

#[test]
fn single_terminal_content_has_right_edge_clearance() {
    let ctx = egui::Context::default();
    let mut config = Config::default();
    config.buttons[0] = interactive_button("Terminal");
    let mut app = ButtonBuilder::new(&ctx, None, config);
    app.launch(0);
    let output = frame(&mut app, &ctx, vec![]);
    let (border, clip) = output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Rect(rect)
                if rect.stroke.color == get_global_color("primary")
                    && rect.stroke.width == 1.0
                    && rect.rect.width() > 500.0 =>
            {
                Some((rect.rect, shape.clip_rect))
            }
            _ => None,
        })
        .expect("The focused terminal should paint its border");
    assert!(
        clip.right() - border.right() >= 2.0,
        "The terminal focus border needs clearance from the right clip edge"
    );
    let (toolbar, clip) = output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Rect(rect)
                if rect.fill == get_global_color("surfaceContainerLow")
                    && rect.rect.width() > 500.0
                    && rect.rect.height() < 50.0 =>
            {
                Some((rect.rect, shape.clip_rect))
            }
            _ => None,
        })
        .expect("The terminal toolbar should paint its background");
    assert!(
        clip.right() - toolbar.right() >= 2.0,
        "The terminal toolbar needs clearance from the right clip edge"
    );
}

#[test]
fn control_f_finds_terminal_text_and_restores_live_updates_when_closed() {
    let ctx = egui::Context::default();
    let mut config = Config::default();
    config.buttons[0] = interactive_button("Search");
    let mut app = ButtonBuilder::new(&ctx, None, config);
    app.launch(0);
    app.workspace.tabs_mut()[0]
        .terminal
        .screen
        .feed(b"\x1b[2J\x1b[Hfirst needle\r\nsecond needle");

    let output = hotkey(&mut app, &ctx, egui::Key::F, false, false);
    assert!(app.workspace.tabs()[0].search.open);
    assert!(app.workspace.tabs()[0].display_paused());
    label_center(&output, "Find");

    frame(&mut app, &ctx, vec![]);
    let output = frame(&mut app, &ctx, vec![egui::Event::Text("needle".into())]);
    label_center(&output, "1 of 2");
    assert!(output.shapes.iter().any(|shape| matches!(
        &shape.shape,
        egui::Shape::Rect(rect) if rect.fill == get_global_color("primary")
    )));

    let output = hotkey(&mut app, &ctx, egui::Key::G, false, false);
    label_center(&output, "2 of 2");
    let output = hotkey(&mut app, &ctx, egui::Key::G, true, false);
    label_center(&output, "1 of 2");

    let output = frame(
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
    assert!(!app.workspace.tabs()[0].search.open);
    assert!(!app.workspace.tabs()[0].display_paused());
    assert!(output.shapes.iter().all(
        |shape| !matches!(&shape.shape, egui::Shape::Text(text) if text.galley.job.text == "Find")
    ));

    app.workspace.tabs_mut()[0].set_display_paused(true);
    app.workspace.tabs_mut()[0].open_search();
    app.workspace.tabs_mut()[0].close_search();
    assert!(
        app.workspace.tabs()[0].display_paused(),
        "Closing find must preserve a pre-existing display pause"
    );
}

#[test]
fn pausing_freezes_only_the_terminal_view_while_output_processing_continues() {
    let ctx = egui::Context::default();
    let mut config = Config::default();
    config.buttons[0] = ButtonConfig {
        label: "Staged output".into(),
        command: "printf 'first update\\n'; sleep 0.2; printf 'second update\\n'".into(),
        shell: true,
        close_on_success: true,
        ..Default::default()
    };
    let mut app = ButtonBuilder::new(&ctx, None, config);
    app.launch(0);
    wait_for_tabs(&mut app, &ctx, |app| {
        app.workspace.tabs()[0]
            .terminal
            .screen
            .text()
            .contains("first update")
    });
    let original_config = serde_yaml_ng::to_string(&app.config).unwrap();

    let output = frame(&mut app, &ctx, vec![]);
    let pause = egui_material3::material_symbol::material_symbol_text("pause");
    click(&mut app, &ctx, label_center(&output, &pause));
    assert!(app.workspace.tabs()[0].display_paused());

    wait_for_tabs(&mut app, &ctx, |app| {
        app.workspace.tabs()[0]
            .terminal
            .screen
            .text()
            .contains("second update")
    });
    assert!(
        !app.workspace.tabs()[0]
            .screen()
            .text()
            .contains("second update"),
        "The frozen view must not change while the live terminal keeps processing output"
    );
    assert_eq!(
        app.workspace.tabs().len(),
        1,
        "A paused view must suppress close-on-success until it is resumed"
    );

    let output = frame(&mut app, &ctx, vec![]);
    let resume = egui_material3::material_symbol::material_symbol_text("play_arrow");
    click(&mut app, &ctx, label_center(&output, &resume));
    assert!(!app.workspace.tabs()[0].display_paused());
    assert!(
        app.workspace.tabs()[0]
            .screen()
            .text()
            .contains("second update"),
        "Resuming must catch the pane up to the live terminal"
    );
    assert!(!app.dirty);
    assert_eq!(
        serde_yaml_ng::to_string(&app.config).unwrap(),
        original_config
    );
}

#[test]
fn line_wrap_and_horizontal_scrolling_are_per_tab_and_do_not_change_configuration() {
    let ctx = egui::Context::default();
    let mut config = Config::default();
    for (index, label) in ["Wide output", "Other tab"].into_iter().enumerate() {
        config.buttons[index] = ButtonConfig {
            label: label.into(),
            command: "/bin/sleep".into(),
            arguments: "60".into(),
            ..Default::default()
        };
    }
    let mut app = ButtonBuilder::new(&ctx, None, config);
    app.launch(0);
    frame(&mut app, &ctx, vec![]);
    let original_config = serde_yaml_ng::to_string(&app.config).unwrap();
    let text = format!("left-{}-right", "x".repeat(400));
    app.workspace.tabs_mut()[0]
        .terminal
        .screen
        .feed(format!("\x1b[2J\x1b[H{text}\r\n").as_bytes());
    let output = frame(&mut app, &ctx, vec![]);
    let cols = app.workspace.tabs()[0].terminal.screen.cols;
    assert!(!app.workspace.tabs()[0].line_wrap);
    assert!(
        app.workspace.tabs()[0]
            .terminal
            .screen
            .text()
            .contains(&text)
    );
    let horizontal_track =
        scrollbar_track(&output, true).expect("Wide output needs a horizontal scrollbar");
    click(
        &mut app,
        &ctx,
        horizontal_track.right_center() - Vec2::new(1.0, 0.0),
    );
    let output = frame(&mut app, &ctx, vec![]);
    assert!(app.workspace.tabs()[0].horizontal_scroll > 0.0);
    let font = egui::FontId::monospace(app.workspace.tabs()[0].style.font_size);
    let width = ctx.fonts_mut(|fonts| fonts.glyph_width(&font, 'M'));
    let anchor = output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.job.text.contains("right") => Some(
                text.pos
                    + Vec2::new(
                        text.galley.job.text.find("right").unwrap() as f32 * width,
                        8.0,
                    ),
            ),
            _ => None,
        })
        .expect("Scrolling reveals the end of the line");
    let head = anchor + Vec2::new(5.0 * width, 0.0);
    frame(
        &mut app,
        &ctx,
        vec![
            egui::Event::PointerMoved(anchor),
            primary_pointer(anchor, true),
        ],
    );
    frame(&mut app, &ctx, vec![egui::Event::PointerMoved(head)]);
    let copied = frame(&mut app, &ctx, vec![primary_pointer(head, false)]);
    assert!(
        copied.platform_output.commands.iter().any(
            |command| matches!(command, egui::OutputCommand::CopyText(text) if text == "right")
        )
    );

    let output = frame(&mut app, &ctx, vec![]);
    click(&mut app, &ctx, symbol_center(&output, "wrap_text"));
    let output = frame(
        &mut app,
        &ctx,
        vec![egui::Event::PointerMoved(egui::Pos2::ZERO)],
    );
    assert!(app.workspace.tabs()[0].line_wrap);
    let wrap = symbol_center(&output, "wrap_text");
    assert!(output.shapes.iter().any(|shape| matches!(
        &shape.shape,
        egui::Shape::Rect(rect)
            if rect.rect.contains(wrap)
                && rect.fill == get_global_color("primaryContainer")
    )));
    assert_eq!(app.workspace.tabs()[0].horizontal_scroll, 0.0);
    assert_eq!(app.workspace.tabs()[0].terminal.screen.cols, cols);
    assert!(output.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Text(text) if text.galley.job.text.contains("right"))));
    assert!(scrollbar_track(&output, true).is_none());
    app.launch(1);
    assert!(!app.workspace.tabs()[1].line_wrap);
    app.workspace.select_tab(0);
    let output = frame(&mut app, &ctx, vec![]);
    click(&mut app, &ctx, symbol_center(&output, "wrap_text"));
    assert!(!app.workspace.tabs()[0].line_wrap);
    assert!(
        app.workspace.tabs()[0]
            .terminal
            .screen
            .text()
            .contains(&text)
    );
    assert!(!app.dirty);
    assert_eq!(
        serde_yaml_ng::to_string(&app.config).unwrap(),
        original_config
    );
}

#[test]
fn idle_terminal_stops_fast_repaints_and_queued_input_wakes_the_ui() {
    let ctx = egui::Context::default();
    let mut config = Config::default();
    config.buttons[0] = interactive_button("Idle");
    let mut app = ButtonBuilder::new(&ctx, None, config);
    app.launch(0);
    wait_for_tabs(&mut app, &ctx, |app| {
        app.workspace.tabs()[0]
            .terminal
            .screen
            .text()
            .contains("Idle-ready")
    });
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let output = frame(&mut app, &ctx, vec![]);
        if output.viewport_output[&egui::ViewportId::ROOT].repaint_delay
            >= Duration::from_millis(100)
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "An idle terminal should stop fast repaints"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    let (send, receive) = std::sync::mpsc::channel();
    ctx.set_request_repaint_callback(move |_| {
        let _ = send.send(());
    });
    app.workspace.tabs_mut()[0].terminal.send(b"hello\n");
    receive.recv_timeout(Duration::from_secs(1)).unwrap();
    // Service only notified frames; idle fallback polling is not needed to deliver input/output.
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        frame(&mut app, &ctx, vec![]);
        if app.workspace.tabs()[0]
            .terminal
            .screen
            .text()
            .contains("Idle:hello")
        {
            break;
        }
        receive
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .unwrap();
    }
}

#[test]
fn switching_tabs_reuses_terminal_layouts_and_updates_text_colors_and_fonts() {
    let ctx = egui::Context::default();
    let mut config = Config::default();
    config.buttons[0] = interactive_button("Alpha");
    config.buttons[1] = interactive_button("Beta");
    let mut app = ButtonBuilder::new(&ctx, None, config);
    app.launch(0);
    app.launch(1);
    wait_for_tabs(&mut app, &ctx, |app| {
        app.workspace
            .tabs()
            .iter()
            .all(|tab| tab.terminal.screen.text().contains("-ready"))
    });
    app.workspace.tabs_mut()[0]
        .terminal
        .screen
        .feed(b"\x1b[2J\x1b[Hcached-alpha");
    let layout = |output: &egui::FullOutput, prefix: &str| {
        output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.job.text.starts_with(prefix) => {
                    Some((text.galley.clone(), text.fallback_color))
                }
                _ => None,
            })
            .unwrap()
    };
    app.workspace.select_tab(0);
    let first = layout(&frame(&mut app, &ctx, vec![]), "cached-alpha").0;
    app.workspace.select_tab(1);
    for _ in 0..3 {
        frame(&mut app, &ctx, vec![]);
    }
    app.workspace.select_tab(0);
    let returned = layout(&frame(&mut app, &ctx, vec![]), "cached-alpha").0;
    assert!(
        std::sync::Arc::ptr_eq(&first, &returned),
        "Returning to a hidden tab should reuse its text layout"
    );
    app.workspace.tabs_mut()[0].style.fg_color = Some([20, 100, 200]);
    let (recolored, color) = layout(&frame(&mut app, &ctx, vec![]), "cached-alpha");
    assert!(std::sync::Arc::ptr_eq(&first, &recolored));
    assert_eq!(color, rgb([20, 100, 200]));
    for hidden in [false, true] {
        if hidden {
            app.workspace.select_tab(1);
        }
        for mode in [
            crate::theme::Appearance::Light,
            crate::theme::Appearance::Dark,
        ] {
            app.theme.mode = mode;
            // Theme changes take effect in the font atlas at the start of the next pass.
            frame(&mut app, &ctx, vec![]);
            frame(&mut app, &ctx, vec![]);
        }
        app.workspace.select_tab(0);
        let refreshed = layout(&frame(&mut app, &ctx, vec![]), "cached-alpha").0;
        let fresh = ctx.fonts_mut(|fonts| fonts.layout_job(refreshed.job.as_ref().clone()));
        assert_eq!(
            refreshed.rows[0].visuals.mesh.vertices, fresh.rows[0].visuals.mesh.vertices,
            "Terminal glyphs must use the current font atlas after a theme switch (hidden={hidden})"
        );
        assert!(!std::sync::Arc::ptr_eq(&recolored, &refreshed));
    }
    app.workspace.tabs_mut()[0]
        .terminal
        .screen
        .feed(b"\x1b[Hchanged-alpha");
    let changed = layout(&frame(&mut app, &ctx, vec![]), "changed-alpha").0;
    assert!(!std::sync::Arc::ptr_eq(&first, &changed));
    app.workspace.tabs_mut()[0].style.font_size += 1.0;
    let resized = layout(&frame(&mut app, &ctx, vec![]), "changed-alpha").0;
    assert!(!std::sync::Arc::ptr_eq(&changed, &resized));
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
    assert_eq!(app.workspace.tabs().len(), 3);
    // Editing button preferences doesn't change the policy for an existing run.
    app.config.buttons[0].close_on_success = false;
    app.config.buttons[1].close_on_success = true;
    wait_for_tabs(&mut app, &ctx, |app| {
        app.workspace.tabs().len() == 2
            && app
                .workspace
                .tabs()
                .iter()
                .all(|tab| !tab.terminal.needs_poll())
    });
    assert_eq!(app.workspace.tabs()[0].label, "Run 1");
    assert!(
        app.workspace.tabs()[0]
            .terminal
            .screen
            .text()
            .contains("result 1")
    );
    assert_eq!(app.workspace.tabs()[1].label, "Run 2");
    assert!(
        app.workspace.tabs()[1]
            .terminal
            .screen
            .text()
            .contains("result 2")
    );
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
        app.workspace.tabs()[0]
            .terminal
            .screen
            .text()
            .contains("Keep open-ready")
    });
    let kept_id = app.workspace.tabs()[0].id();
    app.launch(0);
    app.launch(1);
    app.workspace.select_tab(0);
    frame(&mut app, &ctx, vec![]);
    let focused = ctx.memory(|memory| memory.focused());
    wait_for_tabs(&mut app, &ctx, |app| app.workspace.tabs().len() == 1);
    assert_eq!(app.workspace.active_index(), Some(0));
    assert_eq!(app.workspace.tabs()[0].id(), kept_id);
    assert!(app.workspace.tabs()[0].terminal.is_running());
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
    app.workspace.close_tab(0);
    app.launch(0);
    wait_for_tabs(&mut app, &ctx, |app| app.workspace.tabs().is_empty());
    assert_eq!(app.workspace.active_index(), None);
    let output = frame(&mut app, &ctx, vec![]);
    label_center(&output, "No process output yet.");
    label_center(&output, "Silent 1 completed successfully");
    app.launch(2);
    assert!(app.completion_notice.is_none());
    wait_for_tabs(&mut app, &ctx, |app| {
        app.workspace.tabs()[0]
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
        app.workspace.tabs()[0]
            .terminal
            .screen
            .text()
            .contains("Keep open-ready")
    });
    app.launch(1);
    assert_eq!(app.workspace.active_index(), Some(1));
    wait_for_tabs(&mut app, &ctx, |app| app.workspace.tabs().len() == 1);
    assert_eq!(app.workspace.active_index(), Some(0));
    for index in 2..=5 {
        app.launch(index);
    }
    wait_for_tabs(&mut app, &ctx, |app| {
        app.workspace
            .tabs()
            .iter()
            .skip(1)
            .all(|tab| !tab.terminal.needs_poll())
    });
    assert_eq!(app.workspace.tabs().len(), 5);
    assert_eq!(
        app.workspace.tabs()[1].terminal.status,
        "Process finished: exit status: 7"
    );
    assert!(
        app.workspace.tabs()[1]
            .terminal
            .screen
            .text()
            .contains("failure details")
    );
    assert_eq!(
        app.workspace.tabs()[2].terminal.status,
        "Process finished: exit status: 1"
    );
    assert!(
        app.workspace.tabs()[2]
            .terminal
            .screen
            .text()
            .trim()
            .is_empty()
    );
    assert!(
        app.workspace.tabs()[3]
            .terminal
            .screen
            .text()
            .contains("successful output")
    );
    assert!(
        app.workspace.tabs()[4]
            .terminal
            .screen
            .text()
            .trim()
            .is_empty()
    );
    app.workspace.select_tab(2);
    let output = frame(&mut app, &ctx, vec![]);
    label_center(&output, "Process finished: exit status: 1");
    app.launch(6);
    app.workspace.tabs_mut()[5].terminal.stop();
    frame(&mut app, &ctx, vec![]);
    assert_eq!(app.workspace.tabs().len(), 6);
    assert_eq!(app.workspace.tabs()[5].terminal.status, "Stopped");
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
        !app.workspace.tabs()[0]
            .terminal
            .screen
            .text()
            .trim()
            .is_empty()
    });
    let output = frame(&mut app, &ctx, vec![]);
    click(&mut app, &ctx, label_center(&output, "Quiet"));
    assert_eq!(app.workspace.tabs().len(), 2);
    wait_for_tabs(&mut app, &ctx, |app| {
        app.workspace.tabs()[1]
            .terminal
            .screen
            .text()
            .contains("Quiet-ready")
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
        app.workspace.tabs()[0].terminal.is_running(),
        "The flood deliberately ignores SIGINT"
    );
    let output = frame(&mut app, &ctx, vec![]);
    click(&mut app, &ctx, symbol_center(&output, "stop_circle"));
    assert_eq!(app.workspace.tabs()[0].terminal.status, "Stopped");
    assert!(!app.workspace.tabs()[0].terminal.needs_poll());
    assert!(app.workspace.tabs()[1].terminal.is_running());
    let output = frame(&mut app, &ctx, vec![]);
    click(&mut app, &ctx, label_center(&output, "Flood"));
    assert!(app.workspace.tabs()[2].terminal.is_running());
    let pid: i32 = app.workspace.tabs()[2]
        .terminal
        .status
        .split_whitespace()
        .last()
        .unwrap()
        .parse()
        .unwrap();
    let output = frame(&mut app, &ctx, vec![]);
    click(&mut app, &ctx, symbol_center(&output, "close"));
    assert_eq!(app.workspace.tabs().len(), 2);
    assert_eq!(app.workspace.tabs()[1].label, "Quiet");
    assert_eq!(app.workspace.tabs()[0].terminal.status, "Stopped");
    wait_for_tabs(&mut app, &ctx, |_| {
        // SAFETY: signal zero only observes this test's child.
        (unsafe { libc::kill(pid, 0) }) == -1
    });
    assert!(app.workspace.tabs()[1].terminal.is_running());
}

#[test]
fn terminal_mouse_selection_copies_words_lines_and_dragged_ranges() {
    let ctx = egui::Context::default();
    let mut config = Config::default();
    config.buttons[0] = interactive_button("Alpha");
    let mut app = ButtonBuilder::new(&ctx, None, config);
    app.launch(0);
    wait_for_tabs(&mut app, &ctx, |app| {
        app.workspace.tabs()[0]
            .terminal
            .screen
            .text()
            .contains("Alpha-ready")
    });
    app.workspace.tabs_mut()[0]
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
    let font = egui::FontId::monospace(app.workspace.tabs()[0].style.font_size);
    let (width, height) = ctx.fonts_mut(|fonts| {
        (
            fonts.glyph_width(&font, 'M'),
            fonts.row_height(&font) * app.workspace.tabs()[0].style.line_spacing,
        )
    });
    let cell = |row: usize, col: usize| {
        origin + Vec2::new(col as f32 * width, (row as f32 + 0.5) * height)
    };
    let start = cell(0, 6);
    let end = cell(1, 7);
    let word = cell(0, 7);
    let mut multi_click = || {
        frame(
            &mut app,
            &ctx,
            vec![egui::Event::PointerMoved(word), primary_pointer(word, true)],
        );
        frame(&mut app, &ctx, vec![primary_pointer(word, false)])
    };
    multi_click();
    let copied = multi_click();
    assert!(
        copied.platform_output.commands.iter().any(
            |command| matches!(command, egui::OutputCommand::CopyText(text) if text == "bravo")
        )
    );
    let copied = multi_click();
    assert!(copied.platform_output.commands.iter().any(
        |command| matches!(command, egui::OutputCommand::CopyText(text) if text == "alpha bravo")
    ));
    drop(multi_click);

    for (anchor, head) in [(start, end), (end, start)] {
        frame(
            &mut app,
            &ctx,
            vec![
                egui::Event::PointerMoved(anchor),
                primary_pointer(anchor, true),
            ],
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
        let released = frame(&mut app, &ctx, vec![primary_pointer(head, false)]);
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
    let copy_pos = symbol_center(&copied, "content_copy");
    frame(
        &mut app,
        &ctx,
        vec![
            egui::Event::PointerMoved(copy_pos),
            primary_pointer(copy_pos, true),
        ],
    );
    let copied = frame(&mut app, &ctx, vec![primary_pointer(copy_pos, false)]);
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
    let requested_paste = pointer_click(&mut app, &ctx, word, egui::PointerButton::Middle)
        .iter()
        .any(|output| {
            output.viewport_output[&egui::ViewportId::ROOT]
                .commands
                .contains(&egui::ViewportCommand::RequestPaste)
        });
    assert!(requested_paste);
    frame(
        &mut app,
        &ctx,
        vec![
            egui::Event::Paste("middle-pasted".into()),
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
        app.workspace.tabs()[0]
            .terminal
            .screen
            .text()
            .contains("Alpha:middle-pasted")
    });
    for _ in 0..40 {
        frame(&mut app, &ctx, vec![]);
    }
    click(&mut app, &ctx, start);
    assert!(
        app.workspace.tabs()[0]
            .selection
            .text(&app.workspace.tabs()[0].terminal.screen)
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
        vec![
            egui::Event::PointerMoved(start),
            primary_pointer(start, true),
        ],
    );
    frame(&mut app, &ctx, vec![egui::Event::PointerMoved(end)]);
    frame(&mut app, &ctx, vec![primary_pointer(end, false)]);
    frame(&mut app, &ctx, vec![egui::Event::Text("typed\n".into())]);
    assert!(
        app.workspace.tabs()[0]
            .selection
            .text(&app.workspace.tabs()[0].terminal.screen)
            .is_none()
    );
    wait_for_tabs(&mut app, &ctx, |app| {
        app.workspace.tabs()[0]
            .terminal
            .screen
            .text()
            .contains("Alpha:typed")
    });
    frame(
        &mut app,
        &ctx,
        vec![
            egui::Event::PointerMoved(start),
            primary_pointer(start, true),
        ],
    );
    frame(&mut app, &ctx, vec![egui::Event::PointerMoved(end)]);
    frame(&mut app, &ctx, vec![primary_pointer(end, false)]);
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
    wait_for_tabs(&mut app, &ctx, |app| {
        !app.workspace.tabs()[0].terminal.is_running()
    });
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
        app.workspace
            .tabs()
            .iter()
            .all(|tab| tab.terminal.screen.text().contains("-ready"))
    });
    app.workspace.select_tab(0);
    frame(&mut app, &ctx, vec![]);
    app.workspace.tabs_mut()[0]
        .terminal
        .screen
        .feed(b"\x1b[2J\x1b[H");
    for i in 0..120 {
        app.workspace.tabs_mut()[0]
            .terminal
            .screen
            .feed(format!("history-{i}\r\n").as_bytes());
    }
    app.workspace.tabs_mut()[0].scrollback = app.workspace.tabs()[0].terminal.screen.history.len();
    let output = frame(&mut app, &ctx, vec![]);
    let origin = output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.job.text.starts_with("history-0") => {
                Some(text.pos)
            }
            _ => None,
        })
        .unwrap();
    let font = egui::FontId::monospace(app.workspace.tabs()[0].style.font_size);
    let (width, height) = ctx.fonts_mut(|fonts| {
        (
            fonts.glyph_width(&font, 'M'),
            fonts.row_height(&font) * app.workspace.tabs()[0].style.line_spacing,
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
    app.workspace.select_tab(1);
    frame(&mut app, &ctx, vec![]);
    assert!(
        app.workspace.tabs()[1]
            .selection
            .text(&app.workspace.tabs()[1].terminal.screen)
            .is_none()
    );
    app.workspace.select_tab(0);
    frame(&mut app, &ctx, vec![]);
    assert_eq!(
        app.workspace.tabs()[0]
            .selection
            .text(&app.workspace.tabs()[0].terminal.screen)
            .as_deref(),
        Some("history-0")
    );
    app.workspace.tabs_mut()[0]
        .terminal
        .screen
        .feed(b"new output\r\n");
    let output = frame(&mut app, &ctx, vec![]);
    assert_eq!(
        app.workspace.tabs()[0]
            .selection
            .text(&app.workspace.tabs()[0].terminal.screen)
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
    app.workspace.tabs_mut()[0].scrollback = app.workspace.tabs()[0].terminal.screen.history.len();
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
        app.workspace
            .tabs()
            .iter()
            .all(|tab| tab.terminal.screen.text().contains("-ready"))
    });
    app.workspace.select_tab(0);
    frame(&mut app, &ctx, vec![]);
    for i in 0..120 {
        app.workspace.tabs_mut()[0]
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
        app.workspace.tabs()[0].scrollback,
        0,
        "Grabbing the thumb must not jump"
    );
    frame(
        &mut app,
        &ctx,
        vec![egui::Event::PointerMoved(track.center())],
    );
    assert!(app.workspace.tabs()[0].scrollback > 0);
    assert!(
        app.workspace.tabs()[0].scrollback < app.workspace.tabs()[0].terminal.screen.history.len()
    );
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
    let saved = app.workspace.tabs()[0].scrollback;
    app.workspace.select_tab(1);
    let output = frame(&mut app, &ctx, vec![]);
    assert!(
        scrollbar_rects(&output).is_none(),
        "No scrollbar without history"
    );
    app.workspace.select_tab(0);
    frame(&mut app, &ctx, vec![]);
    assert_eq!(app.workspace.tabs()[0].scrollback, saved);
    let anchor = app.workspace.tabs()[0].terminal.screen.history.len() - saved;
    wheel(
        &mut app,
        &ctx,
        track.center(),
        egui::MouseWheelUnit::Line,
        Vec2::new(0.0, 1.0),
        egui::Modifiers::CTRL,
    );
    assert_eq!(
        app.workspace.tabs()[0].style.font_size,
        15.0,
        "Zoom also works over the scrollbar"
    );
    assert_eq!(
        app.workspace.tabs()[0].terminal.screen.history.len() - app.workspace.tabs()[0].scrollback,
        anchor
    );
    app.workspace.tabs_mut()[0].terminal.send(b"arrived\n");
    wait_for_tabs(&mut app, &ctx, |app| {
        app.workspace.tabs()[0]
            .terminal
            .screen
            .text()
            .contains("Alpha:arrived")
    });
    assert_eq!(
        app.workspace.tabs()[0].terminal.screen.history.len() - app.workspace.tabs()[0].scrollback,
        anchor,
        "Incoming output must keep the viewed text anchored"
    );
    let output = frame(&mut app, &ctx, vec![]);
    let (track, _) = scrollbar_rects(&output).unwrap();
    click(&mut app, &ctx, track.center_top());
    assert_eq!(
        app.workspace.tabs()[0].scrollback,
        app.workspace.tabs()[0].terminal.screen.history.len()
    );
    click(&mut app, &ctx, track.center_bottom() - Vec2::new(0.0, 1.0));
    assert_eq!(
        app.workspace.tabs()[0].scrollback,
        0,
        "Bottom resumes live output"
    );
    click(&mut app, &ctx, track.center_top());
    frame(&mut app, &ctx, vec![egui::Event::Text("hello\n".into())]);
    assert_eq!(app.workspace.tabs()[0].scrollback, 0);
    wait_for_tabs(&mut app, &ctx, |app| {
        app.workspace.tabs()[0]
            .terminal
            .screen
            .text()
            .contains("Alpha:hello")
    });
    assert!(
        !app.workspace.tabs()[1]
            .terminal
            .screen
            .text()
            .contains("hello")
    );
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
        app.workspace.tabs_mut()[0]
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
    assert_eq!(app.workspace.tabs()[0].scrollback, 2000);
    let output = small_frame(&mut app, vec![]);
    let (track, thumb) = scrollbar_rects(&output).unwrap();
    assert_eq!(track.top(), thumb.top());
    let below = egui::pos2(grab.x, 600.0);
    small_frame(&mut app, vec![egui::Event::PointerMoved(below)]);
    assert_eq!(app.workspace.tabs()[0].scrollback, 0);
    small_frame(
        &mut app,
        vec![egui::Event::PointerButton {
            pos: below,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    app.workspace.tabs_mut()[0]
        .terminal
        .screen
        .feed(b"\x1b[?1049h");
    let output = small_frame(&mut app, vec![]);
    assert!(
        scrollbar_rects(&output).is_none(),
        "Full-screen programs use their alternate screen"
    );
    app.workspace.tabs_mut()[0]
        .terminal
        .screen
        .feed(b"\x1b[?1049l");
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
        app.workspace
            .tabs()
            .iter()
            .all(|tab| tab.terminal.screen.text().contains("-ready"))
    });
    app.workspace.select_tab(0);
    frame(&mut app, &ctx, vec![]);
    for i in 0..100 {
        app.workspace.tabs_mut()[0]
            .terminal
            .screen
            .feed(format!("history-{i}\r\n").as_bytes());
    }
    app.workspace.tabs_mut()[0].scrollback = 20;
    app.workspace.tabs_mut()[0].scroll_fraction = 0.5;
    frame(&mut app, &ctx, vec![]);
    let anchor =
        app.workspace.tabs()[0].terminal.screen.history.len() - app.workspace.tabs()[0].scrollback;
    let first_size = app.workspace.terminal_size();
    let second_size = (
        app.workspace.tabs()[1].terminal.screen.cols,
        app.workspace.tabs()[1].terminal.screen.rows,
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
    assert_eq!(app.workspace.tabs()[0].style.font_size, 16.0);
    assert_eq!(app.workspace.tabs()[1].style.font_size, 14.0);
    assert_eq!(app.config.terminal.font_size, 16.0);
    assert!(app.dirty);
    assert!(app.workspace.terminal_size().0 < first_size.0);
    assert!(app.workspace.terminal_size().1 < first_size.1);
    assert_eq!(
        app.workspace.terminal_size(),
        (
            app.workspace.tabs()[0].terminal.screen.cols,
            app.workspace.tabs()[0].terminal.screen.rows
        )
    );
    assert_eq!(
        second_size,
        (
            app.workspace.tabs()[1].terminal.screen.cols,
            app.workspace.tabs()[1].terminal.screen.rows
        )
    );
    assert_eq!(
        app.workspace.tabs()[0].terminal.screen.history.len() - app.workspace.tabs()[0].scrollback,
        anchor
    );
    assert_eq!(app.workspace.tabs()[0].scroll_fraction, 0.5);
    assert_eq!(ctx.zoom_factor(), 1.0);

    wheel(
        &mut app,
        &ctx,
        pane,
        egui::MouseWheelUnit::Line,
        Vec2::new(0.0, -1.0),
        egui::Modifiers::CTRL,
    );
    assert_eq!(app.workspace.tabs()[0].style.font_size, 15.0);
    // egui's smoothed zoom input must not keep changing font size after the wheel stops.
    for _ in 0..12 {
        frame(&mut app, &ctx, vec![]);
    }
    assert_eq!(app.workspace.tabs()[0].style.font_size, 15.0);
    assert_eq!(
        app.workspace.tabs()[0].terminal.screen.history.len() - app.workspace.tabs()[0].scrollback,
        anchor
    );
    let scrollback = app.workspace.tabs()[0].scrollback;
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
        app.workspace.tabs()[0].scrollback > scrollback,
        "Plain scrolling still moves through history"
    );
    assert_eq!(app.workspace.tabs()[0].style.font_size, 15.0);
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
    assert_eq!(app.workspace.tabs()[2].style.font_size, 15.0);
    assert_eq!(app.workspace.tabs()[1].style.font_size, 14.0);
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
    for pos in [egui::pos2(100.0, 300.0), symbol_center(&output, "tune")] {
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
        app.workspace
            .tabs()
            .iter()
            .all(|tab| tab.terminal.screen.text().contains("-ready"))
    });
    // Build history without depending on asynchronous process timing.
    for i in 0..100 {
        app.workspace.tabs_mut()[0]
            .terminal
            .screen
            .feed(format!("history-{i}\r\n").as_bytes());
    }
    app.workspace.tabs_mut()[0].scrollback = 10;
    app.workspace.tabs_mut()[0].scroll_fraction = 0.5;
    app.workspace.tabs_mut()[0].style.font_size = 22.0;
    app.workspace.tabs_mut()[0].style.bg_color = Some([10, 30, 40]);
    app.workspace.select_tab(0);
    let output = frame(&mut app, &ctx, vec![]);
    let history_anchor =
        app.workspace.tabs()[0].terminal.screen.history.len() - app.workspace.tabs()[0].scrollback;
    click(&mut app, &ctx, symbol_center(&output, "tune"));
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
    assert_eq!(app.workspace.tabs()[0].style.font_size, 20.0);
    assert_eq!(app.workspace.tabs()[1].style.font_size, 14.0);
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
    let first_cols = app.workspace.tabs()[0].terminal.screen.cols;
    let first_rows = app.workspace.tabs()[0].terminal.screen.rows;
    app.workspace.select_tab(1);
    let second_output = frame(&mut app, &ctx, vec![]);
    symbol_center(&second_output, "vertical_align_bottom");
    assert_eq!(
        app.workspace.tabs()[0].terminal.screen.history.len() - app.workspace.tabs()[0].scrollback,
        history_anchor
    );
    assert_eq!(app.workspace.tabs()[0].scroll_fraction, 0.5);
    for _ in 0..3 {
        frame_with(
            &mut app,
            &ctx,
            Vec2::new(850.0, 500.0),
            egui::Modifiers::NONE,
            vec![],
        );
    }
    assert!(app.workspace.tabs()[0].terminal.screen.cols < first_cols);
    assert!(app.workspace.tabs()[0].terminal.screen.rows < first_rows);
    assert!(
        app.workspace.tabs()[0].terminal.screen.cols < app.workspace.tabs()[1].terminal.screen.cols
    );
    app.workspace.select_tab(0);
    let output = frame(&mut app, &ctx, vec![]);
    let jump = symbol_center(&output, "vertical_align_bottom");
    let copy = symbol_center(&output, "content_copy");
    let pause = symbol_center(&output, "pause");
    let line_wrap = symbol_center(&output, "wrap_text");
    assert!(
        jump.x < pause.x
            && pause.x < copy.x
            && copy.x < line_wrap.x
            && (jump.y - line_wrap.y).abs() < 1.0,
        "Display controls should read as Jump, Pause, Copy, Line wrap"
    );
    let output = frame(
        &mut app,
        &ctx,
        vec![egui::Event::PointerMoved(symbol_center(
            &output,
            "vertical_align_bottom",
        ))],
    );
    click(
        &mut app,
        &ctx,
        symbol_center(&output, "vertical_align_bottom"),
    );
    assert_eq!(app.workspace.tabs()[0].scrollback, 0);
    assert_eq!(app.workspace.tabs()[0].scroll_fraction, 0.0);
    app.launch(2);
    assert_eq!(
        app.workspace.tabs()[2].style.font_size,
        20.0,
        "New tabs inherit the edited appearance default"
    );
    app.workspace.select_tab(1);
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
    assert_eq!(app.workspace.tabs()[0].style.font_size, 20.0);
    assert_eq!(app.workspace.tabs()[2].style.font_size, 20.0);
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
    app.workspace.tabs_mut()[0].style.bg_color = Some([10, 30, 40]);
    app.workspace.tabs_mut()[1]
        .terminal
        .screen
        .feed(b"Retain me");
    let next_id = app.workspace.next_tab_id();
    app.launch(1);
    assert!(app.error.is_some());
    assert_eq!(app.workspace.tabs().len(), 2);
    assert_eq!(app.workspace.active_index(), Some(1));
    assert_eq!(app.workspace.next_tab_id(), next_id);
    app.workspace.tabs_mut()[1].terminal.stop();
    app.launch(1);
    assert!(app.error.is_some());
    assert!(
        app.workspace.tabs()[1]
            .terminal
            .screen
            .text()
            .contains("Retain me")
    );
    assert_eq!(app.workspace.tabs()[1].id(), 2);
    app.load_config(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/terminal-demo.yaml"))
        .unwrap();
    assert_eq!(app.workspace.tabs().len(), 2);
    assert_eq!(app.workspace.active_index(), Some(1));
    assert!(
        app.workspace.running(),
        "A background process still counts as running"
    );
    assert!(app.workspace.tabs()[0].terminal.is_running());
    assert_eq!(app.workspace.tabs()[0].label, "Alpha");
    assert_eq!(app.workspace.tabs()[0].style.bg_color, Some([10, 30, 40]));
    assert!(
        app.workspace
            .tabs()
            .iter()
            .all(|tab| tab.button_index.is_none())
    );
    app.launch(0);
    assert_eq!(app.workspace.tabs()[2].label, app.config.buttons[0].label);
    assert_eq!(
        app.workspace.tabs()[2].style.font_size,
        app.config.terminal.font_size
    );
    app.workspace.close_tab(0);
    assert_eq!(app.workspace.active_index(), Some(1));
    assert_eq!(app.workspace.active().unwrap().id(), 3);
    app.workspace.close_tab(1);
    assert_eq!(app.workspace.active_index(), Some(0));
    assert_eq!(app.workspace.active().unwrap().id(), 2);
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
    for _ in 0..4 {
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
    for action in ["tune", "content_copy", "stop_circle", "close"] {
        let glyph = egui_material3::material_symbol::material_symbol_text(action);
        let pos = label_center(&output, &glyph);
        assert!(pos.x < size.x && pos.y < size.y);
        assert!(output.shapes.iter().any(|shape| shape.clip_rect.contains(pos)
            && matches!(&shape.shape, egui::Shape::Text(text) if text.galley.job.text == glyph)));
    }
    let interrupt = label_center(&output, "Ctrl+C");
    assert!(interrupt.x < size.x && interrupt.y < size.y);
    let appearance = symbol_center(&output, "tune");
    let stop = symbol_center(&output, "stop_circle");
    let close = symbol_center(&output, "close");
    assert!(
        appearance.x < interrupt.x
            && interrupt.x < stop.x
            && stop.x < close.x
            && (appearance.y - interrupt.y).abs() < 1.0
            && (interrupt.y - stop.y).abs() < 1.0
            && (stop.y - close.y).abs() < 1.0,
        "Process controls should read as Appearance, Interrupt, Stop, Close"
    );
    let copy = symbol_center(&output, "content_copy");
    let pause = symbol_center(&output, "pause");
    let line_wrap = symbol_center(&output, "wrap_text");
    let jump = symbol_center(&output, "vertical_align_bottom");
    assert!(
        jump.x < pause.x
            && pause.x < copy.x
            && copy.x < line_wrap.x
            && (jump.y - pause.y).abs() < 1.0
            && (copy.y - pause.y).abs() < 1.0
            && (copy.y - line_wrap.y).abs() < 1.0,
        "Display controls should stay grouped as Jump, Pause, Copy, Line wrap"
    );
    let left = label_center(&output, "‹");
    let right = label_center(&output, "›");
    assert!(
        left.x < label.1.left() && right.x > label.1.right(),
        "Arrows must stay outside the scrolling tabs"
    );
    assert!(right.x < size.x && (left.y - right.y).abs() < 1.0);
    let pager_button = |center| {
        output.shapes.iter().find_map(|shape| match &shape.shape {
            egui::Shape::Rect(rect)
                if rect.stroke.width > 0.0
                    && rect.rect.contains(center)
                    && (rect.rect.width() - 32.0).abs() < 1.0 =>
            {
                Some((rect.rect, shape.clip_rect))
            }
            _ => None,
        })
    };
    let (left_button, left_clip) =
        pager_button(left).expect("The previous-tab button should have an outline");
    let (right_button, right_clip) =
        pager_button(right).expect("The next-tab button should have an outline");
    assert!(
        left_button.left() - left_clip.left() >= 1.0,
        "The previous-tab outline needs room inside the pane clip"
    );
    assert!(
        right_clip.right() - right_button.right() >= 1.0,
        "The next-tab outline needs room inside the pane clip"
    );
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
        app.workspace.active_index(),
        Some(7),
        "The next arrow stops at the last tab"
    );
    for expected in (0..7).rev() {
        click_arrow(&mut app, left);
        assert_eq!(app.workspace.active_index(), Some(expected));
    }
    click_arrow(&mut app, left);
    assert_eq!(
        app.workspace.active_index(),
        Some(0),
        "The previous arrow stops at the first tab"
    );
    for _ in 0..4 {
        output = frame_with(&mut app, &ctx, size, egui::Modifiers::NONE, vec![]);
    }
    let first = label_center(&output, "1 · A long running command label");
    assert!(first.x > left.x && first.x < right.x);
    for expected in 1..8 {
        click_arrow(&mut app, right);
        assert_eq!(app.workspace.active_index(), Some(expected));
    }
    assert!(
        app.workspace
            .tabs()
            .iter()
            .all(|tab| tab.terminal.is_running())
    );
    // Empty slots and edit mode keep configuring buttons instead of launching.
    app.launch(8);
    assert_eq!(app.workspace.tabs().len(), 8);
    assert_eq!(app.editor.as_ref().unwrap().index, 8);
    app.editor = None;
    app.edit_mode = true;
    let output = frame(&mut app, &ctx, vec![]);
    click(
        &mut app,
        &ctx,
        label_center(&output, "A long running command label"),
    );
    assert_eq!(app.workspace.tabs().len(), 8);
    assert_eq!(app.editor.as_ref().unwrap().index, 0);
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
