//! Extraction of the score and time series from the main line.

use mychess_core::{Game, NodeId};
use mychess_core::{clock_of, emt_of, eval_of};

use super::axes::x_of_ply;

/// Scores are clamped to `-SCORE_LIMIT..=SCORE_LIMIT` pawns, as Scid does
/// for its graph (`tkscid.cpp:9285-9287`).
pub const SCORE_LIMIT: f32 = 10.0;

/// One bar of the score graph.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScorePoint {
    /// `ply * 0.5 + 0.5` (see the module docs).
    pub x: f32,
    /// Score in pawns, already clamped to [`SCORE_LIMIT`].
    pub y: f32,
}

/// Which scores are negated (Scid's "Invert White"/"Invert Black").
///
/// The names follow Scid: `white` negates the score written in the comment
/// after **Black's** moves (the positions where White is to move), `black`
/// the one after **White's** moves. A flag names the side to move in the
/// evaluated position, which is the point of view of engines that report
/// the score for the side to move.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Invert {
    /// Negate the scores of the positions where White is to move.
    pub white: bool,
    /// Negate the scores of the positions where Black is to move.
    pub black: bool,
}

/// One bar of the time graph.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TimePoint {
    /// `ply * 0.5 + 0.5`.
    pub x: f32,
    /// Seconds spent on the move.
    pub seconds: u32,
}

/// Time data of a game.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TimeSeries {
    /// Time of each move that has one.
    pub per_move: Vec<TimePoint>,
    /// `(x, running total in seconds)` for White's moves.
    pub white_total: Vec<(f32, f32)>,
    /// `(x, running total in seconds)` for Black's moves.
    pub black_total: Vec<(f32, f32)>,
}

/// The main line as `(ply, node)` pairs, ply starting at 1.
pub(super) fn mainline_plies(game: &Game) -> Vec<(usize, NodeId)> {
    game.mainline()
        .enumerate()
        .map(|(i, id)| (i + 1, id))
        .collect()
}

fn comment_of(game: &Game, id: NodeId) -> Option<&str> {
    game.comment(id).ok().flatten()
}

/// The score series of the main line: one point per move whose comment holds
/// an evaluation (`[%eval X]`, `#N`, or the old `+0.35` form), clamped to
/// +-[`SCORE_LIMIT`]. Moves without a score leave a gap. Variations are
/// ignored.
///
/// The inversion is applied before the clamp (see [`Invert`]).
pub fn score_series(game: &Game, invert: Invert) -> Vec<ScorePoint> {
    score_series_in(game, &mainline_plies(game), invert)
}

/// [`score_series`] over an already collected main line.
pub(super) fn score_series_in(
    game: &Game,
    plies: &[(usize, NodeId)],
    invert: Invert,
) -> Vec<ScorePoint> {
    plies
        .iter()
        .filter_map(|&(ply, id)| {
            let mut y = eval_of(comment_of(game, id)?)?;
            if !y.is_finite() {
                return None;
            }
            // Odd ply: White just moved, so Black is to move there.
            let flip = if ply % 2 == 1 {
                invert.black
            } else {
                invert.white
            };
            if flip {
                y = -y;
            }
            Some(ScorePoint {
                x: x_of_ply(ply),
                y: y.clamp(-SCORE_LIMIT, SCORE_LIMIT),
            })
        })
        .collect()
}

/// The increment of a `TimeControl` tag of the form `base+increment`
/// (leading integers; anything else gives 0). Scid's `moves/period` branch
/// reads the period as an increment, which is a defect and is not
/// reproduced.
fn increment_of(game: &Game) -> i64 {
    let Some(tc) = game.tags().get_extra("TimeControl") else {
        return 0;
    };
    let digits = |s: &str| -> Option<(i64, usize)> {
        let n = s.bytes().take_while(u8::is_ascii_digit).count();
        if n == 0 || n > 9 {
            return None;
        }
        s[..n].parse().ok().map(|v| (v, n))
    };
    let tc = tc.trim();
    let Some((_, n)) = digits(tc) else {
        return 0;
    };
    match tc[n..].strip_prefix('+').and_then(digits) {
        Some((incr, _)) => incr,
        None => 0,
    }
}

/// The time series of the main line.
///
/// Uses `[%emt]` per move. When the game has more `[%clk]` than `[%emt]`
/// entries and the first clock is that of ply 1, the move times are computed
/// from the clocks instead: `emt = previous - clock + increment` (never
/// below 0), the previous clock starting at the larger of the two first
/// clocks plus one second, and the increment coming from `TimeControl`.
/// If a single clock is missing in the middle, the series ends there.
///
/// Differences from Scid (D763, D772): `%clk` in `m:s` form is accepted (Scid
/// wants three fields) while a four-field clock is rejected (Scid takes it);
/// fractions of a second are truncated, the arithmetic assumes whole-second
/// clocks; and the core's `eval_of` is more lenient than Scid's (for instance
/// `"1-0 +0.5"` yields 0.5), which is kept as a benign difference.
pub fn time_series(game: &Game) -> TimeSeries {
    time_series_in(game, &mainline_plies(game))
}

