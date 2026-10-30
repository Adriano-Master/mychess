//! Line layout of the movetext: the MyChess counterpart of Scid's
//! `TextBuffer` (`src/textbuf.cpp`), without its accidents (spec D32 b, c).
//!
//! The caller feeds *words*. A word is written after a single space, or on a
//! new line when it would pass the width. Pieces can be glued to the
//! neighbours (`(` in front of the next word, `)` and `}` after the last one),
//! so a line never starts with `)` or `}` and never ends with a space.

/// Accumulates the movetext with width-based wrapping.
#[derive(Debug)]
pub(crate) struct Layout {
    out: String,
    newline: &'static str,
    width: usize,
    /// Characters on the current line, indentation included.
    col: usize,
    /// Whether the current line already has a word.
    line_has_content: bool,
    indent: usize,
    /// A line break was requested; it happens before the next word.
    need_break: bool,
    /// The word being built; it is written when the next one starts.
    cur: String,
    cur_chars: usize,
    /// `cur` must not be preceded by a space.
    cur_attached: bool,
    /// Text glued in front of the next word (`(`, `{`).
    prefix: String,
    /// The last written *unit*: a word that started with a space, plus the
    /// glued pieces and the `%` words that followed it. A `%` word that does
    /// not fit takes its unit to the next line, because `%` must never start
    /// a line.
    unit: Unit,
}

/// Where the last written unit starts (see `Layout::unit`).
#[derive(Debug, Default, Clone, Copy)]
struct Unit {
    /// Byte offset in the output of the space that precedes the unit.
    space_at: usize,
    /// Characters of the unit, without that space.
    chars: usize,
    /// The unit is preceded by a space on a line, so it can move down.
    movable: bool,
}

impl Layout {
    /// A layout wrapping at `width` characters, ending lines with `newline`.
    pub(crate) fn new(width: usize, newline: &'static str) -> Layout {
        Layout {
            out: String::new(),
            newline,
            width,
            col: 0,
            line_has_content: false,
            indent: 0,
            need_break: false,
            cur: String::new(),
            cur_chars: 0,
            cur_attached: false,
            prefix: String::new(),
            unit: Unit::default(),
        }
    }

    /// Text to glue in front of the next word.
    pub(crate) fn add_prefix(&mut self, text: &str) {
        self.prefix.push_str(text);
    }

    /// Starts a new word (the previous one is written).
    pub(crate) fn word(&mut self, text: &str) {
        self.flush();
        self.cur_attached = false;
        if !self.prefix.is_empty() {
            self.cur.push_str(&self.prefix);
            self.cur_chars += self.prefix.chars().count();
            self.prefix.clear();
        }
        self.cur.push_str(text);
        self.cur_chars += text.chars().count();
    }

    /// Appends `text` to the current word, without a space.
    pub(crate) fn glue(&mut self, text: &str) {
        if self.cur.is_empty() {
            // Nothing to glue to: start a word that takes no leading space.
            self.cur_attached = true;
        }
        self.cur.push_str(text);
        self.cur_chars += text.chars().count();
    }

    /// Ends the current line before the next word (no-op on an empty line,
    /// so a break never produces a blank line).
    pub(crate) fn break_line(&mut self) {
        self.flush();
        if self.line_has_content {
            self.need_break = true;
        }
    }

    /// Sets the indentation used by the lines that start from now on.
    pub(crate) fn set_indent(&mut self, indent: usize) {
        self.indent = indent;
    }

    /// The indentation currently set.
    pub(crate) fn indent(&self) -> usize {
        self.indent
    }

    /// Writes `text` as a whole line of its own, bypassing the wrapping
    /// (column-style rows).
    pub(crate) fn raw_line(&mut self, text: &str) {
        self.flush();
        if self.line_has_content {
            self.out.push_str(self.newline);
        }
        self.out.push_str(text);
        self.out.push_str(self.newline);
        self.col = 0;
        self.line_has_content = false;
        self.need_break = false;
        self.unit.movable = false;
    }

