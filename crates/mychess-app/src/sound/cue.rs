//! The events that have a sound and the map from a move to its sound (D2802).

use mychess_core::{Move, Position};

/// What happened, as far as the sound is concerned.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[non_exhaustive]
pub enum SoundCue {
    /// An ordinary move (`move.wav` of the Scid).
    Move,
    /// A capture.
    Capture,
    /// A move that gives check (`Check.wav`).
    Check,
    /// A move that gives checkmate (`Mate.wav`).
    Checkmate,
    /// Castling (`CastleK.wav` and `CastleQ.wav` in the Scid).
    Castle,
    /// A promotion (`Promote.wav`).
    Promotion,
    /// Going back one move (`Back.wav`).
    Back,
    /// A game starts (`start.wav`).
    GameStart,
    /// A game ends (`end.wav`).
    GameEnd,
    /// The clock is running out.
    LowTime,
}

impl SoundCue {
    /// Every cue, in a stable order.
    pub const ALL: [SoundCue; 10] = [
        SoundCue::Move,
        SoundCue::Capture,
        SoundCue::Check,
        SoundCue::Checkmate,
        SoundCue::Castle,
        SoundCue::Promotion,
        SoundCue::Back,
        SoundCue::GameStart,
        SoundCue::GameEnd,
        SoundCue::LowTime,
    ];

    /// The name of the file of the user's folder that replaces the sound
    /// (without the extension; the Scid names, D2806).
    #[must_use]
    pub const fn file_stem(self) -> &'static str {
        match self {
            SoundCue::Move => "move",
            SoundCue::Capture => "capture",
            SoundCue::Check => "Check",
            SoundCue::Checkmate => "Mate",
            SoundCue::Castle => "CastleK",
            SoundCue::Promotion => "Promote",
            SoundCue::Back => "Back",
            SoundCue::GameStart => "start",
            SoundCue::GameEnd => "end",
            SoundCue::LowTime => "clock",
        }
    }
}

/// The sound of `mv` played in `before`: checkmate, check, promotion,
/// castling, capture or a plain move, in that order of precedence (D2802).
/// An illegal move gives [`SoundCue::Move`]; this never panics.
#[must_use]
pub fn cue_for_move(before: &Position, mv: Move) -> SoundCue {
    let mut after = before.clone();
    let played = !mv.is_null() && after.play(mv).is_ok();
    if played {
        if after.is_checkmate() {
            return SoundCue::Checkmate;
        }
        if after.is_check() {
            return SoundCue::Check;
        }
    }
    if mv.is_promotion() {
        SoundCue::Promotion
    } else if mv.is_castle() {
        SoundCue::Castle
    } else if mv.is_capture() {
        SoundCue::Capture
    } else {
        SoundCue::Move
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mychess_core::SanMode;

    fn cue(fen: &str, san: &str) -> SoundCue {
        let pos = Position::from_fen(fen).expect("fen");
        let mv = pos.parse_san(san, SanMode::Strict).expect("san").mv;
        cue_for_move(&pos, mv)
    }

    const START: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";

    #[test]
    fn a_plain_move() {
        assert_eq!(cue(START, "e4"), SoundCue::Move);
    }

    #[test]
    fn captures_including_en_passant() {
        assert_eq!(
            cue("4k3/8/8/3p4/4P3/8/8/4K3 w - - 0 1", "exd5"),
            SoundCue::Capture
        );
        assert_eq!(
            cue("4k3/8/8/3pP3/8/8/8/4K3 w - d6 0 1", "exd6"),
            SoundCue::Capture
        );
    }

    #[test]
    fn check_and_mate_beat_the_rest() {
        assert_eq!(
            cue("4k3/8/8/8/8/8/8/R3K3 w - - 0 1", "Ra8+"),
            SoundCue::Check
        );
        assert_eq!(
            cue("6k1/5ppp/8/8/8/8/8/R3K3 w - - 0 1", "Ra8#"),
            SoundCue::Checkmate
        );
        // A capture that checks is a check.
        assert_eq!(
            cue("4k3/8/8/8/8/8/r7/R3K3 w - - 0 1", "Rxa2"),
            SoundCue::Capture
        );
        assert_eq!(
            cue("4k3/4r3/8/8/8/8/8/4R1K1 w - - 0 1", "Rxe7+"),
            SoundCue::Check
        );
    }

    #[test]
    fn castling_and_promotion() {
        assert_eq!(
            cue("4k3/8/8/8/8/8/8/R3K2R w KQ - 0 1", "O-O"),
            SoundCue::Castle
        );
        assert_eq!(
            cue("8/P6k/8/8/8/8/8/4K3 w - - 0 1", "a8=Q"),
            SoundCue::Promotion
        );
        // Promotion with check is a check.
        assert_eq!(
            cue("4k3/P7/8/8/8/8/8/4K3 w - - 0 1", "a8=Q"),
            SoundCue::Check
        );
    }

    #[test]
    fn the_null_move_and_garbage_are_plain_moves() {
        let pos = Position::new();
        assert_eq!(cue_for_move(&pos, Move::Null), SoundCue::Move);
    }

    #[test]
    fn file_stems_are_unique() {
        let mut stems: Vec<_> = SoundCue::ALL.iter().map(|c| c.file_stem()).collect();
        stems.sort_unstable();
        stems.dedup();
        assert_eq!(stems.len(), SoundCue::ALL.len());
    }
}