/// [`time_series`] over an already collected main line (one pass over the
/// comments for both `%emt` and `%clk`).
pub(super) fn time_series_in(game: &Game, plies: &[(usize, NodeId)]) -> TimeSeries {
    let mut emts: Vec<(usize, u32)> = Vec::new();
    let mut clks: Vec<(usize, u32)> = Vec::new();
    for &(p, id) in plies {
        let Some(comment) = comment_of(game, id) else {
            continue;
        };
        if let Some(e) = emt_of(comment) {
            emts.push((p, e));
        }
        if let Some(c) = clock_of(comment) {
            clks.push((p, c));
        }
    }

    let per_ply: Vec<(usize, u32)> =
        if clks.len() > emts.len() && clks.first().is_some_and(|c| c.0 == 1) {
            emts_from_clocks(&clks, increment_of(game))
        } else {
            emts
        };

    let mut out = TimeSeries::default();
    let (mut white, mut black) = (0u64, 0u64);
    for (ply, seconds) in per_ply {
        let x = x_of_ply(ply);
        out.per_move.push(TimePoint { x, seconds });
        if ply % 2 == 1 {
            white += u64::from(seconds);
            out.white_total.push((x, white as f32));
        } else {
            black += u64::from(seconds);
            out.black_total.push((x, black as f32));
        }
    }
    out
}

