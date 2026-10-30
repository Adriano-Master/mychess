//! Writer options: the MyChess counterpart of Scid's `PGN_STYLE_*` mask
//! (`src/game.h:198-211`) plus the export dialog's extras (width, line ending
//! and text encoding).

use mychess_core::FenCastlingStyle;

/// Which header tags are written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum TagSet {
    /// Only the Seven Tag Roster, plus `Variant`, `SetUp` and `FEN` when they
    /// apply (Scid's output without `PGN_STYLE_TAGS`).
    SevenTagRoster,
    /// Every tag: ratings, `ECO`, `EventDate` and the extra tags.
    #[default]
    All,
}

/// How NAGs are written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum NagOutput {
    /// NAGs are left out.
    None,
    /// Numeric form, `$14` (the default).
    #[default]
    Numeric,
    /// ASCII symbols the reader understands (`!`, `?!`, `+=`...) through
    /// `Nag::ascii`; NAGs without a symbol stay numeric.
    Symbolic,
}

/// Header layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum Header {
    /// Standard PGN tag pairs, one per line.
    #[default]
    Pgn,
    /// Scid's three-line summary (`PGN_STYLE_SHORT_HEADER`). It is **not**
    /// PGN and must not be saved to a file.
    Short,
}

/// What to do with null moves (`--`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum NullMoves {
    /// Write `--` (the default).
    #[default]
    Keep,
    /// From the first null move on, the rest of the same line (with its nested
    /// variations) goes inside a single comment: `{-- 2. d4 ...}`.
    ConvertToComment,
}

/// What to do with `{` and `}` inside comment text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum BraceHandling {
    /// Replace `{` with `(` and `}` with `)` (the default).
    #[default]
    ReplaceWithParens,
    /// Delete the braces.
    Strip,
}

/// Maximum line length of the movetext.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum LineWidth {
    /// Never wrap.
    Unlimited,
    /// Wrap so that no line is longer than this many characters (a single
    /// indivisible word may still exceed it). Zero is treated as one.
    Columns(u16),
}

/// Line terminator of the output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum Newline {
    /// `\n` (the default; also what Scid writes).
    #[default]
    Lf,
    /// `\r\n`.
    CrLf,
}

/// Text encoding of the output bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum Encoding {
    /// UTF-8 without a byte order mark (the default).
    #[default]
    Utf8,
    /// UTF-8 starting with a BOM (`EF BB BF`), as Scid writes UTF-8 files.
    Utf8Bom,
    /// ISO 8859-1. Characters that do not fit become `?`, one per character.
    Latin1,
}

/// Options of the PGN writer.
///
/// [`WriteOptions::default`] gives **strict** PGN: full tags, numeric NAGs,
/// variations in parentheses, comments in braces, `1. e4 e5`, 80 columns and
/// UTF-8. [`WriteOptions::scid_export`] reproduces the defaults of Scid's
/// export dialog.
///
/// The struct is `#[non_exhaustive]`: start from a constructor and assign the
/// fields you care about, or chain the `with_*` builders.
///
/// ```
/// use mychess_pgn::write::{LineWidth, NagOutput, WriteOptions};
///
/// let mut opts = WriteOptions::default();
/// opts.nags = NagOutput::Symbolic;
/// opts.width = LineWidth::Unlimited;
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct WriteOptions {
    /// Which tags are written (`PGN_STYLE_TAGS`).
    pub tags: TagSet,
    /// Write comments (`PGN_STYLE_COMMENTS`). Independent of [`Self::nags`].
    pub comments: bool,
    /// NAG style (`PGN_STYLE_SYMBOLS` and the NAG half of `COMMENTS`).
    pub nags: NagOutput,
    /// Write variations (`PGN_STYLE_VARS`).
    pub variations: bool,
    /// Put main-line comments on their own indented lines
    /// (`PGN_STYLE_INDENT_COMMENTS`).
    pub indent_comments: bool,
    /// Start each variation on a new line, indented four spaces per level
    /// (`PGN_STYLE_INDENT_VARS`).
    pub indent_variations: bool,
    /// Header layout (`PGN_STYLE_SHORT_HEADER`).
    pub header: Header,
    /// `1. e4` (true) or `1.e4` (false) (`PGN_STYLE_MOVENUM_SPACE`).
    pub move_number_space: bool,
    /// One full move per line in tab-separated columns (`PGN_STYLE_COLUMN`).
    /// Forces [`Self::indent_comments`] and [`Self::indent_variations`].
    pub column: bool,
    /// Write the `ScidFlags` tag (`PGN_STYLE_SCIDFLAGS`).
    pub scid_flags: bool,
    /// Remove `[%...]` commands from comments (`PGN_STYLE_STRIP_MARKS`).
    pub strip_marks: bool,
    /// Null move handling (`PGN_STYLE_NO_NULL_MOVES`).
    pub null_moves: NullMoves,
    /// Braces inside comments (`PGN_STYLE_STRIP_BRACES`).
    pub braces: BraceHandling,
    /// Maximum line width (`SetWrapColumn`).
    pub width: LineWidth,
    /// Line terminator.
    pub newline: Newline,
    /// Output encoding.
    pub encoding: Encoding,
    /// Notation of the castling field of the `FEN` tag written for Chess960
    /// games (X-FEN by default). Standard games always use `KQkq`.
    pub fen_castling: FenCastlingStyle,
}

