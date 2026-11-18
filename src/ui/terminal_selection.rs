//! Cell-based terminal selection, anchored to retained output rows.

use crate::terminal::screen::{Cell, Screen};
use egui::{PointerButton, Pos2, Rect};
use std::ops::Range;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Position {
    row: usize,
    col: usize,
}

#[derive(Default)]
pub(in crate::app) struct TerminalSelection {
    endpoints: Option<(Position, Position)>,
    dimensions: Option<(usize, usize, bool)>,
    scrolled_lines: usize,
}

pub(super) struct SelectionViewport {
    pub rect: Rect,
    pub char_width: f32,
    pub line_height: f32,
    pub first_row: usize,
}

impl SelectionViewport {
    fn position(&self, pointer: Pos2, screen: &Screen) -> Position {
        Position {
            row: self.first_row
                + (((pointer.y - self.rect.top()) / self.line_height).floor() as usize)
                    .min(screen.rows - 1),
            col: (((pointer.x - self.rect.left()) / self.char_width).round() as usize)
                .min(screen.cols),
        }
    }
}

impl TerminalSelection {
    pub(in crate::app) fn clear(&mut self) {
        self.endpoints = None;
    }

    pub(in crate::app) fn prepare(&mut self, screen: &Screen) {
        let dimensions = (screen.cols, screen.rows, screen.is_alternate());
        let first = first_retained_row(screen);
        let end = live_row(screen) + screen.rows;
        // Resizing or switching buffers changes the meaning of cell coordinates.
        if self.dimensions != Some(dimensions)
            || screen.scrolled_lines < self.scrolled_lines
            || self.endpoints.is_some_and(|(anchor, head)| {
                anchor.row < first || head.row < first || anchor.row >= end || head.row >= end
            })
        {
            self.clear();
        }
        self.dimensions = Some(dimensions);
        self.scrolled_lines = screen.scrolled_lines;
    }

    pub(super) fn interact(
        &mut self,
        ui: &mut egui::Ui,
        response: &egui::Response,
        viewport: &SelectionViewport,
        screen: &Screen,
    ) {
        if response.hovered() || response.dragged_by(PointerButton::Primary) {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Text);
        }
        if response.clicked_by(PointerButton::Primary) {
            self.clear();
        }
        if response.drag_started_by(PointerButton::Primary)
            && let Some(origin) = ui.input(|input| input.pointer.press_origin())
        {
            let anchor = viewport.position(origin, screen);
            self.endpoints = Some((anchor, anchor));
        }
        if (response.dragged_by(PointerButton::Primary)
            || response.drag_stopped_by(PointerButton::Primary))
            && let Some(pointer) = response.interact_pointer_pos()
            && let Some((_, head)) = &mut self.endpoints
        {
            *head = viewport.position(pointer, screen);
        }
        if response.drag_stopped_by(PointerButton::Primary)
            && let Some(text) = self.text(screen)
        {
            ui.ctx().copy_text(text);
        }
    }

    fn ordered(&self) -> Option<(Position, Position)> {
        let (anchor, head) = self.endpoints?;
        (anchor != head).then_some((anchor.min(head), anchor.max(head)))
    }

    pub(super) fn columns(&self, row: usize, line: &[Cell]) -> Range<usize> {
        let Some((start, end)) = self.ordered() else {
            return 0..0;
        };
        if row < start.row || row > end.row {
            return 0..0;
        }
        let mut left = if row == start.row { start.col } else { 0 }.min(line.len());
        let mut right = if row == end.row { end.col } else { line.len() }.min(line.len());
        if left == right {
            return left..right;
        }
        // A wide glyph occupies two cells; include it whole at either boundary.
        if left > 0 && line.get(left).is_some_and(|cell| cell.continuation) {
            left -= 1;
        }
        if line.get(right).is_some_and(|cell| cell.continuation) {
            right += 1;
        }
        left..right
    }

    pub(in crate::app) fn text(&self, screen: &Screen) -> Option<String> {
        let (start, end) = self.ordered()?;
        if self.dimensions != Some((screen.cols, screen.rows, screen.is_alternate()))
            || screen.scrolled_lines < self.scrolled_lines
            || start.row < first_retained_row(screen)
            || end.row >= live_row(screen) + screen.rows
        {
            return None;
        }
        let live = live_row(screen);
        let text = (start.row..=end.row)
            .map(|row| {
                let line = if row < live {
                    &screen.history[row - first_retained_row(screen)]
                } else {
                    screen.line(row - live)
                };
                line[self.columns(row, line)]
                    .iter()
                    .filter(|cell| !cell.continuation)
                    .map(|cell| cell.text.as_str())
                    .collect::<String>()
                    .trim_end()
                    .to_owned()
            })
            .collect::<Vec<_>>()
            .join("\n");
        (!text.is_empty()).then_some(text)
    }
}

