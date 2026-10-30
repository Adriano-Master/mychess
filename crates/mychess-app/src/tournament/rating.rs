//! Elo and likelihood of superiority of the participants (spec M8-2c,
//! D2510-D2513).
//!
//! Pure functions over the finished games. The Elo here is **relative to the
//! opponents** the participant met (0 means a score of 50%), not an absolute
//! rating and not a Bradley-Terry fit.
//!
//! - score `s = (W + D/2) / N`, Elo `= -400 * log10(1/s - 1)`; `s = 0` or
//!   `s = 1` give `-inf` / `+inf`;
//! - 95% interval from the trinomial variance of the score (z = 1.959964),
//!   mapped point by point into Elo, so the bounds are not symmetric;
//! - LOS `= Phi((W - L) / sqrt(W + L))`, draws ignored.
//!
//! With few games the numbers are shown but labelled by [`Reliability`].

use std::cmp::Ordering;

use super::standings::{Finished, GameScore};

/// The two-sided 95% quantile of the normal distribution.
pub const Z95: f64 = 1.959_964;
/// Games below which the estimate is labelled "very uncertain".
pub const TINY_SAMPLE: u32 = 10;
/// Games below which the estimate is labelled "low".
pub const SMALL_SAMPLE: u32 = 30;

/// How much a number of games can be trusted (D2511).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Reliability {
    /// No game at all.
    Empty,
    /// 1 to 9 games.
    VeryUncertain,
    /// 10 to 29 games.
    Low,
    /// 30 games or more.
    Fair,
}

impl Reliability {
    /// The label of a sample of `games` games.
    pub fn of(games: u32) -> Reliability {
        match games {
            0 => Reliability::Empty,
            1..TINY_SAMPLE => Reliability::VeryUncertain,
            TINY_SAMPLE..SMALL_SAMPLE => Reliability::Low,
            _ => Reliability::Fair,
        }
    }
}

/// Wins, draws and losses of one side.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Record {
    /// Wins.
    pub wins: u32,
    /// Draws.
    pub draws: u32,
    /// Losses.
    pub losses: u32,
}

impl Record {
    /// Games played.
    pub fn games(self) -> u32 {
        self.wins
            .saturating_add(self.draws)
            .saturating_add(self.losses)
    }

    /// The same record seen from the opponent.
    #[must_use]
    pub fn flipped(self) -> Record {
        Record {
            wins: self.losses,
            draws: self.draws,
            losses: self.wins,
        }
    }

    /// The score in `[0, 1]`; `None` without games.
    pub fn score(self) -> Option<f64> {
        let games = self.games();
        if games == 0 {
            return None;
        }
        let points = f64::from(self.wins) + f64::from(self.draws) / 2.0;
        Some(points / f64::from(games))
    }

    fn add(&mut self, score: GameScore) {
        match score {
            GameScore::Win => self.wins = self.wins.saturating_add(1),
            GameScore::Draw => self.draws = self.draws.saturating_add(1),
            GameScore::Loss => self.losses = self.losses.saturating_add(1),
        }
    }
}

/// The Elo difference that a score `s` means; `+inf` for 1, `-inf` for 0.
/// `None` for a number outside `[0, 1]` or not a number.
///
/// ```
/// use mychess_app::tournament::elo_from_score;
///
/// assert_eq!(elo_from_score(0.5), Some(0.0));
/// assert!((elo_from_score(0.6).unwrap() - 70.44).abs() < 0.01);
/// assert_eq!(elo_from_score(1.0), Some(f64::INFINITY));
/// ```
pub fn elo_from_score(s: f64) -> Option<f64> {
    if s.is_nan() || !(0.0..=1.0).contains(&s) {
        return None;
    }
    if s <= 0.0 {
        return Some(f64::NEG_INFINITY);
    }
    if s >= 1.0 {
        return Some(f64::INFINITY);
    }
    let elo = -400.0 * (1.0 / s - 1.0).log10();
    // `-0.0` for a score of one half would print as `-0`.
    Some(if elo == 0.0 { 0.0 } else { elo })
}

/// The error function, by the formula 7.1.26 of Abramowitz and Stegun
/// (absolute error below `1.5e-7`).
fn erf(x: f64) -> f64 {
    let sign = if x < 0.0 { -1.0 } else { 1.0 };
    let x = x.abs();
    let t = 1.0 / (1.0 + 0.327_591_1 * x);
    let poly = t
        * (0.254_829_592
            + t * (-0.284_496_736
                + t * (1.421_413_741 + t * (-1.453_152_027 + t * 1.061_405_429))));
    sign * (1.0 - poly * (-x * x).exp())
}

