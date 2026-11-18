//! Terminal viewport rendering, scrollback, resizing, and keyboard input.

use super::terminal_selection::{SelectionViewport, live_row};
use super::{CORNER_RADIUS, rgb};
use crate::app::{ButtonBuilder, TerminalTab};
use crate::layout_config::TerminalStyle;
use crate::terminal::screen::Cell;
use egui::{FontFamily, FontId, Stroke, Vec2};
use egui_material3::get_global_color;

// Reserve the gutter even without history so the PTY width stays stable as output arrives.
pub(in crate::app) const TERMINAL_SCROLLBAR_WIDTH: f32 = 20.0;

impl ButtonBuilder {
    pub(in crate::app) fn terminal_panel(&mut self, ui: &mut egui::Ui) {
        self.terminal_tabs(ui);
        self.terminal_toolbar(ui);
        ui.add_space(4.0);
        let size = ui.available_size().max(Vec2::splat(1.0));
        let id = self.active().map(|tab| tab.id);
        let (response, painter) = ui
            .push_id(("terminal_pane", id), |ui| {
                ui.allocate_painter(size, egui::Sense::click())
            })
            .inner;
        let control_scroll = ui.input(|input| {
            input.modifiers.ctrl
                || input.events.iter().any(|event| {
                    matches!(event, egui::Event::MouseWheel { modifiers, .. } if modifiers.ctrl)
                })
        });
        let pane_hovered = ui.rect_contains_pointer(response.rect);
        if pane_hovered && control_scroll {
            // Read wheel events directly: egui turns Ctrl+wheel into smoothed zoom input.
            // One wheel notch changes the font by 1 pt; pixel scrolling scales continuously.
            let delta: f32 = ui.input(|input| {
                input
                    .events
                    .iter()
                    .filter_map(|event| match event {
                        egui::Event::MouseWheel {
                            unit,
                            delta,
                            modifiers,
                        } if modifiers.ctrl => Some(match unit {
                            egui::MouseWheelUnit::Point => delta.y / 40.0,
                            egui::MouseWheelUnit::Line | egui::MouseWheelUnit::Page => delta.y,
                        }),
                        _ => None,
                    })
                    .sum()
            });
            let style = match self.active_tab {
                Some(index) => &mut self.tabs[index].style,
                None => &mut self.config.terminal,
            };
            let font_size = (style.font_size + delta).clamp(8.0, 32.0);
            if font_size != style.font_size {
                style.font_size = font_size;
                self.config.terminal = style.clone();
                self.dirty = true;
            }
        }
        let style = match self.active_tab {
            Some(index) => &self.tabs[index].style,
            None => &self.config.terminal,
        };
        let font = terminal_font(style, &self.fonts);
        let foreground = rgb(style.fg_color);
        let (char_width, line_height) = ui.fonts_mut(|fonts| {
            (
                fonts.glyph_width(&font, 'M'),
                fonts.row_height(&font) * style.line_spacing,
            )
        });
        let content_width = (size.x - 16.0 - TERMINAL_SCROLLBAR_WIDTH).max(0.0);
        let cols = ((content_width / char_width) as usize).clamp(2, 500);
        let rows = (((size.y - 16.0) / line_height) as usize).clamp(2, 200);
        self.terminal_size = (cols, rows);
        let rect = response.rect;
        painter.rect_filled(rect, CORNER_RADIUS, rgb(style.bg_color));
        // Inactive processes also receive viewport changes, using their own font metrics.
        for tab in &mut self.tabs {
            let tab_font = terminal_font(&tab.style, &self.fonts);
            let (width, height) = ui.fonts_mut(|fonts| {
                (
                    fonts.glyph_width(&tab_font, 'M'),
                    fonts.row_height(&tab_font) * tab.style.line_spacing,
                )
            });
            tab.resize(
                ((content_width / width) as usize).clamp(2, 500),
                (((size.y - 16.0) / height) as usize).clamp(2, 200),
            );
        }
        let mut scrollbar_interacted = false;
        if let Some(index) = self.active_tab {
            let tab = &mut self.tabs[index];
            let screen = &tab.terminal.screen;
            let history_len = if screen.is_alternate() {
                0
            } else {
                screen.history.len()
            };
            if pane_hovered && !control_scroll {
                let delta = ui.input(|input| input.smooth_scroll_delta.y);
                tab.scroll_fraction += delta / line_height;
                let lines = tab.scroll_fraction.trunc() as isize;
                tab.scroll_fraction -= lines as f32;
                tab.scrollback = tab.scrollback.saturating_add_signed(lines);
            }
            tab.scrollback = tab.scrollback.min(history_len);
            scrollbar_interacted =
                terminal_scrollbar(ui, &response, rows, history_len, &mut tab.scrollback);
            if scrollbar_interacted {
                tab.scroll_fraction = 0.0;
            }
        }
        let origin = rect.min + Vec2::splat(8.0);
        let text_rect = egui::Rect::from_min_max(
            origin,
            egui::pos2(
                (rect.right() - TERMINAL_SCROLLBAR_WIDTH - 8.0).max(origin.x),
                (rect.bottom() - 8.0).max(origin.y),
            ),
        );
        let selection_response = ui.interact(
            text_rect,
            response.id.with("selection"),
            egui::Sense::click_and_drag(),
        );
        if let Some(index) = self.active_tab {
            let tab = &mut self.tabs[index];
            let viewport = SelectionViewport {
                rect: text_rect,
                char_width,
                line_height,
                first_row: live_row(&tab.terminal.screen).saturating_sub(tab.scrollback),
            };
            tab.selection
                .interact(ui, &selection_response, &viewport, &tab.terminal.screen);
        }
        if response.clicked()
            || selection_response.clicked()
            || selection_response.drag_started()
            || scrollbar_interacted
            || self.focus_terminal
        {
            response.request_focus();
            self.focus_terminal = false;
        }
        if response.has_focus() {
            ui.memory_mut(|memory| {
                memory.set_focus_lock_filter(
                    response.id,
                    egui::EventFilter {
                        tab: true,
                        horizontal_arrows: true,
                        vertical_arrows: true,
                        escape: true,
                    },
                )
            });
            painter.rect_stroke(
                rect,
                CORNER_RADIUS,
                Stroke::new(1.0_f32, get_global_color("primary")),
                egui::StrokeKind::Inside,
            );
        }
        let painter = painter.with_clip_rect(text_rect);
        let Some(index) = self.active_tab else {
            painter.text(
                origin,
                egui::Align2::LEFT_TOP,
                "No process output yet.",
                font,
                foreground,
            );
            return;
        };
        let tab = &mut self.tabs[index];
        let style = &tab.style;
        let terminal = &mut tab.terminal;
        let screen = &terminal.screen;
        let history_len = if screen.is_alternate() {
            0
        } else {
            screen.history.len()
        };
        let start = history_len.saturating_sub(tab.scrollback);
        for visible_row in 0..rows {
            let index = start + visible_row;
            let line = if index < history_len {
                &screen.history[index]
            } else {
                screen.line(index - history_len)
            };
            paint_line(
                &painter,
                origin + Vec2::new(0.0, visible_row as f32 * line_height),
                line,
                &font,
                char_width,
                line_height,
                cols,
                style,
                tab.selection.columns(
                    live_row(screen).saturating_sub(tab.scrollback) + visible_row,
                    line,
                ),
                ui.visuals().selection,
            );
        }
        if tab.scrollback == 0
            && screen.cursor_visible
            && response.has_focus()
            && terminal.is_running()
        {
            let (x, y) = screen.cursor();
            let cursor = egui::Rect::from_min_size(
                origin + Vec2::new(x as f32 * char_width, y as f32 * line_height),
                Vec2::new(char_width, line_height),
            );
            painter.rect_stroke(
                cursor,
                0.0,
                Stroke::new(1.0_f32, rgb(style.fg_color)),
                egui::StrokeKind::Inside,
            );
        }
        if response.has_focus() {
            handle_terminal_input(ui, tab);
        }
    }
}