/// Scid's `%clk` to `%emt` conversion (`graphs.tcl:512-588`).
fn emts_from_clocks(clks: &[(usize, u32)], incr: i64) -> Vec<(usize, u32)> {
    // Needs at least the first White and Black clocks to guess the start.
    let (Some(&(_, w0)), Some(&(_, b0))) = (clks.first(), clks.get(1)) else {
        return Vec::new();
    };
    let initial = i64::from(w0.max(b0)) + 1;
    let (mut prev_white, mut prev_black) = (initial, initial);
    let mut out = Vec::new();
    for pair in clks.chunks(2) {
        let [first, ..] = pair else { break };
        let (wp, wc) = *first;
        // Mismatched data (not White's ply, or Black's reply is not even):
        // the graph ends here.
        if wp % 2 == 0 || pair.get(1).is_some_and(|b| b.0 % 2 == 1) {
            break;
        }
        let clock = i64::from(wc);
        let emt = (prev_white - clock + incr).max(0);
        prev_white = clock;
        out.push((wp, u32::try_from(emt).unwrap_or(u32::MAX)));
        if let Some(&(bp, bc)) = pair.get(1) {
            let clock = i64::from(bc);
            let emt = (prev_black - clock + incr).max(0);
            prev_black = clock;
            out.push((bp, u32::try_from(emt).unwrap_or(u32::MAX)));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::super::test_util::{TWELVE, game_with};
    use super::*;

    fn ys(g: &Game, inv: Invert) -> Vec<(f32, f32)> {
        score_series(g, inv).iter().map(|p| (p.x, p.y)).collect()
    }

    #[test]
    fn eval_forms() {
        let g = game_with(
            &TWELVE[..6],
            &[
                "[%eval 0.35]",
                "[%eval -1.2]",
                "[%eval #3]",
                "[%eval #-2]",
                "+0.35: good",
                "[%eval 25]",
            ],
        );
        let got = ys(&g, Invert::default());
        assert_eq!(
            got,
            vec![
                (1.0, 0.35),
                (1.5, -1.2),
                (2.0, 10.0),
                (2.5, -10.0),
                (3.0, 0.35),
                (3.5, 10.0)
            ]
        );
    }

    #[test]
    fn no_score_leaves_gaps() {
        let g = game_with(
            &TWELVE[..5],
            &["[%eval 1]", "just text", "1-0", "[%eval ]", "[%eval -0.5]"],
        );
        assert_eq!(ys(&g, Invert::default()), vec![(1.0, 1.0), (3.0, -0.5)]);
        assert!(score_series(&Game::new(), Invert::default()).is_empty());
    }

    #[test]
    fn inversion_by_side_to_move() {
        let g = game_with(&TWELVE[..4], &["[%eval 1]"; 4]);
        let at = |inv| ys(&g, inv).iter().map(|p| p.1).collect::<Vec<_>>();
        assert_eq!(at(Invert::default()), vec![1.0; 4]);
        // `black`: positions where Black is to move = after White's moves.
        assert_eq!(
            at(Invert {
                white: false,
                black: true
            }),
            vec![-1.0, 1.0, -1.0, 1.0]
        );
        assert_eq!(
            at(Invert {
                white: true,
                black: false
            }),
            vec![1.0, -1.0, 1.0, -1.0]
        );
        assert_eq!(
            at(Invert {
                white: true,
                black: true
            }),
            vec![-1.0; 4]
        );
    }

    #[test]
    fn invert_before_clamp() {
        let g = game_with(&TWELVE[..1], &["[%eval #-2]"]);
        let inv = Invert {
            white: false,
            black: true,
        };
        assert_eq!(ys(&g, inv), vec![(1.0, 10.0)]);
    }

    #[test]
    fn variations_ignored() {
        let mut g = game_with(&TWELVE[..2], &["[%eval 1]", "[%eval 2]"]);
        let root = g.root();
        let mut pos = mychess_core::Position::new();
        let mv = pos.parse_uci("d2d4").unwrap();
        pos.play(mv).unwrap();
        let v = g.add_move(root, mv).unwrap();
        g.set_comment(v, "[%eval 9]").unwrap();
        assert_eq!(ys(&g, Invert::default()), vec![(1.0, 1.0), (1.5, 2.0)]);
    }

    fn clk(secs: u32) -> String {
        format!(
            "[%clk {}:{:02}:{:02}]",
            secs / 3600,
            secs / 60 % 60,
            secs % 60
        )
    }

    #[test]
    fn emt_formats() {
        let g = game_with(
            &TWELVE[..4],
            &["[%emt 0:00:07]", "[%emt 1:05]", "x", "[%emt 1:00:00]"],
        );
        let t = time_series(&g);
        let got: Vec<_> = t.per_move.iter().map(|p| (p.x, p.seconds)).collect();
        assert_eq!(got, vec![(1.0, 7), (1.5, 65), (2.5, 3600)]);
        assert_eq!(t.white_total, vec![(1.0, 7.0)]);
        assert_eq!(t.black_total, vec![(1.5, 65.0), (2.5, 3665.0)]);
    }

    #[test]
    fn clocks_with_increment() {
        // 5+3: clocks after each ply.
        let clocks = [300, 300, 295, 298, 290, 301, 400];
        let texts: Vec<String> = clocks.iter().map(|&c| clk(c)).collect();
        let refs: Vec<&str> = texts.iter().map(String::as_str).collect();
        let mut g = game_with(&TWELVE[..7], &refs);
        g.tags_mut().set_extra("TimeControl", "300+3").unwrap();
        let t = time_series(&g);
        let secs: Vec<u32> = t.per_move.iter().map(|p| p.seconds).collect();
        // initial = 301; w: 301-300+3=4; b: 301-300+3=4; w: 300-295+3=8;
        // b: 300-298+3=5; w: 295-290+3=8; b: 298-301+3=0 (clamped from 0);
        // w: 290-400+3 < 0 -> 0.
        assert_eq!(secs, vec![4, 4, 8, 5, 8, 0, 0]);
        assert_eq!(t.white_total.last(), Some(&(4.0, 20.0)));
        assert_eq!(t.black_total.last(), Some(&(3.5, 9.0)));
    }

    #[test]
    fn clocks_without_increment_and_period_form() {
        let texts: Vec<String> = [100, 100, 90, 95].iter().map(|&c| clk(c)).collect();
        let refs: Vec<&str> = texts.iter().map(String::as_str).collect();
        let mut g = game_with(&TWELVE[..4], &refs);
        let secs =
            |g: &Game| -> Vec<u32> { time_series(g).per_move.iter().map(|p| p.seconds).collect() };
        assert_eq!(secs(&g), vec![1, 1, 10, 5]);
        g.tags_mut().set_extra("TimeControl", "40/7200").unwrap();
        assert_eq!(secs(&g), vec![1, 1, 10, 5]);
    }

    #[test]
    fn missing_clock_ends_series() {
        let a = clk(100);
        let b = clk(90);
        // ply 3 has no clock: the pair (ply 4 first) is mismatched.
        let g = game_with(&TWELVE[..5], &[&a, &a, "", &b, &b]);
        let t = time_series(&g);
        assert_eq!(t.per_move.len(), 2);
    }

    #[test]
    fn more_emt_than_clk_uses_emt() {
        let c = format!("[%emt 0:05] {}", clk(100));
        let g = game_with(&TWELVE[..2], &[&c, &c]);
        let secs: Vec<u32> = time_series(&g).per_move.iter().map(|p| p.seconds).collect();
        assert_eq!(secs, vec![5, 5]);
    }

    #[test]
    fn first_clock_not_ply_one_uses_emt() {
        let a = clk(100);
        let g = game_with(&TWELVE[..3], &["", &a, &a]);
        assert!(time_series(&g).per_move.is_empty());
    }

    #[test]
    fn strange_text_does_not_panic() {
        let g = game_with(
            &TWELVE[..3],
            &["[%emt ::]", "[%clk 1:2:3:4]", "[%emt 99999999999:1]"],
        );
        assert!(time_series(&g).per_move.is_empty());
        assert_eq!(increment_of(&g), 0);
    }
}
