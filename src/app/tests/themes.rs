//! Application themes tests.

use super::*;

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
    wait_for_tabs(&mut app, &ctx, |app| {
        !app.workspace.tabs()[0].terminal.needs_poll()
    });
    app.workspace.tabs_mut()[0]
        .terminal
        .screen
        .feed(b"\x1b[2J\x1b[H");
    for _ in 0..100 {
        app.workspace.tabs_mut()[0]
            .terminal
            .screen
            .feed(b"\x1b[32mansi\x1b[38;2;205;49;49mexact\x1b[0mplain\r\n");
    }
    app.workspace.tabs_mut()[0].scrollback = app.workspace.tabs()[0].terminal.screen.history.len();
    let original = serde_yaml_ng::to_string(&app.config).unwrap();
    let scrollback = app.workspace.tabs()[0].scrollback;
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
                        Some(text.fallback_color)
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
        assert_eq!(app.workspace.tabs()[0].scrollback, scrollback);
    }
    assert!(!app.dirty);
    assert_eq!(serde_yaml_ng::to_string(&app.config).unwrap(), original);
    app.workspace.tabs_mut()[0].style.fg_color = Some([210, 220, 230]);
    app.workspace.tabs_mut()[0].style.bg_color = Some([5, 10, 15]);
    app.theme.mode = Appearance::Light;
    let output = frame(&mut app, &ctx, vec![]);
    check_colors(
        &output,
        &Palette::new(false, Some([210, 220, 230]), Some([5, 10, 15])),
    );

    let font_size = app.workspace.tabs()[0].style.font_size;
    app.appearance_open = true;
    frame(&mut app, &ctx, vec![]);
    let output = frame(&mut app, &ctx, vec![]);
    click(&mut app, &ctx, label_center(&output, "Use theme colors"));
    assert_eq!(app.workspace.tabs()[0].style.fg_color, None);
    assert_eq!(app.workspace.tabs()[0].style.bg_color, None);
    assert_eq!(app.config.terminal.fg_color, None);
    assert_eq!(app.config.terminal.bg_color, None);
    assert_eq!(app.workspace.tabs()[0].style.font_size, font_size);
    assert!(app.dirty);
    app.appearance_open = false;
    let output = frame(&mut app, &ctx, vec![]);
    check_colors(&output, &Palette::new(false, None, None));
    crate::ui::setup(&ctx);
}

#[test]
fn footer_theme_change_recolors_a_running_terminal() {
    use crate::terminal::colors::Palette;

    let ctx = egui::Context::default();
    let mut config = Config::default();
    config.buttons[0] = interactive_button("Theme terminal");
    let mut app = ButtonBuilder::new(&ctx, None, config);
    app.launch(0);
    wait_for_tabs(&mut app, &ctx, |app| {
        app.workspace.tabs()[0]
            .terminal
            .screen
            .text()
            .contains("Theme terminal-ready")
    });

    let select_theme =
        |app: &mut ButtonBuilder, ctx: &egui::Context, label: &str| -> egui::FullOutput {
            let output = frame(app, ctx, vec![]);
            let palette = egui_material3::material_symbol::material_symbol_text("palette");
            click(app, ctx, label_center(&output, &palette));
            let menu = frame(app, ctx, vec![]);
            click(app, ctx, label_center(&menu, label));
            let themed = frame(app, ctx, vec![]);
            click(app, ctx, label_center(&themed, &palette));
            frame(app, ctx, vec![])
        };
    let terminal_galley = |output: &egui::FullOutput, expected: &Palette| {
        assert!(output.shapes.iter().any(|shape| {
            matches!(&shape.shape, egui::Shape::Rect(rect)
                if rect.rect.width() > 300.0 && rect.fill == rgb(expected.background))
        }));
        let foreground = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(text)
                    if text.galley.job.text.starts_with("Theme terminal-ready") =>
                {
                    Some((text.galley.clone(), text.fallback_color))
                }
                _ => None,
            })
            .unwrap();
        assert_eq!(foreground.1, rgb(expected.foreground));
        foreground.0
    };

    let light = select_theme(&mut app, &ctx, "Light");
    let expected = Palette::new(false, None, None);
    let light_galley = terminal_galley(&light, &expected);
    let dark = select_theme(&mut app, &ctx, "Dark");
    let expected = Palette::new(true, None, None);
    let dark_galley = terminal_galley(&dark, &expected);
    assert!(!std::sync::Arc::ptr_eq(&light_galley, &dark_galley));
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
