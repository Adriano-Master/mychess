//! From the answers of `shakmaty-syzygy` to the results of this crate (spec
//! m8-8, D2555, D2558).
//!
//! Everything that decides what a Syzygy value *means* is here as plain
//! functions over a small trait, [`RawTables`], so the tests of CI can feed
//! hand-made [`Wdl`] and [`Dtz`] values; the real files are only needed by
//! the `#[ignore]` tests that prove the glue (`MYCHESS_TEST_SYZYGY`).
//!
//! **Every call into `shakmaty-syzygy` that reads a table goes through
//! [`guarded`]** (`catch_unwind`): the probing code is written for intact
//! files and a damaged one could overflow. The other calls into the crate
//! (`Tablebase::new`, `max_pieces`) read no file and cannot panic; `add_file`
//! is guarded in `local.rs`.

use std::panic::{AssertUnwindSafe, catch_unwind};

use mychess_core::{Move, Position, Role};
use shakmaty::fen::Fen;
use shakmaty::{CastlingMode, Chess};
use shakmaty_syzygy::{AmbiguousWdl, Dtz, MaybeRounded, Metric, SyzygyError, Tablebase, Wdl};

use crate::category::{Category, Evaluation, MoveEntry, PositionReport, Source};
use crate::error::{Result, TablebaseError};
use crate::probe::{check_applicable, piece_count};

/// What went wrong in a raw probe, without the types of `shakmaty-syzygy`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RawError {
    /// The DTZ table (`.rtbz`) of this material is not there.
    MissingDtz(String),
    /// The WDL table (`.rtbw`) of this material is not there.
    MissingWdl(String),
    /// A table of this material is corrupted.
    Corrupt(String),
    /// A castling right is left.
    Castling,
    /// More pieces than the tables cover.
    TooManyPieces,
}

/// The two probes the conversion needs.
pub(crate) trait RawTables {
    /// The DTZ of the position (needs the WDL and the DTZ tables).
    fn dtz(&self, chess: &Chess) -> std::result::Result<MaybeRounded<Dtz>, RawError>;
    /// The WDL of the position as if the clock were zero (WDL tables only).
    fn wdl_after_zeroing(&self, chess: &Chess) -> std::result::Result<Wdl, RawError>;
}

impl RawTables for Tablebase<Chess> {
    fn dtz(&self, chess: &Chess) -> std::result::Result<MaybeRounded<Dtz>, RawError> {
        self.probe_dtz(chess).map_err(raw_error)
    }

    fn wdl_after_zeroing(&self, chess: &Chess) -> std::result::Result<Wdl, RawError> {
        self.probe_wdl_after_zeroing(chess).map_err(raw_error)
    }
}

fn raw_error(error: SyzygyError) -> RawError {
    match error {
        SyzygyError::Castling => RawError::Castling,
        SyzygyError::TooManyPieces => RawError::TooManyPieces,
        SyzygyError::MissingTable { metric, material } => match metric {
            Metric::Dtz => RawError::MissingDtz(material.to_string()),
            Metric::Wdl => RawError::MissingWdl(material.to_string()),
        },
        SyzygyError::ProbeFailed { material, .. } => RawError::Corrupt(material.to_string()),
    }
}

/// Runs `probe` so that a panic of the Syzygy code becomes
/// [`TablebaseError::Internal`] (D2558).
pub(crate) fn guarded<T>(probe: impl FnOnce() -> Result<T>) -> Result<T> {
    catch_unwind(AssertUnwindSafe(probe)).unwrap_or_else(|_| {
        tracing::warn!("a Syzygy probe panicked; the position was abandoned");
        Err(TablebaseError::Internal)
    })
}

/// The result for the side to move from a DTZ: the sign says who wins, the
/// halfmove clock settles the 50-move rule, and a rounded DTZ may leave the
/// answer open (`MaybeWin`, `MaybeLoss`).
pub(crate) fn dtz_evaluation(dtz: MaybeRounded<Dtz>, halfmoves: u32) -> Evaluation {
    // `AmbiguousWdl::from_dtz_and_halfmoves` adds the clock with a checked
    // sum; a clock so large that it would overflow is read as zero.
    let halfmoves = halfmoves.min(1000);
    Evaluation {
        category: ambiguous_category(AmbiguousWdl::from_dtz_and_halfmoves(dtz, halfmoves)),
        dtz: Some(dtz.ignore_rounding().0),
        dtz_rounded: matches!(dtz, MaybeRounded::Rounded(_)),
        dtm: None,
    }
}

