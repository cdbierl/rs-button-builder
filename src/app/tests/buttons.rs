//! Application buttons tests.

use super::*;

#[test]
fn ctrl_click_starts_an_independent_terminal_and_normal_click_reuses_running_instances() {
    let ctx = egui::Context::default();
    let mut config = Config::default();
    config.buttons[0] = interactive_button("Alpha");
    config.buttons[1] = interactive_button("Beta");
    let mut app = ButtonBuilder::new(&ctx, None, config);
    app.launch(0);
    app.launch(1);
    app.workspace.split_terminal(1, Split::Right);

    let output = frame(&mut app, &ctx, vec![]);
    click_with_modifiers(
        &mut app,
        &ctx,
        label_center(&output, "Alpha"),
        egui::Modifiers::CTRL,
    );
    assert_eq!(app.workspace.tabs().len(), 3);
    assert_eq!(app.workspace.active_index(), Some(2));
    assert_eq!(app.workspace.tabs()[0].pane(), 0);
    assert_eq!(app.workspace.tabs()[2].pane(), 1);
    assert_eq!(app.workspace.tabs()[2].button_index, Some(0));
    assert_ne!(
        app.workspace.tabs()[0].terminal.status,
        app.workspace.tabs()[2].terminal.status,
        "Duplicate tabs must have separate processes"
    );
    wait_for_tabs(&mut app, &ctx, |app| {
        app.workspace
            .tabs()
            .iter()
            .all(|tab| tab.terminal.is_running() && tab.terminal.screen.text().contains("-ready"))
    });

    for (selected, expected) in [(2, 2), (0, 0), (1, 2)] {
        app.workspace.select_tab(selected);
        let output = frame(&mut app, &ctx, vec![]);
        click(&mut app, &ctx, label_center(&output, "Alpha"));
        assert_eq!(app.workspace.active_index(), Some(expected));
        assert_eq!(app.workspace.tabs().len(), 3);
    }
    app.workspace.tabs_mut()[2]
        .terminal
        .send(b"duplicate-input\r");
    wait_for_tabs(&mut app, &ctx, |app| {
        app.workspace.tabs()[2]
            .terminal
            .screen
            .text()
            .contains("Alpha:duplicate-input")
    });
    assert!(
        !app.workspace.tabs()[0]
            .terminal
            .screen
            .text()
            .contains("duplicate-input")
    );

    app.workspace.tabs_mut()[2].terminal.stop();
    let output = frame(&mut app, &ctx, vec![]);
    click(&mut app, &ctx, label_center(&output, "Alpha"));
    assert_eq!(app.workspace.active_index(), Some(0));
    assert_eq!(app.workspace.tabs().len(), 3);

    app.launch_new(2); // Empty slots still open the editor.
    assert_eq!(app.editor.as_ref().unwrap().index, 2);
    assert_eq!(app.workspace.tabs().len(), 3);
    app.editor = None;
    app.edit_mode = true;
    let output = frame(&mut app, &ctx, vec![]);
    click_with_modifiers(
        &mut app,
        &ctx,
        label_center(&output, "Beta"),
        egui::Modifiers::CTRL,
    );
    assert_eq!(app.editor.as_ref().unwrap().index, 1);
    assert_eq!(app.workspace.tabs().len(), 3);
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
    assert_eq!(app.workspace.tabs().len(), 1);
    let output = frame(&mut app, &ctx, vec![]);
    click(&mut app, &ctx, label_center(&output, "Beta"));
    assert_eq!(
        app.workspace.tabs().len(),
        2,
        "Clicking a different button opens a tab without replacing a running process"
    );
    let output = frame(&mut app, &ctx, vec![]);
    click(&mut app, &ctx, label_center(&output, "Alpha"));
    assert_eq!(app.workspace.active_index(), Some(0));
    assert_eq!(
        app.workspace.tabs().len(),
        2,
        "Clicking a running button focuses its tab"
    );
    let output = frame(&mut app, &ctx, vec![]);
    click(&mut app, &ctx, label_center(&output, "Beta"));
    assert_eq!(app.workspace.tabs().len(), 2);
    assert_eq!(app.workspace.active_index(), Some(1));
    assert!(
        app.workspace
            .tabs()
            .iter()
            .all(|tab| tab.terminal.is_running())
    );
    wait_for_tabs(&mut app, &ctx, |app| {
        app.workspace
            .tabs()
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
        app.workspace.tabs()[1]
            .terminal
            .screen
            .text()
            .contains("Beta:only-beta")
    });
    assert!(
        !app.workspace.tabs()[0]
            .terminal
            .screen
            .text()
            .contains("only-beta")
    );
    app.workspace.tabs_mut()[0].terminal.send(b"background\r");
    wait_for_tabs(&mut app, &ctx, |app| {
        app.workspace.tabs()[0]
            .terminal
            .screen
            .text()
            .contains("Alpha:background")
    });
    assert_eq!(
        app.workspace.active_index(),
        Some(1),
        "Inactive terminals must keep polling"
    );

    let output = frame(&mut app, &ctx, vec![]);
    click(&mut app, &ctx, label_center(&output, "1 · Alpha"));
    assert_eq!(app.workspace.active_index(), Some(0));
    let output = frame(&mut app, &ctx, vec![]);
    let copy_pos = symbol_center(&output, "content_copy");
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
    wait_for_tabs(&mut app, &ctx, |app| {
        !app.workspace.tabs()[0].terminal.is_running()
    });
    assert!(app.workspace.tabs()[1].terminal.is_running());

    let output = frame(&mut app, &ctx, vec![]);
    click(&mut app, &ctx, label_center(&output, "Alpha"));
    assert_eq!(
        app.workspace.tabs().len(),
        3,
        "Rerunning preserves the finished tab"
    );
    assert_eq!(app.workspace.tabs()[0].id(), 1);
    assert!(
        app.workspace.tabs()[0]
            .terminal
            .screen
            .text()
            .contains("Alpha:background")
    );
    assert!(!app.workspace.tabs()[0].terminal.is_running());
    assert_eq!(app.workspace.active_index(), Some(2));
    assert_eq!(app.workspace.tabs()[2].id(), 3);
    assert!(app.workspace.tabs()[2].terminal.is_running());
    let output = frame(&mut app, &ctx, vec![]);
    click(&mut app, &ctx, symbol_center(&output, "stop_circle"));
    assert!(!app.workspace.tabs()[0].terminal.is_running());
    assert!(app.workspace.tabs()[1].terminal.is_running());
    assert_eq!(app.workspace.tabs()[2].terminal.status, "Stopped");

    let output = frame(&mut app, &ctx, vec![]);
    click(&mut app, &ctx, label_center(&output, "2 · Beta"));
    let pid: i32 = app
        .workspace
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
    click(&mut app, &ctx, symbol_center(&output, "close"));
    assert_eq!(app.workspace.tabs().len(), 2);
    assert_eq!(app.workspace.active_index(), Some(1));
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
    click(&mut app, &ctx, symbol_center(&output, "close"));
    assert_eq!(app.workspace.tabs().len(), 1);
    let output = frame(&mut app, &ctx, vec![]);
    click(&mut app, &ctx, symbol_center(&output, "close"));
    assert!(app.workspace.tabs().is_empty() && app.workspace.active_index().is_none());
    let output = frame(&mut app, &ctx, vec![]);
    click(&mut app, &ctx, label_center(&output, "Alpha"));
    assert_eq!(
        app.workspace.tabs()[0].id(),
        4,
        "New tabs must not reuse old UI IDs"
    );
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
        app.workspace
            .tabs()
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
    assert_eq!(app.workspace.tabs().len(), 2);
    assert!(
        app.workspace
            .tabs()
            .iter()
            .all(|tab| tab.terminal.is_running())
    );
    assert_eq!(app.workspace.tabs()[0].button_index, None);
    assert_eq!(app.workspace.tabs()[1].button_index, Some(2));
    assert!(
        app.workspace.tabs()[0]
            .terminal
            .screen
            .text()
            .contains("Alpha-ready")
    );
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
    assert!(app.dirty && app.workspace.tabs().is_empty() && app.editor.is_none());
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
    assert!(!app.dirty && app.workspace.tabs().is_empty() && app.editor.is_none());
    assert_eq!(
        serde_yaml_ng::to_string(&Config::load_existing(&path).unwrap()).unwrap(),
        serde_yaml_ng::to_string(&expected).unwrap()
    );
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
        assert!(!app.dirty && app.workspace.tabs().is_empty() && app.editor.is_none());
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
    assert!(!app.dirty && app.workspace.tabs().is_empty() && app.editor.is_none());
    let output = frame(&mut app, &ctx, vec![]);
    click_app_action(&mut app, &ctx, &output, "Edit layout");
    assert!(app.edit_mode);
    let output = frame(&mut app, &ctx, vec![]);
    let source = label_center(&output, "Alpha");
    let target = empty_button_centers(&output)[0];
    start_button_drag(&mut app, &ctx, source, target);
    frame(&mut app, &ctx, vec![primary_pointer(target, false)]);
    assert!(app.dirty && app.editor.is_none() && app.workspace.tabs().is_empty());
    assert_eq!(app.config.buttons[1].label, "Alpha");
    click(&mut app, &ctx, target);
    assert_eq!(app.editor.as_ref().unwrap().index, 1);
    let output = frame(&mut app, &ctx, vec![]);
    click(&mut app, &ctx, label_center(&output, "Cancel"));
    let output = frame(&mut app, &ctx, vec![]);
    click(&mut app, &ctx, label_center(&output, "Done"));
    assert!(!app.edit_mode);
    let output = frame(&mut app, &ctx, vec![]);
    let target = label_center(&output, "Alpha");
    click(&mut app, &ctx, target);
    assert_eq!(app.workspace.tabs().len(), 1);
    assert_eq!(app.workspace.tabs()[0].button_index, Some(1));
    assert_eq!(app.workspace.tabs()[0].label, "Alpha");
}

