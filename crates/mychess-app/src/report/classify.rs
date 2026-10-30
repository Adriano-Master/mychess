//! Win probability, move classes, accuracy and key moments (D1423-D1430).

use mychess_core::{Color, Move};

use crate::annotate::PositionEval;

/// The class of a move (D1424). `Excellent` is the third tier of the loss
/// scale ("Ótimo" in Portuguese); `Great` is the only-move class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum MoveClass {
    /// A sound sacrifice that keeps the position good.
    Brilliant,
    /// The best move where the second best loses a lot.
    Great,
    /// The engine's move (or within the best tolerance).
    Best,
    /// A small loss.
    Excellent,
    /// A modest loss.
    Good,
    /// A move inside a known opening line.
    Book,
    /// A loss between the good and the mistake limits.
    Inaccuracy,
    /// A clear loss.
    Mistake,
    /// A chance the opponent gave and this move wasted.
    Miss,
    /// A decisive loss.
    Blunder,
}

impl MoveClass {
    /// Every class, in display order.
    pub const ALL: [MoveClass; 10] = [
        MoveClass::Brilliant,
        MoveClass::Great,
        MoveClass::Best,
        MoveClass::Excellent,
        MoveClass::Good,
        MoveClass::Book,
        MoveClass::Inaccuracy,
        MoveClass::Mistake,
        MoveClass::Miss,
        MoveClass::Blunder,
    ];

    /// The Fluent key of the name of the class.
    pub fn name_key(self) -> &'static str {
        match self {
            MoveClass::Brilliant => "report-class-brilliant",
            MoveClass::Great => "report-class-great",
            MoveClass::Best => "report-class-best",
            MoveClass::Excellent => "report-class-excellent",
            MoveClass::Good => "report-class-good",
            MoveClass::Book => "report-class-book",
            MoveClass::Inaccuracy => "report-class-inaccuracy",
            MoveClass::Mistake => "report-class-mistake",
            MoveClass::Miss => "report-class-miss",
            MoveClass::Blunder => "report-class-blunder",
        }
    }

    /// Whether the class is a fault (shown in the guided review, D1435).
    pub fn is_fault(self) -> bool {
        matches!(
            self,
            MoveClass::Inaccuracy | MoveClass::Mistake | MoveClass::Miss | MoveClass::Blunder
        )
    }

    /// The index in [`MoveClass::ALL`].
    fn index(self) -> usize {
        MoveClass::ALL
            .iter()
            .position(|c| *c == self)
            .unwrap_or_default()
    }
}

/// The limits of the loss scale, in percentage points of win probability
/// (D1425), and the parameters of the special classes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReportThresholds {
    /// Up to this loss a move is `Best`.
    pub best: f32,
    /// Up to this loss: `Excellent`.
    pub excellent: f32,
    /// Up to this loss: `Good`.
    pub good: f32,
    /// Up to this loss: `Inaccuracy`.
    pub inaccuracy: f32,
    /// Up to this loss: `Mistake`; above it, `Blunder`.
    pub mistake: f32,
    /// Win-probability gap to the second line that makes a move `Great`
    /// (D1427).
    pub great_gap: f32,
    /// Win-probability gain handed over by the opponent that makes a wasted
    /// chance a `Miss` (D1426).
    pub miss_gain: f32,
    /// The least win probability a `Brilliant` sacrifice must keep (D1428).
    pub brilliant_floor: f32,
}

impl Default for ReportThresholds {
    fn default() -> ReportThresholds {
        ReportThresholds {
            best: 0.5,
            excellent: 2.0,
            good: 5.0,
            inaccuracy: 10.0,
            mistake: 20.0,
            great_gap: 15.0,
            miss_gain: 15.0,
            brilliant_floor: 40.0,
        }
    }
}

/// The win probability, in percent, of an evaluation in centipawns for the
/// side it favours when positive (D1423):
/// `50 + 50 * (2 / (1 + exp(-0.00368208 * cp)) - 1)`. A value that is not
/// finite gives 50.
pub fn win_percent(centipawns: f32) -> f32 {
    if !centipawns.is_finite() {
        return 50.0;
    }
    let cp = centipawns.clamp(-100_000.0, 100_000.0);
    50.0 + 50.0 * (2.0 / (1.0 + (-0.003_682_08 * cp).exp()) - 1.0)
}