fn handle_terminal_input(ui: &egui::Ui, tab: &mut TerminalTab) {
    let events = ui.input(|input| input.events.clone());
    let copy_output = ui.input(|input| input.modifiers.ctrl && input.modifiers.shift)
        && events.iter().any(|event| {
            matches!(
                event,
                egui::Event::Copy
                    | egui::Event::Key {
                        key: egui::Key::C,
                        pressed: true,
                        ..
                    }
            )
        });
    if copy_output {
        ui.ctx().copy_text(tab.copy_text());
    }
    let copied = events
        .iter()
        .any(|event| matches!(event, egui::Event::Copy));
    let cut = events.iter().any(|event| matches!(event, egui::Event::Cut));
    for event in events {
        let bytes = match event {
            egui::Event::Copy if !copy_output => vec![3],
            egui::Event::Cut => vec![24],
            egui::Event::Text(text) => text.into_bytes(),
            egui::Event::Paste(text) => text.replace("\r\n", "\n").replace('\n', "\r").into_bytes(),
            egui::Event::Key {
                key,
                pressed: true,
                modifiers,
                ..
            } => {
                if (key == egui::Key::C && (copied || copy_output)) || (key == egui::Key::X && cut)
                {
                    continue;
                }
                key_bytes(key, modifiers, tab.terminal.screen.application_cursor)
            }
            _ => Vec::new(),
        };
        if !bytes.is_empty() {
            if bytes == [3] {
                tab.terminal.interrupt();
            } else {
                tab.terminal.send(&bytes);
            }
            tab.scrollback = 0;
            tab.selection.clear();
        }
    }
}