#[test]
fn repositioning_buttons_preserves_tab_associations_and_editor_drafts() {
    let ctx = egui::Context::default();
    let mut config = Config::default();
    config.buttons[0] = interactive_button("Alpha");
    config.buttons[1] = interactive_button("Beta");
    let mut app = ButtonBuilder::new(&ctx, None, config);
    app.launch(0);
    app.workspace.tabs_mut()[0].button_index = None; // A tab retained from a previous layout.
    app.launch(0);
    app.launch(1);
    app.edit(0);
    app.editor.as_mut().unwrap().button.label = "Alpha draft".into();
    app.reposition_button(0, 1);
    assert_eq!(
        app.workspace
            .tabs()
            .iter()
            .map(|tab| tab.button_index)
            .collect::<Vec<_>>(),
        [None, Some(1), Some(0)]
    );
    assert!(
        app.workspace
            .tabs()
            .iter()
            .all(|tab| tab.terminal.is_running())
    );
    assert_eq!(app.workspace.active_index(), Some(2));
    app.launch(1);
    assert_eq!(app.workspace.tabs().len(), 3);
    assert_eq!(
        app.workspace.active_index(),
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
    app.save_as.as_mut().unwrap().file_picker.path =
        directory.join("missing/layout.yaml").display().to_string();
    let output = frame(&mut app, &ctx, vec![]);
    click(&mut app, &ctx, label_center(&output, "Save config"));
    assert!(
        app.save_as
            .as_ref()
            .unwrap()
            .file_picker
            .error
            .as_ref()
            .unwrap()
            .contains("Could not save")
    );
    assert!(app.config_path.is_none() && app.editor.is_some());
    assert!(app.config.buttons[0].command.is_empty());
    app.save_as.as_mut().unwrap().file_picker.path =
        existing.with_extension("").display().to_string();
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
    assert!(
        app.workspace.tabs().is_empty(),
        "Saving a config must not run commands"
    );
    app.launch(0);
    wait_for_tabs(&mut app, &ctx, |app| {
        app.workspace.tabs()[0]
            .terminal
            .screen
            .text()
            .contains("first button works")
    });
}
