//! The lines of the user: moves entered as text and the evaluation that ends
//! each one (spec M8-6, D2302, D2304, D2334).

use std::fmt::Write;

use mychess_core::{Color, Move, Position, SanMode, SanSuffix};

/// The most plies of one line (D2304).
pub const MAX_LINE_PLIES: usize = 40;
/// The most lines of one exercise (D2304).
pub const MAX_LINES: usize = 12;
/// The most characters of one entry of text (D2334).
pub const MAX_ENTRY_CHARS: usize = 400;

/// The seven symbols that end a line: the evaluation of the user, from the
/// point of view of White (Scid: the NAG buttons of `calvar.tcl`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Assessment {
    /// `-+`: Black is winning.
    BlackWinning,
    /// `-/+`: Black is clearly better.
    BlackBetter,
    /// `=+`: Black is slightly better.
    BlackSlightly,
    /// `=`: equal.
    Equal,
    /// `+=`: White is slightly better.
    WhiteSlightly,
    /// `+/-`: White is clearly better.
    WhiteBetter,
    /// `+-`: White is winning.
    WhiteWinning,
}

impl Assessment {
    /// Every value, from the best for Black to the best for White.
    pub const ALL: [Assessment; 7] = [
        Assessment::BlackWinning,
        Assessment::BlackBetter,
        Assessment::BlackSlightly,
        Assessment::Equal,
        Assessment::WhiteSlightly,
        Assessment::WhiteBetter,
        Assessment::WhiteWinning,
    ];

    /// The usual symbol (`+=`, `-/+`, ...).
    pub fn symbol(self) -> &'static str {
        match self {
            Assessment::BlackWinning => "-+",
            Assessment::BlackBetter => "-/+",
            Assessment::BlackSlightly => "=+",
            Assessment::Equal => "=",
            Assessment::WhiteSlightly => "+=",
            Assessment::WhiteBetter => "+/-",
            Assessment::WhiteWinning => "+-",
        }
    }

    /// Reads a symbol of [`Assessment::symbol`].
    pub fn from_symbol(text: &str) -> Option<Assessment> {
        Assessment::ALL.into_iter().find(|a| a.symbol() == text)
    }

    /// A number from -3 (Black winning) to 3 (White winning).
    pub fn rank(self) -> i8 {
        match self {
            Assessment::BlackWinning => -3,
            Assessment::BlackBetter => -2,
            Assessment::BlackSlightly => -1,
            Assessment::Equal => 0,
            Assessment::WhiteSlightly => 1,
            Assessment::WhiteBetter => 2,
            Assessment::WhiteWinning => 3,
        }
    }

    /// How close two evaluations are: 1 when equal, 0.5 when neighbours,
    /// 0 otherwise (D2314).
    pub fn closeness(self, other: Assessment) -> f32 {
        match (self.rank() - other.rank()).unsigned_abs() {
            0 => 1.0,
            1 => 0.5,
            _ => 0.0,
        }
    }
}

/// A line the user calculated: moves from the position of the exercise and
/// the evaluation that ends it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserLine {
    /// The moves, legal one after the other from the start position.
    pub moves: Vec<Move>,
    /// What the user thinks of the end of the line.
    pub assessment: Assessment,
}

/// The line in SAN with move numbers (`1. e4 e5 2. Nf3`), starting from
/// `start`. A move that does not fit the position ends the text (a line
/// made by the machine never has one).
pub fn san_line(start: &Position, moves: &[Move]) -> String {
    let mut pos = start.clone();
    let mut out = String::new();
    for (index, mv) in moves.iter().enumerate() {
        let Ok(san) = pos.san_with(*mv, SanSuffix::CheckOrMate) else {
            break;
        };
        if !out.is_empty() {
            out.push(' ');
        }
        let number = pos.fullmove_number();
        match pos.turn() {
            Color::White => {
                let _ = write!(out, "{number}. ");
            }
            Color::Black if index == 0 => {
                let _ = write!(out, "{number}... ");
            }
            Color::Black => {}
        }
        out.push_str(&san);
        let Ok(next) = pos.after(*mv) else {
            break;
        };
        pos = next;
    }
    out
}

/// What [`read_moves`] understood.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadMoves {
    /// The moves accepted, in order.
    pub moves: Vec<Move>,
    /// The first piece of text that is not a legal move there, if any; the
    /// reading stopped at it (D2334).
    pub rejected: Option<String>,
}

/// Strips a move number (`12.`, `12...`, `12…`) from the front of a token.
/// Digits count as a number only when a `.` or `…` follows them (D2350): a
/// token of digits alone is ignored, and `0-0` is a castling, not a number.
fn strip_number(token: &str) -> &str {
    let rest = token.trim_start_matches(|c: char| c.is_ascii_digit());
    if rest.is_empty() {
        // Only digits (including an empty token): nothing to read.
        return "";
    }
    if rest.len() == token.len() || !rest.starts_with(['.', '\u{2026}']) {
        return token;
    }
    rest.trim_start_matches(['.', '\u{2026}'])
}