/// The accuracy of one move for a loss in percentage points (D1430):
/// `103.1668 * e^(-0.04354 * loss) - 3.1669`, kept in `0..=100`.
pub fn accuracy_of_loss(loss: f32) -> f32 {
    if !loss.is_finite() {
        return 0.0;
    }
    (103.1668 * (-0.04354 * loss.clamp(0.0, 100.0)).exp() - 3.1669).clamp(0.0, 100.0)
}

/// White's win probability of an evaluation (White's point of view).
fn white_win(eval: &PositionEval) -> f32 {
    match eval.mate {
        Some(n) if n > 0 => 100.0,
        Some(n) if n < 0 => 0.0,
        _ => win_percent(eval.score * 100.0),
    }
}

fn win_for(eval: &PositionEval, color: Color) -> f32 {
    let white = white_win(eval);
    match color {
        Color::White => white,
        Color::Black => 100.0 - white,
    }
}

/// What the report needs to know about one move.
#[derive(Debug, Clone)]
pub struct MoveInput {
    /// Who moved.
    pub mover: Color,
    /// The evaluation of the position before the move (with the engine's
    /// best line, and the second line when it exists).
    pub before: PositionEval,
    /// The evaluation of the position after the move.
    pub after: PositionEval,
    /// The move played.
    pub played: Move,
    /// The move is inside a known opening line (D1429).
    pub book: bool,
    /// The move gives up material the line does not win back at once; the
    /// caller decides it from the material (D1428).
    pub sacrifice: bool,
}

/// The verdict on one move.
#[derive(Debug, Clone, PartialEq)]
pub struct MoveReport {
    /// The ply (1 is White's first move).
    pub ply: usize,
    /// Who moved.
    pub mover: Color,
    /// The class.
    pub class: MoveClass,
    /// The loss in percentage points of win probability, `0..=100`.
    pub loss: f32,
    /// The accuracy of the move, `0..=100`.
    pub accuracy: f32,
    /// The move played.
    pub played: Move,
    /// The engine's best move in the position, when known.
    pub best: Option<Move>,
}

/// The totals of one side.
#[derive(Debug, Clone, PartialEq)]
pub struct SideReport {
    /// The accuracy of the side, `0..=100`; `None` when it has no move
    /// outside the book (D1430).
    pub accuracy: Option<f32>,
    /// How many moves of each class, indexed like [`MoveClass::ALL`].
    pub counts: [u32; 10],
}

impl SideReport {
    /// How many moves had `class`.
    pub fn count(&self, class: MoveClass) -> u32 {
        self.counts[class.index()]
    }

    /// The number of moves of the side; the counts of the table add up to it.
    pub fn total(&self) -> u32 {
        self.counts.iter().sum()
    }
}

/// The report of a game.
#[derive(Debug, Clone, PartialEq)]
pub struct GameReport {
    /// One entry per move of the main line.
    pub moves: Vec<MoveReport>,
    /// White's totals.
    pub white: SideReport,
    /// Black's totals.
    pub black: SideReport,
}

impl GameReport {
    /// The totals of `color`.
    pub fn side(&self, color: Color) -> &SideReport {
        match color {
            Color::White => &self.white,
            Color::Black => &self.black,
        }
    }
}