pub(super) fn live_row(screen: &Screen) -> usize {
    if screen.is_alternate() {
        0
    } else {
        screen.scrolled_lines
    }
}

fn first_retained_row(screen: &Screen) -> usize {
    live_row(screen).saturating_sub(screen.history.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terminal::screen::HISTORY_LIMIT;

    fn select(screen: &Screen, start: (usize, usize), end: (usize, usize)) -> TerminalSelection {
        let mut selection = TerminalSelection::default();
        selection.prepare(screen);
        selection.endpoints = Some((
            Position {
                row: start.0,
                col: start.1,
            },
            Position {
                row: end.0,
                col: end.1,
            },
        ));
        selection
    }

    #[test]
    fn copies_multiline_text_in_both_directions_without_ansi_or_padding() {
        let mut screen = Screen::new(20, 4);
        screen.feed(b"\x1b[31mfirst line\x1b[0m\r\n  second\r\n\r\nlast");
        for (start, end) in [((0, 6), (3, 2)), ((3, 2), (0, 6))] {
            let selection = select(&screen, start, end);
            assert_eq!(
                selection.text(&screen).as_deref(),
                Some("line\n  second\n\nla")
            );
        }
        assert!(select(&screen, (0, 2), (0, 2)).text(&screen).is_none());
        assert!(select(&screen, (2, 0), (2, 5)).text(&screen).is_none());
    }

    #[test]
    fn wide_and_combining_characters_are_copied_and_highlighted_whole() {
        let mut screen = Screen::new(12, 2);
        screen.feed("a界e\u{301}z".as_bytes());
        for (start, end) in [((0, 1), (0, 2)), ((0, 2), (0, 3))] {
            let selection = select(&screen, start, end);
            assert_eq!(selection.text(&screen).as_deref(), Some("界"));
            assert_eq!(selection.columns(0, screen.line(0)), 1..3);
        }
        assert_eq!(
            select(&screen, (0, 3), (0, 4)).text(&screen).as_deref(),
            Some("e\u{301}")
        );
    }

    #[test]
    fn selection_tracks_rows_into_history_and_clears_when_they_are_evicted() {
        let mut screen = Screen::new(12, 3);
        screen.feed(b"first\r\nsecond\r\nthird");
        let mut selection = select(&screen, (0, 0), (1, 6));
        screen.feed(b"\r\nfourth\r\nfifth");
        selection.prepare(&screen);
        assert_eq!(selection.text(&screen).as_deref(), Some("first\nsecond"));
        for _ in 0..HISTORY_LIMIT {
            screen.feed(b"\r\nmore");
        }
        assert!(selection.text(&screen).is_none());
        selection.prepare(&screen);
        assert!(selection.endpoints.is_none());
    }

    #[test]
    fn resize_alternate_screen_and_reset_clear_stale_selection() {
        let mut screen = Screen::new(12, 3);
        screen.feed(b"first\r\nsecond\r\nthird\r\nfourth");
        let mut selection = select(&screen, (1, 0), (1, 6));
        screen.resize(10, 3);
        assert!(selection.text(&screen).is_none());
        selection.prepare(&screen);
        assert!(selection.endpoints.is_none());
        selection = select(&screen, (1, 0), (1, 6));
        screen.feed(b"\x1b[?1049hALT");
        selection.prepare(&screen);
        assert!(selection.endpoints.is_none());
        selection = select(&screen, (0, 0), (0, 3));
        assert_eq!(selection.text(&screen).as_deref(), Some("ALT"));
        screen.feed(b"\x1b[?1049l");
        selection.prepare(&screen);
        assert!(selection.endpoints.is_none());
        selection = select(&screen, (1, 0), (1, 6));
        screen.feed(b"\x1bc");
        selection.prepare(&screen);
        assert!(selection.endpoints.is_none());
    }
}
