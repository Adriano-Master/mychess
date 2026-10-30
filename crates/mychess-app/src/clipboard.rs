//! Clipboard helpers: purposes, limits and the normalisation of pasted FEN
//! text (spec M2-2, D115-D117; Scid: `pasteFEN`, `tcl/edit.tcl:21-96`).
//!
//! The clipboard itself is a *host effect* executed by the front-end; this
//! module only holds the pure parts.

use mychess_core::{FenError, Position};

/// Largest clipboard text accepted, in bytes (D115).
pub const MAX_CLIPBOARD_BYTES: usize = 32 << 20;

/// Longest piece of a pasted text echoed back in a [`PasteFenError`].
pub const FEN_EXCERPT_CHARS: usize = 80;

/// How much of a pasted text is looked at when it is read as a FEN. A real
/// FEN has fewer than 100 characters; the rest is garbage.
const FEN_SCAN_CHARS: usize = 4096;

/// Why the clipboard text was requested; returned with the answer so that
/// `update` knows what to do with it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ClipboardPurpose {
    /// "Paste FEN": replace the game with this position.
    Fen,
    /// The "Paste FEN" button of the setup dialog: load the draft.
    SetupFen,
    /// Pre-fill the PGN import dialog.
    ImportPrefill,
    /// "Paste moves": parse as a variation at the cursor.
    Moves,
}

/// Why no clipboard text could be used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ClipboardError {
    /// The clipboard is empty or holds no text.
    Empty,
    /// The text is longer than [`MAX_CLIPBOARD_BYTES`].
    TooLarge {
        /// The limit that was exceeded.
        limit: usize,
    },
    /// The clipboard could not be read.
    Unavailable,
}

impl ClipboardError {
    /// The Fluent key of the message.
    pub fn message_key(&self) -> &'static str {
        match self {
            ClipboardError::Empty => "clipboard-error-empty",
            ClipboardError::TooLarge { .. } => "clipboard-error-too-large",
            ClipboardError::Unavailable => "clipboard-error-unavailable",
        }
    }

    /// Checks the text the host read: empty (after trimming) and oversize
    /// texts are errors.
    pub fn check(text: &str) -> Result<(), ClipboardError> {
        if text.len() > MAX_CLIPBOARD_BYTES {
            Err(ClipboardError::TooLarge {
                limit: MAX_CLIPBOARD_BYTES,
            })
        } else if text.trim().is_empty() {
            Err(ClipboardError::Empty)
        } else {
            Ok(())
        }
    }
}

/// Why a pasted text is not a position.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum PasteFenError {
    /// Nothing but white space.
    Empty,
    /// The text is not a valid FEN.
    Invalid {
        /// The first 80 characters of the normalised text (no ellipsis).
        excerpt: String,
        /// What the FEN reader said.
        error: FenError,
    },
}

impl PasteFenError {
    /// The Fluent key of the message.
    pub fn message_key(&self) -> &'static str {
        match self {
            PasteFenError::Empty => "paste-fen-error-empty",
            PasteFenError::Invalid { .. } => "paste-fen-error-invalid",
        }
    }
}

/// Cleans text pasted as a FEN, like the Scid does (D116):
///
/// 1. removes non-ASCII characters and literal `\uXXXX` escapes;
/// 2. removes a leading `FEN:` (any case);
/// 3. joins the words with one space (line breaks become spaces);
/// 4. removes the trailing `/` of the board field;
/// 5. turns a `W`/`B` side-to-move field into lower case.
///
/// The function is idempotent.
///
/// ```
/// use mychess_app::normalize_fen_text;
///
/// assert_eq!(
///     normalize_fen_text("FEN: 8/8/8/8/8/8/8/K6k/ B 0 1"),
///     "8/8/8/8/8/8/8/K6k b 0 1"
/// );
/// ```
pub fn normalize_fen_text(raw: &str) -> String {
    let ascii = strip_unicode(raw);
    let mut rest = ascii.trim();
    while let Some(tail) = strip_fen_prefix(rest) {
        rest = tail.trim_start();
    }
    let mut words: Vec<&str> = rest.split_whitespace().collect();
    // A leading word made only of slashes is no board at all: drop it (and
    // keep dropping, so that the result is a fixed point).
    while let Some(board) = words.first_mut() {
        *board = board.trim_end_matches('/');
        if board.is_empty() {
            words.remove(0);
        } else {
            break;
        }
    }
    let lowered;
    if let Some(turn) = words.get_mut(1)
        && (*turn == "W" || *turn == "B")
    {
        lowered = turn.to_ascii_lowercase();
        *turn = &lowered;
    }
    words.join(" ")
}