fn terminal_font(style: &TerminalStyle, fonts: &[String]) -> FontId {
    let family = if style.font == "Monospace" || !fonts.contains(&style.font) {
        FontFamily::Monospace
    } else {
        FontFamily::Name(style.font.clone().into())
    };
    FontId::new(style.font_size, family)
}

/// Map the full scrollback range onto a proportional thumb, keeping zero at live output.
fn terminal_scrollbar(
    ui: &mut egui::Ui,
    pane: &egui::Response,
    rows: usize,
    history_len: usize,
    scrollback: &mut usize,
) -> bool {
    let id = pane.id.with("scrollbar");
    let grab_id = id.with("grab_offset");
    if history_len == 0
        || pane.rect.height() <= 16.0
        || pane.rect.width() <= TERMINAL_SCROLLBAR_WIDTH
    {
        ui.ctx().data_mut(|data| data.remove::<f32>(grab_id));
        return false;
    }
    let gutter = egui::Rect::from_min_max(
        egui::pos2(
            pane.rect.right() - TERMINAL_SCROLLBAR_WIDTH,
            pane.rect.top() + 8.0,
        ),
        egui::pos2(pane.rect.right(), pane.rect.bottom() - 8.0),
    );
    let track = gutter.shrink2(Vec2::new(4.0, 0.0));
    let thumb_height = (track.height() * rows as f32 / (history_len + rows) as f32)
        .max(24.0)
        .min(track.height());
    let travel = track.height() - thumb_height;
    let thumb_rect = |scrollback: usize| {
        egui::Rect::from_min_size(
            track.min
                + Vec2::new(
                    0.0,
                    travel * (history_len - scrollback) as f32 / history_len as f32,
                ),
            Vec2::new(track.width(), thumb_height),
        )
    };
    let response = ui.interact(gutter, id, egui::Sense::click_and_drag());
    let interacting =
        response.is_pointer_button_down_on() || response.dragged() || response.clicked();
    if interacting && let Some(pointer) = response.interact_pointer_pos() {
        let thumb = thumb_rect(*scrollback);
        // Preserve where the thumb was grabbed; clicking the track centers it at the pointer.
        let grab_offset = ui.ctx().data_mut(|data| {
            *data.get_temp_mut_or_insert_with(grab_id, || {
                if thumb.contains(pointer) {
                    pointer.y - thumb.top()
                } else {
                    thumb_height / 2.0
                }
            })
        });
        if travel > 0.0 {
            let fraction = ((pointer.y - grab_offset - track.top()) / travel).clamp(0.0, 1.0);
            *scrollback = history_len - (fraction * history_len as f32).round() as usize;
        }
    }
    if !ui.input(|input| input.pointer.any_down()) {
        ui.ctx().data_mut(|data| data.remove::<f32>(grab_id));
    }
    let color = if response.hovered() || interacting {
        get_global_color("primary")
    } else {
        get_global_color("outline")
    };
    if response.hovered() || interacting {
        ui.ctx().set_cursor_icon(if interacting {
            egui::CursorIcon::Grabbing
        } else {
            egui::CursorIcon::Grab
        });
    }
    ui.painter().rect_filled(
        track,
        CORNER_RADIUS,
        get_global_color("surfaceContainerHighest"),
    );
    ui.painter()
        .rect_filled(thumb_rect(*scrollback), CORNER_RADIUS, color);
    interacting
}