/// Classifies one move from its loss and the facts around it. `previous` is
/// the class of the opponent's last move and the win probability (for the
/// mover) before that move, for the `Miss` test.
fn classify(
    input: &MoveInput,
    loss: f32,
    previous: Option<(MoveClass, f32)>,
    t: &ReportThresholds,
) -> MoveClass {
    if input.book {
        return MoveClass::Book;
    }
    let best_win = win_for(&input.before, input.mover);
    let after_win = win_for(&input.after, input.mover);
    let is_best = input.before.pv.first() == Some(&input.played) || loss <= t.best;
    // A chance wasted right after the opponent erred (D1426).
    if loss > t.good
        && let Some((class, before_their_move)) = previous
        && matches!(class, MoveClass::Mistake | MoveClass::Blunder)
        && best_win - before_their_move >= t.miss_gain
    {
        return MoveClass::Miss;
    }
    let scale = if loss <= t.best || is_best && loss <= t.excellent {
        MoveClass::Best
    } else if loss <= t.excellent {
        MoveClass::Excellent
    } else if loss <= t.good {
        MoveClass::Good
    } else if loss <= t.inaccuracy {
        MoveClass::Inaccuracy
    } else if loss <= t.mistake {
        MoveClass::Mistake
    } else {
        MoveClass::Blunder
    };
    if matches!(scale, MoveClass::Best | MoveClass::Excellent) {
        if input.sacrifice && after_win >= t.brilliant_floor {
            return MoveClass::Brilliant;
        }
        // The only move: the second line is far behind, in a position that
        // is not already decided (D1427).
        if scale == MoveClass::Best
            && let Some(second) = input.before.second
        {
            let second_win = match input.mover {
                Color::White => win_percent(second * 100.0),
                Color::Black => 100.0 - win_percent(second * 100.0),
            };
            if best_win - second_win >= t.great_gap && (3.0..=97.0).contains(&best_win) {
                return MoveClass::Great;
            }
        }
    }
    scale
}

/// The accuracy of a side: the mean of the volatility-weighted mean and the
/// harmonic mean of its moves' accuracies (D1430). `None` without moves.
fn side_accuracy(items: &[(f32, f32)]) -> Option<f32> {
    if items.is_empty() {
        return None;
    }
    let weight_sum: f32 = items.iter().map(|(_, w)| w).sum();
    let weighted = items.iter().map(|(a, w)| a * w).sum::<f32>() / weight_sum.max(f32::EPSILON);
    #[allow(clippy::cast_precision_loss)] // a game has far fewer than 2^24 moves
    let count = items.len() as f32;
    let harmonic = count / items.iter().map(|(a, _)| 1.0 / a.max(1.0)).sum::<f32>();
    Some(f32::midpoint(weighted, harmonic).clamp(0.0, 100.0))
}

/// The standard deviation of a window of win probabilities.
fn deviation(window: &[f32]) -> f32 {
    #[allow(clippy::cast_precision_loss)]
    let n = window.len() as f32;
    if window.len() < 2 {
        return 0.0;
    }
    let mean = window.iter().sum::<f32>() / n;
    (window.iter().map(|v| (v - mean).powi(2)).sum::<f32>() / n).sqrt()
}

/// Builds the report of the moves `inputs`, in game order (D1420-D1430).
/// Every number is bounded; an empty list gives an empty report.
pub fn build_report(inputs: &[MoveInput], thresholds: &ReportThresholds) -> GameReport {
    // White's win probability of every position, for the volatility weight.
    let mut series: Vec<f32> = inputs
        .first()
        .map(|i| white_win(&i.before))
        .into_iter()
        .collect();
    series.extend(inputs.iter().map(|i| white_win(&i.after)));
    let window = (inputs.len() / 10).clamp(2, 8);

    let mut moves = Vec::with_capacity(inputs.len());
    let mut previous: Option<(MoveClass, f32)> = None;
    for (i, input) in inputs.iter().enumerate() {
        let before_win = win_for(&input.before, input.mover);
        let after_win = win_for(&input.after, input.mover);
        // The engine's own move cannot be a fault: a worse `after` is horizon
        // noise between two searches (D1581).
        let loss = if input.before.pv.first() == Some(&input.played) {
            0.0
        } else {
            (before_win - after_win).clamp(0.0, 100.0)
        };
        let class = classify(input, loss, previous, thresholds);
        moves.push(MoveReport {
            ply: i + 1,
            mover: input.mover,
            class,
            loss,
            accuracy: accuracy_of_loss(loss),
            played: input.played,
            best: input.before.pv.first().copied(),
        });
        // The opponent's next move compares with the position before this one.
        previous = Some((class, win_for(&input.before, input.mover.other())));
    }

    let side = |color: Color| {
        let mut counts = [0u32; 10];
        let mut items = Vec::new();
        for (i, m) in moves.iter().enumerate() {
            if m.mover != color {
                continue;
            }
            counts[m.class.index()] += 1;
            if m.class != MoveClass::Book {
                let end = (i + window).min(series.len());
                let slice = series.get(i..end).unwrap_or_default();
                items.push((m.accuracy, deviation(slice).clamp(0.5, 12.0)));
            }
        }
        SideReport {
            accuracy: side_accuracy(&items),
            counts,
        }
    };
    GameReport {
        white: side(Color::White),
        black: side(Color::Black),
        moves,
    }
}

