//! The `style 12` board line of FICS (spec m8-9, D2606).
//!
//! ```text
//! <12> rnbqkbnr pppppppp -------- -------- ----P--- -------- PPPP-PPP RNBQKBNR
//!      B 4 1 1 1 1 0 12 GuestAAAA GuestBBBB -1 5 0 39 39 300 300 1 P/e2-e4 (0:00) e4 0
//! ```
//!
//! [`Style12::parse`] is pure and total: a malformed line is a
//! [`ParseError`], never a panic.

use std::fmt::Write as _;

use mychess_core::Color;

/// Why a `<12>` line was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ParseError {
    /// Not a `<12>` line.
    #[error("not a style 12 line")]
    NotStyle12,
    /// Fewer fields than the format has.
    #[error("style 12 line too short")]
    TooShort,
    /// A rank that is not eight board characters, or a bad character.
    #[error("invalid board rank")]
    BadBoard,
    /// A number or flag out of its domain.
    #[error("invalid field in style 12 line")]
    BadField,
}

/// How the user relates to the game (the `my relation` field).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Relation {
    /// `-3`: an isolated position (`ref`, `sposition`).
    Isolated,
    /// `-2`: observing an examined game.
    ObservingExamined,
    /// `-1`: playing, the opponent to move.
    PlayingOpponentToMove,
    /// `0`: observing a game being played.
    Observing,
    /// `1`: playing, the user to move.
    PlayingMyMove,
    /// `2`: examining.
    Examining,
    /// Any other value.
    Other(i32),
}

impl Relation {
    fn from_code(code: i32) -> Relation {
        match code {
            -3 => Relation::Isolated,
            -2 => Relation::ObservingExamined,
            -1 => Relation::PlayingOpponentToMove,
            0 => Relation::Observing,
            1 => Relation::PlayingMyMove,
            2 => Relation::Examining,
            other => Relation::Other(other),
        }
    }

    /// The user plays this game.
    #[must_use]
    pub fn is_playing(self) -> bool {
        matches!(
            self,
            Relation::PlayingOpponentToMove | Relation::PlayingMyMove
        )
    }

    /// The user watches this game without moving.
    #[must_use]
    pub fn is_observing(self) -> bool {
        matches!(self, Relation::Observing | Relation::ObservingExamined)
    }
}

/// The previous move in verbose coordinate notation (`P/e2-e4`, `o-o`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerboseMove {
    /// A move between two squares (files and ranks 0-7), with the
    /// promotion piece (lower case) when there is one.
    Step {
        /// File and rank of the origin.
        from: (u8, u8),
        /// File and rank of the target.
        to: (u8, u8),
        /// `q`, `r`, `b` or `n`.
        promotion: Option<char>,
    },
    /// `o-o`.
    CastleShort,
    /// `o-o-o`.
    CastleLong,
}

impl VerboseMove {
    /// Parses `P/e2-e4`, `N/g1-f3`, `P/e7-e8=Q`, `o-o`, `o-o-o`; `none` and
    /// anything else give `None`.
    #[must_use]
    pub fn parse(text: &str) -> Option<VerboseMove> {
        match text {
            "o-o" | "O-O" => return Some(VerboseMove::CastleShort),
            "o-o-o" | "O-O-O" => return Some(VerboseMove::CastleLong),
            _ => {}
        }
        let body = text.split_once('/').map_or(text, |(_, rest)| rest);
        let (squares, promo) = match body.split_once('=') {
            Some((s, p)) => (s, Some(p)),
            None => (body, None),
        };
        let (a, b) = squares.split_once('-')?;
        let promotion = match promo {
            None => None,
            Some(p) => {
                let c = p.chars().next()?.to_ascii_lowercase();
                if p.chars().count() != 1 || !matches!(c, 'q' | 'r' | 'b' | 'n') {
                    return None;
                }
                Some(c)
            }
        };
        Some(VerboseMove::Step {
            from: square(a)?,
            to: square(b)?,
            promotion,
        })
    }