    /// Writes the pending word.
    fn flush(&mut self) {
        if self.cur.is_empty() {
            return;
        }
        // A line that starts with `%` is an escape line in PGN, so such a
        // word is never moved to the start of a line: it stays after the
        // previous unit, and if it does not fit, that unit comes along.
        // A glued closer (`)` after another one) is in the same position: it
        // may not start a line either, and a pending line break waits for the
        // next word (Scid breaks before the closer; we follow D32 instead).
        let sticky = self.cur.starts_with('%') || self.cur_attached;
        if sticky && self.line_has_content {
            let space = usize::from(!self.cur_attached);
            let fits = self
                .col
                .checked_add(space)
                .and_then(|c| c.checked_add(self.cur_chars))
                .is_some_and(|c| c <= self.width);
            if !fits && self.unit.movable {
                self.move_unit_down();
            }
            if !self.cur_attached {
                self.need_break = false;
                self.out.push(' ');
            }
            self.out.push_str(&self.cur);
            self.col += space + self.cur_chars;
            self.unit.chars += space + self.cur_chars;
            self.clear_cur();
            return;
        }
        if self.line_has_content {
            let space = usize::from(!self.cur_attached);
            let overflows = self
                .col
                .checked_add(space)
                .and_then(|c| c.checked_add(self.cur_chars))
                .is_none_or(|c| c > self.width);
            if self.need_break || overflows {
                self.out.push_str(self.newline);
                self.col = 0;
                self.line_has_content = false;
            }
        }
        self.need_break = false;
        if self.line_has_content {
            // Here the word is never attached: those return above.
            self.unit = Unit {
                space_at: self.out.len(),
                chars: self.cur_chars,
                movable: true,
            };
            self.out.push(' ');
            self.col += 1;
        } else {
            for _ in 0..self.indent {
                self.out.push(' ');
            }
            self.col = self.indent;
            self.unit = Unit {
                space_at: self.out.len(),
                chars: self.cur_chars,
                movable: false,
            };
        }
        self.out.push_str(&self.cur);
        self.col += self.cur_chars;
        self.line_has_content = true;
        self.clear_cur();
    }

    fn clear_cur(&mut self) {
        self.cur.clear();
        self.cur_chars = 0;
        self.cur_attached = false;
    }

    /// Moves the last unit from the end of the current line to the start of
    /// a new one.
    fn move_unit_down(&mut self) {
        let text = self.out.split_off(self.unit.space_at + 1);
        self.out.truncate(self.unit.space_at);
        self.out.push_str(self.newline);
        for _ in 0..self.indent {
            self.out.push(' ');
        }
        self.out.push_str(&text);
        self.col = self.indent + self.unit.chars;
        self.unit = Unit {
            space_at: self.out.len() - text.len(),
            chars: self.unit.chars,
            movable: false,
        };
    }

