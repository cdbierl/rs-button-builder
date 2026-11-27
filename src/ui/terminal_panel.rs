//! Terminal viewport rendering, scrollback, resizing, and keyboard input.

use super::terminal_scrollbar::{Scrollbar, TERMINAL_SCROLLBAR_WIDTH};
use super::terminal_search::{SearchAction, SearchMatch};
use super::terminal_selection::SelectionViewport;
use super::terminal_viewport::TerminalViewport;
use super::{CORNER_RADIUS, rgb};
use crate::app::ButtonBuilder;
use crate::app::workspace::TerminalTab;
use crate::layout_config::TerminalStyle;
use crate::terminal::colors::Palette;
use crate::terminal::screen::Cell;
use egui::{FontFamily, FontId, Stroke, Vec2};
use egui_material3::get_global_color;
use std::sync::Arc;

const TERMINAL_PADDING: f32 = 12.0;

// Retain only the last visible viewport for each tab. egui evicts layouts that were not
// drawn in the previous frame, so its own cache cannot preserve hidden terminal text.
#[derive(Default)]
pub(in crate::app) struct TerminalTextCache {
    key: Option<(FontId, f32, bool)>,
    atlas_reference: Option<Arc<egui::Galley>>,
    rows: Vec<Vec<Arc<egui::Galley>>>,
}

impl TerminalTextCache {
    fn prepare(&mut self, painter: &egui::Painter, font: &FontId, rows: usize, dark: bool) {
        // Keep one shared layout alive in egui's cache each frame. Its identity changes
        // when egui rebuilds the font atlas, invalidating the texture coordinates in our
        // retained layouts. The explicit key handles theme and font changes.
        let reference = painter.layout_no_wrap(
            "M".into(),
            FontId::monospace(12.0),
            egui::Color32::PLACEHOLDER,
        );
        let key = (font.clone(), painter.ctx().pixels_per_point(), dark);
        if self.key.as_ref() != Some(&key)
            || self
                .atlas_reference
                .as_ref()
                .is_none_or(|previous| !Arc::ptr_eq(previous, &reference))
        {
            self.rows.clear();
            self.key = Some(key);
        }
        self.atlas_reference = Some(reference);
        self.rows.resize_with(rows, Vec::new);
    }
}

