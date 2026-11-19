//! Map retained terminal rows to visible rows without changing the PTY dimensions.

use crate::terminal::screen::{Cell, Screen, line_width};

pub(super) struct VisualRow<'a> {
    pub logical_row: usize,
    pub first_col: usize,
    pub end_col: usize,
    pub line: &'a [Cell],
}

pub(super) struct TerminalViewport<'a> {
    pub rows: Vec<VisualRow<'a>>,
    pub live_start: usize,
    pub width: usize,
    pub cursor: (usize, usize),
}

impl<'a> TerminalViewport<'a> {
    pub fn content_width(screen: &Screen) -> usize {
        let history = if screen.is_alternate() {
            0
        } else {
            screen.history.iter().map(Vec::len).max().unwrap_or(0)
        };
        let live = (0..screen.rows)
            .map(|row| line_width(screen.line(row)))
            .max()
            .unwrap_or(0);
        history.max(live).max(screen.cursor().0 + 1)
    }

    pub fn new(screen: &'a Screen, wrap: bool, cols: usize, visible_rows: usize) -> Self {
        let wrap = wrap && !screen.is_alternate();
        let history = if screen.is_alternate() {
            0
        } else {
            screen.history.len()
        };
        let first = if history == 0 {
            screen.scrolled_lines
        } else {
            screen.scrolled_lines.saturating_sub(history)
        };
        let (cursor_x, cursor_y) = screen.cursor();
        let mut view = Self {
            rows: Vec::new(),
            live_start: 0,
            width: cols,
            cursor: (0, 0),
        };
        let mut history_rows = 0;
        let mut used_rows = 0;
        for index in 0..history + screen.rows {
            let line = if index < history {
                &screen.history[index]
            } else {
                screen.line(index - history)
            };
            let cursor_line = index == history + cursor_y;
            let occupied = if index < history {
                line.len()
            } else {
                line_width(line)
            };
            let extent = occupied.max(if cursor_line { cursor_x + 1 } else { 0 });
            view.width = view.width.max(extent);
            let mut start = 0;
            loop {
                let mut end = if wrap {
                    (start + cols).min(extent)
                } else {
                    line.len()
                };
                if wrap && end < extent && line.get(end).is_some_and(|cell| cell.continuation) {
                    if end > start + 1 {
                        end -= 1;
                    } else {
                        end = (end + 1).min(extent);
                    }
                }
                if cursor_line && cursor_x >= start && (!wrap || cursor_x < end.max(start + 1)) {
                    view.cursor = (view.rows.len(), cursor_x - start);
                }
                view.rows.push(VisualRow {
                    logical_row: if screen.is_alternate() {
                        index
                    } else {
                        first + index
                    },
                    first_col: start,
                    end_col: end.min(line.len()),
                    line,
                });
                if !wrap || end >= extent {
                    break;
                }
                start = end;
            }
            if index < history {
                history_rows = view.rows.len();
            } else if occupied > 0 || index <= history + cursor_y {
                used_rows = view.rows.len();
            }
        }
        view.live_start = history_rows.max(used_rows.saturating_sub(visible_rows));
        view
    }

    pub fn visible(&self, scrollback: usize, count: usize) -> &[VisualRow<'a>] {
        let start = self.live_start.saturating_sub(scrollback);
        &self.rows[start..(start + count).min(self.rows.len())]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrapping_existing_output_preserves_wide_glyphs_and_logical_rows() {
        let mut screen = Screen::new(4, 5);
        screen.preserve_long_lines();
        screen.feed("abc界e\u{301}z\r\nnext".as_bytes());
        let view = TerminalViewport::new(&screen, true, 4, 5);
        let text = |row: &VisualRow<'_>| {
            row.line[row.first_col..row.end_col]
                .iter()
                .filter(|cell| !cell.continuation)
                .map(|cell| cell.text.as_str())
                .collect::<String>()
        };
        let rows = view.visible(0, 5);
        assert_eq!(text(&rows[0]), "abc");
        assert_eq!(text(&rows[1]), "界e\u{301}z");
        assert_eq!(rows[0].logical_row, rows[1].logical_row);
        assert_eq!(text(&rows[2]), "next");
        assert_eq!(screen.cols, 4);
        assert!(screen.text().starts_with("abc界e\u{301}z\nnext"));
    }

    #[test]
    fn wrapping_a_wide_glyph_in_one_column_still_advances() {
        let mut screen = Screen::new(2, 2);
        screen.preserve_long_lines();
        screen.feed("界x".as_bytes());
        let view = TerminalViewport::new(&screen, true, 1, 2);
        assert_eq!((view.rows[0].first_col, view.rows[0].end_col), (0, 2));
        assert_eq!((view.rows[1].first_col, view.rows[1].end_col), (2, 3));
    }
}