impl Default for WriteOptions {
    /// Strict PGN, see the type documentation.
    fn default() -> WriteOptions {
        WriteOptions {
            tags: TagSet::All,
            comments: true,
            nags: NagOutput::Numeric,
            variations: true,
            indent_comments: false,
            indent_variations: false,
            header: Header::Pgn,
            move_number_space: true,
            column: false,
            scid_flags: false,
            strip_marks: false,
            null_moves: NullMoves::Keep,
            braces: BraceHandling::ReplaceWithParens,
            width: LineWidth::Columns(80),
            newline: Newline::Lf,
            encoding: Encoding::Utf8,
            fen_castling: FenCastlingStyle::XFen,
        }
    }
}

impl WriteOptions {
    /// The defaults of Scid's export dialog (`tcl/start.tcl:838-850`):
    /// indented variations, 75 columns, null moves converted to comments and
    /// Latin-1 output. Everything else equals [`WriteOptions::default`].
    pub fn scid_export() -> WriteOptions {
        WriteOptions {
            indent_variations: true,
            null_moves: NullMoves::ConvertToComment,
            width: LineWidth::Columns(75),
            encoding: Encoding::Latin1,
            ..WriteOptions::default()
        }
    }

    /// The options of the text that header searches match against
    /// (`sc_search header -pgn`, `tkscid.cpp:17900-17907`): all tags,
    /// comments and variations, NAGs as symbols (`!`, `+=`), `1.e4` without a
    /// space after the move number, no line wrapping, `\n` and UTF-8.
    ///
    /// Not meant to be saved: it is only the haystack of a substring search.
    pub fn search_text() -> WriteOptions {
        WriteOptions {
            nags: NagOutput::Symbolic,
            move_number_space: false,
            width: LineWidth::Unlimited,
            ..WriteOptions::default()
        }
    }

    // Builders: the struct is `#[non_exhaustive]`, so code outside the crate
    // cannot use struct-update syntax.

    /// Sets [`Self::tags`].
    #[must_use]
    pub fn with_tags(mut self, tags: TagSet) -> WriteOptions {
        self.tags = tags;
        self
    }

    /// Sets [`Self::comments`].
    #[must_use]
    pub fn with_comments(mut self, comments: bool) -> WriteOptions {
        self.comments = comments;
        self
    }

    /// Sets [`Self::nags`].
    #[must_use]
    pub fn with_nags(mut self, nags: NagOutput) -> WriteOptions {
        self.nags = nags;
        self
    }

    /// Sets [`Self::variations`].
    #[must_use]
    pub fn with_variations(mut self, variations: bool) -> WriteOptions {
        self.variations = variations;
        self
    }

    /// Sets [`Self::indent_comments`].
    #[must_use]
    pub fn with_indent_comments(mut self, indent_comments: bool) -> WriteOptions {
        self.indent_comments = indent_comments;
        self
    }

    /// Sets [`Self::indent_variations`].
    #[must_use]
    pub fn with_indent_variations(mut self, indent_variations: bool) -> WriteOptions {
        self.indent_variations = indent_variations;
        self
    }

    /// Sets [`Self::header`].
    #[must_use]
    pub fn with_header(mut self, header: Header) -> WriteOptions {
        self.header = header;
        self
    }

    /// Sets [`Self::move_number_space`].
    #[must_use]
    pub fn with_move_number_space(mut self, move_number_space: bool) -> WriteOptions {
        self.move_number_space = move_number_space;
        self
    }

    /// Sets [`Self::column`].
    #[must_use]
    pub fn with_column(mut self, column: bool) -> WriteOptions {
        self.column = column;
        self
    }

    /// Sets [`Self::scid_flags`].
    #[must_use]
    pub fn with_scid_flags(mut self, scid_flags: bool) -> WriteOptions {
        self.scid_flags = scid_flags;
        self
    }

    /// Sets [`Self::strip_marks`].
    #[must_use]
    pub fn with_strip_marks(mut self, strip_marks: bool) -> WriteOptions {
        self.strip_marks = strip_marks;
        self
    }

    /// Sets [`Self::null_moves`].
    #[must_use]
    pub fn with_null_moves(mut self, null_moves: NullMoves) -> WriteOptions {
        self.null_moves = null_moves;
        self
    }

    /// Sets [`Self::braces`].
    #[must_use]
    pub fn with_braces(mut self, braces: BraceHandling) -> WriteOptions {
        self.braces = braces;
        self
    }

    /// Sets [`Self::width`].
    #[must_use]
    pub fn with_width(mut self, width: LineWidth) -> WriteOptions {
        self.width = width;
        self
    }