    /// Ends the text with a line terminator and returns it.
    pub(crate) fn finish(mut self) -> String {
        self.flush();
        if self.line_has_content {
            self.out.push_str(self.newline);
        }
        self.out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    fn lay(width: usize, words: &[&str]) -> String {
        let mut l = Layout::new(width, "\n");
        for w in words {
            l.word(w);
        }
        l.finish()
    }

    #[rstest]
    #[case(10, &["aaaa", "bbbb", "cccc"], "aaaa bbbb\ncccc\n")]
    #[case(9, &["aaaa", "bbbb", "cccc"], "aaaa bbbb\ncccc\n")]
    #[case(8, &["aaaa", "bbbb", "cccc"], "aaaa\nbbbb\ncccc\n")]
    #[case(3, &["aaaa", "bb"], "aaaa\nbb\n")]
    #[case(usize::MAX, &["aaaa", "bbbb", "cccc"], "aaaa bbbb cccc\n")]
    fn wraps_without_splitting_words(
        #[case] width: usize,
        #[case] words: &[&str],
        #[case] expected: &str,
    ) {
        assert_eq!(lay(width, words), expected);
    }

    #[test]
    fn counts_characters_not_bytes() {
        // "ééé" is 3 characters and 6 bytes.
        assert_eq!(lay(7, &["ééé", "ééé"]), "ééé ééé\n");
        assert_eq!(lay(6, &["ééé", "ééé"]), "ééé\nééé\n");
    }

    #[test]
    fn glue_keeps_pieces_together_and_prefix_leads() {
        let mut l = Layout::new(11, "\n");
        l.word("aaaa");
        l.add_prefix("(");
        l.word("bbbb");
        l.glue(")");
        l.word("c");
        assert_eq!(l.finish(), "aaaa (bbbb)\nc\n");
    }

    #[test]
    fn glued_closer_never_starts_a_line() {
        let mut l = Layout::new(5, "\n");
        l.word("aaaa");
        l.word("bbbb");
        l.glue("}");
        assert_eq!(l.finish(), "aaaa\nbbbb}\n");
    }

    #[test]
    fn break_line_is_lazy_and_never_blank() {
        let mut l = Layout::new(80, "\n");
        l.break_line();
        l.word("a");
        l.break_line();
        l.break_line();
        l.set_indent(4);
        l.word("b");
        l.break_line();
        assert_eq!(l.finish(), "a\n    b\n");
    }

    #[test]
    fn indent_counts_towards_the_width() {
        let mut l = Layout::new(8, "\n");
        l.set_indent(4);
        l.word("aa");
        l.word("bb");
        l.word("cc");
        assert_eq!(l.finish(), "    aa\n    bb\n    cc\n");
    }

    #[test]
    fn percent_word_is_not_moved_to_line_start() {
        let mut l = Layout::new(4, "\n");
        l.word("aaaa");
        l.word("%x");
        assert_eq!(l.finish(), "aaaa %x\n");
        let mut l = Layout::new(80, "\n");
        l.word("a");
        l.break_line();
        l.word("%x");
        assert_eq!(l.finish(), "a %x\n");
    }

    #[test]
    fn overflowing_percent_word_takes_its_unit_down() {
        let mut l = Layout::new(9, "\n");
        l.word("aa");
        l.word("bbbb");
        l.word("%cc");
        l.word("%dd");
        assert_eq!(l.finish(), "aa\nbbbb %cc\n%dd\n".replace("\n%dd", " %dd"));
        let mut l = Layout::new(10, "\n");
        l.set_indent(2);
        l.word("aa");
        l.word("bbbb");
        l.word("%cc");
        assert_eq!(l.finish(), "  aa\n  bbbb %cc\n");
    }

    #[test]
    fn stacked_closers_stay_on_the_line_of_the_last_move() {
        let mut l = Layout::new(80, "\n");
        l.word("a");
        l.glue(")");
        l.break_line();
        l.glue(")");
        l.break_line();
        l.word("b");
        assert_eq!(l.finish(), "a))\nb\n");
        // A closer that does not fit takes its word along.
        let mut l = Layout::new(7, "\n");
        l.word("aa");
        l.word("bbbb");
        l.glue(")");
        l.break_line();
        l.glue(")");
        assert_eq!(l.finish(), "aa\nbbbb))\n");
    }

    #[test]
    fn crlf_line_ends() {
        let mut l = Layout::new(3, "\r\n");
        l.word("aa");
        l.word("bb");
        assert_eq!(l.finish(), "aa\r\nbb\r\n");
    }

    #[test]
    fn raw_line_stands_alone() {
        let mut l = Layout::new(3, "\n");
        l.word("a");
        l.raw_line("\tlong row");
        l.word("b");
        assert_eq!(l.finish(), "a\n\tlong row\nb\n");
    }

    #[test]
    fn empty_layout_finishes_empty() {
        assert_eq!(Layout::new(80, "\n").finish(), "");
    }
}