/// Removes non-ASCII characters and `\uXXXX` literals (four hex digits).
fn strip_unicode(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len().min(FEN_SCAN_CHARS * 4));
    let mut chars = raw.chars().peekable();
    while let Some(c) = chars.next() {
        if !c.is_ascii() {
            continue;
        }
        if c == '\\' && chars.peek() == Some(&'u') {
            let mut ahead = chars.clone();
            ahead.next();
            let digits: Vec<char> = ahead.by_ref().take(4).collect();
            if digits.len() == 4 && digits.iter().all(char::is_ascii_hexdigit) {
                chars = ahead;
                continue;
            }
        }
        out.push(c);
    }
    out
}

fn strip_fen_prefix(text: &str) -> Option<&str> {
    let head = text.get(..4)?;
    head.eq_ignore_ascii_case("fen:").then(|| &text[4..])
}

/// Normalises `raw` and reads it as a position (tolerant FEN reader,
/// castling mode chosen automatically).
///
/// # Errors
///
/// [`PasteFenError::Empty`] for blank text, [`PasteFenError::Invalid`] for
/// anything the FEN reader refuses.
pub fn read_pasted_fen(raw: &str) -> Result<Position, PasteFenError> {
    let head: String = raw.chars().take(FEN_SCAN_CHARS).collect();
    let text = normalize_fen_text(&head);
    if text.is_empty() {
        return Err(PasteFenError::Empty);
    }
    Position::from_fen(&text).map_err(|error| PasteFenError::Invalid {
        excerpt: text.chars().take(FEN_EXCERPT_CHARS).collect(),
        error,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_prefix_trailing_slash_and_turn() {
        assert_eq!(
            normalize_fen_text("FEN: rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR/ W KQkq - 0 1"),
            "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1"
        );
        assert_eq!(
            normalize_fen_text("  fen:\n8/8/8/8/8/8/8/K6k  b  "),
            "8/8/8/8/8/8/8/K6k b"
        );
    }

    #[test]
    fn strips_non_ascii_and_unicode_escapes() {
        assert_eq!(normalize_fen_text("8/8\u{e9}/8 w"), "8/8/8 w");
        assert_eq!(normalize_fen_text("8/8\\u00e9/8 w"), "8/8/8 w");
        assert_eq!(normalize_fen_text("8\\u00 w"), "8\\u00 w");
        assert_eq!(normalize_fen_text("   \n\t "), "");
    }

    #[test]
    fn normalization_is_idempotent() {
        for raw in ["FEN:FEN: a//", "x  Y", "\\u", "fen:", "ab/ W 1/"] {
            let once = normalize_fen_text(raw);
            assert_eq!(normalize_fen_text(&once), once, "input {raw:?}");
        }
    }

    #[test]
    fn read_pasted_fen_reports_empty_and_invalid() {
        assert_eq!(read_pasted_fen(""), Err(PasteFenError::Empty));
        let Err(PasteFenError::Invalid { excerpt, .. }) = read_pasted_fen(&"x".repeat(500)) else {
            panic!("expected an invalid FEN");
        };
        assert_eq!(excerpt.chars().count(), FEN_EXCERPT_CHARS);
    }

    #[test]
    fn read_pasted_fen_is_tolerant() {
        let pos = read_pasted_fen("8/8/8/8/8/8/8/K6k w").map(|p| p.fen());
        assert_eq!(pos.as_deref(), Ok("8/8/8/8/8/8/8/K6k w - - 0 1"));
    }

    #[test]
    fn clipboard_check_enforces_limits() {
        assert_eq!(ClipboardError::check(" \n"), Err(ClipboardError::Empty));
        assert_eq!(ClipboardError::check("e4"), Ok(()));
        let big = "a".repeat(MAX_CLIPBOARD_BYTES + 1);
        assert!(matches!(
            ClipboardError::check(&big),
            Err(ClipboardError::TooLarge { .. })
        ));
    }
}
