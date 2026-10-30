//! The console of FICS (spec m8-9, D2604): the text of the server and what
//! the user typed, bounded.

use std::collections::VecDeque;

/// Most lines kept; the oldest go first.
pub const MAX_LINES: usize = 2000;

/// Where a line comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum LineKind {
    /// Typed by the user (masked when it holds a password).
    Input,
    /// Said by the server.
    Server,
    /// A tell, a shout or a channel.
    Chat,
    /// Said by the app (connected, disconnected).
    System,
}

/// One line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsoleLine {
    /// Where it comes from.
    pub kind: LineKind,
    /// The text, without control characters.
    pub text: String,
}

/// The lines.
#[derive(Debug, Clone, Default)]
pub struct Console {
    lines: VecDeque<ConsoleLine>,
}

impl Console {
    /// An empty console.
    #[must_use]
    pub fn new() -> Console {
        Console::default()
    }

    /// Adds a line. Two empty server lines in a row are one.
    pub fn push(&mut self, kind: LineKind, text: &str) {
        let text: String = text.chars().filter(|c| !c.is_control()).collect();
        if kind == LineKind::Server
            && text.trim().is_empty()
            && self
                .lines
                .back()
                .is_none_or(|l| l.kind == LineKind::Server && l.text.is_empty())
        {
            return;
        }
        if self.lines.len() >= MAX_LINES {
            self.lines.pop_front();
        }
        self.lines.push_back(ConsoleLine { kind, text });
    }

    /// The lines, oldest first.
    pub fn lines(&self) -> impl ExactSizeIterator<Item = &ConsoleLine> {
        self.lines.iter()
    }

    /// How many lines.
    #[must_use]
    pub fn len(&self) -> usize {
        self.lines.len()
    }

    /// Whether there are none.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    /// Removes everything.
    pub fn clear(&mut self) {
        self.lines.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_and_blank_lines_collapse() {
        let mut c = Console::new();
        c.push(LineKind::Server, "");
        assert!(c.is_empty(), "no leading blank");
        c.push(LineKind::Server, "a");
        c.push(LineKind::Server, "");
        c.push(LineKind::Server, "  ");
        assert_eq!(c.len(), 2);
        for i in 0..(MAX_LINES + 5) {
            c.push(LineKind::Chat, &i.to_string());
        }
        assert_eq!(c.len(), MAX_LINES);
        assert_eq!(c.lines().last().map(|l| l.text.as_str()), Some("2004"));
    }

    #[test]
    fn control_characters_never_get_in() {
        let mut c = Console::new();
        c.push(LineKind::Server, "a\u{1b}[31mb\u{7}");
        assert_eq!(c.lines().next().map(|l| l.text.as_str()), Some("a[31mb"));
    }
}