/// The result from a WDL alone (no `.rtbz`): no distance, clock ignored.
pub(crate) fn wdl_evaluation(wdl: Wdl) -> Evaluation {
    Evaluation::of(match wdl {
        Wdl::Loss => Category::Loss,
        Wdl::BlessedLoss => Category::BlessedLoss,
        Wdl::Draw => Category::Draw,
        Wdl::CursedWin => Category::CursedWin,
        Wdl::Win => Category::Win,
    })
}

fn ambiguous_category(wdl: AmbiguousWdl) -> Category {
    match wdl {
        AmbiguousWdl::Loss => Category::Loss,
        AmbiguousWdl::MaybeLoss => Category::MaybeLoss,
        AmbiguousWdl::BlessedLoss => Category::BlessedLoss,
        AmbiguousWdl::Draw => Category::Draw,
        AmbiguousWdl::CursedWin => Category::CursedWin,
        AmbiguousWdl::MaybeWin => Category::MaybeWin,
        AmbiguousWdl::Win => Category::Win,
    }
}

fn typed(error: RawError, pieces: u8, max_pieces: u8) -> TablebaseError {
    match error {
        RawError::MissingDtz(material) | RawError::MissingWdl(material) => {
            TablebaseError::MissingTable { material }
        }
        RawError::Corrupt(material) => TablebaseError::Corrupt { material },
        RawError::Castling => TablebaseError::Castling,
        RawError::TooManyPieces => TablebaseError::TooManyPieces {
            pieces,
            max: max_pieces,
        },
    }
}

/// The result of `chess` for the side to move: the DTZ when the table is
/// there, else the WDL alone (D2555). Guarded against panics.
pub(crate) fn evaluate_chess<R: RawTables + ?Sized>(
    raw: &R,
    chess: &Chess,
    max_pieces: u8,
) -> Result<Evaluation> {
    let pieces = u8::try_from(shakmaty_board(chess).occupied().count()).unwrap_or(u8::MAX);
    guarded(|| {
        match raw.dtz(chess) {
            Ok(dtz) => Ok(dtz_evaluation(dtz, shakmaty_halfmoves(chess))),
            // Only the DTZ file is missing: the WDL alone still tells the
            // theoretical result (the clock is ignored).
            Err(RawError::MissingDtz(_)) => raw
                .wdl_after_zeroing(chess)
                .map(wdl_evaluation)
                .map_err(|e| typed(e, pieces, max_pieces)),
            Err(error) => Err(typed(error, pieces, max_pieces)),
        }
    })
}

fn shakmaty_board(chess: &Chess) -> &shakmaty::Board {
    use shakmaty::Position as _;
    chess.board()
}

fn shakmaty_halfmoves(chess: &Chess) -> u32 {
    use shakmaty::Position as _;
    chess.halfmoves()
}

pub(crate) fn to_chess(position: &Position) -> Result<Chess> {
    let fen: Fen = position
        .fen()
        .parse()
        .map_err(|_| TablebaseError::InvalidPosition)?;
    fen.into_position(CastlingMode::Standard)
        .map_err(|_| TablebaseError::InvalidPosition)
}

fn evaluate_position<R: RawTables + ?Sized>(
    raw: &R,
    position: &Position,
    max_pieces: u8,
) -> Result<Evaluation> {
    evaluate_chess(raw, &to_chess(position)?, max_pieces)
}

/// Whether the move resets the 50-move clock: a capture or a pawn move.
pub(crate) fn is_zeroing(mv: Move) -> bool {
    match mv {
        Move::Normal { role, capture, .. } => role == Role::Pawn || capture.is_some(),
        Move::EnPassant { .. } => true,
        Move::Castle { .. } | Move::Null => false,
    }
}

