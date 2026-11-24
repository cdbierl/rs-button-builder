//! A bounded VT100 screen with common ANSI color and alternate-screen extensions.
use super::colors::Color;
use std::collections::VecDeque;
use std::time::Instant;
use unicode_width::UnicodeWidthChar;

pub const HISTORY_LIMIT: usize = 2000;
// Safety-wrap very long output instead of allowing a single row to grow without bound.
const MAX_LINE_COLS: usize = 4096;

#[derive(Clone, Copy, Default, Debug, PartialEq)]
pub struct Style {
    pub fg_color: Option<Color>,
    pub bg_color: Option<Color>,
    pub bold: bool,
    pub underline: bool,
    pub inverse: bool,
}

#[derive(Clone)]
pub struct Cell {
    pub text: String,
    pub style: Style,
    pub continuation: bool,
}

impl Cell {
    fn blank(style: Style) -> Self {
        Self {
            text: " ".into(),
            style,
            continuation: false,
        }
    }
}

#[derive(Clone)]
struct Buffer {
    lines: Vec<Vec<Cell>>,
    x: usize,
    y: usize,
    saved: (usize, usize, Style),
    top: usize,
    bottom: usize,
    wrap_pending: bool,
}

impl Buffer {
    fn new(cols: usize, rows: usize) -> Self {
        Self {
            lines: vec![vec![Cell::blank(Style::default()); cols]; rows],
            x: 0,
            y: 0,
            saved: (0, 0, Style::default()),
            top: 0,
            bottom: rows - 1,
            wrap_pending: false,
        }
    }

    fn resize(&mut self, cols: usize, rows: usize, preserve_lines: bool) {
        for (row, line) in self.lines.iter_mut().enumerate() {
            let cursor = if row == self.y { self.x + 1 } else { 0 };
            line.resize(
                if preserve_lines {
                    cols.max(line_width(line)).max(cursor).min(MAX_LINE_COLS)
                } else {
                    cols
                },
                Cell::blank(Style::default()),
            );
        }
        self.lines
            .resize(rows, vec![Cell::blank(Style::default()); cols]);
        self.x = self.x.min(if preserve_lines {
            MAX_LINE_COLS - 1
        } else {
            cols - 1
        });
        self.y = self.y.min(rows - 1);
        self.top = 0;
        self.bottom = rows - 1;
        self.wrap_pending = false;
    }
}

#[derive(Clone, Default)]
enum Parser {
    #[default]
    Ground,
    Escape,
    EscapeIntermediate,
    Csi(Vec<u8>),
    String {
        escaped: bool,
    },
    Charset(u8),
}

#[derive(Clone)]
pub struct Screen {
    pub cols: usize,
    pub rows: usize,
    pub history: VecDeque<Vec<Cell>>,
    pub cursor_visible: bool,
    pub application_cursor: bool,
    pub scrolled_lines: usize,
    buffer: Buffer,
    alternate: Option<Buffer>,
    style: Style,
    parser: Parser,
    utf8: Vec<u8>,
    auto_wrap: bool,
    preserve_long_lines: bool,
    origin: bool,
    charsets: [bool; 2],
    charset: usize,
    printed_text: bool,
}

impl Screen {
    pub fn new(cols: usize, rows: usize) -> Self {
        let cols = cols.clamp(2, 500);
        let rows = rows.clamp(2, 200);
        Self {
            cols,
            rows,
            history: VecDeque::new(),
            cursor_visible: true,
            application_cursor: false,
            scrolled_lines: 0,
            buffer: Buffer::new(cols, rows),
            alternate: None,
            style: Style::default(),
            parser: Parser::Ground,
            utf8: Vec::new(),
            auto_wrap: true,
            preserve_long_lines: false,
            origin: false,
            charsets: [false; 2],
            charset: 0,
            printed_text: false,
        }
    }

