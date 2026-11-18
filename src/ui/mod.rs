//! UI sections and shared Material styling.

mod appearance;
mod button;
mod button_editor;
mod button_grid;
mod footer;
mod header;
mod layout_dialogs;
mod style;
mod terminal_panel;
mod terminal_selection;
mod terminal_tabs;
mod terminal_toolbar;

pub(crate) use button::Button;
pub(crate) use style::{
    CORNER_RADIUS, dialog, dialog_footer, filled_button, font_picker, form, material_icon,
    outlined_button, setup, text_button, text_input,
};

use style::styled_button;
pub(super) use style::{button_colors, rgb};
pub(super) use terminal_selection::TerminalSelection;

#[cfg(test)]
pub(super) use terminal_panel::{TERMINAL_SCROLLBAR_WIDTH, key_bytes};
