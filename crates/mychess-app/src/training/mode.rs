//! The modes, the reasons they end and the rules of a finished game.

use std::ops::RangeInclusive;
use std::time::Duration;

use mychess_core::{GameEnd, Position, ZobristHash};

/// The default thinking time of the automove (`analysis(automoveTime)` 3000,
/// `analysis.tcl` 75).
pub const DEFAULT_AUTOMOVE: Duration = Duration::from_millis(3000);

/// The thinking time accepted: 0.1 s to 300 s, like the autoplay delay.
pub const AUTOMOVE_RANGE: RangeInclusive<Duration> =
    Duration::from_millis(100)..=Duration::from_secs(300);

/// How long the automove waits before it asks again after an engine that
/// gave no usable move (`after 1000 "automove $n"`, `analysis.tcl` 4324).
pub const AUTOMOVE_RETRY_DELAY: Duration = Duration::from_secs(1);

/// How many times the automove asks again (after the first ask: up to 11
/// searches for one move) before it gives up (D493 as amended by D856; the
/// Scid never gives up).
pub const AUTOMOVE_RETRIES: u8 = 10;

/// How long past its `movetime` a search of a mode may stay silent before
/// the watchdog gives the engine up (D855). Like the annotation's.
pub const SEARCH_STALL_GRACE: Duration = Duration::from_secs(30);

/// Where the automove is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutomoveState {
    /// The engine is thinking ("Thinking.....", `analysis.tcl` 3511).
    EngineThinking,
    /// It is the user's turn ("Your move.....", `analysis.tcl` 3513).
    UserToMove,
}

/// What an engine execution is doing besides analysing the board.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineMode {
    /// Nothing special.
    Idle,
    /// The game is in trial mode after a null move and the engine analyses
    /// that position.
    Threat,
    /// The engine plays both sides until the game ends.
    ShootOut {
        /// Changes at every step: the answer of an older one is dropped.
        generation: u64,
    },
    /// The engine answers the moves of the user.
    Automove {
        /// Changes at every move of the user and every cancellation.
        generation: u64,
        /// The thinking time of one move, in milliseconds.
        thinking_ms: u32,
        /// Whose turn it is.
        state: AutomoveState,
    },
}

/// Which buttons of a section can be used now (D490-D493, the spec's
/// disabled states). A button of a mode that is on stays enabled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ModeAvailability {
    /// "Threat".
    pub threat: bool,
    /// "Finish game".
    pub finish_game: bool,
    /// "Training".
    pub training: bool,
}

/// Why a shoot-out or an automove ended (the front-end translates it).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum StopReason {
    /// The side to move is checkmated.
    Checkmate,
    /// The side to move has no move and is not in check.
    Stalemate,
    /// Neither side can mate.
    InsufficientMaterial,
    /// 100 halfmoves without a capture or a pawn move.
    FiftyMoves,
    /// Three times the same position.
    Repetition,
    /// The engine had no move to give.
    NoMove,
    /// The engine failed, died or kept giving unplayable moves.
    EngineError,
    /// The user stopped it.
    UserStopped,
    /// The game was replaced, edited or navigated by someone else.
    GameChanged,
}

impl StopReason {
    /// The Fluent key of the message.
    pub fn message_key(self) -> &'static str {
        match self {
            StopReason::Checkmate => "analysis-mode-ended-checkmate",
            StopReason::Stalemate => "analysis-mode-ended-stalemate",
            StopReason::InsufficientMaterial => "analysis-mode-ended-insufficient-material",
            StopReason::FiftyMoves => "analysis-mode-ended-fifty-moves",
            StopReason::Repetition => "analysis-mode-ended-repetition",
            StopReason::NoMove => "analysis-mode-ended-no-move",
            StopReason::EngineError => "analysis-mode-ended-engine-error",
            StopReason::UserStopped => "analysis-mode-ended-user-stopped",
            StopReason::GameChanged => "analysis-mode-ended-game-changed",
        }
    }
}

/// The reason the game is over in `position`, by the rules: mate,
/// stalemate, insufficient material, fifty moves, threefold repetition.
/// `history` holds the hashes of the **earlier** positions of the game (not
/// `position`).
pub fn terminal_reason(position: &Position, history: &[ZobristHash]) -> Option<StopReason> {
    Some(match position.game_end(history)? {
        GameEnd::Checkmate { .. } => StopReason::Checkmate,
        GameEnd::Stalemate => StopReason::Stalemate,
        GameEnd::InsufficientMaterial => StopReason::InsufficientMaterial,
        GameEnd::FiftyMoves => StopReason::FiftyMoves,
        GameEnd::ThreefoldRepetition => StopReason::Repetition,
        // A rule the core learns later is not known here: play on.
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pos(fen: &str) -> Position {
        Position::from_fen(fen).expect("fen")
    }

    #[test]
    fn mate_stalemate_and_the_draws_by_material() {
        let cases = [
            (
                "rnb1kbnr/pppp1ppp/8/4p3/6Pq/5P2/PPPPP2P/RNBQKBNR w KQkq - 1 3",
                StopReason::Checkmate,
            ),
            ("7k/5Q2/6K1/8/8/8/8/8 b - - 0 1", StopReason::Stalemate),
            (
                "8/8/4k3/8/8/3K4/8/8 w - - 0 1",
                StopReason::InsufficientMaterial,
            ),
            (
                "8/8/4k3/8/8/3KN3/8/8 w - - 0 1",
                StopReason::InsufficientMaterial,
            ),
            (
                "8/8/3bk3/8/8/3KB3/8/8 w - - 0 1",
                StopReason::InsufficientMaterial,
            ),
            ("4k3/8/8/8/8/8/4P3/4K3 w - - 100 80", StopReason::FiftyMoves),
        ];
        for (fen, reason) in cases {
            assert_eq!(terminal_reason(&pos(fen), &[]), Some(reason), "{fen}");
        }
    }

    #[test]
    fn a_playable_position_is_not_terminal() {
        assert_eq!(terminal_reason(&Position::new(), &[]), None);
        // Two knights of different colours can still mate by a blunder.
        assert_eq!(
            terminal_reason(&pos("8/8/3nk3/8/8/3KN3/8/8 w - - 0 1"), &[]),
            None
        );
        assert_eq!(
            terminal_reason(&pos("4k3/8/8/8/8/8/4P3/4K3 w - - 99 80"), &[]),
            None
        );
    }

    #[test]
    fn the_third_occurrence_is_a_repetition() {
        let p = Position::new();
        let h = p.zobrist();
        assert_eq!(terminal_reason(&p, &[h]), None);
        assert_eq!(terminal_reason(&p, &[h, h]), Some(StopReason::Repetition));
    }

    #[test]
    fn mate_wins_over_the_draw_rules() {
        let mate = pos("rnb1kbnr/pppp1ppp/8/4p3/6Pq/5P2/PPPPP2P/RNBQKBNR w KQkq - 1 3");
        let h = mate.zobrist();
        assert_eq!(terminal_reason(&mate, &[h, h]), Some(StopReason::Checkmate));
    }

    #[test]
    fn every_reason_has_its_own_key() {
        let all = [
            StopReason::Checkmate,
            StopReason::Stalemate,
            StopReason::InsufficientMaterial,
            StopReason::FiftyMoves,
            StopReason::Repetition,
            StopReason::NoMove,
            StopReason::EngineError,
            StopReason::UserStopped,
            StopReason::GameChanged,
        ];
        let mut keys: Vec<&str> = all.iter().map(|r| r.message_key()).collect();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), all.len());
    }
}
