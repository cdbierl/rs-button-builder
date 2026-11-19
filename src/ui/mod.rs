//! UI sections and shared Material styling.

mod app_menu;
mod appearance;
mod button;
mod button_editor;
mod button_grid;
mod checkbox;
mod footer;
mod layout_dialogs;
mod shortcuts;
mod style;
mod terminal_panel;
mod terminal_scrollbar;
mod terminal_selection;
mod terminal_tabs;
mod terminal_toolbar;
mod terminal_viewport;
mod terminal_workspace;

pub(crate) use button::Button;
pub(crate) use checkbox::Checkbox;
pub(crate) use style::{
    CORNER_RADIUS, dialog, dialog_footer, filled_button, font_picker, form, outlined_button, setup,
    text_button, text_input,
};

use style::styled_button;
pub(super) use style::{button_colors, rgb};
pub(super) use terminal_panel::TerminalTextCache;
pub(super) use terminal_selection::TerminalSelection;

#[cfg(test)]
pub(super) use terminal_panel::key_bytes;
#[cfg(test)]
pub(super) use terminal_scrollbar::TERMINAL_SCROLLBAR_WIDTH;