impl ButtonBuilder {
    pub(in crate::app) fn terminal_panel(&mut self, ui: &mut egui::Ui, pane: usize) {
        self.terminal_tabs(ui, pane);
        self.terminal_toolbar(ui, pane);
        let active = self.workspace.pane_tab(pane);
        ui.add_space(4.0);
        let available = ui.available_size();
        let size = Vec2::new(super::clipped_available_width(ui), available.y.max(1.0));
        let id = active.map(|index| self.workspace.tabs()[index].id());
        let (response, _) = ui
            .push_id(("terminal_pane", id), |ui| {
                ui.allocate_painter(size, egui::Sense::click())
            })
            .inner;
        // Leave antialiasing room around the focus stroke instead of clipping it to
        // the allocated terminal rectangle.
        let painter = ui.painter().with_clip_rect(ui.clip_rect());
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
            let style = match active {
                Some(index) => &mut self.workspace.tabs_mut()[index].style,
                None => &mut self.config.terminal,
            };
            let font_size = (style.font_size + delta).clamp(8.0, 32.0);
            if font_size != style.font_size {
                style.font_size = font_size;
                self.config.terminal = style.clone();
                self.dirty = true;
            }
        }
        let style = match active {
            Some(index) => &self.workspace.tabs()[index].style,
            None => &self.config.terminal,
        };
        let font = terminal_font(style, &self.fonts);
        let dark = self.theme.is_dark(ui.ctx().system_theme());
        let colors = Palette::new(dark, style.fg_color, style.bg_color);
        let foreground = rgb(colors.foreground);
        let (char_width, line_height) = ui.fonts_mut(|fonts| {
            (
                fonts.glyph_width(&font, 'M'),
                fonts.row_height(&font) * style.line_spacing,
            )
        });
        let content_width = (size.x - 2.0 * TERMINAL_PADDING - TERMINAL_SCROLLBAR_WIDTH).max(0.0);
        let cols = ((content_width / char_width) as usize).clamp(2, 500);
        let rect = response.rect;
        painter.rect_filled(rect, CORNER_RADIUS, rgb(colors.background));
        // Keep PTY dimensions tied to the pane, including for full-screen applications.
        for tab in self
            .workspace
            .tabs_mut()
            .iter_mut()
            .filter(|tab| tab.pane() == pane)
        {
            let tab_font = terminal_font(&tab.style, &self.fonts);
            let (width, height) = ui.fonts_mut(|fonts| {
                (
                    fonts.glyph_width(&tab_font, 'M'),
                    fonts.row_height(&tab_font) * tab.style.line_spacing,
                )
            });
            let tab_cols = ((content_width / width) as usize).clamp(2, 500);
            let screen = tab.screen();
            let horizontal = !tab.line_wrap
                && !screen.is_alternate()
                && TerminalViewport::content_width(screen) > tab_cols;
            let gutter = if horizontal {
                TERMINAL_SCROLLBAR_WIDTH
            } else {
                0.0
            };
            tab.resize(
                tab_cols,
                (((size.y - 2.0 * TERMINAL_PADDING - gutter) / height) as usize).clamp(2, 200),
            );
        }
        let horizontal = active.is_some_and(|index| {
            let tab = &self.workspace.tabs()[index];
            let screen = tab.screen();
            !tab.line_wrap
                && !screen.is_alternate()
                && TerminalViewport::content_width(screen) > cols
        });
        let horizontal_gutter = if horizontal {
            TERMINAL_SCROLLBAR_WIDTH
        } else {
            0.0
        };
        let rows = (((size.y - 2.0 * TERMINAL_PADDING - horizontal_gutter) / line_height) as usize)
            .clamp(2, 200);
        self.workspace.resize_target(pane, cols, rows);
        let base_origin = rect.min + Vec2::splat(TERMINAL_PADDING);
        let text_rect = egui::Rect::from_min_max(
            base_origin,
            egui::pos2(
                (rect.right() - TERMINAL_SCROLLBAR_WIDTH - TERMINAL_PADDING).max(base_origin.x),
                (rect.bottom() - TERMINAL_PADDING - horizontal_gutter).max(base_origin.y),
            ),
        );
        let selection_response = ui.interact(
            text_rect,
            response.id.with("selection"),
            egui::Sense::click_and_drag(),
        );
        let middle_paste = response.middle_clicked() || selection_response.middle_clicked();
        let mut scrollbar_interacted = false;
        let mut close_search = false;
        if let Some(index) = active {
            let tab = &mut self.workspace.tabs_mut()[index];
            let screen = tab.paused_screen.as_ref().unwrap_or(&tab.terminal.screen);
            let view = TerminalViewport::new(screen, tab.line_wrap, cols, rows);
            let search_action = tab
                .search
                .show(ui.ctx(), response.id.with("find"), rect, screen);
            match search_action {
                SearchAction::None => {}
                SearchAction::Close => close_search = true,
                SearchAction::Navigate(found) => {
                    if let Some((scrollback, horizontal_scroll)) =
                        match_position(&view, &found, rows, cols, char_width, tab.horizontal_scroll)
                    {
                        tab.scrollback = scrollback;
                        tab.scroll_fraction = 0.0;
                        if let Some(horizontal_scroll) = horizontal_scroll {
                            tab.horizontal_scroll = horizontal_scroll;
                        }
                    }
                }
            }
            if pane_hovered && !control_scroll {
                let delta = ui.input(|input| input.smooth_scroll_delta);
                tab.scroll_fraction += delta.y / line_height;
                let lines = tab.scroll_fraction.trunc() as isize;
                tab.scroll_fraction -= lines as f32;
                tab.scrollback = tab.scrollback.saturating_add_signed(lines);
                tab.horizontal_scroll -= delta.x;
            }
            tab.scrollback = tab.scrollback.min(view.live_start);
            let mut vertical = (view.live_start - tab.scrollback) as f32;
            scrollbar_interacted |= Scrollbar {
                id: response.id.with("scrollbar"),
                rect: egui::Rect::from_min_max(
                    egui::pos2(rect.right() - TERMINAL_SCROLLBAR_WIDTH, base_origin.y),
                    egui::pos2(rect.right(), text_rect.bottom()),
                ),
                horizontal: false,
                visible: rows as f32,
                total: (view.live_start + rows) as f32,
            }
            .show(ui, &mut vertical);
            tab.scrollback = view.live_start.saturating_sub(vertical.round() as usize);
            if horizontal {
                scrollbar_interacted |= Scrollbar {
                    id: response.id.with("horizontal_scrollbar"),
                    rect: egui::Rect::from_min_max(
                        egui::pos2(
                            base_origin.x,
                            rect.bottom() - TERMINAL_PADDING - TERMINAL_SCROLLBAR_WIDTH,
                        ),
                        egui::pos2(text_rect.right(), rect.bottom() - TERMINAL_PADDING),
                    ),
                    horizontal: true,
                    visible: text_rect.width(),
                    total: view.width as f32 * char_width,
                }
                .show(ui, &mut tab.horizontal_scroll);
            } else {
                tab.horizontal_scroll = 0.0;
            }
            if scrollbar_interacted {
                tab.scroll_fraction = 0.0;
            }
            let viewport = SelectionViewport {
                rect: text_rect,
                char_width,
                line_height,
                rows: view.visible(tab.scrollback, rows),
                horizontal_scroll: tab.horizontal_scroll,
            };
            tab.selection
                .interact(ui, &selection_response, &viewport, screen);
        }
        if middle_paste && active.is_some() {
            response.request_focus();
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::RequestPaste);
        }
        if response.clicked()
            || selection_response.clicked()
            || selection_response.drag_started()
            || middle_paste
            || scrollbar_interacted
            || self.workspace.focus_requested(pane)
        {
            if let Some(index) = active {
                self.workspace.select_tab(index);
            }
            response.request_focus();
            self.workspace.acknowledge_focus();
        }
        let focused_pane = self.workspace.focused_pane() == pane;
        let search_open = active.is_some_and(|index| self.workspace.tabs()[index].search.open);
        if response.has_focus() && focused_pane {
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
        }
        if focused_pane && (response.has_focus() || search_open) {
            painter.rect_stroke(
                rect,
                CORNER_RADIUS,
                Stroke::new(1.0_f32, get_global_color("primary")),
                egui::StrokeKind::Inside,
            );
        }
        let painter = painter.with_clip_rect(text_rect);
        let Some(index) = active else {
            painter.text(
                base_origin,
                egui::Align2::LEFT_TOP,
                "No process output yet.",
                font,
                foreground,
            );
            return;
        };
        let tab = &mut self.workspace.tabs_mut()[index];
        tab.text_cache.prepare(&painter, &font, rows, dark);
        let display_paused = tab.display_paused();
        let screen = tab.paused_screen.as_ref().unwrap_or(&tab.terminal.screen);
        let view = TerminalViewport::new(screen, tab.line_wrap, cols, rows);
        for (visible_row, row) in view.visible(tab.scrollback, rows).iter().enumerate() {
            if row.first_col >= row.line.len() {
                continue;
            }
            let mut first =
                (row.first_col + (tab.horizontal_scroll / char_width) as usize).min(row.end_col);
            if first > 0 && row.line.get(first).is_some_and(|cell| cell.continuation) {
                first -= 1;
            }
            let end = (first + cols + 2).min(row.end_col);
            let selected = tab.selection.columns(row.logical_row, row.line);
            let found: Vec<_> = tab
                .search
                .matches
                .iter()
                .enumerate()
                .filter(|(_, found)| tab.search.open && found.row == row.logical_row)
                .map(|(index, found)| {
                    (
                        found.columns.start.saturating_sub(first)
                            ..found.columns.end.saturating_sub(first),
                        tab.search.current == Some(index),
                    )
                })
                .collect();
            paint_line(
                &painter,
                base_origin
                    + Vec2::new(
                        (first - row.first_col) as f32 * char_width - tab.horizontal_scroll,
                        visible_row as f32 * line_height,
                    ),
                &row.line[first..end],
                &font,
                char_width,
                line_height,
                end - first,
                &colors,
                selected.start.saturating_sub(first)..selected.end.saturating_sub(first),
                &found,
                ui.visuals().selection,
                &mut tab.text_cache.rows[visible_row],
            );
        }
        if tab.scrollback == 0
            && !display_paused
            && screen.cursor_visible
            && tab.terminal.is_running()
            && view.cursor.0 >= view.live_start
            && view.cursor.0 < view.live_start + rows
        {
            let focused = response.has_focus() && focused_pane;
            let (now, typing) = ui.input(|input| {
                (
                    input.time,
                    input.events.iter().any(|event| {
                        matches!(
                            event,
                            egui::Event::Key { pressed: true, .. }
                                | egui::Event::Text(_)
                                | egui::Event::Paste(_)
                                | egui::Event::Copy
                                | egui::Event::Cut
                        )
                    }),
                )
            });
            // Keep typing visible, then alternate 500 ms on/off. Unfocused stays outlined.
            let reset = !focused || response.gained_focus() || typing;
            let phase = ui.data_mut(|data| {
                let start =
                    data.get_temp_mut_or_insert_with(response.id.with("cursor_blink"), || now);
                if reset {
                    *start = now;
                }
                (now - *start).rem_euclid(1.0)
            });
            if focused {
                ui.ctx()
                    .request_repaint_after(std::time::Duration::from_secs_f64(if phase < 0.5 {
                        0.5 - phase
                    } else {
                        1.0 - phase
                    }));
            }
            let (x, y) = screen.cursor();
            let cursor_origin = base_origin
                + Vec2::new(
                    view.cursor.1 as f32 * char_width - tab.horizontal_scroll,
                    (view.cursor.0 - view.live_start) as f32 * line_height,
                );
            let cursor =
                egui::Rect::from_min_size(cursor_origin, Vec2::new(char_width, line_height));
            if focused && phase < 0.5 {
                painter.rect_filled(cursor, 0.0, foreground);
                // Keep the character under the block cursor readable.
                let line = screen.line(y);
                let column = if line.get(x).is_some_and(|cell| cell.continuation) {
                    x.saturating_sub(1)
                } else {
                    x
                };
                if let Some(cell) = line.get(column) {
                    painter.with_clip_rect(cursor.intersect(text_rect)).text(
                        cursor_origin - Vec2::new((x - column) as f32 * char_width, 0.0),
                        egui::Align2::LEFT_TOP,
                        &cell.text,
                        font.clone(),
                        rgb(colors.background),
                    );
                }
            } else if !focused {
                painter.rect_stroke(
                    cursor,
                    0.0,
                    Stroke::new(1.0_f32, foreground),
                    egui::StrokeKind::Inside,
                );
            }
        }
        if response.has_focus() && focused_pane {
            handle_terminal_input(ui, tab);
        }
        if close_search {
            tab.close_search();
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

fn match_position(
    view: &TerminalViewport<'_>,
    found: &SearchMatch,
    visible_rows: usize,
    visible_cols: usize,
    char_width: f32,
    horizontal_scroll: f32,
) -> Option<(usize, Option<f32>)> {
    let row = view.rows.iter().position(|row| {
        row.logical_row == found.row
            && found.columns.start < row.end_col.max(row.first_col + 1)
            && found.columns.end > row.first_col
    })?;
    let start = row.saturating_sub(visible_rows / 2);
    let column = found.columns.start;
    let horizontal = if column < (horizontal_scroll / char_width) as usize
        || column >= (horizontal_scroll / char_width) as usize + visible_cols
    {
        Some(column.saturating_sub(2) as f32 * char_width)
    } else {
        None
    };
    Some((view.live_start.saturating_sub(start), horizontal))
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
    colors: &Palette,
    selected: std::ops::Range<usize>,
    found: &[(std::ops::Range<usize>, bool)],
    selection_style: egui::style::Selection,
    cache: &mut Vec<Arc<egui::Galley>>,
) {
    let mut start = 0;
    let mut run = 0;
    while start < line.len().min(cols) {
        let style = line[start].style;
        let mut end = start + 1;
        let is_selected = selected.contains(&start);
        let found_state = search_state(found, start);
        while end < line.len().min(cols)
            && line[end].style == style
            && selected.contains(&end) == is_selected
            && search_state(found, end) == found_state
        {
            end += 1;
        }
        let mut foreground = rgb(style
            .fg_color
            .map_or(colors.foreground, |c| colors.resolve(c)));
        let mut background = rgb(style
            .bg_color
            .map_or(colors.background, |c| colors.resolve(c)));
        if style.inverse {
            std::mem::swap(&mut foreground, &mut background);
        }
        if is_selected {
            background = selection_style.bg_fill;
            foreground = selection_style.stroke.color;
        } else if let Some(active) = found_state {
            background = get_global_color(if active {
                "primary"
            } else {
                "primaryContainer"
            });
            foreground = get_global_color(if active {
                "onPrimary"
            } else {
                "onPrimaryContainer"
            });
        }
        let position = origin + Vec2::new(start as f32 * char_width, 0.0);
        let width = (end - start) as f32 * char_width;
        let rect = egui::Rect::from_min_size(position, Vec2::new(width, line_height));
        if background != rgb(colors.background) {
            painter.rect_filled(rect, 0.0, background);
        }
        let text: String = line[start..end]
            .iter()
            .filter(|cell| !cell.continuation)
            .map(|cell| cell.text.as_str())
            .collect();
        let galley = if let Some(cached) = cache.get(run)
            && cached.job.text == text
        {
            cached.clone()
        } else {
            let galley = painter.layout_no_wrap(text, font.clone(), egui::Color32::PLACEHOLDER);
            if run < cache.len() {
                cache[run] = galley.clone();
            } else {
                cache.push(galley.clone());
            }
            galley
        };
        painter.galley(position, galley.clone(), foreground);
        if style.bold {
            painter.galley(position + Vec2::new(0.5, 0.0), galley, foreground);
        }
        if style.underline {
            painter.line_segment(
                [rect.left_bottom(), rect.right_bottom()],
                Stroke::new(1.0_f32, foreground),
            );
        }
        start = end;
        run += 1;
    }
    cache.truncate(run);
}

fn search_state(found: &[(std::ops::Range<usize>, bool)], column: usize) -> Option<bool> {
    found
        .iter()
        .find_map(|(range, active)| range.contains(&column).then_some(*active))
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
