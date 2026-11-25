//! The text a terminal would show for what a program printed.
//!
//! pacman redraws its progress bars by moving the cursor, so its output only
//! makes sense once those moves have been followed. This covers what command
//! line tools use: cursor movement, erasing, and skipping everything else.
//! Full-screen programs are out of scope.

#[derive(Debug, Default)]
pub struct Screen {
    lines: Vec<Vec<char>>,
    row: usize,
    col: usize,
    state: State,
    /// Start of a character whose remaining bytes have not arrived yet.
    partial: Vec<u8>,
}

#[derive(Debug, Default)]
enum State {
    #[default]
    Text,
    Escape,
    /// `ESC [`, collecting parameters.
    Control(String),
    /// `ESC ]`, such as a window title. Runs to a bell or `ESC \`.
    Title,
    TitleEscape,
}

/// A hostile or broken program must not make the grid grow without bounds.
const MAX_COLUMNS: usize = 1000;
const MAX_JUMP: usize = 200;

impl Screen {
    /// Takes in output and returns the rows it touched, in order.
    pub fn feed(&mut self, bytes: &[u8]) -> Vec<usize> {
        let mut touched = std::collections::BTreeSet::new();
        self.partial.extend_from_slice(bytes);
        let bytes = std::mem::take(&mut self.partial);
        let mut rest = bytes.as_slice();
        while !rest.is_empty() {
            match std::str::from_utf8(rest) {
                Ok(text) => {
                    text.chars().for_each(|c| self.put(c, &mut touched));
                    break;
                }
                Err(error) => {
                    let (valid, tail) = rest.split_at(error.valid_up_to());
                    let valid = std::str::from_utf8(valid).unwrap_or_default();
                    valid.chars().for_each(|c| self.put(c, &mut touched));
                    let Some(bad) = error.error_len() else {
                        self.partial = tail.to_vec();
                        break;
                    };
                    self.put(char::REPLACEMENT_CHARACTER, &mut touched);
                    rest = &tail[bad..];
                }
            }
        }
        touched.into_iter().collect()
    }

    pub fn len(&self) -> usize {
        self.lines.len()
    }

    /// A row without trailing blanks.
    pub fn line(&self, row: usize) -> String {
        let line: String = self.lines.get(row).into_iter().flatten().collect();
        line.trim_end().to_owned()
    }

    /// The line the cursor is on, if nothing follows it. A program that waits
    /// for an answer leaves its question there.
    pub fn open_line(&self) -> Option<String> {
        (self.row + 1 == self.lines.len() && self.col > 0)
            .then(|| self.line(self.row))
            .filter(|line| !line.trim().is_empty())
    }

    fn put(&mut self, c: char, touched: &mut std::collections::BTreeSet<usize>) {
        match &mut self.state {
            State::Text => match c {
                '\x1b' => self.state = State::Escape,
                '\n' => {
                    self.row += 1;
                    self.reach();
                }
                '\r' => self.col = 0,
                '\x08' => self.col = self.col.saturating_sub(1),
                '\t' => self.col = (self.col / 8 + 1) * 8,
                c if c.is_control() => {}
                c => {
                    self.reach();
                    let col = self.col.min(MAX_COLUMNS);
                    let line = &mut self.lines[self.row];
                    if line.len() <= col {
                        line.resize(col + 1, ' ');
                    }
                    line[col] = c;
                    self.col = col + 1;
                    touched.insert(self.row);
                }
            },
            State::Escape => {
                self.state = match c {
                    '[' => State::Control(String::new()),
                    ']' => State::Title,
                    _ => State::Text,
                }
            }
            State::Control(parameters) => match c {
                '\x40'..='\x7e' => {
                    let parameters = std::mem::take(parameters);
                    self.state = State::Text;
                    self.control(c, &parameters, touched);
                }
                c => parameters.push(c),
            },
            State::Title => match c {
                '\x07' => self.state = State::Text,
                '\x1b' => self.state = State::TitleEscape,
                _ => {}
            },
            State::TitleEscape => self.state = State::Text,
        }
    }

    fn control(
        &mut self,
        command: char,
        parameters: &str,
        touched: &mut std::collections::BTreeSet<usize>,
    ) {
        let first = parameters.split(';').next().and_then(|n| n.parse::<usize>().ok());
        let count = first.unwrap_or(1).clamp(1, MAX_JUMP);
        match command {
            'A' | 'F' => self.row = self.row.saturating_sub(count),
            'B' | 'E' => self.row += count,
            'C' => self.col += count,
            'D' => self.col = self.col.saturating_sub(count),
            'G' => self.col = count - 1,
            'K' => {
                self.reach();
                let line = &mut self.lines[self.row];
                match first.unwrap_or(0) {
                    0 => line.truncate(self.col),
                    1 => line.iter_mut().take(self.col + 1).for_each(|c| *c = ' '),
                    _ => line.clear(),
                }
                touched.insert(self.row);
            }
            _ => {}
        }
        if matches!(command, 'E' | 'F') {
            self.col = 0;
        }
        self.reach();
    }

    /// Makes sure the row under the cursor exists.
    fn reach(&mut self) {
        if self.lines.len() <= self.row {
            self.lines.resize(self.row + 1, Vec::new());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn show(bytes: &[u8]) -> Vec<String> {
        let mut screen = Screen::default();
        screen.feed(bytes);
        (0..screen.len()).map(|row| screen.line(row)).collect()
    }

    #[test]
    fn follows_the_cursor() {
        assert_eq!(show(b"one\r\ntwo\r\n"), ["one", "two", ""]);
        assert_eq!(show(b" 5%\r12%"), ["12%"]);
        // Two bars, the upper one redrawn: pacman goes up, prints, and returns.
        assert_eq!(show(b"a 1%\r\nb 1%\r\x1b[1Fa 9%\r\x1b[1Eb 2%"), ["a 9%", "b 2%"]);
        assert_eq!(show(b"gone\r\x1b[Knew"), ["new"]);
    }

    #[test]
    fn skips_what_is_not_text() {
        assert_eq!(show(b"\x1b[?25l\x1b[1;34m::\x1b[0;1m Hello\x1b[0m"), [":: Hello"]);
        assert_eq!(show(b"\x1b]0;title\x07ok"), ["ok"]);
        assert_eq!(show(b"\x1b]0;title\x1b\\ok"), ["ok"]);
    }

    #[test]
    fn survives_split_and_broken_input() {
        let mut screen = Screen::default();
        let text = "prüfen\x1b[1;34m→".as_bytes();
        for byte in text {
            screen.feed(&[*byte]);
        }
        assert_eq!(screen.line(0), "prüfen→");
        assert_eq!(show(b"a\xffb"), ["a\u{fffd}b"]);
        assert!(show(b"\x1b[999999999999B").len() < 1000);
        assert_eq!(show(b"\x1b[99999Bx").len(), MAX_JUMP + 1);
    }

    #[test]
    fn reports_touched_rows_and_open_questions() {
        let mut screen = Screen::default();
        assert_eq!(screen.feed(b"first\r\nsecond"), [0, 1]);
        assert_eq!(screen.feed(b" more"), [1]);
        assert_eq!(screen.open_line().as_deref(), Some("second more"));
        screen.feed(b"\r\n");
        assert_eq!(screen.open_line(), None);
    }
}