    /// `e2e4` or `e7e8q`; castling has no coordinates here (it depends on
    /// the position), so it gives `None`.
    #[must_use]
    pub fn uci(self) -> Option<String> {
        let VerboseMove::Step {
            from,
            to,
            promotion,
        } = self
        else {
            return None;
        };
        let name = |(f, r): (u8, u8)| format!("{}{}", char::from(b'a' + f), char::from(b'1' + r));
        let mut out = format!("{}{}", name(from), name(to));
        if let Some(p) = promotion {
            out.push(p);
        }
        Some(out)
    }
}

fn square(text: &str) -> Option<(u8, u8)> {
    let b = text.as_bytes();
    if b.len() != 2 || !(b'a'..=b'h').contains(&b[0]) || !(b'1'..=b'8').contains(&b[1]) {
        return None;
    }
    Some((b[0] - b'a', b[1] - b'1'))
}

/// One `<12>` line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Style12 {
    /// The board, `board[0]` is the 8th rank; `-` empty, letters pieces
    /// (upper case White).
    pub board: [[u8; 8]; 8],
    /// Side to move.
    pub turn: Color,
    /// File (0-7) of a double pawn push just made.
    pub double_push: Option<u8>,
    /// White may castle short.
    pub white_short: bool,
    /// White may castle long.
    pub white_long: bool,
    /// Black may castle short.
    pub black_short: bool,
    /// Black may castle long.
    pub black_long: bool,
    /// Plies since the last irreversible move.
    pub reversible: u32,
    /// The number of the game.
    pub game: u32,
    /// White's name.
    pub white: String,
    /// Black's name.
    pub black: String,
    /// The relation of the user to the game.
    pub relation: Relation,
    /// Initial time in minutes.
    pub initial_minutes: u32,
    /// Increment in seconds.
    pub increment: u32,
    /// White's remaining time, in seconds (can be negative).
    pub white_time: i64,
    /// Black's remaining time, in seconds.
    pub black_time: i64,
    /// The number of the move about to be made.
    pub move_number: u32,
    /// The previous move, `None` at the start.
    pub verbose: Option<VerboseMove>,
    /// The raw text of the previous move (`P/e2-e4`, `o-o`), `None` when
    /// `none`.
    pub verbose_text: Option<String>,
    /// Time taken by the previous move, in milliseconds.
    pub move_time_ms: Option<u32>,
    /// The previous move in SAN, `None` at the start.
    pub san: Option<String>,
    /// Black is at the bottom.
    pub flipped: bool,
}

/// Longest name kept from the server.
const MAX_NAME: usize = 32;

impl Style12 {
    /// Parses a `<12>` line.
    ///
    /// # Errors
    /// [`ParseError`] when the line is not a well-formed style 12 line.
    pub fn parse(line: &str) -> Result<Style12, ParseError> {
        let mut it = line.split_whitespace();
        if it.next() != Some("<12>") {
            return Err(ParseError::NotStyle12);
        }
        let tokens: Vec<&str> = it.collect();
        // 30 fields after the tag; the flip field (the last) is missing in
        // very old servers.
        if tokens.len() < 29 {
            return Err(ParseError::TooShort);
        }
        let mut board = [[b'-'; 8]; 8];
        for (rank, token) in tokens[..8].iter().enumerate() {
            let bytes = token.as_bytes();
            if bytes.len() != 8 || !bytes.iter().all(|b| b"-PNBRQKpnbrqk".contains(b)) {
                return Err(ParseError::BadBoard);
            }
            board[rank].copy_from_slice(bytes);
        }
        let turn = match tokens[8] {
            "W" => Color::White,
            "B" => Color::Black,
            _ => return Err(ParseError::BadField),
        };
        let num = |i: usize| -> Result<i64, ParseError> {
            tokens
                .get(i)
                .and_then(|t| t.parse::<i64>().ok())
                .ok_or(ParseError::BadField)
        };
        let uns = |i: usize| -> Result<u32, ParseError> {
            u32::try_from(num(i)?).map_err(|_| ParseError::BadField)
        };
        let flag = |i: usize| -> Result<bool, ParseError> {
            match tokens.get(i).copied() {
                Some("0") => Ok(false),
                Some("1") => Ok(true),
                _ => Err(ParseError::BadField),
            }
        };
        let double_push = match num(9)? {
            -1 => None,
            f @ 0..=7 => Some(u8::try_from(f).map_err(|_| ParseError::BadField)?),
            _ => return Err(ParseError::BadField),
        };
        let name = |i: usize| -> String {
            tokens[i]
                .chars()
                .filter(|c| !c.is_control())
                .take(MAX_NAME)
                .collect()
        };
        let verbose_text = tokens
            .get(26)
            .copied()
            .filter(|t| *t != "none")
            .map(str::to_owned);
        let verbose = verbose_text.as_deref().and_then(VerboseMove::parse);
        let san = tokens
            .get(28)
            .copied()
            .filter(|t| *t != "none")
            .map(|t| t.chars().filter(|c| !c.is_control()).take(16).collect());
        Ok(Style12 {
            board,
            turn,
            double_push,
            white_short: flag(10)?,
            white_long: flag(11)?,
            black_short: flag(12)?,
            black_long: flag(13)?,
            reversible: uns(14)?,
            game: uns(15)?,
            white: name(16),
            black: name(17),
            relation: Relation::from_code(
                i32::try_from(num(18)?).map_err(|_| ParseError::BadField)?,
            ),
            initial_minutes: uns(19)?,
            increment: uns(20)?,
            white_time: num(23)?,
            black_time: num(24)?,
            move_number: uns(25)?.max(1),
            verbose,
            verbose_text,
            move_time_ms: tokens.get(27).and_then(|t| parse_move_time(t)),
            san,
            flipped: tokens.get(29).is_some_and(|t| *t == "1"),
        })
    }