/// The standard normal distribution function.
pub fn normal_cdf(x: f64) -> f64 {
    f64::midpoint(1.0, erf(x / std::f64::consts::SQRT_2))
}

/// The likelihood of superiority of the side with `record`, in `[0, 1]`
/// (D2512). `None` when no game was decisive.
///
/// ```
/// use mychess_app::tournament::{Record, los};
///
/// let even = Record { wins: 5, draws: 9, losses: 5 };
/// assert_eq!(los(even), Some(0.5));
/// assert_eq!(los(Record { wins: 0, draws: 4, losses: 0 }), None);
/// ```
pub fn los(record: Record) -> Option<f64> {
    let decisive = record.wins.saturating_add(record.losses);
    if decisive == 0 {
        return None;
    }
    if record.wins == record.losses {
        return Some(0.5);
    }
    let diff = f64::from(record.wins) - f64::from(record.losses);
    Some(normal_cdf(diff / f64::from(decisive).sqrt()).clamp(0.0, 1.0))
}

/// The interval of the Elo of a record.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EloInterval {
    /// The lower end.
    pub lower: f64,
    /// The upper end.
    pub upper: f64,
}

impl EloInterval {
    /// Half the width; `+inf` when an end is infinite.
    pub fn margin(self) -> f64 {
        (self.upper - self.lower) / 2.0
    }
}

/// What a record says about the strength of a side.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RatingEstimate {
    /// The record.
    pub record: Record,
    /// The score, `None` without games.
    pub score: Option<f64>,
    /// The Elo relative to the opponents; may be infinite; `None` without
    /// games.
    pub elo: Option<f64>,
    /// The 95% interval; `None` when the sample says nothing about the
    /// spread (fewer than 2 games, or all results equal).
    pub interval: Option<EloInterval>,
    /// The likelihood of superiority.
    pub los: Option<f64>,
    /// How much to trust it.
    pub reliability: Reliability,
}

impl RatingEstimate {
    /// The half-width of the interval, if there is one.
    pub fn margin(&self) -> Option<f64> {
        self.interval.map(EloInterval::margin)
    }
}

/// Estimates the strength behind `record` (D2510, D2511).
pub fn estimate(record: Record) -> RatingEstimate {
    let games = record.games();
    let score = record.score();
    let elo = score.and_then(elo_from_score);
    let interval = score.and_then(|s| interval_of(record, s));
    RatingEstimate {
        record,
        score,
        elo,
        interval,
        los: los(record),
        reliability: Reliability::of(games),
    }
}

fn interval_of(record: Record, s: f64) -> Option<EloInterval> {
    let games = record.games();
    if games < 2 {
        return None;
    }
    let n = f64::from(games);
    let variance = (f64::from(record.wins) * (1.0 - s).powi(2)
        + f64::from(record.draws) * (0.5 - s).powi(2)
        + f64::from(record.losses) * s.powi(2))
        / n;
    if variance <= 0.0 {
        return None;
    }
    let se = (variance / n).sqrt();
    let low = elo_from_score((s - Z95 * se).clamp(0.0, 1.0))?;
    let high = elo_from_score((s + Z95 * se).clamp(0.0, 1.0))?;
    Some(EloInterval {
        lower: low,
        upper: high,
    })
}

/// The record of `a` against `b` in `games` (both orders of colours).
pub fn pairwise_record(games: &[Finished], a: usize, b: usize) -> Record {
    let mut record = Record::default();
    for game in games {
        let white_a = game.white == a && game.black == b;
        let white_b = game.white == b && game.black == a;
        if !white_a && !white_b {
            continue;
        }
        if let Some(score) = super::standings::score_of(game, a) {
            record.add(score);
        }
    }
    record
}

/// The record of `participant` against everybody.
pub fn field_record(games: &[Finished], participant: usize) -> Record {
    let mut record = Record::default();
    for game in games {
        if let Some(score) = super::standings::score_of(game, participant) {
            record.add(score);
        }
    }
    record
}

/// A line of the table of ratings (D2513).
#[derive(Debug, Clone, PartialEq)]
pub struct RatingRow {
    /// Position in the table, from 1.
    pub rank: usize,
    /// The participant.
    pub participant: usize,
    /// The estimate against the whole field.
    pub estimate: RatingEstimate,
    /// The rating performance: the mean registered Elo of the opponents,
    /// weighted by the games, plus the relative Elo. Only when every
    /// opponent has a registered Elo and the relative Elo is finite.
    pub performance: Option<f64>,
    /// The participant left the tournament.
    pub withdrawn: bool,
}

