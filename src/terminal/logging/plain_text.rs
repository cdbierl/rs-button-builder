//! Streaming conversion from terminal output to readable UTF-8 logs.

#[derive(Default)]
enum AnsiState {
    #[default]
    Ground,
    Escape,
    EscapeIntermediate,
    Csi,
    String {
        escaped: bool,
    },
    Charset,
}

#[derive(Default)]
pub(super) struct PlainText {
    state: AnsiState,
    utf8: Vec<u8>,
    pending_cr: bool,
    line_has_text: bool,
}

impl PlainText {
    pub(super) fn push(&mut self, bytes: &[u8], output: &mut Vec<u8>) {
        for &byte in bytes {
            if matches!(byte, 0x18 | 0x1a) && !matches!(self.state, AnsiState::Ground) {
                self.state = AnsiState::Ground;
                continue;
            }
            if byte == 0x1b && !matches!(self.state, AnsiState::Ground | AnsiState::String { .. }) {
                self.finish_utf8(output);
                self.state = AnsiState::Escape;
                continue;
            }
            let state = std::mem::take(&mut self.state);
            match state {
                AnsiState::Ground => self.ground(byte, output),
                AnsiState::Escape => match byte {
                    b'[' => self.state = AnsiState::Csi,
                    b']' | b'P' | b'X' | b'^' | b'_' => {
                        self.state = AnsiState::String { escaped: false };
                    }
                    b'(' | b')' => self.state = AnsiState::Charset,
                    0x20..=0x2f => self.state = AnsiState::EscapeIntermediate,
                    0x00..=0x1f | 0x7f => self.state = AnsiState::Escape,
                    _ => {}
                },
                AnsiState::EscapeIntermediate => {
                    if !(0x30..=0x7e).contains(&byte) {
                        self.state = AnsiState::EscapeIntermediate;
                    }
                }
                AnsiState::Csi => {
                    if !(0x40..=0x7e).contains(&byte) {
                        self.state = AnsiState::Csi;
                    }
                }
                AnsiState::String { escaped } => {
                    if byte != 7 && byte != 0x9c && !(escaped && byte == b'\\') {
                        self.state = AnsiState::String {
                            escaped: byte == 0x1b,
                        };
                    }
                }
                AnsiState::Charset => {
                    if byte < 0x20 || byte == 0x7f {
                        self.state = AnsiState::Charset;
                    }
                }
            }
        }
    }

    fn ground(&mut self, byte: u8, output: &mut Vec<u8>) {
        if byte == 0x1b {
            self.finish_utf8(output);
            self.state = AnsiState::Escape;
        } else if byte.is_ascii() {
            self.finish_utf8(output);
            self.character(char::from(byte), output);
        } else if self.utf8.is_empty() && (0x80..=0x9f).contains(&byte) {
            self.state = match byte {
                0x90 | 0x98 | 0x9d..=0x9f => AnsiState::String { escaped: false },
                0x9b => AnsiState::Csi,
                _ => AnsiState::Ground,
            };
        } else {
            self.utf8.push(byte);
            match std::str::from_utf8(&self.utf8) {
                Ok(text) => {
                    let character = text.chars().next().unwrap();
                    self.utf8.clear();
                    self.character(character, output);
                }
                Err(error) if error.error_len().is_some() => {
                    let remaining = self.utf8.split_off(error.valid_up_to() + 1);
                    self.utf8.clear();
                    self.character('\u{fffd}', output);
                    for byte in remaining {
                        self.ground(byte, output);
                    }
                }
                Err(_) => {}
            }
        }
    }

    fn character(&mut self, character: char, output: &mut Vec<u8>) {
        match character {
            '\r' => {
                self.pending_cr |= self.line_has_text;
                self.line_has_text = false;
            }
            '\n' => {
                output.push(b'\n');
                self.pending_cr = false;
                self.line_has_text = false;
            }
            '\t' => {
                self.flush_carriage_return(output);
                output.push(b'\t');
                self.line_has_text = true;
            }
            '\u{0090}' | '\u{0098}' | '\u{009d}'..='\u{009f}' => {
                self.state = AnsiState::String { escaped: false };
            }
            '\u{009b}' => self.state = AnsiState::Csi,
            character if !character.is_control() => {
                self.flush_carriage_return(output);
                let mut encoded = [0; 4];
                output.extend_from_slice(character.encode_utf8(&mut encoded).as_bytes());
                self.line_has_text = true;
            }
            _ => {}
        }
    }

    fn flush_carriage_return(&mut self, output: &mut Vec<u8>) {
        if std::mem::take(&mut self.pending_cr) {
            output.push(b'\n');
        }
    }

    fn finish_utf8(&mut self, output: &mut Vec<u8>) {
        if !self.utf8.is_empty() {
            self.utf8.clear();
            self.character('\u{fffd}', output);
        }
    }

    pub(super) fn finish(&mut self, output: &mut Vec<u8>) {
        self.finish_utf8(output);
        self.flush_carriage_return(output);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_fragmented_terminal_formatting() {
        let input =
            b"\x1b[31mred\x1b[0m\r\nprogress 1\r\x1b[2Kprogress 2\t\xe7\x95\x8c\x1b]0;title\x07";
        for split in 0..=input.len() {
            let mut plain = PlainText::default();
            let mut output = Vec::new();
            plain.push(&input[..split], &mut output);
            plain.push(&input[split..], &mut output);
            plain.finish(&mut output);
            assert_eq!(output, "red\nprogress 1\nprogress 2\t界".as_bytes());
        }
    }

    #[test]
    fn malformed_utf8_does_not_discard_following_text() {
        let mut plain = PlainText::default();
        let mut output = Vec::new();
        plain.push(b"before \xe2after \xffdone", &mut output);
        plain.finish(&mut output);
        assert_eq!(output, "before �after �done".as_bytes());
    }
}