    pub fn resize(&mut self, cols: usize, rows: usize) {
        self.cols = cols.clamp(2, 500);
        self.rows = rows.clamp(2, 200);
        let shift = (self.buffer.y + 1).saturating_sub(self.rows);
        for _ in 0..shift {
            let line = self.buffer.lines.remove(0);
            if self.alternate.is_none() {
                self.push_history(line);
            }
        }
        self.buffer.y -= shift;
        self.buffer.resize(
            self.cols,
            self.rows,
            self.preserve_long_lines && self.alternate.is_none(),
        );
        if let Some(saved) = &mut self.alternate {
            saved.resize(self.cols, self.rows, self.preserve_long_lines);
        }
    }

    fn column_limit(&self) -> usize {
        if self.preserve_long_lines && self.alternate.is_none() && self.auto_wrap {
            MAX_LINE_COLS
        } else {
            self.cols
        }
    }

    pub(crate) fn preserve_long_lines(&mut self) {
        self.preserve_long_lines = true;
    }

    pub fn line(&self, row: usize) -> &[Cell] {
        &self.buffer.lines[row]
    }
    pub fn cursor(&self) -> (usize, usize) {
        (self.buffer.x, self.buffer.y)
    }
    pub fn is_alternate(&self) -> bool {
        self.alternate.is_some()
    }

    /// Remember non-whitespace glyphs with a display width, even after a clear/reset.
    /// Zero-width marks alone do not count; their printable base already does.
    pub fn has_printed_text(&self) -> bool {
        self.printed_text
    }