/// The table of ratings, ordered by Elo (descending, `+inf` first), then by
/// points, then by order of registration (D2513). `elos[i]` is the registered
/// Elo of participant `i`.
pub fn rating_table(
    elos: &[Option<u16>],
    withdrawn: &[bool],
    games: &[Finished],
) -> Vec<RatingRow> {
    let n = elos.len();
    let mut rows: Vec<RatingRow> = (0..n)
        .map(|participant| {
            let record = field_record(games, participant);
            RatingRow {
                rank: 0,
                participant,
                estimate: estimate(record),
                performance: performance(elos, games, participant),
                withdrawn: withdrawn.get(participant).copied().unwrap_or(false),
            }
        })
        .collect();
    rows.sort_by(compare_rows);
    for (i, row) in rows.iter_mut().enumerate() {
        row.rank = i + 1;
    }
    rows
}

fn compare_rows(a: &RatingRow, b: &RatingRow) -> Ordering {
    // Without games the Elo is unknown and goes last.
    let elo = |row: &RatingRow| row.estimate.elo;
    let by_elo = match (elo(a), elo(b)) {
        (Some(x), Some(y)) => y.total_cmp(&x),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    };
    by_elo
        .then_with(|| {
            let points = |row: &RatingRow| {
                u64::from(row.estimate.record.wins) * 2 + u64::from(row.estimate.record.draws)
            };
            points(b).cmp(&points(a))
        })
        .then(a.participant.cmp(&b.participant))
}

fn performance(elos: &[Option<u16>], games: &[Finished], participant: usize) -> Option<f64> {
    let record = field_record(games, participant);
    let relative = record.score().and_then(elo_from_score)?;
    if !relative.is_finite() {
        return None;
    }
    let mut total = 0.0;
    let mut count = 0u32;
    for game in games {
        let opponent = if game.white == participant {
            game.black
        } else if game.black == participant {
            game.white
        } else {
            continue;
        };
        if super::standings::score_of(game, participant).is_none() {
            continue;
        }
        total += f64::from((*elos.get(opponent)?).as_ref()?.to_owned());
        count += 1;
    }
    if count == 0 {
        return None;
    }
    Some(total / f64::from(count) + relative)
}

/// The likelihood of superiority of every participant over every other:
/// `matrix[i][j]` is the LOS of `i` over `j` (`None` on the diagonal and
/// without decisive games).
pub fn los_matrix(n: usize, games: &[Finished]) -> Vec<Vec<Option<f64>>> {
    (0..n)
        .map(|i| {
            (0..n)
                .map(|j| {
                    if i == j {
                        None
                    } else {
                        los(pairwise_record(games, i, j))
                    }
                })
                .collect()
        })
        .collect()
}

/// An Elo as text for the tables: `+168`, `-12`, `0`, `+inf`, `-inf`; `-`
/// for none. Infinite values use plain words so every font draws them.
pub fn elo_text(elo: Option<f64>) -> String {
    match elo {
        None => "-".to_owned(),
        Some(e) if e == f64::INFINITY => "+inf".to_owned(),
        Some(e) if e == f64::NEG_INFINITY => "-inf".to_owned(),
        Some(e) => {
            let rounded = e.round();
            if rounded == 0.0 {
                "0".to_owned()
            } else {
                format!("{rounded:+.0}")
            }
        }
    }
}

/// The margin as text: `49`, `inf`, `-` for none.
pub fn margin_text(margin: Option<f64>) -> String {
    match margin {
        None => "-".to_owned(),
        Some(m) if m.is_infinite() => "inf".to_owned(),
        Some(m) => format!("{:.0}", m.round()),
    }
}

/// The LOS as a percentage text: `93.2`, `-` for none.
pub fn los_text(los: Option<f64>) -> String {
    match los {
        None => "-".to_owned(),
        Some(l) => format!("{:.1}", l * 100.0),
    }
}

#[cfg(test)]
mod tests {
    use mychess_core::GameResult;

    use super::*;

    fn rec(wins: u32, draws: u32, losses: u32) -> Record {
        Record {
            wins,
            draws,
            losses,
        }
    }

    fn game(number: u32, white: usize, black: usize, result: GameResult) -> Finished {
        Finished {
            number,
            white,
            black,
            result,
        }
    }

    #[test]
    fn elo_follows_the_logistic_formula() {
        assert_eq!(elo_from_score(0.5), Some(0.0));
        assert!(
            elo_from_score(0.5).expect("elo").is_sign_positive(),
            "not -0.0"
        );
        let e = elo_from_score(0.6).expect("elo");
        assert!((e - 70.44).abs() < 0.01, "{e}");
        let e = elo_from_score(0.725).expect("elo");
        assert!((e - 168.4).abs() < 0.1, "{e}");
        assert_eq!(elo_from_score(1.0), Some(f64::INFINITY));
        assert_eq!(elo_from_score(0.0), Some(f64::NEG_INFINITY));
        assert_eq!(elo_from_score(1.5), None);
        assert_eq!(elo_from_score(f64::NAN), None);
    }