/// The canonical spelling of a castling written with letters O or zeros
/// (`0-0`, `o-o-o`, `OO`), without the check marks; `None` when the token is
/// not one (D2351). `OO` is read as short castling only: the tolerant
/// reading of the core would turn an impossible `OO` into long castling.
fn castling_spelling(token: &str) -> Option<&'static str> {
    let bare = token.trim_end_matches(['+', '#', '!', '?']);
    if bare.is_empty() || !bare.chars().all(|c| matches!(c, '0' | 'O' | 'o' | '-')) {
        return None;
    }
    match bare.chars().filter(|c| *c != '-').count() {
        2 => Some("O-O"),
        3 => Some("O-O-O"),
        _ => None,
    }
}

/// Reads one token with the tolerant rules of the core, except castling,
/// which is read strictly from its canonical spelling.
fn parse_token(pos: &Position, token: &str) -> Option<Move> {
    let parsed = match castling_spelling(token) {
        Some(canonical) => pos.parse_san(canonical, SanMode::Strict),
        None if token
            .chars()
            .all(|c| matches!(c, '0' | 'O' | 'o' | '-' | '+' | '#')) =>
        {
            // Zeros and O's that are not a castling: no move.
            return None;
        }
        None => pos.parse_san(token, SanMode::Tolerant),
    };
    parsed.ok().map(|p| p.mv).filter(|mv| mv.from().is_some())
}

/// Reads moves in SAN or UCI, separated by spaces, from `start` (D2334).
/// Move numbers are ignored; the null move is not a move of a line. Never
/// panics, whatever the text.
pub fn read_moves(start: &Position, text: &str) -> ReadMoves {
    let mut pos = start.clone();
    let mut moves = Vec::new();
    let text: String = text.chars().take(MAX_ENTRY_CHARS).collect();
    for token in text.split_whitespace() {
        let token = strip_number(token);
        if token.is_empty() {
            continue;
        }
        let parsed = parse_token(&pos, token);
        let next = parsed.and_then(|mv| pos.after(mv).ok().map(|after| (mv, after)));
        let Some((mv, after)) = next else {
            return ReadMoves {
                moves,
                rejected: Some(token.chars().take(32).collect()),
            };
        };
        moves.push(mv);
        pos = after;
    }
    ReadMoves {
        moves,
        rejected: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_symbols_round_trip_and_are_ordered() {
        for a in Assessment::ALL {
            assert_eq!(Assessment::from_symbol(a.symbol()), Some(a));
        }
        assert_eq!(Assessment::from_symbol("?"), None);
        assert!(Assessment::BlackWinning < Assessment::Equal);
        assert!(Assessment::Equal < Assessment::WhiteWinning);
    }

    #[test]
    fn closeness_is_one_for_equal_half_for_neighbours() {
        assert!((Assessment::Equal.closeness(Assessment::Equal) - 1.0).abs() < f32::EPSILON);
        assert!((Assessment::Equal.closeness(Assessment::WhiteSlightly) - 0.5).abs() < 1e-6);
        assert!(Assessment::Equal.closeness(Assessment::WhiteBetter).abs() < 1e-6);
    }

    #[test]
    fn a_line_is_written_with_move_numbers() {
        let start = Position::new();
        let read = read_moves(&start, "1. e4 e5 2. Nf3 Nc6");
        assert_eq!(read.moves.len(), 4);
        assert_eq!(read.rejected, None);
        assert_eq!(san_line(&start, &read.moves), "1. e4 e5 2. Nf3 Nc6");
        let black =
            Position::from_fen("rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq - 0 1")
                .expect("fen");
        let read = read_moves(&black, "1...e5 2.Nf3");
        assert_eq!(read.moves.len(), 2);
        assert_eq!(san_line(&black, &read.moves), "1... e5 2. Nf3");
    }

    #[test]
    fn reading_stops_at_the_first_illegal_move_and_keeps_the_earlier_ones() {
        let start = Position::new();
        let read = read_moves(&start, "e4 e5 Qh5 Qxh5");
        assert_eq!(read.moves.len(), 3);
        assert_eq!(read.rejected.as_deref(), Some("Qxh5"));
        let none = read_moves(&start, "Ke2");
        assert!(none.moves.is_empty());
        assert_eq!(none.rejected.as_deref(), Some("Ke2"));
    }

    #[test]
    fn uci_text_and_the_null_move_are_handled() {
        let start = Position::new();
        assert_eq!(read_moves(&start, "e2e4 e7e5").moves.len(), 2);
        let null = read_moves(&start, "e4 --");
        assert_eq!(null.moves.len(), 1);
        assert_eq!(null.rejected.as_deref(), Some("--"));
        assert!(read_moves(&start, "").moves.is_empty());
        assert!(read_moves(&start, "   ").rejected.is_none());
    }

    #[test]
    fn huge_or_strange_text_is_cut_not_a_panic() {
        let start = Position::new();
        let long = "e4 ".repeat(1000);
        let read = read_moves(&start, &long);
        assert!(read.moves.len() <= MAX_ENTRY_CHARS / 3 + 1);
        let weird = read_moves(&start, "\u{0}\u{ffff}\u{1F600} 9999999999999999999999.");
        assert!(weird.moves.is_empty());
    }
}
