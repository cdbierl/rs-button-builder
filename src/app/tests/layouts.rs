//! Application layouts tests.

use super::*;

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

#[test]
fn layout_shortcuts_save_drafts_and_preserve_unsaved_changes_and_dialogs() {
    let ctx = egui::Context::default();
    let directory = std::env::temp_dir().join(format!(
        "rs-button-builder-shortcuts-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let path = directory.join("layout.yaml");
    let mut app = ButtonBuilder::new(&ctx, Some(path.clone()), Config::default())
        .with_settings(SettingsStore::open(directory.join("settings.yaml")));
    app.layout_name_edit = Some("Saved by shortcut".into());
    hotkey(&mut app, &ctx, egui::Key::S, false, false);
    assert_eq!(
        Config::load_existing(&path).unwrap().display_name(),
        "Saved by shortcut"
    );
    assert!(!app.dirty && app.layout_name_edit.is_none());

    app.config.display_name = "Save as draft".into();
    app.dirty = true;
    hotkey(&mut app, &ctx, egui::Key::S, true, false);
    assert_eq!(
        app.save_as.as_ref().unwrap().config.display_name(),
        "Save as draft"
    );
    assert_eq!(
        Config::load_existing(&path).unwrap().display_name(),
        "Saved by shortcut"
    );
    hotkey(&mut app, &ctx, egui::Key::N, false, false);
    assert!(app.save_as.is_some() && !app.new_layout_requested);
    app.save_as = None;

    hotkey(&mut app, &ctx, egui::Key::N, false, false);
    assert!(app.new_layout_requested);
    assert_eq!(app.config.display_name(), "Save as draft");
    app.new_layout_requested = false;
    hotkey(&mut app, &ctx, egui::Key::O, false, false);
    assert!(app.config_picker.is_some());
    assert_eq!(app.config.display_name(), "Save as draft");
    let output = hotkey(&mut app, &ctx, egui::Key::Q, true, false);
    assert!(
        !output.viewport_output[&egui::ViewportId::ROOT]
            .commands
            .contains(&egui::ViewportCommand::Close)
    );
    app.config_picker = None;

    app.edit(0);
    hotkey(&mut app, &ctx, egui::Key::S, false, false);
    assert!(app.editor.is_some() && app.dirty);
    assert_eq!(
        Config::load_existing(&path).unwrap().display_name(),
        "Saved by shortcut"
    );
    app.editor = None;
    hotkey(&mut app, &ctx, egui::Key::S, false, false);
    assert!(!app.dirty);
    hotkey(&mut app, &ctx, egui::Key::N, false, false);
    assert!(app.config_path.is_none() && !app.new_layout_requested);
    assert_eq!(app.config.display_name(), DEFAULT_LAYOUT_NAME);
    let output = hotkey(&mut app, &ctx, egui::Key::Q, true, false);
    assert!(
        output.viewport_output[&egui::ViewportId::ROOT]
            .commands
            .contains(&egui::ViewportCommand::Close)
    );
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
    for label in [
        DEFAULT_LAYOUT_NAME,
        "Rows",
        "Columns",
        "Editing layout",
        "Done",
    ] {
        assert!(output.shapes.iter().all(|shape| !matches!(&shape.shape,
            egui::Shape::Text(text) if text.galley.job.text == label)));
    }
    click_app_action(&mut app, &ctx, &output, "Edit layout");
    assert!(app.edit_mode && !app.has_unsaved_changes());
    let output = frame(&mut app, &ctx, vec![]);
    label_center(&output, "Rows");
    label_center(&output, "Columns");
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
    assert_title(&output, "Button Builder - Workstation · Local *");
    assert_eq!(app.config.display_name, "Workstation · Local");
    assert!(app.dirty && app.layout_name_edit.is_none());
    assert!(
        app.workspace.tabs().is_empty(),
        "Renaming must not launch a process"
    );
    let output = frame(&mut app, &ctx, vec![]);
    label_center(&output, "Workstation · Local *");
    assert_no_title_change(&output);
    click_app_action(&mut app, &ctx, &output, "Save layout *");
    assert_eq!(
        Config::load_existing(&path).unwrap().display_name,
        "Workstation · Local"
    );

    let output = frame(&mut app, &ctx, vec![]);
    assert_no_title_change(&output);
    label_center(&output, "Workstation · Local");
    app.config.resize_grid(4, 3).unwrap();
    app.dirty = true;
    let output = frame(&mut app, &ctx, vec![]);
    assert_title(&output, "Button Builder - Workstation · Local *");
    label_center(&output, "Workstation · Local *");
    let output = hotkey(&mut app, &ctx, egui::Key::S, false, false);
    assert_title(&output, "Button Builder - Workstation · Local");
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
    click(&mut app, &ctx, label_center(&output, "Done"));
    assert_eq!(app.config.display_name, DEFAULT_LAYOUT_NAME);
    assert!(app.dirty && app.layout_name_edit.is_none() && !app.edit_mode);

    app.load_config(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/json-streams.yaml"))
        .unwrap();
    let output = frame(&mut app, &ctx, vec![]);
    assert_title(&output, "Button Builder - JSON Streams");
    assert!(!app.dirty && app.layout_name_edit.is_none());
    click_app_action(&mut app, &ctx, &output, "Edit layout");
    let output = frame(&mut app, &ctx, vec![]);
    label_center(&output, "JSON Streams");
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
    assert_eq!(app.workspace.tabs().len(), 2);
    assert_eq!(
        app.workspace
            .tabs()
            .iter()
            .map(|tab| (tab.label.as_str(), tab.button_index))
            .collect::<Vec<_>>(),
        [("Alpha", Some(0)), ("Beta", Some(1))]
    );
    wait_for_tabs(&mut app, &ctx, |app| {
        app.workspace
            .tabs()
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

    assert_eq!(app.workspace.tabs().len(), 3);
    assert!(
        app.workspace.tabs()[..2]
            .iter()
            .all(|tab| tab.button_index.is_none())
    );
    assert_eq!(app.workspace.tabs()[2].label, "Gamma");
    assert_eq!(app.workspace.tabs()[2].button_index, Some(0));
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
    assert!(
        app.workspace.tabs().is_empty(),
        "Saving must not execute the command"
    );
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
    assert!(!app.workspace.tabs().is_empty());
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while app.workspace.running() {
        assert!(std::time::Instant::now() < deadline);
        frame(&mut app, &ctx, vec![]);
        std::thread::sleep(Duration::from_millis(10));
    }
    frame(&mut app, &ctx, vec![]);
    assert!(
        app.workspace
            .active()
            .unwrap()
            .terminal
            .screen
            .text()
            .contains("hello from a button")
    );
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
        "rs-button-builder-footer-save-as-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
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
        app.workspace
            .tabs()
            .iter()
            .all(|tab| tab.terminal.screen.text().contains("-ready"))
    });
    for size in [Vec2::new(1200.0, 760.0), Vec2::new(850.0, 500.0)] {
        frame_with(&mut app, &ctx, size, egui::Modifiers::NONE, vec![]);
        let output = frame_with(&mut app, &ctx, size, egui::Modifiers::NONE, vec![]);
        let trigger = label_center(&output, "Button Builder");
        assert!(trigger.y > size.y - 40.0);
        let terminal_sizes: Vec<_> = app
            .workspace
            .tabs()
            .iter()
            .map(|tab| (tab.terminal.screen.cols, tab.terminal.screen.rows))
            .collect();
        click(&mut app, &ctx, trigger);
        let output = frame_with(&mut app, &ctx, size, egui::Modifiers::NONE, vec![]);
        let labels = [
            "New layout",
            "Load config…",
            "Edit layout",
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
            app.workspace
                .tabs()
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
    let active_id = app.workspace.active().unwrap().id();
    let text = app.workspace.tabs()[0].terminal.screen.text();
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
    assert_eq!(app.workspace.tabs().len(), 2);
    assert_eq!(app.workspace.active().unwrap().id(), active_id);
    assert_eq!(app.workspace.tabs()[0].terminal.screen.text(), text);
    assert!(
        app.workspace
            .tabs()
            .iter()
            .all(|tab| tab.terminal.is_running() && tab.button_index.is_none())
    );
    assert!(
        app.workspace
            .tabs()
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
    assert!(app.workspace.tabs().is_empty());
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
    assert!(
        app.workspace.tabs().is_empty(),
        "Loading must not execute a command"
    );
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
    app.workspace.tabs_mut()[0].terminal = terminal;
    app.load_config(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/terminal-demo.yaml"))
        .unwrap();
    assert!(app.workspace.running());
    assert_eq!(app.workspace.active().unwrap().terminal.status, status);
    assert!(
        app.workspace
            .active()
            .unwrap()
            .terminal
            .screen
            .text()
            .contains("Retained output")
    );
    assert!(app.workspace.active().unwrap().button_index.is_none() && app.editor.is_none());
    assert_eq!((app.config.rows, app.config.columns), (2, 2));
}