/// Which side(s) the guided review visits (D1435).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SideFilter {
    /// White's moves.
    White,
    /// Black's moves.
    Black,
    /// Both.
    Both,
}

/// A key moment of the guided review: a move to look at again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Moment {
    /// The ply of the move.
    pub ply: usize,
    /// Its class.
    pub class: MoveClass,
}

/// The key moments of the report for the review (D1435, D1436): the
/// mistakes, blunders and missed chances, then the inaccuracies, all in game
/// order; with `highlights` also the brilliant and great moves.
pub fn key_moments(report: &GameReport, side: SideFilter, highlights: bool) -> Vec<Moment> {
    report
        .moves
        .iter()
        .filter(|m| match side {
            SideFilter::White => m.mover == Color::White,
            SideFilter::Black => m.mover == Color::Black,
            SideFilter::Both => true,
        })
        .filter(|m| {
            m.class.is_fault()
                || (highlights && matches!(m.class, MoveClass::Brilliant | MoveClass::Great))
        })
        .map(|m| Moment {
            ply: m.ply,
            class: m.class,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn eval(pawns: f32) -> PositionEval {
        PositionEval {
            score: pawns,
            mate: None,
            pv: Vec::new(),
            depth: 12,
            second: None,
        }
    }

    fn some_move() -> Move {
        mychess_core::Position::new()
            .parse_uci("e2e4")
            .expect("legal move")
    }

    fn other_move() -> Move {
        mychess_core::Position::new()
            .parse_uci("d2d4")
            .expect("legal move")
    }

    fn input(mover: Color, before: f32, after: f32) -> MoveInput {
        MoveInput {
            mover,
            before: eval(before),
            after: eval(after),
            played: some_move(),
            book: false,
            sacrifice: false,
        }
    }

    fn class_of(i: MoveInput) -> MoveClass {
        build_report(&[i], &ReportThresholds::default()).moves[0].class
    }

    #[test]
    fn win_percent_is_centered_and_bounded() {
        assert!((win_percent(0.0) - 50.0).abs() < 1e-4);
        assert!(win_percent(100.0) > 50.0 && win_percent(-100.0) < 50.0);
        assert!((win_percent(300.0) + win_percent(-300.0) - 100.0).abs() < 1e-3);
        assert!(win_percent(1e9) <= 100.0 && win_percent(-1e9) >= 0.0);
        assert_eq!(win_percent(f32::NAN), 50.0);
        // The published curve: +1 pawn is about 59%.
        assert!((win_percent(100.0) - 59.1).abs() < 0.3);
    }

    #[test]
    fn accuracy_falls_with_the_loss_and_stays_in_range() {
        assert!((accuracy_of_loss(0.0) - 100.0).abs() < 0.01);
        assert!(accuracy_of_loss(5.0) > accuracy_of_loss(20.0));
        assert_eq!(accuracy_of_loss(f32::NAN), 0.0);
        assert!(accuracy_of_loss(100.0) >= 0.0);
        assert!(accuracy_of_loss(-5.0) <= 100.0);
    }

    #[test]
    fn the_loss_scale_picks_the_class() {
        // White at +0.0 loses ground: 0 -> -1.0 pawn is about 9 points.
        assert_eq!(class_of(input(Color::White, 0.0, 0.0)), MoveClass::Best);
        assert_eq!(
            class_of(input(Color::White, 0.0, -1.0)),
            MoveClass::Inaccuracy
        );
        assert_eq!(class_of(input(Color::White, 0.0, -2.0)), MoveClass::Mistake);
        assert_eq!(class_of(input(Color::White, 0.0, -6.0)), MoveClass::Blunder);
        // Black's point of view is mirrored: a rising score hurts Black.
        assert_eq!(class_of(input(Color::Black, 0.0, 2.0)), MoveClass::Mistake);
        assert_eq!(class_of(input(Color::Black, 0.0, -2.0)), MoveClass::Best);
    }

    #[test]
    fn the_engine_move_is_best_even_with_a_small_loss() {
        let mut i = input(Color::White, 0.0, -0.2);
        i.before.pv = vec![some_move()];
        assert_eq!(class_of(i.clone()), MoveClass::Best);
        i.before.pv = vec![other_move()];
        assert_eq!(class_of(i), MoveClass::Excellent);
    }

    #[test]
    fn the_engine_move_is_never_a_fault_even_if_the_next_search_is_worse() {
        let mut i = input(Color::White, 0.0, -6.0);
        i.before.pv = vec![some_move()];
        let report = build_report(&[i], &ReportThresholds::default());
        assert_eq!(report.moves[0].class, MoveClass::Best);
        assert_eq!(report.moves[0].loss, 0.0);
        assert!(key_moments(&report, SideFilter::Both, false).is_empty());
    }

    #[test]
    fn a_decided_position_does_not_inflate_the_loss() {
        // +12 to +9 is still winning: the loss is small in win probability.
        assert!(matches!(
            class_of(input(Color::White, 12.0, 9.0)),
            MoveClass::Best | MoveClass::Excellent | MoveClass::Good
        ));
    }

    #[test]
    fn mate_counts_as_a_certain_result_without_dividing_by_zero() {
        let mut i = input(Color::White, 0.0, 0.0);
        i.after.mate = Some(-3);
        i.after.score = -327.0;
        let r = build_report(&[i], &ReportThresholds::default());
        assert_eq!(r.moves[0].class, MoveClass::Blunder);
        assert!(r.moves[0].loss <= 100.0 && r.moves[0].loss >= 50.0);
    }

    #[test]
    fn book_moves_are_counted_but_leave_the_accuracy() {
        let mut book = input(Color::White, 0.0, -3.0);
        book.book = true;
        let r = build_report(&[book], &ReportThresholds::default());
        assert_eq!(r.moves[0].class, MoveClass::Book);
        assert_eq!(r.white.count(MoveClass::Book), 1);
        assert_eq!(r.white.accuracy, None);
        assert_eq!(r.black.accuracy, None);
    }

    #[test]
    fn a_great_move_needs_a_far_second_line() {
        let mut i = input(Color::White, 1.0, 1.0);
        i.before.pv = vec![some_move()];
        i.before.second = Some(-3.0);
        assert_eq!(class_of(i.clone()), MoveClass::Great);
        i.before.second = Some(0.8);
        assert_eq!(class_of(i.clone()), MoveClass::Best);
        // Decided: +12 against +8 is not an only move.
        let mut decided = input(Color::White, 12.0, 12.0);
        decided.before.pv = vec![some_move()];
        decided.before.second = Some(-3.0);
        assert_eq!(class_of(decided), MoveClass::Best);
    }

    #[test]
    fn a_brilliant_move_is_a_sound_sacrifice() {
        let mut i = input(Color::White, 1.0, 1.0);
        i.before.pv = vec![some_move()];
        i.sacrifice = true;
        assert_eq!(class_of(i.clone()), MoveClass::Brilliant);
        // A sacrifice that leaves the side lost is not brilliant.
        i.after = eval(-5.0);
        assert_ne!(class_of(i), MoveClass::Brilliant);
    }

    #[test]
    fn a_wasted_chance_after_the_opponents_blunder_is_a_miss() {
        // Black blundered (0 -> +3 for White), then White plays a 2-pawn
        // slip: 3.0 -> 0.8.
        let black = input(Color::Black, 0.0, 3.0);
        let white = input(Color::White, 3.0, 0.8);
        let r = build_report(&[black, white], &ReportThresholds::default());
        assert_eq!(r.moves[0].class, MoveClass::Blunder);
        assert_eq!(r.moves[1].class, MoveClass::Miss);
        // Without the opponent's error it is a plain mistake or worse.
        let calm = build_report(
            &[input(Color::White, 3.0, 0.8)],
            &ReportThresholds::default(),
        );
        assert_ne!(calm.moves[0].class, MoveClass::Miss);
    }

    #[test]
    fn the_counts_add_up_to_the_moves_of_each_side() {
        let inputs: Vec<MoveInput> = (0..12)
            .map(|i| {
                let mover = if i % 2 == 0 {
                    Color::White
                } else {
                    Color::Black
                };
                input(mover, 0.0, if i % 3 == 0 { -2.5 } else { 0.0 })
            })
            .collect();
        let r = build_report(&inputs, &ReportThresholds::default());
        assert_eq!(r.white.total(), 6);
        assert_eq!(r.black.total(), 6);
        for side in [&r.white, &r.black] {
            let acc = side.accuracy.expect("accuracy");
            assert!((0.0..=100.0).contains(&acc));
        }
    }

    #[test]
    fn the_same_analysis_gives_the_same_numbers() {
        let inputs: Vec<MoveInput> = (0..8)
            .map(|i| {
                input(
                    if i % 2 == 0 {
                        Color::White
                    } else {
                        Color::Black
                    },
                    0.2,
                    -0.4,
                )
            })
            .collect();
        let t = ReportThresholds::default();
        assert_eq!(build_report(&inputs, &t), build_report(&inputs, &t));
    }

    #[test]
    fn an_empty_game_has_an_empty_report() {
        let r = build_report(&[], &ReportThresholds::default());
        assert!(r.moves.is_empty());
        assert_eq!(r.white.accuracy, None);
        assert_eq!(r.white.total(), 0);
    }

    #[test]
    fn key_moments_follow_the_side_and_the_highlights() {
        let inputs = vec![
            input(Color::White, 0.0, -2.0),  // mistake
            input(Color::Black, 0.0, 0.0),   // best
            input(Color::White, -2.0, -2.0), // best
            input(Color::Black, 0.0, 6.0),   // blunder
        ];
        let r = build_report(&inputs, &ReportThresholds::default());
        let both = key_moments(&r, SideFilter::Both, false);
        assert_eq!(both.iter().map(|m| m.ply).collect::<Vec<_>>(), vec![1, 4]);
        let white = key_moments(&r, SideFilter::White, false);
        assert_eq!(white.len(), 1);
        assert_eq!(white[0].class, MoveClass::Mistake);
        let black = key_moments(&r, SideFilter::Black, true);
        assert_eq!(black.len(), 1);
    }

    proptest! {
        #[test]
        fn a_bigger_loss_never_gives_a_better_class_or_accuracy(
            before in -15.0f32..15.0,
            a in 0.0f32..20.0,
            b in 0.0f32..20.0,
        ) {
            let (small, big) = if a <= b { (a, b) } else { (b, a) };
            let t = ReportThresholds::default();
            let one = build_report(&[input(Color::White, before, before - small)], &t);
            let two = build_report(&[input(Color::White, before, before - big)], &t);
            // The scale classes are ordered Best < ... < Blunder (the
            // special classes need other inputs, absent here).
            prop_assert!(one.moves[0].loss <= two.moves[0].loss + 1e-3);
            prop_assert!(one.moves[0].accuracy + 1e-3 >= two.moves[0].accuracy);
            let rank = |c: MoveClass| match c {
                MoveClass::Best => 0,
                MoveClass::Excellent => 1,
                MoveClass::Good => 2,
                MoveClass::Inaccuracy => 3,
                MoveClass::Mistake => 4,
                _ => 5,
            };
            prop_assert!(rank(one.moves[0].class) <= rank(two.moves[0].class));
        }
    }
}
