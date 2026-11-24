//! Primary actions shared by menus and keyboard shortcuts.

use super::LayoutFilePicker;
use crate::app::{ButtonBuilder, SaveAs};
use egui::{Key, KeyboardShortcut, Modifiers};

#[derive(Clone, Copy)]
pub(super) enum Action {
    NewLayout,
    LoadConfig,
    EditLayout,
    ToggleButtons,
    ToggleFullscreen,
    SaveLayout,
    SaveLayoutAs,
    Exit,
    CloseTab,
    PreviousTab,
    NextTab,
}

impl Action {
    pub(super) fn shortcut(self) -> KeyboardShortcut {
        let (modifiers, key) = match self {
            Self::NewLayout => (Modifiers::COMMAND, Key::N),
            Self::LoadConfig => (Modifiers::COMMAND, Key::O),
            Self::EditLayout => (Modifiers::COMMAND, Key::E),
            Self::ToggleButtons => (Modifiers::COMMAND, Key::B),
            Self::ToggleFullscreen => (Modifiers::COMMAND, Key::F11),
            Self::SaveLayout => (Modifiers::COMMAND, Key::S),
            Self::SaveLayoutAs => (Modifiers::COMMAND | Modifiers::SHIFT, Key::S),
            Self::Exit => (Modifiers::COMMAND | Modifiers::SHIFT, Key::Q),
            Self::CloseTab => (Modifiers::COMMAND | Modifiers::SHIFT, Key::W),
            Self::PreviousTab => (Modifiers::CTRL, Key::PageUp),
            Self::NextTab => (Modifiers::CTRL, Key::PageDown),
        };
        KeyboardShortcut::new(modifiers, key)
    }
}

impl ButtonBuilder {
    pub(in crate::app) fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        let actions = ctx.input_mut(|input| {
            let mut actions = Vec::new();
            input.events.retain(|event| {
                let egui::Event::Key {
                    key,
                    modifiers,
                    pressed: true,
                    repeat,
                    ..
                } = event
                else {
                    return true;
                };
                let action = [
                    Action::NewLayout,
                    Action::LoadConfig,
                    Action::EditLayout,
                    Action::ToggleButtons,
                    Action::ToggleFullscreen,
                    Action::SaveLayout,
                    Action::SaveLayoutAs,
                    Action::Exit,
                    Action::CloseTab,
                    Action::PreviousTab,
                    Action::NextTab,
                ]
                .into_iter()
                .find(|action| {
                    let shortcut = action.shortcut();
                    *key == shortcut.logical_key && modifiers.matches_exact(shortcut.modifiers)
                });
                let Some(action) = action else {
                    return true;
                };
                // Consume repeats too, so holding Close never closes several tabs
                // and handled shortcuts never reach the PTY or text fields.
                if !repeat {
                    actions.push(action);
                }
                false
            });
            actions
        });
        for action in actions {
            if self.editor.is_none()
                && !self.appearance_open
                && self.layout_file_picker.is_none()
                && self.save_as.is_none()
                && !self.new_layout_requested
            {
                self.perform_action(ctx, action);
            }
        }
    }

    pub(super) fn perform_action(&mut self, ctx: &egui::Context, action: Action) {
        match action {
            Action::NewLayout => self.request_new_layout(),
            Action::LoadConfig => {
                self.layout_file_picker = Some(LayoutFilePicker::new(self.config_path.as_deref()));
            }
            Action::EditLayout => {
                if self.edit_mode {
                    self.apply_layout_name();
                }
                self.edit_mode = !self.edit_mode;
                self.dragged_button = None;
            }
            Action::ToggleButtons => self.buttons_panel_visible = !self.buttons_panel_visible,
            Action::ToggleFullscreen => {
                self.fullscreen = !self.fullscreen;
                ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(self.fullscreen));
            }
            Action::SaveLayout => self.save(),
            Action::SaveLayoutAs => {
                self.apply_layout_name();
                self.save_as = Some(SaveAs {
                    file_picker: LayoutFilePicker::save(self.config_path.as_deref()),
                    config: self.config.clone(),
                    close_editor: false,
                    create_new_layout: false,
                });
            }
            Action::Exit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            Action::CloseTab => {
                if let Some(index) = self.workspace.active_index() {
                    self.workspace.close_tab(index);
                }
            }
            Action::PreviousTab => self.workspace.cycle_tab(false),
            Action::NextTab => self.workspace.cycle_tab(true),
        }
    }
}