    /// The FEN of the position (the en passant square is only given when the
    /// double push was just made, as the server says).
    #[must_use]
    pub fn to_fen(&self) -> String {
        let mut out = String::new();
        for (i, rank) in self.board.iter().enumerate() {
            if i > 0 {
                out.push('/');
            }
            let mut empty = 0u8;
            for &c in rank {
                if c == b'-' {
                    empty += 1;
                } else {
                    if empty > 0 {
                        out.push(char::from(b'0' + empty));
                        empty = 0;
                    }
                    out.push(char::from(c));
                }
            }
            if empty > 0 {
                out.push(char::from(b'0' + empty));
            }
        }
        out.push(' ');
        out.push(if self.turn == Color::White { 'w' } else { 'b' });
        out.push(' ');
        let mut castling = String::new();
        for (ok, c) in [
            (self.white_short, 'K'),
            (self.white_long, 'Q'),
            (self.black_short, 'k'),
            (self.black_long, 'q'),
        ] {
            if ok {
                castling.push(c);
            }
        }
        out.push_str(if castling.is_empty() { "-" } else { &castling });
        out.push(' ');
        match self.double_push {
            Some(file) => {
                let rank = if self.turn == Color::White { '6' } else { '3' };
                out.push(char::from(b'a' + file));
                out.push(rank);
            }
            None => out.push('-'),
        }
        write!(out, " {} {}", self.reversible, self.move_number).ok();
        out
    }
}

