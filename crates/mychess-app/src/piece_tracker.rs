//! The piece tracker: a heat map of the squares a piece (or a pair, or the
//! pawns of one colour) goes to over the games of the filter (spec
//! `docs/specs/m6-4-tabela-estatisticas-rastreador.md`, slice c, with the
//! controller of slice M6-4 "app", decisions D3385 to D3394 and D3410 to
//! D3429).
//!
//! - this file: the selection of the pieces ([`sister_squares`],
//!   [`initial_pieces`]), the mirrors of the mode and the colour scheme, and
//!   the `[ptracker]` preferences ([`PieceTrackerPrefs`]);
//! - `message`: [`PieceTrackerMessage`], [`PieceTrackerEffect`] and
//!   `perform`;
//! - `view`: [`PieceTrackerView`], the heat map with the colours already
//!   computed.
//!
//! The flow (the panel follows the filter, debounce, "Stop" with the partial
//! result) is in `state/piece_tracker_flow.rs`.

mod message;
mod view;

use mychess_core::{Color, File, Piece, Rank, Role, Square};
use mychess_db::{ColorScheme, TrackMode};
use serde::{Serialize, Serializer};

use crate::bases::{SharedSource, SlotId};
use crate::panel_calc::{FilterStamp, Refresh, named_enum};

pub(crate) use message::perform;
pub use message::{
    DEBOUNCE_MS, PieceTrackerEffect, PieceTrackerFailure, PieceTrackerMessage, PieceTrackerNotice,
};
pub(crate) use view::build_view;
pub use view::{PieceTrackerView, TrackerCell, TrackerRank};

named_enum! {
    /// What the counts mean (the "Statistic" choice).
    pub enum TrackerMode {
        /// `% games with move to square`.
        #[default]
        Games => "games",
        /// `% time on each square`.
        Time => "time",
    }
}

named_enum! {
    /// The colour of the heat map (`::ptrack::color`).
    pub enum TrackerScheme {
        /// Black.
        Black => "black",
        /// Red.
        Red => "red",
        /// Yellow.
        Yellow => "yellow",
        /// Cyan.
        Cyan => "cyan",
        /// Blue (the default).
        #[default]
        Blue => "blue",
        /// Inverted black.
        XBlack => "xblack",
        /// Inverted red.
        XRed => "xred",
        /// Inverted yellow.
        XYellow => "xyellow",
        /// Inverted cyan.
        XCyan => "xcyan",
        /// Inverted blue.
        XBlue => "xblue",
    }
}

impl TrackerMode {
    pub(crate) fn engine(self) -> TrackMode {
        match self {
            TrackerMode::Games => TrackMode::Games,
            TrackerMode::Time => TrackMode::Time,
        }
    }
}

impl TrackerScheme {
    pub(crate) fn engine(self) -> ColorScheme {
        match self {
            TrackerScheme::Black => ColorScheme::Black,
            TrackerScheme::Red => ColorScheme::Red,
            TrackerScheme::Yellow => ColorScheme::Yellow,
            TrackerScheme::Cyan => ColorScheme::Cyan,
            TrackerScheme::Blue => ColorScheme::Blue,
            TrackerScheme::XBlack => ColorScheme::XBlack,
            TrackerScheme::XRed => ColorScheme::XRed,
            TrackerScheme::XYellow => ColorScheme::XYellow,
            TrackerScheme::XCyan => ColorScheme::XCyan,
            TrackerScheme::XBlue => ColorScheme::XBlue,
        }
    }
}

/// Highest move number of the window (`ptracker.tcl` accepts 0 to 999).
pub const MAX_TRACK_MOVE: u16 = 999;

/// The 32 pieces of the starting position, White's first, from a1 (the
/// drawings below the heat map).
pub fn initial_pieces() -> Vec<(Square, Piece)> {
    let back = [
        Role::Rook,
        Role::Knight,
        Role::Bishop,
        Role::Queen,
        Role::King,
        Role::Bishop,
        Role::Knight,
        Role::Rook,
    ];
    let mut out = Vec::with_capacity(32);
    for (color, back_rank, pawn_rank) in [
        (Color::White, Rank::First, Rank::Second),
        (Color::Black, Rank::Eighth, Rank::Seventh),
    ] {
        for (file, role) in File::ALL.into_iter().zip(back) {
            out.push((Square::from_coords(file, back_rank), Piece { color, role }));
        }
        for file in File::ALL {
            out.push((
                Square::from_coords(file, pawn_rank),
                Piece {
                    color,
                    role: Role::Pawn,
                },
            ));
        }
    }
    out
}

/// The piece that starts on `square`, if one does.
pub fn initial_piece(square: Square) -> Option<Piece> {
    initial_pieces()
        .into_iter()
        .find(|(s, _)| *s == square)
        .map(|(_, p)| p)
}