    #[test]
    fn a_record_without_games_says_nothing() {
        let e = estimate(Record::default());
        assert_eq!(e.elo, None);
        assert_eq!(e.interval, None);
        assert_eq!(e.los, None);
        assert_eq!(e.reliability, Reliability::Empty);
    }

    #[test]
    fn all_wins_is_infinite_without_an_interval() {
        let e = estimate(rec(10, 0, 0));
        assert_eq!(e.elo, Some(f64::INFINITY));
        assert_eq!(e.interval, None);
        assert_eq!(e.margin(), None);
        assert!(e.los.expect("los") > 0.99);
    }

    #[test]
    fn all_draws_is_zero_with_no_spread() {
        let e = estimate(rec(0, 8, 0));
        assert_eq!(e.elo, Some(0.0));
        assert_eq!(e.interval, None);
        assert_eq!(e.los, None);
    }

    #[test]
    fn one_game_has_no_interval() {
        let e = estimate(rec(1, 0, 0));
        assert_eq!(e.interval, None);
        assert_eq!(e.reliability, Reliability::VeryUncertain);
    }

    #[test]
    fn the_interval_matches_the_reference_case() {
        // 55 W, 35 D, 10 L: s = 0.725.
        let e = estimate(rec(55, 35, 10));
        let elo = e.elo.expect("elo");
        assert!((elo - 168.4).abs() < 0.1, "{elo}");
        let interval = e.interval.expect("interval");
        assert!((interval.lower - 114.8).abs() < 0.5, "{interval:?}");
        assert!((interval.upper - 230.8).abs() < 0.5, "{interval:?}");
        assert!((e.margin().expect("margin") - 58.0).abs() < 0.5);
        assert!(interval.lower < elo && elo < interval.upper);
    }

    #[test]
    fn a_six_four_match_has_the_expected_elo() {
        let e = estimate(rec(6, 0, 4));
        assert!((e.elo.expect("elo") - 70.44).abs() < 0.01);
        // Wide: ten games say little.
        assert!(e.margin().expect("margin") > 100.0);
        assert_eq!(e.reliability, Reliability::Low);
    }

    #[test]
    fn los_reference_values() {
        assert_eq!(los(rec(7, 3, 7)), Some(0.5));
        let l = los(rec(100, 0, 80)).expect("los");
        assert!((l - 0.932).abs() < 0.002, "{l}");
        assert_eq!(los(rec(0, 5, 0)), None);
        // One lonely win is not proof.
        assert!((los(rec(1, 0, 0)).expect("los") - 0.841).abs() < 0.002);
        assert!(los(rec(0, 0, 9)).expect("los") < 0.01);
    }

    #[test]
    fn los_of_both_sides_adds_up_to_one() {
        for (w, d, l) in [(3, 2, 9), (40, 10, 5), (1, 0, 1), (12, 30, 11)] {
            let a = los(rec(w, d, l)).expect("a");
            let b = los(rec(w, d, l).flipped()).expect("b");
            assert!((a + b - 1.0).abs() < 1e-6, "{a} {b}");
        }
    }

    #[test]
    fn the_elo_of_the_two_sides_is_opposite() {
        let a = estimate(rec(12, 7, 5));
        let b = estimate(rec(12, 7, 5).flipped());
        let (x, y) = (a.elo.expect("a"), b.elo.expect("b"));
        assert!((x + y).abs() < 1e-9);
    }

    #[test]
    fn reliability_bands() {
        assert_eq!(Reliability::of(0), Reliability::Empty);
        assert_eq!(Reliability::of(9), Reliability::VeryUncertain);
        assert_eq!(Reliability::of(10), Reliability::Low);
        assert_eq!(Reliability::of(29), Reliability::Low);
        assert_eq!(Reliability::of(30), Reliability::Fair);
    }

    #[test]
    fn normal_cdf_has_the_known_points() {
        assert!((normal_cdf(0.0) - 0.5).abs() < 1e-7);
        assert!((normal_cdf(1.959_964) - 0.975).abs() < 1e-5);
        assert!((normal_cdf(-1.0) - 0.158_655).abs() < 1e-5);
    }