    /// Sets [`Self::newline`].
    #[must_use]
    pub fn with_newline(mut self, newline: Newline) -> WriteOptions {
        self.newline = newline;
        self
    }

    /// Sets [`Self::encoding`].
    #[must_use]
    pub fn with_encoding(mut self, encoding: Encoding) -> WriteOptions {
        self.encoding = encoding;
        self
    }

    /// Sets [`Self::fen_castling`].
    #[must_use]
    pub fn with_fen_castling(mut self, style: FenCastlingStyle) -> WriteOptions {
        self.fen_castling = style;
        self
    }

    /// `indent_comments` after applying the `column` override.
    pub(crate) fn effective_indent_comments(&self) -> bool {
        self.indent_comments || self.column
    }

    /// `indent_variations` after applying the `column` override.
    pub(crate) fn effective_indent_variations(&self) -> bool {
        self.indent_variations || self.column
    }

    /// The wrap width in characters.
    pub(crate) fn wrap_width(&self) -> usize {
        match self.width {
            LineWidth::Unlimited => usize::MAX,
            LineWidth::Columns(n) => usize::from(n.max(1)),
        }
    }
}

#[cfg(test)]
#[allow(clippy::field_reassign_with_default)] // tests tweak one option at a time
mod tests {
    use super::*;

    #[test]
    fn default_is_strict_pgn() {
        let o = WriteOptions::default();
        assert_eq!(o.tags, TagSet::All);
        assert!(o.comments && o.variations && o.move_number_space);
        assert_eq!(o.nags, NagOutput::Numeric);
        assert!(!o.indent_comments && !o.indent_variations && !o.column);
        assert_eq!(o.width, LineWidth::Columns(80));
        assert_eq!(o.encoding, Encoding::Utf8);
        assert_eq!(o.null_moves, NullMoves::Keep);
        assert_eq!(o.braces, BraceHandling::ReplaceWithParens);
    }

    #[test]
    fn scid_export_matches_the_dialog_defaults() {
        let o = WriteOptions::scid_export();
        assert!(o.indent_variations && !o.indent_comments);
        assert_eq!(o.width, LineWidth::Columns(75));
        assert_eq!(o.null_moves, NullMoves::ConvertToComment);
        assert_eq!(o.encoding, Encoding::Latin1);
        assert_eq!(o.nags, NagOutput::Numeric);
        assert!(!o.scid_flags && !o.strip_marks && !o.column);
    }

    #[test]
    fn search_text_is_plain_symbolic_and_unwrapped() {
        let o = WriteOptions::search_text();
        assert_eq!(o.nags, NagOutput::Symbolic);
        assert_eq!(o.width, LineWidth::Unlimited);
        assert!(!o.move_number_space && o.comments && o.variations);
        assert_eq!(o.tags, TagSet::All);
        assert_eq!(o.encoding, Encoding::Utf8);
    }

    #[test]
    fn builders_set_their_field() {
        let o = WriteOptions::default()
            .with_tags(TagSet::SevenTagRoster)
            .with_comments(false)
            .with_nags(NagOutput::Symbolic)
            .with_variations(false)
            .with_indent_comments(true)
            .with_indent_variations(true)
            .with_header(Header::Short)
            .with_move_number_space(false)
            .with_column(true)
            .with_scid_flags(true)
            .with_strip_marks(true)
            .with_null_moves(NullMoves::ConvertToComment)
            .with_braces(BraceHandling::Strip)
            .with_width(LineWidth::Unlimited)
            .with_newline(Newline::CrLf)
            .with_encoding(Encoding::Latin1)
            .with_fen_castling(FenCastlingStyle::Shredder);
        let mut e = WriteOptions::default();
        e.tags = TagSet::SevenTagRoster;
        e.comments = false;
        e.nags = NagOutput::Symbolic;
        e.variations = false;
        e.indent_comments = true;
        e.indent_variations = true;
        e.header = Header::Short;
        e.move_number_space = false;
        e.column = true;
        e.scid_flags = true;
        e.strip_marks = true;
        e.null_moves = NullMoves::ConvertToComment;
        e.braces = BraceHandling::Strip;
        e.width = LineWidth::Unlimited;
        e.newline = Newline::CrLf;
        e.encoding = Encoding::Latin1;
        e.fen_castling = FenCastlingStyle::Shredder;
        assert_eq!(o, e);
    }

    #[test]
    fn column_forces_indentation() {
        let mut o = WriteOptions::default();
        o.column = true;
        assert!(o.effective_indent_comments() && o.effective_indent_variations());
    }

    #[test]
    fn wrap_width_handles_zero_and_unlimited() {
        let mut o = WriteOptions::default();
        o.width = LineWidth::Columns(0);
        assert_eq!(o.wrap_width(), 1);
        o.width = LineWidth::Unlimited;
        assert_eq!(o.wrap_width(), usize::MAX);
    }
}