#[allow(clippy::too_many_arguments)]
fn paint_line(
    painter: &egui::Painter,
    origin: egui::Pos2,
    line: &[Cell],
    font: &FontId,
    char_width: f32,
    line_height: f32,
    cols: usize,
    terminal_style: &TerminalStyle,
    selected: std::ops::Range<usize>,
    selection_style: egui::style::Selection,
) {
    let mut start = 0;
    while start < line.len().min(cols) {
        let style = line[start].style;
        let mut end = start + 1;
        let is_selected = selected.contains(&start);
        while end < line.len().min(cols)
            && line[end].style == style
            && selected.contains(&end) == is_selected
        {
            end += 1;
        }
        let mut foreground = rgb(style.fg_color.unwrap_or(terminal_style.fg_color));
        let mut background = rgb(style.bg_color.unwrap_or(terminal_style.bg_color));
        if style.inverse {
            std::mem::swap(&mut foreground, &mut background);
        }
        if is_selected {
            background = selection_style.bg_fill;
            foreground = selection_style.stroke.color;
        }
        let position = origin + Vec2::new(start as f32 * char_width, 0.0);
        let width = (end - start) as f32 * char_width;
        let rect = egui::Rect::from_min_size(position, Vec2::new(width, line_height));
        if background != rgb(terminal_style.bg_color) {
            painter.rect_filled(rect, 0.0, background);
        }
        let text: String = line[start..end]
            .iter()
            .filter(|cell| !cell.continuation)
            .map(|cell| cell.text.as_str())
            .collect();
        painter.text(
            position,
            egui::Align2::LEFT_TOP,
            &text,
            font.clone(),
            foreground,
        );
        if style.bold {
            painter.text(
                position + Vec2::new(0.5, 0.0),
                egui::Align2::LEFT_TOP,
                &text,
                font.clone(),
                foreground,
            );
        }
        if style.underline {
            painter.line_segment(
                [rect.left_bottom(), rect.right_bottom()],
                Stroke::new(1.0_f32, foreground),
            );
        }
        start = end;
    }
}

pub(in crate::app) fn key_bytes(
    key: egui::Key,
    modifiers: egui::Modifiers,
    application_cursor: bool,
) -> Vec<u8> {
    use egui::Key;
    if modifiers.ctrl {
        let letter = match key {
            Key::A => 1,
            Key::B => 2,
            Key::C => 3,
            Key::D => 4,
            Key::E => 5,
            Key::F => 6,
            Key::G => 7,
            Key::H => 8,
            Key::I => 9,
            Key::J => 10,
            Key::K => 11,
            Key::L => 12,
            Key::M => 13,
            Key::N => 14,
            Key::O => 15,
            Key::P => 16,
            Key::Q => 17,
            Key::R => 18,
            Key::S => 19,
            Key::T => 20,
            Key::U => 21,
            Key::V => return Vec::new(),
            Key::W => 23,
            Key::X => 24,
            Key::Y => 25,
            Key::Z => 26,
            Key::Space => 0,
            Key::OpenBracket => 27,
            Key::Backslash => 28,
            Key::CloseBracket => 29,
            _ => return Vec::new(),
        };
        return vec![letter];
    }
    let arrow_prefix = if application_cursor { "\x1bO" } else { "\x1b[" };
    match key {
        Key::ArrowUp => format!("{arrow_prefix}A").into_bytes(),
        Key::ArrowDown => format!("{arrow_prefix}B").into_bytes(),
        Key::ArrowRight => format!("{arrow_prefix}C").into_bytes(),
        Key::ArrowLeft => format!("{arrow_prefix}D").into_bytes(),
        Key::Enter => vec![b'\r'],
        Key::Tab => vec![b'\t'],
        Key::Backspace => vec![0x7f],
        Key::Escape => vec![0x1b],
        Key::Home => b"\x1b[H".to_vec(),
        Key::End => b"\x1b[F".to_vec(),
        Key::Insert => b"\x1b[2~".to_vec(),
        Key::Delete => b"\x1b[3~".to_vec(),
        Key::PageUp => b"\x1b[5~".to_vec(),
        Key::PageDown => b"\x1b[6~".to_vec(),
        Key::F1 => b"\x1bOP".to_vec(),
        Key::F2 => b"\x1bOQ".to_vec(),
        Key::F3 => b"\x1bOR".to_vec(),
        Key::F4 => b"\x1bOS".to_vec(),
        _ => Vec::new(),
    }
}
