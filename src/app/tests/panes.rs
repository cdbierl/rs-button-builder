//! Application panes tests.

use super::*;

#[test]
fn terminal_shortcuts_wrap_within_the_focused_pane_and_consume_repeats() {
    let ctx = egui::Context::default();
    let mut config = Config::default();
    for (index, label) in ["Alpha", "Beta", "Gamma"].into_iter().enumerate() {
        config.buttons[index] = interactive_button(label);
    }
    let mut app = ButtonBuilder::new(&ctx, None, config);
    for index in 0..3 {
        app.launch(index);
    }
    wait_for_tabs(&mut app, &ctx, |app| {
        app.workspace
            .tabs()
            .iter()
            .all(|tab| tab.terminal.screen.text().contains("-ready"))
    });
    app.workspace.split_terminal(2, Split::Right);
    app.workspace.select_tab(0);
    frame(&mut app, &ctx, vec![]);
    hotkey(&mut app, &ctx, egui::Key::PageUp, false, false);
    assert_eq!(app.workspace.active_index(), Some(1));
    hotkey(&mut app, &ctx, egui::Key::PageDown, false, false);
    assert_eq!(app.workspace.active_index(), Some(0));
    assert_eq!(app.workspace.focused_pane(), 0);
    assert_eq!(app.workspace.pane_tab(1), Some(2));
    assert!(!ctx.input(|input| {
        input
            .events
            .iter()
            .any(|event| matches!(event, egui::Event::Key { pressed: true, .. }))
    }));

    app.workspace.focus_pane(1);
    hotkey(&mut app, &ctx, egui::Key::PageDown, false, false);
    assert_eq!(
        app.workspace.active_index(),
        Some(2),
        "A single-tab pane stays selected"
    );
    hotkey(&mut app, &ctx, egui::Key::W, false, false);
    assert_eq!(
        app.workspace.tabs().len(),
        3,
        "Ctrl+W belongs to the terminal"
    );
    hotkey(&mut app, &ctx, egui::Key::W, true, false);
    assert_eq!(app.workspace.tabs().len(), 2);
    assert!(app.workspace.split().is_none());
    hotkey(&mut app, &ctx, egui::Key::W, true, true);
    assert_eq!(
        app.workspace.tabs().len(),
        2,
        "Holding Close must not close more tabs"
    );
    assert!(!ctx.input(|input| {
        input
            .events
            .iter()
            .any(|event| matches!(event, egui::Event::Key { pressed: true, .. }))
    }));

    hotkey(&mut app, &ctx, egui::Key::N, false, false);
    assert_eq!(
        app.workspace.tabs().len(),
        2,
        "New layouts preserve existing terminal sessions"
    );
    assert!(
        app.workspace
            .tabs()
            .iter()
            .all(|tab| tab.terminal.is_running())
    );
    for _ in 0..2 {
        hotkey(&mut app, &ctx, egui::Key::W, true, false);
    }
    hotkey(&mut app, &ctx, egui::Key::W, true, false);
    hotkey(&mut app, &ctx, egui::Key::PageUp, false, false);
    assert!(app.workspace.tabs().is_empty() && app.workspace.active_index().is_none());
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
    app.workspace.split_terminal(0, Split::Right);
    assert!(
        app.workspace.split().is_none(),
        "A split needs two existing terminals"
    );
    app.launch(1);
    wait_for_tabs(&mut app, &ctx, |app| {
        app.workspace
            .tabs()
            .iter()
            .all(|tab| tab.terminal.screen.text().contains("-ready"))
    });
    let sessions: Vec<_> = app
        .workspace
        .tabs()
        .iter()
        .map(|tab| (tab.id(), tab.terminal.status.clone()))
        .collect();
    app.workspace.select_tab(0);
    let menu = open_tab_menu(&mut app, &ctx, "2 · Beta");
    click(&mut app, &ctx, label_center(&menu, "Split right"));
    assert!(app.workspace.split() == Some(Split::Right));
    assert_eq!(app.workspace.pane_tab(0), Some(0));
    assert_eq!(app.workspace.pane_tab(1), Some(1));
    assert_eq!(app.workspace.focused_pane(), 1);
    assert_eq!(
        sessions,
        app.workspace
            .tabs()
            .iter()
            .map(|tab| (tab.id(), tab.terminal.status.clone()))
            .collect::<Vec<_>>()
    );
    frame(&mut app, &ctx, vec![]);
    let output = frame(&mut app, &ctx, vec![]);
    let alpha = terminal_output_position(&output, "Alpha-ready");
    let beta = terminal_output_position(&output, "Beta-ready");
    assert!(alpha.x < beta.x && (alpha.y - beta.y).abs() < 1.0);
    assert_eq!(
        app.workspace.tabs()[0].terminal.screen.cols,
        app.workspace.tabs()[1].terminal.screen.cols
    );
    assert!(app.workspace.tabs()[0].terminal.screen.cols < 80);
    click(&mut app, &ctx, alpha + Vec2::new(30.0, 30.0));
    assert_eq!(app.workspace.active_index(), Some(0));
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
        app.workspace.tabs()[0]
            .terminal
            .screen
            .text()
            .contains("Alpha:only-alpha")
    });
    assert!(
        !app.workspace.tabs()[1]
            .terminal
            .screen
            .text()
            .contains("only-alpha")
    );
    app.launch(2);
    assert_eq!(
        app.workspace.tabs()[2].pane(),
        0,
        "New tabs belong to the focused pane"
    );
    let menu = open_tab_menu(&mut app, &ctx, "3 · Gamma");
    frame(
        &mut app,
        &ctx,
        vec![egui::Event::PointerMoved(label_center(
            &menu,
            "Move to pane",
        ))],
    );
    let destinations = frame(&mut app, &ctx, vec![]);
    click(&mut app, &ctx, label_center(&destinations, "Pane 2 · Beta"));
    assert_eq!(app.workspace.tabs()[2].pane(), 1);
    assert_eq!(app.workspace.pane_tab(0), Some(0));
    assert_eq!(app.workspace.pane_tab(1), Some(2));
    app.launch(1);
    assert_eq!(
        app.workspace.tabs().len(),
        3,
        "Running buttons focus across panes without duplicating"
    );
    assert_eq!(app.workspace.active_index(), Some(1));
    assert_eq!(app.workspace.focused_pane(), 1);
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
    click(&mut app, &ctx, label_center(&menu, "Merge all panes"));
    assert!(app.workspace.split().is_none());
    assert!(
        app.workspace
            .tabs()
            .iter()
            .all(|tab| tab.pane() == 0 && tab.terminal.is_running())
    );
    assert_eq!(app.workspace.active_index(), Some(1));
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
        app.workspace
            .tabs()
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
        app.workspace.tabs()[0].terminal.screen.rows,
        app.workspace.tabs()[1].terminal.screen.rows,
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
    assert!(app.workspace.tabs()[0].terminal.screen.rows > rows[0]);
    assert!(app.workspace.tabs()[1].terminal.screen.rows < rows[1]);
    for direction in [Split::Right, Split::Below] {
        app.workspace.merge_terminals();
        app.workspace.split_terminal(1, direction);
        let output = frame_with(
            &mut app,
            &ctx,
            Vec2::new(850.0, 500.0),
            egui::Modifiers::NONE,
            vec![],
        );
        assert!(
            app.workspace
                .tabs()
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
    app.workspace.split_terminal(1, Split::Right);
    app.workspace.select_tab(0);
    app.workspace.close_tab(0);
    assert!(app.workspace.split().is_some());
    assert_eq!(
        app.workspace.active().unwrap().label,
        "Gamma",
        "Closing selects another tab in the same pane"
    );
    let gamma_id = app.workspace.active().unwrap().id();
    app.workspace.close_tab(0); // Beta was the only tab in the other pane.
    assert!(app.workspace.split().is_none());
    assert_eq!(app.workspace.active().unwrap().id(), gamma_id);
    assert!(app.workspace.active().unwrap().terminal.is_running());
    app.launch(1);
    app.workspace.split_terminal(1, Split::Below);
    let button = ButtonConfig {
        command: "/usr/bin/printf".into(),
        arguments: "success".into(),
        close_on_success: true,
        ..Default::default()
    };
    app.workspace.tabs_mut()[1].terminal = Terminal::start(&button, 80, 24).unwrap();
    app.workspace.select_tab(0);
    wait_for_tabs(&mut app, &ctx, |app| app.workspace.tabs().len() == 1);
    assert!(app.workspace.split().is_none());
    assert_eq!(app.workspace.active().unwrap().id(), gamma_id);
    assert_eq!(app.workspace.active().unwrap().pane(), 0);
    assert!(app.workspace.active().unwrap().terminal.is_running());
}

#[test]
fn nested_terminal_splits_resize_route_input_and_collapse_only_empty_sections() {
    use Split;
    let ctx = egui::Context::default();
    let mut config = Config::default();
    for (index, label) in ["Alpha", "Beta", "Gamma", "Delta"].into_iter().enumerate() {
        config.buttons[index] = interactive_button(label);
    }
    let mut app = ButtonBuilder::new(&ctx, None, config);
    for index in 0..4 {
        app.launch(index);
    }
    wait_for_tabs(&mut app, &ctx, |app| {
        app.workspace
            .tabs()
            .iter()
            .all(|tab| tab.terminal.screen.text().contains("-ready"))
    });
    let ids: Vec<_> = app.workspace.tabs().iter().map(|tab| tab.id()).collect();
    let menu = open_tab_menu(&mut app, &ctx, "2 · Beta");
    click(&mut app, &ctx, label_center(&menu, "Split right"));
    assert!(
        !app.workspace.can_split(1),
        "Other panes' tabs cannot satisfy a split"
    );
    app.workspace.split_terminal(1, Split::Below);
    assert_eq!(app.workspace.pane_ids().len(), 2);
    let menu = open_tab_menu(&mut app, &ctx, "4 · Delta");
    frame(
        &mut app,
        &ctx,
        vec![egui::Event::PointerMoved(label_center(
            &menu,
            "Move to pane",
        ))],
    );
    let menu = frame(&mut app, &ctx, vec![]);
    click(&mut app, &ctx, label_center(&menu, "Pane 2 · Beta"));
    let menu = open_tab_menu(&mut app, &ctx, "4 · Delta");
    click(&mut app, &ctx, label_center(&menu, "Split below"));
    let menu = open_tab_menu(&mut app, &ctx, "3 · Gamma");
    click(&mut app, &ctx, label_center(&menu, "Split below"));
    assert_eq!(app.workspace.pane_ids(), vec![0, 3, 1, 2]);
    assert_eq!(
        app.workspace
            .tabs()
            .iter()
            .map(|tab| tab.id())
            .collect::<Vec<_>>(),
        ids
    );
    let output = frame(&mut app, &ctx, vec![]);
    let positions: Vec<_> = ["Alpha", "Beta", "Gamma", "Delta"]
        .iter()
        .map(|label| terminal_output_position(&output, &format!("{label}-ready")))
        .collect();
    assert!(positions[0].x < positions[1].x && positions[0].y < positions[2].y);
    assert_eq!(positions[0].x, positions[2].x);
    assert_eq!(positions[1].x, positions[3].x);
    assert!(positions[1].y < positions[3].y);
    for size in [Vec2::new(850.0, 500.0), Vec2::new(1200.0, 760.0)] {
        let output = frame_with(&mut app, &ctx, size, egui::Modifiers::NONE, vec![]);
        for label in ["Alpha", "Beta", "Gamma", "Delta"] {
            let prefix = format!("{label}-ready");
            let position = terminal_output_position(&output, &prefix);
            assert!(output.shapes.iter().any(|shape| shape.clip_rect.contains(position)
                && matches!(&shape.shape, egui::Shape::Text(text) if text.galley.job.text.starts_with(&prefix))),
                "Terminal text should stay within its pane at {size:?}");
        }
    }
    let sizes: Vec<_> = app
        .workspace
        .tabs()
        .iter()
        .map(|tab| (tab.terminal.screen.cols, tab.terminal.screen.rows))
        .collect();
    let divider = output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::LineSegment { points, stroke }
                if points[0].x > positions[1].x - 30.0
                    && points[0].y == points[1].y
                    && points[1].x - points[0].x > 200.0
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
            primary_pointer(divider, true),
        ],
    );
    let destination = divider + Vec2::new(0.0, 35.0);
    frame(&mut app, &ctx, vec![egui::Event::PointerMoved(destination)]);
    frame(&mut app, &ctx, vec![primary_pointer(destination, false)]);
    frame(&mut app, &ctx, vec![]);
    assert_eq!(
        sizes[0],
        (
            app.workspace.tabs()[0].terminal.screen.cols,
            app.workspace.tabs()[0].terminal.screen.rows
        )
    );
    assert_eq!(
        sizes[2],
        (
            app.workspace.tabs()[2].terminal.screen.cols,
            app.workspace.tabs()[2].terminal.screen.rows
        )
    );
    assert!(app.workspace.tabs()[1].terminal.screen.rows > sizes[1].1);
    assert!(app.workspace.tabs()[3].terminal.screen.rows < sizes[3].1);
    let output = frame(&mut app, &ctx, vec![]);
    let gamma = terminal_output_position(&output, "Gamma-ready");
    click(&mut app, &ctx, gamma + Vec2::new(20.0, 20.0));
    frame(
        &mut app,
        &ctx,
        vec![
            egui::Event::Text("nested-input".into()),
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
        app.workspace.tabs()[2]
            .terminal
            .screen
            .text()
            .contains("Gamma:nested-input")
    });
    assert!(
        app.workspace
            .tabs()
            .iter()
            .enumerate()
            .all(|(index, tab)| index == 2 || !tab.terminal.screen.text().contains("nested-input"))
    );
    app.launch(2);
    assert_eq!(app.workspace.tabs().len(), 4);
    assert_eq!(app.workspace.focused_pane(), 3);
    // Removing the top-left leaf must preserve the independently resized right split.
    app.workspace.move_terminal(0, 2);
    assert_eq!(app.workspace.pane_ids(), vec![3, 1, 2]);
    app.workspace.close_tab(2); // Gamma, the only tab in the left subtree.
    assert_eq!(app.workspace.pane_ids(), vec![1, 2]);
    assert_eq!(app.workspace.active().unwrap().id(), ids[0]);
    assert_eq!(app.workspace.split(), Some(Split::Below));
    let before = app
        .workspace
        .tabs()
        .iter()
        .map(|tab| tab.id())
        .collect::<Vec<_>>();
    let button = ButtonConfig {
        command: "/usr/bin/printf".into(),
        arguments: "success".into(),
        close_on_success: true,
        ..Default::default()
    };
    app.workspace.tabs_mut()[1].terminal = Terminal::start(&button, 80, 24).unwrap();
    wait_for_tabs(&mut app, &ctx, |app| app.workspace.tabs().len() == 2);
    assert_eq!(
        app.workspace.pane_ids(),
        vec![2],
        "Promoting a sibling preserves its pane ID"
    );
    assert_eq!(app.workspace.active().unwrap().id(), ids[0]);
    assert!(
        app.workspace
            .tabs()
            .iter()
            .all(|tab| tab.terminal.is_running())
    );
    app.workspace.merge_terminals();
    assert_eq!(app.workspace.pane_ids(), vec![0]);
    assert_eq!(
        app.workspace
            .tabs()
            .iter()
            .map(|tab| tab.id())
            .collect::<Vec<_>>(),
        vec![before[0], before[2]]
    );
}
