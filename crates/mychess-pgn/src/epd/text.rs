//! Editing the operations of one record as plain text (spec M7-5a, D3807).
//!
//! The text has one operation per line, `opcode operands`, without the
//! trailing `;`. This is what the EPD window shows in its editor.

use super::record::{EpdOperation, EpdRecord, is_standard_opcode};

/// Longest standard opcode, in characters (spec M7-5, rule 6).
const MAX_OPCODE_LEN: usize = 14;

/// A soft problem found by [`EpdRecord::set_operations_text`]. The line is
/// kept anyway (like the reader does), the caller decides whether to warn.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum TextWarning {
    /// The opcode is not `[A-Za-z][A-Za-z0-9_]*` of at most 14 characters.
    NonStandardOpcode {
        /// The 1-based line of the text.
        line: usize,
        /// The opcode as found.
        opcode: String,
    },
}

impl EpdRecord {
    /// The operations as text: one `opcode operands` line per operation
    /// (just `opcode` when there are no operands), each ending in `\n`.
    /// No `;` is written, so operands keep their quotes verbatim.
    ///
    /// ```
    /// use mychess_core::Position;
    /// use mychess_pgn::epd::{EpdOperation, EpdRecord};
    ///
    /// let rec = EpdRecord::with_operations(
    ///     Position::new(),
    ///     vec![EpdOperation::new("bm", "e4"), EpdOperation::new("noop", "")],
    /// );
    /// assert_eq!(rec.operations_text(), "bm e4\nnoop\n");
    /// ```
    pub fn operations_text(&self) -> String {
        let mut out = String::new();
        for op in self.operations() {
            out.push_str(&op.opcode);
            if !op.operands.is_empty() {
                out.push(' ');
                out.push_str(&op.operands);
            }
            out.push('\n');
        }
        out
    }

    /// Replaces the operations with those parsed from `text` (see
    /// [`EpdRecord::operations_text`]). Lines end in `\n`, `\r\n` or `\r`;
    /// blank lines are dropped; the opcode is the first word, the operands
    /// the rest, trimmed. Nothing is validated beyond the opcode shape: a
    /// non-standard opcode is **kept** and reported. Empty text leaves the
    /// record with the position only. A `;` outside quotes cannot be
    /// written back in the standard dialect; writing the book then fails
    /// with `EpdError::Unwritable`.
    ///
    /// Scid: `tcl/file/epd.tcl` and `sc_epd set` store the text without any
    /// validation; the warning is new.
    pub fn set_operations_text(&mut self, text: &str) -> Vec<TextWarning> {
        let mut warnings = Vec::new();
        let mut operations = Vec::new();
        for (i, raw) in split_lines(text).enumerate() {
            let line = raw.trim();
            if line.is_empty() {
                continue;
            }
            let end = line.find(char::is_whitespace).unwrap_or(line.len());
            let (opcode, operands) = line.split_at(end);
            if !is_standard_opcode(opcode) || opcode.chars().count() > MAX_OPCODE_LEN {
                warnings.push(TextWarning::NonStandardOpcode {
                    line: i + 1,
                    opcode: opcode.to_owned(),
                });
            }
            operations.push(EpdOperation::new(opcode, operands));
        }
        *self.operations_mut() = operations;
        warnings
    }
}

/// Splits on `\n`, `\r\n` and `\r`.
fn split_lines(text: &str) -> impl Iterator<Item = &str> {
    let mut rest = Some(text);
    std::iter::from_fn(move || {
        let cur = rest?;
        if let Some(i) = cur.find(['\n', '\r']) {
            let (head, tail) = cur.split_at(i);
            rest = Some(
                tail.strip_prefix("\r\n")
                    .or_else(|| tail.get(1..))
                    .unwrap_or(""),
            );
            Some(head)
        } else {
            rest = None;
            Some(cur)
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::epd::{EpdBook, EpdOptions};
    use mychess_core::Position;

    fn rec() -> EpdRecord {
        EpdRecord::new(Position::new())
    }

    fn pairs(r: &EpdRecord) -> Vec<(&str, &str)> {
        r.operations()
            .iter()
            .map(|o| (o.opcode.as_str(), o.operands.as_str()))
            .collect()
    }

    #[test]
    fn roundtrips_typical_record() {
        let mut r = rec();
        let text = "id \"WAC.001\"\nbm Qxh7+\nce 25\n";
        assert_eq!(r.set_operations_text(text).len(), 0);
        assert_eq!(
            pairs(&r),
            vec![("id", "\"WAC.001\""), ("bm", "Qxh7+"), ("ce", "25")]
        );
        assert_eq!(r.operations_text(), text);
    }

    #[test]
    fn semicolon_inside_quotes_survives_the_book_roundtrip() {
        let mut r = rec();
        r.set_operations_text("id \"a; b\"\nc0 x\\y\n");
        assert_eq!(r.operations_text(), "id \"a; b\"\nc0 x\\y\n");
        for opts in [EpdOptions::standard(), EpdOptions::scid()] {
            let mut book = EpdBook::new();
            book.insert(Position::new(), r.operations().to_vec());
            let mut buf = Vec::new();
            book.write(&mut buf, opts).expect("write");
            let back = EpdBook::read(buf.as_slice(), opts).expect("read").book;
            assert_eq!(back, book);
        }
    }

    #[test]
    fn non_standard_opcode_is_kept_and_reported_with_its_line() {
        let mut r = rec();
        let w = r.set_operations_text("bm e4\n1x foo\nabcdefghijklmno x\n");
        assert_eq!(
            w,
            vec![
                TextWarning::NonStandardOpcode {
                    line: 2,
                    opcode: "1x".into()
                },
                TextWarning::NonStandardOpcode {
                    line: 3,
                    opcode: "abcdefghijklmno".into()
                },
            ]
        );
        assert_eq!(r.operations().len(), 3);
    }

    #[test]
    fn empty_text_clears_operations_and_blank_lines_are_dropped() {
        let mut r = rec();
        r.set_operations_text("bm e4");
        assert_eq!(r.set_operations_text("").len(), 0);
        assert_eq!(r.operations().len(), 0);
        r.set_operations_text("\n  \nbm e4\r\n\r\n\tce 1  \r");
        assert_eq!(pairs(&r), vec![("bm", "e4"), ("ce", "1")]);
        assert_eq!(r.set_operations_text("  \n\n").len(), 0);
        assert_eq!(r.operations().len(), 0);
    }

    #[test]
    fn operand_without_operands_has_no_trailing_space() {
        let mut r = rec();
        r.set_operations_text("noop");
        assert_eq!(r.operations_text(), "noop\n");
    }

    #[test]
    fn unicode_text_is_kept() {
        let mut r = rec();
        r.set_operations_text("c0 \"Zürich, 日本\"");
        assert_eq!(r.operations_text(), "c0 \"Zürich, 日本\"\n");
    }
}