    #[test]
    fn pairwise_and_field_records_count_both_colours() {
        let games = [
            game(1, 0, 1, GameResult::White),
            game(2, 1, 0, GameResult::Draw),
            game(3, 0, 2, GameResult::Black),
            game(4, 1, 2, GameResult::Unknown),
        ];
        assert_eq!(pairwise_record(&games, 0, 1), rec(1, 1, 0));
        assert_eq!(pairwise_record(&games, 1, 0), rec(0, 1, 1));
        assert_eq!(field_record(&games, 0), rec(1, 1, 1));
        // The unfinished game does not count.
        assert_eq!(field_record(&games, 2), rec(1, 0, 0));
    }

    #[test]
    fn the_table_orders_by_elo_with_infinity_first_and_empty_last() {
        let games = [
            game(1, 0, 1, GameResult::White),
            game(2, 1, 0, GameResult::White),
            game(3, 0, 2, GameResult::Draw),
            game(4, 1, 2, GameResult::Black),
        ];
        // Participant 3 has no games. 0: 1W 1D 1L; 1: 1W 0D 2L; 2: 1W 1D.
        let table = rating_table(&[None; 4], &[false; 4], &games);
        let order: Vec<usize> = table.iter().map(|r| r.participant).collect();
        assert_eq!(order, [2, 0, 1, 3]);
        assert_eq!(table[0].rank, 1);
        assert_eq!(table[3].estimate.elo, None);
    }

    #[test]
    fn ties_keep_the_order_of_registration() {
        let games = [
            game(1, 0, 1, GameResult::Draw),
            game(2, 2, 3, GameResult::Draw),
        ];
        let table = rating_table(&[None; 4], &[false; 4], &games);
        let order: Vec<usize> = table.iter().map(|r| r.participant).collect();
        assert_eq!(order, [0, 1, 2, 3]);
    }

    #[test]
    fn performance_needs_every_opponent_rated() {
        let games = [
            game(1, 0, 1, GameResult::White),
            game(2, 1, 0, GameResult::Draw),
            game(3, 0, 2, GameResult::Draw),
            game(4, 2, 0, GameResult::Draw),
        ];
        let rated = [Some(2000), Some(2200), Some(2400)];
        let table = rating_table(&rated, &[false; 3], &games);
        let row = table.iter().find(|r| r.participant == 0).expect("row");
        // Score 2.5/4 against a mean of (2200 + 2200 + 2400 + 2400) / 4.
        let expected = 2300.0 + elo_from_score(0.625).expect("elo");
        assert!((row.performance.expect("perf") - expected).abs() < 1e-9);
        // One opponent without Elo: no performance.
        let partial = [Some(2000), Some(2200), None];
        let table = rating_table(&partial, &[false; 3], &games);
        let row = table.iter().find(|r| r.participant == 0).expect("row");
        assert_eq!(row.performance, None);
        // An infinite relative Elo has no performance either.
        let sweep = [game(1, 0, 1, GameResult::White)];
        let table = rating_table(&[Some(2000), Some(2000)], &[false; 2], &sweep);
        assert_eq!(table[0].performance, None);
    }

    #[test]
    fn the_matrix_is_complementary_and_blank_on_the_diagonal() {
        let games = [
            game(1, 0, 1, GameResult::White),
            game(2, 1, 0, GameResult::Black),
            game(3, 0, 1, GameResult::Draw),
        ];
        let m = los_matrix(2, &games);
        assert_eq!(m[0][0], None);
        let (a, b) = (m[0][1].expect("a"), m[1][0].expect("b"));
        assert!((a + b - 1.0).abs() < 1e-6);
        assert!(a > 0.5);
    }

    #[test]
    fn texts_are_plain_and_total() {
        assert_eq!(elo_text(Some(168.4)), "+168");
        assert_eq!(elo_text(Some(-12.2)), "-12");
        assert_eq!(elo_text(Some(0.2)), "0");
        assert_eq!(elo_text(Some(f64::INFINITY)), "+inf");
        assert_eq!(elo_text(Some(f64::NEG_INFINITY)), "-inf");
        assert_eq!(elo_text(None), "-");
        assert_eq!(margin_text(Some(58.3)), "58");
        assert_eq!(margin_text(Some(f64::INFINITY)), "inf");
        assert_eq!(los_text(Some(0.9321)), "93.2");
        assert_eq!(los_text(None), "-");
    }

    #[test]
    fn nothing_is_ever_nan() {
        for w in 0..6 {
            for d in 0..6 {
                for l in 0..6 {
                    let e = estimate(rec(w, d, l));
                    for value in [e.elo, e.los, e.margin()].into_iter().flatten() {
                        assert!(!value.is_nan(), "{w}-{d}-{l}");
                    }
                }
            }
        }
    }
}