/// `(0:40)`, `(1:02:38)` or `(0:03.250)` to milliseconds.
#[must_use]
pub fn parse_move_time(text: &str) -> Option<u32> {
    let inner = text.strip_prefix('(')?.strip_suffix(')')?;
    let mut parts = inner.split(':');
    let mut total_ms: u64 = 0;
    let mut count = 0;
    let mut fields: Vec<&str> = Vec::new();
    for p in parts.by_ref() {
        fields.push(p);
        count += 1;
        if count > 3 {
            return None;
        }
    }
    let last = fields.pop()?;
    let (secs, millis) = match last.split_once('.') {
        Some((s, ms)) => {
            let ms: String = ms.chars().take(3).collect();
            let scale = 10u64.pow(3 - u32::try_from(ms.len()).ok()?);
            (s, ms.parse::<u64>().ok()? * scale)
        }
        None => (last, 0),
    };
    total_ms += secs.parse::<u64>().ok()? * 1000 + millis;
    let mut unit = 60_000u64;
    for f in fields.iter().rev() {
        total_ms += f.parse::<u64>().ok()? * unit;
        unit *= 60;
    }
    u32::try_from(total_ms).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    const START_E4: &str = "<12> rnbqkbnr pppppppp -------- -------- ----P--- -------- PPPP-PPP RNBQKBNR B 4 1 1 1 1 0 12 GuestAAAA GuestBBBB -1 5 0 39 39 300 300 1 P/e2-e4 (0:00) e4 0";

    #[test]
    fn parses_a_normal_line() {
        let s = Style12::parse(START_E4).expect("parses");
        assert_eq!(s.turn, Color::Black);
        assert_eq!(s.game, 12);
        assert_eq!(s.white, "GuestAAAA");
        assert_eq!(s.relation, Relation::PlayingOpponentToMove);
        assert!(s.relation.is_playing());
        assert_eq!((s.initial_minutes, s.increment), (5, 0));
        assert_eq!((s.white_time, s.black_time), (300, 300));
        assert_eq!(s.move_number, 1);
        assert_eq!(s.san.as_deref(), Some("e4"));
        assert_eq!(s.move_time_ms, Some(0));
        assert!(!s.flipped);
        assert_eq!(s.double_push, Some(4));
        assert_eq!(
            s.to_fen(),
            "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq e3 0 1"
        );
        assert_eq!(
            s.verbose.and_then(VerboseMove::uci).as_deref(),
            Some("e2e4")
        );
    }

    #[test]
    fn start_position_has_no_previous_move() {
        let line = "<12> rnbqkbnr pppppppp -------- -------- -------- -------- PPPPPPPP RNBQKBNR W -1 1 1 1 1 0 7 a b 1 2 12 39 39 120 120 1 none (0:00) none 0";
        let s = Style12::parse(line).expect("parses");
        assert_eq!(s.verbose, None);
        assert_eq!(s.san, None);
        assert_eq!(s.relation, Relation::PlayingMyMove);
        assert_eq!(
            s.to_fen(),
            "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1"
        );
    }

    #[test]
    fn castling_and_promotion_verbose_moves() {
        assert_eq!(VerboseMove::parse("o-o"), Some(VerboseMove::CastleShort));
        assert_eq!(VerboseMove::parse("o-o-o"), Some(VerboseMove::CastleLong));
        assert_eq!(
            VerboseMove::parse("P/e7-e8=Q")
                .and_then(VerboseMove::uci)
                .as_deref(),
            Some("e7e8q")
        );
        assert_eq!(VerboseMove::parse("none"), None);
        assert_eq!(VerboseMove::parse("P/e9-e8"), None);
        assert_eq!(VerboseMove::parse("P/e7-e8=X"), None);
    }

    #[test]
    fn move_times() {
        assert_eq!(parse_move_time("(0:40)"), Some(40_000));
        assert_eq!(parse_move_time("(1:02:38)"), Some(3_758_000));
        assert_eq!(parse_move_time("(0:03.25)"), Some(3_250));
        assert_eq!(parse_move_time("0:40"), None);
        assert_eq!(parse_move_time("(a:b)"), None);
        assert_eq!(parse_move_time("(1:2:3:4)"), None);
    }

    #[test]
    fn malformed_lines_are_errors_not_panics() {
        assert_eq!(Style12::parse("hello"), Err(ParseError::NotStyle12));
        assert_eq!(Style12::parse("<12> a b"), Err(ParseError::TooShort));
        let bad_rank = START_E4.replacen("rnbqkbnr", "rnbqkbn", 1);
        assert_eq!(Style12::parse(&bad_rank), Err(ParseError::BadBoard));
        let bad_turn = START_E4.replacen(" B 4", " X 4", 1);
        assert_eq!(Style12::parse(&bad_turn), Err(ParseError::BadField));
        let bad_game = START_E4.replacen(" 12 Guest", " -9 Guest", 1);
        assert_eq!(Style12::parse(&bad_game), Err(ParseError::BadField));
    }

    #[test]
    fn negative_times_and_old_servers_without_the_flip_field() {
        let line = START_E4.replace("300 300 1 P/e2-e4 (0:00) e4 0", "-4 12 1 P/e2-e4 (0:00) e4");
        let s = Style12::parse(&line).expect("parses");
        assert_eq!((s.white_time, s.black_time), (-4, 12));
        assert!(!s.flipped);
    }
}