/// The squares selected by the right button on `square`: the "sister" pieces
/// (both rooks, both knights, both bishops, the eight pawns of the colour);
/// the queen and the king only themselves. Empty when no piece starts there.
pub fn sister_squares(square: Square) -> Vec<Square> {
    let Some(piece) = initial_piece(square) else {
        return Vec::new();
    };
    match piece.role {
        Role::Queen | Role::King => vec![square],
        role => initial_pieces()
            .into_iter()
            .filter(|(_, p)| {
                *p == Piece {
                    color: piece.color,
                    role,
                }
            })
            .map(|(s, _)| s)
            .collect(),
    }
}

/// The window and the piece, in `[ptracker]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PieceTrackerPrefs {
    /// The starting squares of the followed pieces (initially `d1`).
    pub squares: Vec<Square>,
    /// First move counted, 0 to 999.
    pub start_move: u16,
    /// Last move counted, never below `start_move`.
    pub end_move: u16,
    /// Games or time.
    pub mode: TrackerMode,
    /// The colour of the map.
    pub scheme: TrackerScheme,
}

impl Default for PieceTrackerPrefs {
    fn default() -> PieceTrackerPrefs {
        PieceTrackerPrefs {
            squares: vec![Square::D1],
            start_move: 1,
            end_move: 20,
            mode: TrackerMode::Games,
            scheme: TrackerScheme::Blue,
        }
    }
}

impl PieceTrackerPrefs {
    /// The preferences with only starting squares (no repeats), moves up to
    /// 999 and the end not below the start.
    #[must_use]
    pub fn sanitized(mut self) -> PieceTrackerPrefs {
        let mut seen = Vec::new();
        for s in self.squares {
            if initial_piece(s).is_some() && !seen.contains(&s) {
                seen.push(s);
            }
        }
        seen.sort_by_key(|s| s.to_usize());
        self.squares = seen;
        self.start_move = self.start_move.min(MAX_TRACK_MOVE);
        self.end_move = self.end_move.min(MAX_TRACK_MOVE).max(self.start_move);
        self
    }
}

#[derive(Serialize)]
struct Table {
    squares: Vec<String>,
    start_move: u16,
    end_move: u16,
    mode: &'static str,
    scheme: &'static str,
}

impl Serialize for PieceTrackerPrefs {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        Table {
            squares: self.squares.iter().map(ToString::to_string).collect(),
            start_move: self.start_move,
            end_move: self.end_move,
            mode: self.mode.name(),
            scheme: self.scheme.name(),
        }
        .serialize(serializer)
    }
}

/// What the tracker was asked for. The colour scheme is not part of it: it
/// only paints.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TrackKey {
    pub(crate) slot: SlotId,
    pub(crate) source: SharedSource,
    pub(crate) len: u32,
    pub(crate) filter: FilterStamp,
    pub(crate) squares: Vec<Square>,
    pub(crate) start_move: u16,
    pub(crate) end_move: u16,
    pub(crate) mode: TrackerMode,
}

/// The state of the piece tracker.
#[derive(Debug, Default)]
pub struct PieceTrackerUi {
    pub(crate) open: bool,
    pub(crate) refresh: Refresh<TrackKey>,
    pub(crate) shown: Option<(TrackKey, mychess_db::PieceTrackResult)>,
    pub(crate) issue: Option<PieceTrackerNotice>,
}

impl PieceTrackerUi {
    /// Whether the panel is open.
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Whether a count is running.
    pub fn is_running(&self) -> bool {
        self.refresh.running()
    }

    /// Stops the count (the program is ending).
    pub(crate) fn cancel(&mut self) {
        self.refresh.cancel();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_start_position_has_32_pieces() {
        let pieces = initial_pieces();
        assert_eq!(pieces.len(), 32);
        assert_eq!(
            initial_piece(Square::E1),
            Some(Piece {
                color: Color::White,
                role: Role::King
            })
        );
        assert_eq!(initial_piece(Square::E4), None);
    }

    #[test]
    fn sisters_follow_the_scid() {
        assert_eq!(sister_squares(Square::A1), vec![Square::A1, Square::H1]);
        assert_eq!(sister_squares(Square::B8), vec![Square::B8, Square::G8]);
        assert_eq!(sister_squares(Square::F1), vec![Square::C1, Square::F1]);
        assert_eq!(sister_squares(Square::D1), vec![Square::D1]);
        assert_eq!(sister_squares(Square::E8), vec![Square::E8]);
        let pawns = sister_squares(Square::E2);
        assert_eq!(pawns.len(), 8);
        assert!(pawns.iter().all(|s| s.rank() == Rank::Second));
        assert!(sister_squares(Square::E4).is_empty());
    }

    #[test]
    fn sanitizing_keeps_start_squares_and_orders_the_window() {
        let p = PieceTrackerPrefs {
            squares: vec![Square::H1, Square::E4, Square::A1, Square::A1],
            start_move: 5000,
            end_move: 3,
            ..PieceTrackerPrefs::default()
        }
        .sanitized();
        assert_eq!(p.squares, vec![Square::A1, Square::H1]);
        assert_eq!((p.start_move, p.end_move), (999, 999));
    }

    #[test]
    fn names_round_trip() {
        for s in TrackerScheme::ALL {
            assert_eq!(TrackerScheme::from_name(s.name()), Some(*s));
        }
        assert_eq!(TrackerScheme::ALL.len(), 10);
    }
}