/// The result of the position and of each of its moves (D2555).
pub(crate) fn probe_position<R: RawTables + ?Sized>(
    raw: &R,
    position: &Position,
    max_pieces: u8,
) -> Result<PositionReport> {
    let pieces = check_applicable(position, max_pieces)?;
    let moves = position.legal_moves();
    if moves.is_empty() {
        // The game is over: no table is needed.
        let category = if position.is_check() {
            Category::Loss
        } else {
            Category::Draw
        };
        let evaluation = Evaluation {
            dtz: Some(0),
            ..Evaluation::of(category)
        };
        return Ok(PositionReport::new(
            Source::Local,
            pieces,
            evaluation,
            Vec::new(),
        ));
    }
    let root = if position.is_insufficient_material() {
        Evaluation::of(Category::Draw)
    } else {
        evaluate_position(raw, position, max_pieces)?
    };
    let mut entries = Vec::with_capacity(moves.len());
    for mv in moves {
        entries.push(entry(raw, position, mv, max_pieces)?);
    }
    debug_assert_eq!(pieces, piece_count(position));
    Ok(PositionReport::new(Source::Local, pieces, root, entries))
}

fn entry<R: RawTables + ?Sized>(
    raw: &R,
    position: &Position,
    mv: Move,
    max_pieces: u8,
) -> Result<MoveEntry> {
    let after = position
        .after(mv)
        .map_err(|_| TablebaseError::InvalidPosition)?;
    let checkmate = after.is_checkmate();
    let stalemate = after.is_stalemate();
    let insufficient = !checkmate && !stalemate && after.is_insufficient_material();
    let evaluation = if checkmate {
        Evaluation {
            category: Category::Win,
            dtz: Some(1),
            dtz_rounded: false,
            dtm: Some(1),
        }
    } else if stalemate || insufficient {
        Evaluation {
            dtz: Some(0),
            ..Evaluation::of(Category::Draw)
        }
    } else {
        // The table says how the *opponent* stands; the mover sees the
        // opposite.
        match evaluate_position(raw, &after, max_pieces) {
            Ok(evaluation) => evaluation.negate(),
            // A table that is not there leaves just this move unknown.
            Err(error) if error.is_not_covered() => Evaluation::UNKNOWN,
            Err(error) => return Err(error),
        }
    };
    let uci = position.uci(mv);
    let san = position.san(mv).unwrap_or_else(|_| uci.clone());
    Ok(MoveEntry {
        mv,
        san,
        uci,
        evaluation,
        zeroing: is_zeroing(mv),
        checkmate,
        stalemate,
        insufficient_material: insufficient,
    })
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    type Dtzs = dyn Fn(&Chess) -> std::result::Result<MaybeRounded<Dtz>, RawError>;
    type Wdls = dyn Fn(&Chess) -> std::result::Result<Wdl, RawError>;

    struct Fake {
        dtz: Box<Dtzs>,
        wdl: Box<Wdls>,
        dtz_calls: Cell<usize>,
        wdl_calls: Cell<usize>,
    }

    impl Fake {
        fn new(
            dtz: impl Fn(&Chess) -> std::result::Result<MaybeRounded<Dtz>, RawError> + 'static,
            wdl: impl Fn(&Chess) -> std::result::Result<Wdl, RawError> + 'static,
        ) -> Fake {
            Fake {
                dtz: Box::new(dtz),
                wdl: Box::new(wdl),
                dtz_calls: Cell::new(0),
                wdl_calls: Cell::new(0),
            }
        }
    }

    impl RawTables for Fake {
        fn dtz(&self, chess: &Chess) -> std::result::Result<MaybeRounded<Dtz>, RawError> {
            self.dtz_calls.set(self.dtz_calls.get() + 1);
            (self.dtz)(chess)
        }

        fn wdl_after_zeroing(&self, chess: &Chess) -> std::result::Result<Wdl, RawError> {
            self.wdl_calls.set(self.wdl_calls.get() + 1);
            (self.wdl)(chess)
        }
    }

    fn pos(fen: &str) -> Position {
        Position::from_fen(fen).expect("fen")
    }

    fn turn_is_white(chess: &Chess) -> bool {
        use shakmaty::Position as _;
        chess.turn() == shakmaty::Color::White
    }

    fn precise(n: i32) -> MaybeRounded<Dtz> {
        MaybeRounded::Precise(Dtz(n))
    }

    fn rounded(n: i32) -> MaybeRounded<Dtz> {
        MaybeRounded::Rounded(Dtz(n))
    }

    #[test]
    fn the_sign_of_the_dtz_says_who_wins() {
        let win = dtz_evaluation(precise(23), 0);
        assert_eq!(
            (win.category, win.dtz, win.dtz_rounded, win.dtm),
            (Category::Win, Some(23), false, None)
        );
        assert_eq!(dtz_evaluation(precise(-23), 0).category, Category::Loss);
        assert_eq!(dtz_evaluation(precise(-23), 0).dtz, Some(-23));
        let draw = dtz_evaluation(precise(0), 0);
        assert_eq!((draw.category, draw.dtz), (Category::Draw, Some(0)));
    }

    #[test]
    fn beyond_one_hundred_plies_the_fifty_move_rule_makes_it_a_draw() {
        assert_eq!(dtz_evaluation(precise(100), 0).category, Category::Win);
        assert_eq!(
            dtz_evaluation(precise(101), 0).category,
            Category::CursedWin
        );
        assert_eq!(dtz_evaluation(precise(-100), 0).category, Category::Loss);
        assert_eq!(
            dtz_evaluation(precise(-101), 0).category,
            Category::BlessedLoss
        );
    }

    #[test]
    fn the_halfmove_clock_is_part_of_the_answer() {
        // 60 plies to zero with 45 already played: 105, a cursed win.
        assert_eq!(
            dtz_evaluation(precise(60), 45).category,
            Category::CursedWin
        );
        assert_eq!(dtz_evaluation(precise(60), 10).category, Category::Win);
        assert_eq!(
            dtz_evaluation(precise(-60), 45).category,
            Category::BlessedLoss
        );
    }

    #[test]
    fn a_rounded_dtz_on_the_edge_is_maybe() {
        let maybe_win = dtz_evaluation(rounded(95), 5);
        assert_eq!(maybe_win.category, Category::MaybeWin);
        assert!(maybe_win.dtz_rounded);
        assert_eq!(
            dtz_evaluation(rounded(-95), 5).category,
            Category::MaybeLoss
        );
        // Right after a capture or a pawn move the answer is settled.
        assert_eq!(dtz_evaluation(rounded(100), 0).category, Category::Win);
        // Away from the edge the rounding does not matter.
        assert_eq!(dtz_evaluation(rounded(20), 5).category, Category::Win);
        assert!(dtz_evaluation(rounded(20), 5).dtz_rounded);
    }

    #[test]
    fn an_absurd_clock_does_not_overflow() {
        let e = dtz_evaluation(precise(i32::MAX - 1), u32::MAX);
        assert_eq!(e.category, Category::CursedWin);
        assert_eq!(
            dtz_evaluation(precise(5), u32::MAX).category,
            Category::CursedWin
        );
    }

    #[test]
    fn the_five_wdl_values_map_one_to_one_without_distances() {
        for (wdl, category) in [
            (Wdl::Loss, Category::Loss),
            (Wdl::BlessedLoss, Category::BlessedLoss),
            (Wdl::Draw, Category::Draw),
            (Wdl::CursedWin, Category::CursedWin),
            (Wdl::Win, Category::Win),
        ] {
            let e = wdl_evaluation(wdl);
            assert_eq!(e, Evaluation::of(category));
        }
    }

    const KQ: &str = "8/8/8/8/8/2k5/8/KQ6 w - - 0 1";

    #[test]
    fn without_the_dtz_table_the_wdl_alone_answers() {
        let fake = Fake::new(
            |_| Err(RawError::MissingDtz("KQvK".into())),
            |_| Ok(Wdl::Win),
        );
        let e = evaluate_position(&fake, &pos(KQ), 5).unwrap();
        assert_eq!(e, Evaluation::of(Category::Win));
        assert_eq!((fake.dtz_calls.get(), fake.wdl_calls.get()), (1, 1));
    }

    #[test]
    fn without_the_wdl_table_there_is_no_fallback() {
        let fake = Fake::new(
            |_| Err(RawError::MissingWdl("KQvK".into())),
            |_| Ok(Wdl::Win),
        );
        assert_eq!(
            evaluate_position(&fake, &pos(KQ), 5).unwrap_err(),
            TablebaseError::MissingTable {
                material: "KQvK".into()
            }
        );
        assert_eq!(fake.wdl_calls.get(), 0);
    }

    #[test]
    fn a_missing_wdl_table_in_the_fallback_is_a_missing_table() {
        let fake = Fake::new(
            |_| Err(RawError::MissingDtz("KQvK".into())),
            |_| Err(RawError::MissingWdl("KQvK".into())),
        );
        assert!(matches!(
            evaluate_position(&fake, &pos(KQ), 5).unwrap_err(),
            TablebaseError::MissingTable { .. }
        ));
    }

    #[test]
    fn raw_errors_become_typed_errors_with_real_numbers() {
        let castling = Fake::new(|_| Err(RawError::Castling), |_| Ok(Wdl::Draw));
        assert_eq!(
            evaluate_position(&castling, &pos(KQ), 5).unwrap_err(),
            TablebaseError::Castling
        );
        let corrupt = Fake::new(|_| Err(RawError::Corrupt("KQvK".into())), |_| Ok(Wdl::Draw));
        assert_eq!(
            evaluate_position(&corrupt, &pos(KQ), 5).unwrap_err(),
            TablebaseError::Corrupt {
                material: "KQvK".into()
            }
        );
        let many = Fake::new(|_| Err(RawError::TooManyPieces), |_| Ok(Wdl::Draw));
        assert_eq!(
            evaluate_position(&many, &pos(KQ), 5).unwrap_err(),
            TablebaseError::TooManyPieces { pieces: 3, max: 5 }
        );
    }

    /// White to move: 11 plies to zero; black to move: -10 (so a white move
    /// leaves the opponent lost, and the mover wins in 10).
    fn kq_tables() -> Fake {
        Fake::new(
            |chess| {
                Ok(if turn_is_white(chess) {
                    precise(11)
                } else {
                    precise(-10)
                })
            },
            |_| Ok(Wdl::Win),
        )
    }

    #[test]
    fn the_moves_are_seen_from_the_side_that_plays_them() {
        let fake = kq_tables();
        let report = probe_position(&fake, &pos(KQ), 5).unwrap();
        assert_eq!(report.source, Source::Local);
        assert_eq!(report.pieces, 3);
        assert_eq!(report.evaluation.category, Category::Win);
        assert_eq!(report.evaluation.dtz, Some(11));
        assert!(report.moves.len() > 3);
        for m in &report.moves {
            assert_eq!(m.evaluation.category, Category::Win, "{}", m.san);
            assert_eq!(m.evaluation.dtz, Some(10), "{}", m.san);
        }
        assert_eq!(report.best_group().len(), report.moves.len());
        assert!(!report.has_dtm());
    }

    #[test]
    fn a_move_that_mates_a_stalemate_and_a_dead_position_need_no_table() {
        // Qh8 mates; the tables are never asked about that move.
        let fake = Fake::new(
            |chess| {
                if turn_is_white(chess) {
                    Ok(precise(1))
                } else {
                    Err(RawError::Corrupt("must not be asked".into()))
                }
            },
            |_| Ok(Wdl::Win),
        );
        let mate = pos("k7/8/1K6/8/8/8/7Q/8 w - - 0 1");
        // Other moves would ask about black-to-move positions: the corrupt
        // answer is an error for the whole report, except for the mate.
        let err = probe_position(&fake, &mate, 5).unwrap_err();
        assert!(matches!(err, TablebaseError::Corrupt { .. }));
        let tables = Fake::new(
            |chess| {
                if turn_is_white(chess) {
                    Ok(precise(1))
                } else {
                    Ok(precise(-2))
                }
            },
            |_| Ok(Wdl::Win),
        );
        let report = probe_position(&tables, &mate, 5).unwrap();
        let qh8 = report.moves.iter().find(|m| m.uci == "h2h8").unwrap();
        assert!(qh8.checkmate && !qh8.stalemate);
        assert_eq!(qh8.evaluation.category, Category::Win);
        assert_eq!((qh8.evaluation.dtz, qh8.evaluation.dtm), (Some(1), Some(1)));
        assert_eq!(report.moves[0].uci, "h2h8", "the mate is the best move");

        // Qc7 stalemates the king on a8.
        let stale = pos("k7/8/1K6/8/8/8/7Q/8 w - - 0 1");
        let report = probe_position(&tables, &stale, 5).unwrap();
        let qc7 = report.moves.iter().find(|m| m.uci == "h2c7").unwrap();
        assert!(qc7.stalemate && !qc7.checkmate);
        assert_eq!(qc7.evaluation.category, Category::Draw);
        assert_eq!(qc7.evaluation.dtz, Some(0));
    }

    #[test]
    fn a_capture_into_a_missing_table_leaves_only_that_move_unknown() {
        let fake = Fake::new(
            |chess| {
                use shakmaty::Position as _;
                if chess.board().occupied().count() == 3 {
                    Err(RawError::MissingWdl("KQvK".into()))
                } else if turn_is_white(chess) {
                    Ok(precise(9))
                } else {
                    Ok(precise(-8))
                }
            },
            |_| Ok(Wdl::Win),
        );
        let position = pos("7k/8/8/8/8/8/1r6/KQ6 w - - 0 1");
        let report = probe_position(&fake, &position, 5).unwrap();
        let capture = report.moves.iter().find(|m| m.uci == "b1b2").unwrap();
        assert!(capture.zeroing);
        assert_eq!(capture.evaluation, Evaluation::UNKNOWN);
        let quiet = report.moves.iter().find(|m| m.uci == "b1c1").unwrap();
        assert!(!quiet.zeroing);
        assert_eq!(quiet.evaluation.category, Category::Win);
        assert_eq!(quiet.evaluation.dtz, Some(8));
        // Unknown sorts last.
        assert_eq!(
            report.moves.last().map(|m| m.evaluation.category),
            Some(Category::Unknown)
        );
    }

    #[test]
    fn a_corrupted_table_for_a_move_fails_the_whole_report() {
        let fake = Fake::new(
            |chess| {
                if turn_is_white(chess) {
                    Ok(precise(9))
                } else {
                    Err(RawError::Corrupt("KQvK".into()))
                }
            },
            |_| Ok(Wdl::Win),
        );
        assert_eq!(
            probe_position(&fake, &pos(KQ), 5).unwrap_err(),
            TablebaseError::Corrupt {
                material: "KQvK".into()
            }
        );
    }

    #[test]
    fn zeroing_moves_are_captures_and_pawn_moves() {
        let p = pos("4k3/8/8/3p4/4P3/8/8/4K3 w - - 0 1");
        let by = |uci: &str| p.parse_uci(uci).unwrap();
        assert!(is_zeroing(by("e4e5")));
        assert!(is_zeroing(by("e4d5")));
        assert!(!is_zeroing(by("e1e2")));
        let ep = pos("4k3/8/8/3pP3/8/8/8/4K3 w - d6 0 1");
        assert!(is_zeroing(ep.parse_uci("e5d6").unwrap()));
    }

    #[test]
    fn a_panic_inside_the_probe_becomes_an_internal_error() {
        let fake = Fake::new(|_| panic!("overflow in a damaged table"), |_| Ok(Wdl::Win));
        assert_eq!(
            evaluate_position(&fake, &pos(KQ), 5).unwrap_err(),
            TablebaseError::Internal
        );
        assert_eq!(
            probe_position(&fake, &pos(KQ), 5).unwrap_err(),
            TablebaseError::Internal
        );
        // The fallback probe is guarded as well.
        let wdl_panics = Fake::new(
            |_| Err(RawError::MissingDtz("KQvK".into())),
            |_| panic!("overflow in the WDL table"),
        );
        assert_eq!(
            evaluate_position(&wdl_panics, &pos(KQ), 5).unwrap_err(),
            TablebaseError::Internal
        );
    }

    #[test]
    fn guarded_passes_results_through() {
        assert_eq!(guarded(|| Ok(3)), Ok(3));
        assert_eq!(
            guarded::<u8>(|| Err(TablebaseError::NoTables)),
            Err(TablebaseError::NoTables)
        );
    }
}