    pub fn text(&self) -> String {
        self.history
            .iter()
            .take(if self.is_alternate() {
                0
            } else {
                self.history.len()
            })
            .chain(self.buffer.lines.iter())
            .map(|line| {
                line.iter()
                    .filter(|cell| !cell.continuation)
                    .map(|cell| cell.text.as_str())
                    .collect::<String>()
                    .trim_end()
                    .to_owned()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[cfg(test)]
    pub fn feed(&mut self, bytes: &[u8]) -> Vec<u8> {
        let mut replies = Vec::new();
        for &byte in bytes {
            self.feed_byte(byte, &mut replies);
        }
        replies
    }

    pub fn feed_until(&mut self, bytes: &[u8], deadline: Instant) -> (usize, Vec<u8>) {
        let mut replies = Vec::new();
        let mut count = 0;
        for &byte in bytes {
            if Instant::now() >= deadline {
                break;
            }
            self.feed_byte(byte, &mut replies);
            count += 1;
        }
        (count, replies)
    }

    /// At EOF, malformed UTF-8 must remain visible rather than count as silence.
    pub fn finish(&mut self) -> bool {
        if self.utf8.is_empty() {
            return false;
        }
        self.utf8.clear();
        self.put('\u{fffd}');
        true
    }

    fn feed_byte(&mut self, byte: u8, replies: &mut Vec<u8>) {
        // CAN/SUB cancel any partial terminal command; ESC starts a fresh command.
        if matches!(byte, 0x18 | 0x1a) && !matches!(self.parser, Parser::Ground) {
            self.parser = Parser::Ground;
            return;
        }
        if byte == 0x1b && !matches!(self.parser, Parser::Ground | Parser::String { .. }) {
            self.parser = Parser::Escape;
            return;
        }
        let parser = std::mem::take(&mut self.parser);
        match parser {
            Parser::Ground => self.ground(byte),
            Parser::Escape => match byte {
                b'[' => self.parser = Parser::Csi(Vec::new()),
                b']' | b'P' | b'X' | b'^' | b'_' => self.parser = Parser::String { escaped: false },
                b'(' | b')' => self.parser = Parser::Charset(byte),
                0x20..=0x2f => self.parser = Parser::EscapeIntermediate,
                b'7' => self.save_cursor(),
                b'8' => self.restore_cursor(),
                b'D' => self.line_feed(),
                b'E' => {
                    self.buffer.x = 0;
                    self.line_feed();
                }
                b'M' => {
                    if self.buffer.y == self.buffer.top {
                        self.scroll_down(1);
                    } else {
                        self.buffer.y = self.buffer.y.saturating_sub(1);
                    }
                }
                b'c' => {
                    let printed_text = self.printed_text;
                    let preserve_long_lines = self.preserve_long_lines;
                    *self = Self::new(self.cols, self.rows);
                    self.printed_text = printed_text;
                    self.preserve_long_lines = preserve_long_lines;
                }
                b'Z' => replies.extend_from_slice(b"\x1b[?1;2c"),
                0x00..=0x1f | 0x7f => self.parser = Parser::Escape,
                _ => {}
            },
            Parser::EscapeIntermediate => {
                if !(0x30..=0x7e).contains(&byte) {
                    self.parser = Parser::EscapeIntermediate;
                }
            }
            Parser::Csi(mut sequence) => {
                if (0x40..=0x7e).contains(&byte) {
                    self.csi(&sequence, byte, replies);
                } else {
                    if byte >= 0x20 && byte != 0x7f && sequence.len() < 256 {
                        sequence.push(byte);
                    }
                    self.parser = Parser::Csi(sequence);
                }
            }
            Parser::String { escaped } => {
                if byte != 7 && !(escaped && byte == b'\\') {
                    self.parser = Parser::String {
                        escaped: byte == 0x1b,
                    };
                }
            }
            Parser::Charset(which) => {
                if byte < 0x20 || byte == 0x7f {
                    self.parser = Parser::Charset(which);
                } else {
                    self.charsets[usize::from(which == b')')] = byte == b'0';
                }
            }
        }
    }

    fn ground(&mut self, byte: u8) {
        if byte < 0x20 || byte == 0x7f {
            if !self.utf8.is_empty() {
                self.utf8.clear();
                self.put('\u{fffd}');
            }
            match byte {
                0x1b => self.parser = Parser::Escape,
                b'\r' => {
                    self.buffer.x = 0;
                    self.buffer.wrap_pending = false;
                }
                b'\n' | 0x0b | 0x0c => self.line_feed(),
                b'\t' => {
                    self.buffer.x = ((self.buffer.x / 8 + 1) * 8).min(self.column_limit() - 1);
                    self.buffer.wrap_pending = false;
                }
                8 => {
                    self.buffer.x = self.buffer.x.saturating_sub(1);
                    self.buffer.wrap_pending = false;
                }
                0x0e => self.charset = 1,
                0x0f => self.charset = 0,
                _ => {}
            }
        } else if byte.is_ascii() {
            if !self.utf8.is_empty() {
                self.utf8.clear();
                self.put('\u{fffd}');
            }
            let ch = if self.charsets[self.charset] {
                match byte {
                    b'j' => '┘',
                    b'k' => '┐',
                    b'l' => '┌',
                    b'm' => '└',
                    b'n' => '┼',
                    b'q' => '─',
                    b't' => '├',
                    b'u' => '┤',
                    b'v' => '┴',
                    b'w' => '┬',
                    b'x' => '│',
                    _ => byte as char,
                }
            } else {
                byte as char
            };
            self.put(ch);
        } else {
            self.utf8.push(byte);
            match std::str::from_utf8(&self.utf8) {
                Ok(text) => {
                    let ch = text.chars().next().unwrap();
                    self.utf8.clear();
                    self.put(ch);
                }
                Err(error) if error.error_len().is_none() && self.utf8.len() < 4 => {}
                Err(_) => {
                    self.utf8.clear();
                    self.put('\u{fffd}');
                }
            }
        }
    }

    fn put(&mut self, ch: char) {
        let width = ch.width().unwrap_or(0).min(2);
        self.printed_text |= !ch.is_whitespace() && width > 0;
        if width == 0 {
            let x = if self.buffer.wrap_pending {
                self.buffer.x
            } else {
                self.buffer.x.saturating_sub(1)
            };
            if x >= self.buffer.lines[self.buffer.y].len() {
                self.buffer.lines[self.buffer.y]
                    .resize((x + 1).min(MAX_LINE_COLS), Cell::blank(self.style));
            }
            let x = if self.buffer.lines[self.buffer.y][x].continuation {
                x.saturating_sub(1)
            } else {
                x
            };
            // Repeated combining marks must not grow a cell without limit or stall text layout.
            let text = &mut self.buffer.lines[self.buffer.y][x].text;
            if text.len() + ch.len_utf8() <= 64 {
                text.push(ch);
            }
            return;
        }
        let limit = self.column_limit();
        if self.auto_wrap && (self.buffer.wrap_pending || self.buffer.x + width > limit) {
            self.buffer.x = 0;
            self.line_feed();
        }
        self.buffer.wrap_pending = false;
        self.buffer.x = self.buffer.x.min(limit - 1);
        let x = self.buffer.x;
        let line = &mut self.buffer.lines[self.buffer.y];
        if x + width > line.len() {
            line.resize((x + width).min(limit), Cell::blank(self.style));
        }
        if line[x].continuation && x > 0 {
            line[x - 1] = Cell::blank(self.style);
        }
        if x + 1 < line.len() && line[x + 1].continuation {
            line[x + 1] = Cell::blank(self.style);
        }
        line[x] = Cell {
            text: ch.to_string(),
            style: self.style,
            continuation: false,
        };
        if width == 2 && x + 1 < line.len() {
            line[x + 1] = Cell {
                text: String::new(),
                style: self.style,
                continuation: true,
            };
        }
        if x + width >= limit {
            self.buffer.x = limit - 1;
            self.buffer.wrap_pending = true;
        } else {
            self.buffer.x += width;
        }
    }

    fn line_feed(&mut self) {
        self.buffer.wrap_pending = false;
        if self.buffer.y == self.buffer.bottom {
            self.scroll_up(1);
        } else {
            self.buffer.y = (self.buffer.y + 1).min(self.rows - 1);
        }
    }

    fn scroll_up(&mut self, count: usize) {
        for _ in 0..count.min(self.buffer.bottom - self.buffer.top + 1) {
            let line = self.buffer.lines.remove(self.buffer.top);
            if self.buffer.top == 0
                && self.buffer.bottom == self.rows - 1
                && self.alternate.is_none()
            {
                self.push_history(line);
            }
            self.buffer
                .lines
                .insert(self.buffer.bottom, vec![Cell::blank(self.style); self.cols]);
        }
    }

    fn push_history(&mut self, mut line: Vec<Cell>) {
        line.truncate(line_width(&line));
        self.scrolled_lines = self.scrolled_lines.saturating_add(1);
        self.history.push_back(line);
        if self.history.len() > HISTORY_LIMIT {
            self.history.pop_front();
        }
    }

    fn scroll_down(&mut self, count: usize) {
        for _ in 0..count.min(self.buffer.bottom - self.buffer.top + 1) {
            self.buffer.lines.remove(self.buffer.bottom);
            self.buffer
                .lines
                .insert(self.buffer.top, vec![Cell::blank(self.style); self.cols]);
        }
    }

    fn save_cursor(&mut self) {
        self.buffer.saved = (self.buffer.x, self.buffer.y, self.style);
    }
    fn restore_cursor(&mut self) {
        let (x, y, style) = self.buffer.saved;
        self.buffer.x = x.min(self.column_limit() - 1);
        self.buffer.y = y.min(self.rows - 1);
        self.style = style;
        self.buffer.wrap_pending = false;
    }

    fn csi(&mut self, sequence: &[u8], command: u8, replies: &mut Vec<u8>) {
        let private = sequence.first() == Some(&b'?');
        let raw = if private { &sequence[1..] } else { sequence };
        let params: Vec<usize> = raw
            .split(|&b| b == b';')
            .map(|part| {
                if part.is_empty() {
                    0
                } else {
                    std::str::from_utf8(part)
                        .ok()
                        .and_then(|s| s.parse::<u16>().ok())
                        .unwrap_or(u16::MAX) as usize
                }
            })
            .collect();
        let p = params[0];
        let count = p.max(1);
        let (x, y) = (self.buffer.x, self.buffer.y);
        if x >= self.buffer.lines[y].len() {
            self.buffer.lines[y].resize((x + 1).min(MAX_LINE_COLS), Cell::blank(self.style));
        }
        let line_end = self.buffer.lines[y].len();
        let column_limit = self.column_limit();
        let y_min = if self.origin { self.buffer.top } else { 0 };
        let y_max = if self.origin {
            self.buffer.bottom
        } else {
            self.rows - 1
        };
        match command {
            b'A' => self.buffer.y = y.saturating_sub(count).max(y_min),
            b'B' | b'e' => self.buffer.y = (y + count).min(y_max),
            b'C' | b'a' => self.buffer.x = (x + count).min(column_limit - 1),
            b'D' => self.buffer.x = x.saturating_sub(count),
            b'E' => {
                self.buffer.y = (y + count).min(y_max);
                self.buffer.x = 0;
            }
            b'F' => {
                self.buffer.y = y.saturating_sub(count).max(y_min);
                self.buffer.x = 0;
            }
            b'G' | b'`' => self.buffer.x = (count - 1).min(column_limit - 1),
            b'd' => self.buffer.y = (y_min + count - 1).min(y_max),
            b'H' | b'f' => {
                self.buffer.y = (y_min + count - 1).min(y_max);
                self.buffer.x = params
                    .get(1)
                    .copied()
                    .unwrap_or(1)
                    .max(1)
                    .saturating_sub(1)
                    .min(column_limit - 1);
            }
            b'J' => match p {
                0 => {
                    self.erase_line(x, line_end);
                    for row in y + 1..self.rows {
                        self.buffer.lines[row].fill(Cell::blank(self.style));
                    }
                }
                1 => {
                    self.erase_line(0, x + 1);
                    for row in 0..y {
                        self.buffer.lines[row].fill(Cell::blank(self.style));
                    }
                }
                2 => {
                    for line in &mut self.buffer.lines {
                        line.fill(Cell::blank(self.style));
                    }
                }
                3 => self.history.clear(),
                _ => {}
            },
            b'K' => match p {
                0 => self.erase_line(x, line_end),
                1 => self.erase_line(0, x + 1),
                2 => self.erase_line(0, line_end),
                _ => {}
            },
            b'X' => self.erase_line(x, (x + count).min(line_end)),
            b'P' | b'@' => {
                let count = count.min(line_end.saturating_sub(x));
                let line = &mut self.buffer.lines[y];
                if command == b'P' {
                    line[x..].rotate_left(count);
                    line[line_end - count..].fill(Cell::blank(self.style));
                } else {
                    line[x..].rotate_right(count);
                    line[x..x + count].fill(Cell::blank(self.style));
                }
            }
            b'L' | b'M' if y >= self.buffer.top && y <= self.buffer.bottom => {
                let top = self.buffer.top;
                self.buffer.top = y;
                if command == b'L' {
                    self.scroll_down(count);
                } else {
                    self.scroll_up(count);
                }
                self.buffer.top = top;
            }
            b'S' => self.scroll_up(count),
            b'T' => self.scroll_down(count),
            b'm' => self.sgr(&params),
            b's' => self.save_cursor(),
            b'u' => self.restore_cursor(),
            b'r' if !private => {
                let top = count - 1;
                let bottom = params
                    .get(1)
                    .copied()
                    .filter(|&v| v != 0)
                    .unwrap_or(self.rows)
                    - 1;
                if top < bottom && bottom < self.rows {
                    self.buffer.top = top;
                    self.buffer.bottom = bottom;
                    self.buffer.x = 0;
                    self.buffer.y = if self.origin { top } else { 0 };
                }
            }
            b'h' | b'l' if private => {
                let enabled = command == b'h';
                for mode in params {
                    match mode {
                        1 => self.application_cursor = enabled,
                        6 => {
                            self.origin = enabled;
                            self.buffer.x = 0;
                            self.buffer.y = if enabled { self.buffer.top } else { 0 };
                        }
                        7 => self.auto_wrap = enabled,
                        25 => self.cursor_visible = enabled,
                        47 | 1047 | 1049 => {
                            if enabled && self.alternate.is_none() {
                                self.save_cursor();
                                self.alternate = Some(std::mem::replace(
                                    &mut self.buffer,
                                    Buffer::new(self.cols, self.rows),
                                ));
                            } else if !enabled && let Some(saved) = self.alternate.take() {
                                self.buffer = saved;
                                self.restore_cursor();
                            }
                        }
                        _ => {}
                    }
                }
            }
            b'n' if !private && p == 5 => replies.extend_from_slice(b"\x1b[0n"),
            b'n' if !private && p == 6 => {
                replies.extend_from_slice(format!("\x1b[{};{}R", y + 1, x + 1).as_bytes())
            }
            b'c' if !private => replies.extend_from_slice(b"\x1b[?1;2c"),
            _ => {}
        }
        if !matches!(command, b'm' | b'n' | b'c' | b'h' | b'l') {
            self.buffer.wrap_pending = false;
        }
    }

    fn erase_line(&mut self, start: usize, end: usize) {
        let line = &mut self.buffer.lines[self.buffer.y];
        let start = start.min(line.len());
        let end = end.min(line.len()).max(start);
        line[start..end].fill(Cell::blank(self.style));
    }

    fn sgr(&mut self, params: &[usize]) {
        let mut i = 0;
        while i < params.len() {
            match params[i] {
                0 => self.style = Style::default(),
                1 => self.style.bold = true,
                4 => self.style.underline = true,
                7 => self.style.inverse = true,
                22 => self.style.bold = false,
                24 => self.style.underline = false,
                27 => self.style.inverse = false,
                30..=37 => self.style.fg_color = Some(Color::Indexed((params[i] - 30) as u8)),
                40..=47 => self.style.bg_color = Some(Color::Indexed((params[i] - 40) as u8)),
                90..=97 => self.style.fg_color = Some(Color::Indexed((params[i] - 90 + 8) as u8)),
                100..=107 => {
                    self.style.bg_color = Some(Color::Indexed((params[i] - 100 + 8) as u8))
                }
                39 => self.style.fg_color = None,
                49 => self.style.bg_color = None,
                38 | 48 => {
                    let is_fg_color = params[i] == 38;
                    let color = if params.get(i + 1) == Some(&5) && i + 2 < params.len() {
                        i += 2;
                        Some(Color::Indexed(params[i].min(255) as u8))
                    } else if params.get(i + 1) == Some(&2) && i + 4 < params.len() {
                        let color = [
                            params[i + 2].min(255) as u8,
                            params[i + 3].min(255) as u8,
                            params[i + 4].min(255) as u8,
                        ];
                        i += 4;
                        Some(Color::Rgb(color))
                    } else {
                        None
                    };
                    if let Some(color) = color {
                        if is_fg_color {
                            self.style.fg_color = Some(color);
                        } else {
                            self.style.bg_color = Some(color);
                        }
                    }
                }
                _ => {}
            }
            i += 1;
        }
    }
}

pub(crate) fn line_width(line: &[Cell]) -> usize {
    line.iter()
        .rposition(|cell| {
            (!cell.text.is_empty() && cell.text != " ")
                || cell.continuation
                || cell.style.bg_color.is_some()
                || cell.style.inverse
                || cell.style.underline
        })
        .map_or(0, |last| last + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unwrapped_lines_survive_resize_history_and_alternate_screen() {
        let mut screen = Screen::new(4, 3);
        screen.preserve_long_lines();
        let text = "abc界e\u{301}-tail";
        screen.feed(text.as_bytes());
        assert_eq!(screen.cols, 4, "Long output must not widen the PTY");
        assert!(screen.text().starts_with(text));
        screen.resize(3, 3);
        assert!(screen.text().starts_with(text));
        let cursor = screen.cursor();
        screen.feed(b"\x1b[?1049habcdef");
        assert!(screen.text().starts_with("abc\ndef"));
        screen.feed(b"\x1b[?1049l");
        assert_eq!(screen.cursor(), cursor);
        assert!(screen.text().starts_with(text));
        screen.feed(b"\r\nnext\r\nlast\r\n");
        assert!(
            screen.text().starts_with(text),
            "Scrollback retains the complete line"
        );
    }

    #[test]
    fn unwrapped_cursor_edits_reset_and_safety_wrap_preserve_output() {
        let mut screen = Screen::new(4, 3);
        screen.preserve_long_lines();
        screen.feed(b"abcdefgh\x1b[2DXY\x1b[K");
        assert!(screen.text().starts_with("abcdefXY"));
        screen.feed(b"\r\x1b[2K0123456\tX");
        assert!(screen.text().starts_with("0123456 X"));
        screen.feed(b"\x1bcabcdef");
        assert!(
            screen.text().starts_with("abcdef"),
            "Reset keeps the wrap choice"
        );
        screen.feed(b"\x1bc");
        screen.feed(&vec![b'x'; MAX_LINE_COLS + 10]);
        assert_eq!(screen.text().replace('\n', "").len(), MAX_LINE_COLS + 10);
        assert!((0..screen.rows).all(|row| screen.line(row).len() <= MAX_LINE_COLS));
    }

    #[test]
    fn color_indices_survive_scrollback_and_remain_distinct_from_truecolor() {
        let mut screen = Screen::new(20, 2);
        let bytes = b"\x1b[31;104mA\x1b[38;5;1;48;5;255mB\x1b[38;2;205;49;49;48;2;1;2;3mC\x1b[39;49mD\r\n\r\n";
        for byte in bytes {
            screen.feed(&[*byte]);
        }
        let line = &screen.history[0];
        assert_eq!(line[0].style.fg_color, Some(Color::Indexed(1)));
        assert_eq!(line[0].style.bg_color, Some(Color::Indexed(12)));
        assert_eq!(line[1].style.fg_color, Some(Color::Indexed(1)));
        assert_eq!(line[1].style.bg_color, Some(Color::Indexed(255)));
        assert_eq!(line[2].style.fg_color, Some(Color::Rgb([205, 49, 49])));
        assert_eq!(line[2].style.bg_color, Some(Color::Rgb([1, 2, 3])));
        assert_eq!(line[3].style, Style::default());
    }

    #[test]
    fn meaningful_output_detection_handles_chunked_controls_and_unicode() {
        let silent: &[&[u8]] = &[
            b"\r\x1b[2K\x1b[3D", // Pause EO's exact output.
            b" \t\r\n\0\x07\x08\x7f",
            b"\x1b[31m\x1b[0m\x1b[?25l\x1b[?1049h\x1b[?1049l\x1bc",
            b"\x1b]0;window title\x07\x1b]8;;https://example.test\x1b\\",
            b"\x1bPignored payload\x1b\\\x1bXignored\x1b\\\x1b^ignored\x1b\\\x1b_ignored\x1b\\",
            b"\x1b%G\x1b(B\x1b)0\x1b[12\x18\x1b]cancelled\x1a",
            b"\x1b[999;", // An unfinished control sequence is still not text.
            "\u{a0}\u{2003}\u{200b}\u{200d}\u{feff}\u{fe0f}".as_bytes(),
        ];
        for bytes in silent {
            for split in 0..=bytes.len() {
                let mut screen = Screen::new(80, 24);
                screen.feed(&bytes[..split]);
                screen.feed(&bytes[split..]);
                screen.finish();
                assert!(!screen.has_printed_text(), "Silent bytes: {bytes:?}");
            }
        }
        for bytes in [
            b"\x1b[31mmessage\x1b[0m".as_slice(),
            "é 界 e\u{301} 👩\u{200d}💻".as_bytes(),
            b"\x1b(0q\x1b(B", // DEC line drawing is visible text.
            b"\x1b[?1049hbrief output\x1b[?1049l",
            b"\xff",     // Invalid UTF-8 renders a replacement glyph.
            b"\xe2\x82", // So does incomplete UTF-8 at EOF.
        ] {
            let mut screen = Screen::new(80, 24);
            for byte in bytes {
                screen.feed(&[*byte]);
            }
            screen.finish();
            assert!(screen.has_printed_text(), "Visible bytes: {bytes:?}");
            screen.feed(b"\x1b[2J\x1b[H\x1bc");
            assert!(
                screen.has_printed_text(),
                "Clears/resets cannot forget text"
            );
        }
        let mut screen = Screen::new(80, 24);
        screen.feed(b"\x1b[12\x18visible after cancellation");
        assert!(screen.has_printed_text());
    }

    #[test]
    fn parsing_yields_at_a_deadline_and_preserves_partial_sequences() {
        let mut screen = Screen::new(80, 24);
        let bytes = "\x1b[32mé\x1b[0m".as_bytes();
        assert_eq!(screen.feed_until(bytes, Instant::now()).0, 0);
        assert!(screen.text().trim().is_empty());
        for chunk in bytes.chunks(2) {
            assert_eq!(
                screen
                    .feed_until(chunk, Instant::now() + std::time::Duration::from_secs(1))
                    .0,
                chunk.len()
            );
        }
        assert_eq!(screen.line(0)[0].text, "é");
        assert!(screen.line(0)[0].style.fg_color.is_some());
        let flood = vec![b'x'; 1_000_000];
        let started = Instant::now();
        let (count, _) = screen.feed_until(&flood, started + std::time::Duration::from_millis(1));
        assert!(
            count < flood.len(),
            "A flood must yield before parsing the entire buffer"
        );
        assert!(started.elapsed() < std::time::Duration::from_millis(500));
    }

    #[test]
    fn repeated_combining_marks_do_not_grow_a_cell_without_limit() {
        let mut screen = Screen::new(80, 24);
        screen.feed(format!("a{}b", "\u{301}".repeat(100_000)).as_bytes());
        assert!(screen.line(0)[0].text.len() <= 64);
        assert!(screen.line(0)[0].text.starts_with("a\u{301}"));
        assert_eq!(screen.line(0)[1].text, "b");
    }

    #[test]
    fn redraw_erase_colors_and_cursor_queries() {
        let mut screen = Screen::new(20, 4);
        screen.feed(b"old content\r\nsecond\x1b[H\x1b[31;1mnew\x1b[K");
        assert_eq!(screen.text().lines().next(), Some("new"));
        assert_eq!(screen.line(0)[0].style.fg_color, Some(Color::Indexed(1)));
        assert!(screen.line(0)[0].style.bold);
        assert_eq!(screen.feed(b"\x1b[6n"), b"\x1b[1;4R");
    }

    #[test]
    fn chunked_utf8_and_escape_sequences_do_not_leak_controls() {
        let mut screen = Screen::new(30, 3);
        for byte in "\x1b[32mcafé 界 e\u{301}\x1b[0m\x1b]0;ignored title\x07!".as_bytes() {
            screen.feed(&[*byte]);
        }
        assert_eq!(screen.text().trim(), "café 界 e\u{301}!");
        assert!(screen.line(0)[6].continuation);
    }

    #[test]
    fn scrolling_is_bounded_and_alternate_screen_restores_content() {
        let mut screen = Screen::new(10, 3);
        for _ in 0..2100 {
            screen.feed(b"line\r\n");
        }
        assert_eq!(screen.history.len(), HISTORY_LIMIT);
        let text = screen.text();
        screen.feed(b"\x1b[?1049hother");
        assert_eq!(screen.text().trim(), "other");
        screen.feed(b"\x1b[?1049l");
        assert_eq!(screen.text(), text);
        screen.resize(5, 2);
        screen.feed(b"\x1b[999999999999;999999999999H!\x1b[9999999999P");
        assert!(screen.cursor().0 < 5 && screen.cursor().1 < 2);
    }

    #[test]
    fn carriage_return_wrap_and_scrolling_region() {
        let mut screen = Screen::new(5, 4);
        screen.feed(b"12345\rabc\r\nrow2\r\nrow3\r\nrow4");
        assert_eq!(screen.text().lines().next(), Some("abc45"));
        screen.feed(b"\x1b[2;3r\x1b[3;1H\n");
        assert_eq!(screen.text().lines().nth(1), Some("row3"));
        assert_eq!(screen.text().lines().nth(3), Some("row4"));
        assert!(screen.history.is_empty());
    }

    #[test]
    fn shrinking_height_preserves_recent_output_and_moves_old_lines_to_history() {
        let mut screen = Screen::new(10, 4);
        screen.feed(b"one\r\ntwo\r\nthree\r\nfour");
        screen.resize(10, 2);
        assert_eq!(screen.text(), "one\ntwo\nthree\nfour");
        assert_eq!(screen.history.len(), 2);
        assert_eq!(screen.cursor(), (4, 1));
    }
}
