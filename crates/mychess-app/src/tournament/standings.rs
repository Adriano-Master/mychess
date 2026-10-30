//! The table of standings and the crosstable (spec M8-2, D1282, D1283).
//!
//! A win is 1 point, a draw 1/2, a loss 0 (the Scid counts 2/1/0 and shows
//! half, `comp.tcl` 460). Points are kept in half points (`u32`) so nothing
//! needs floating point until it is shown. Tie-breaks, in order: direct
//! encounter (only among the tied), Sonneborn-Berger, wins, wins with Black,
//! order of registration (stable, no draw).

use mychess_core::GameResult;

/// A finished game that counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Finished {
    /// The number of the game in the schedule.
    pub number: u32,
    /// The participant with White.
    pub white: usize,
    /// The participant with Black.
    pub black: usize,
    /// The result; `GameResult::Unknown` (`*`) does not count.
    pub result: GameResult,
}

/// What one participant scored in one game.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameScore {
    /// A win.
    Win,
    /// A draw.
    Draw,
    /// A loss.
    Loss,
}

impl GameScore {
    /// The value in half points.
    pub fn half_points(self) -> u32 {
        match self {
            GameScore::Win => 2,
            GameScore::Draw => 1,
            GameScore::Loss => 0,
        }
    }

    /// `1`, `½` or `0`.
    pub fn symbol(self) -> &'static str {
        match self {
            GameScore::Win => "1",
            GameScore::Draw => "\u{bd}",
            GameScore::Loss => "0",
        }
    }
}

/// The score of `participant` in `game`, if the game counts and he played.
pub(crate) fn score_of(game: &Finished, participant: usize) -> Option<GameScore> {
    let white = game.white == participant;
    if !white && game.black != participant {
        return None;
    }
    match game.result {
        GameResult::Draw => Some(GameScore::Draw),
        GameResult::White => Some(if white {
            GameScore::Win
        } else {
            GameScore::Loss
        }),
        GameResult::Black => Some(if white {
            GameScore::Loss
        } else {
            GameScore::Win
        }),
        _ => None,
    }
}

/// A line of the table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StandingRow {
    /// Position in the table, from 1.
    pub rank: usize,
    /// The participant (index in the configuration).
    pub participant: usize,
    /// Games played.
    pub played: u32,
    /// Wins.
    pub wins: u32,
    /// Draws.
    pub draws: u32,
    /// Losses.
    pub losses: u32,
    /// Wins with the black pieces.
    pub black_wins: u32,
    /// Points, in half points.
    pub half_points: u32,
    /// Sonneborn-Berger, in quarter points.
    pub sonneborn_berger: u64,
    /// Points obtained against the participants tied with this one, in half
    /// points.
    pub head_to_head: u32,
    /// The participant left the tournament.
    pub withdrawn: bool,
}

impl StandingRow {
    /// The points as text: `3.5`.
    pub fn points_text(&self) -> String {
        points_text(self.half_points)
    }

    /// The percentage of the points over the games played.
    #[allow(clippy::cast_precision_loss)] // small counts
    pub fn percent(&self) -> f32 {
        if self.played == 0 {
            0.0
        } else {
            self.half_points as f32 * 50.0 / self.played as f32
        }
    }
}

/// Half points as `3.5` (`3` when whole).
pub fn points_text(half_points: u32) -> String {
    if half_points.is_multiple_of(2) {
        (half_points / 2).to_string()
    } else {
        format!("{}.5", half_points / 2)
    }
}

/// The table, sorted by position (D1282). `withdrawn[i]` marks a participant
/// that left; it keeps the points it earned.
pub fn standings(n: usize, withdrawn: &[bool], games: &[Finished]) -> Vec<StandingRow> {
    let mut rows: Vec<StandingRow> = (0..n)
        .map(|participant| StandingRow {
            rank: 0,
            participant,
            played: 0,
            wins: 0,
            draws: 0,
            losses: 0,
            black_wins: 0,
            half_points: 0,
            sonneborn_berger: 0,
            head_to_head: 0,
            withdrawn: withdrawn.get(participant).copied().unwrap_or(false),
        })
        .collect();
    for game in games {
        for (participant, black) in [(game.white, false), (game.black, true)] {
            let Some(score) = score_of(game, participant) else {
                continue;
            };
            let Some(row) = rows.get_mut(participant) else {
                continue;
            };
            row.played += 1;
            row.half_points += score.half_points();
            match score {
                GameScore::Win => {
                    row.wins += 1;
                    row.black_wins += u32::from(black);
                }
                GameScore::Draw => row.draws += 1,
                GameScore::Loss => row.losses += 1,
            }
        }
    }
    // Sonneborn-Berger: the points of the beaten opponents plus half the
    // points of the drawn ones. In quarter points: win = 2 * half points of
    // the opponent, draw = 1 * half points of the opponent.
    let totals: Vec<u32> = rows.iter().map(|r| r.half_points).collect();
    for game in games {
        for (participant, other) in [(game.white, game.black), (game.black, game.white)] {
            let (Some(score), Some(&opp)) = (score_of(game, participant), totals.get(other)) else {
                continue;
            };
            let gain = u64::from(opp) * u64::from(score.half_points());
            if let Some(row) = rows.get_mut(participant) {
                row.sonneborn_berger += gain;
            }
        }
    }
    // The direct encounter only counts among those with the same points.
    let tied: Vec<(usize, u32)> = rows
        .iter()
        .map(|r| (r.participant, r.half_points))
        .collect();
    for row in &mut rows {
        let peers: Vec<usize> = tied
            .iter()
            .filter(|(p, pts)| *p != row.participant && *pts == row.half_points)
            .map(|(p, _)| *p)
            .collect();
        row.head_to_head = games
            .iter()
            .filter(|g| {
                let other = if g.white == row.participant {
                    g.black
                } else {
                    g.white
                };
                peers.contains(&other)
            })
            .filter_map(|g| score_of(g, row.participant))
            .map(GameScore::half_points)
            .sum();
    }
    rows.sort_by(|a, b| {
        b.half_points
            .cmp(&a.half_points)
            .then(b.head_to_head.cmp(&a.head_to_head))
            .then(b.sonneborn_berger.cmp(&a.sonneborn_berger))
            .then(b.wins.cmp(&a.wins))
            .then(b.black_wins.cmp(&a.black_wins))
            .then(a.participant.cmp(&b.participant))
    });
    for (i, row) in rows.iter_mut().enumerate() {
        row.rank = i + 1;
    }
    rows
}

/// The winners: everybody tied on the most **points**, in table order (the
/// Scid lists them separated by commas, `comp.tcl` 447-457). The tie-breaks
/// (direct encounter, wins, wins with Black) only order the list, they do not
/// remove anybody from it: a participant that leads the table by a tie-break
/// is the first of the list, not the only winner (D1316). Empty while nothing
/// was played.
pub fn winners(rows: &[StandingRow]) -> Vec<usize> {
    let best = rows.iter().map(|r| r.half_points).max().unwrap_or(0);
    if rows.iter().all(|r| r.played == 0) {
        return Vec::new();
    }
    rows.iter()
        .filter(|r| r.half_points == best)
        .map(|r| r.participant)
        .collect()
}

/// The crosstable: `cells[i][j]` are the scores of `i` against `j`, in the
/// order of the games.
pub fn cross_table(n: usize, games: &[Finished]) -> Vec<Vec<Vec<(u32, GameScore)>>> {
    let mut cells = vec![vec![Vec::new(); n]; n];
    for game in games {
        for (participant, other) in [(game.white, game.black), (game.black, game.white)] {
            let Some(score) = score_of(game, participant) else {
                continue;
            };
            if let Some(list) = cells.get_mut(participant).and_then(|r| r.get_mut(other)) {
                list.push((game.number, score));
            }
        }
    }
    cells
}

#[cfg(test)]
mod tests {
    use super::*;

    fn g(number: u32, white: usize, black: usize, result: GameResult) -> Finished {
        Finished {
            number,
            white,
            black,
            result,
        }
    }

    fn order(rows: &[StandingRow]) -> Vec<usize> {
        rows.iter().map(|r| r.participant).collect()
    }

    #[test]
    fn counts_one_half_and_zero() {
        let games = [
            g(1, 0, 1, GameResult::White),
            g(2, 1, 0, GameResult::Draw),
            g(3, 0, 2, GameResult::Black),
        ];
        let rows = standings(3, &[], &games);
        let by = |p: usize| rows.iter().find(|r| r.participant == p).expect("row");
        assert_eq!(
            (by(0).wins, by(0).draws, by(0).losses, by(0).half_points),
            (1, 1, 1, 3)
        );
        assert_eq!(by(0).points_text(), "1.5");
        assert_eq!((by(1).played, by(1).half_points), (2, 1));
        assert_eq!((by(2).wins, by(2).black_wins, by(2).half_points), (1, 1, 2));
        assert_eq!(by(2).percent(), 100.0);
    }

    #[test]
    fn an_unfinished_game_does_not_count() {
        let rows = standings(2, &[], &[g(1, 0, 1, GameResult::Unknown)]);
        assert!(rows.iter().all(|r| r.played == 0 && r.half_points == 0));
        assert!(winners(&rows).is_empty());
    }

    #[test]
    fn the_direct_encounter_breaks_a_tie_of_two() {
        // 0 and 1 have 2 points each; 0 beat 1.
        let games = [
            g(1, 0, 1, GameResult::White),
            g(2, 1, 2, GameResult::White),
            g(3, 2, 0, GameResult::White),
            g(4, 2, 1, GameResult::Draw),
            g(5, 0, 2, GameResult::Draw),
        ];
        let rows = standings(3, &[], &games);
        // points: 0 = 1 + .5 + 0 = 1.5; 1 = 0 + 1 + .5 = 1.5; 2 = 1 + .5 + .5 = 2
        assert_eq!(order(&rows), [2, 0, 1]);
        assert_eq!(rows[1].head_to_head, 2);
        assert_eq!(rows[2].head_to_head, 0);
    }

    #[test]
    fn a_cyclic_direct_encounter_falls_back_to_sonneborn_berger() {
        // 0 beats 1, 1 beats 2, 2 beats 0: all tied and the head to head is
        // equal. Player 3 loses to everybody except 0, who he draws with...
        let games = [
            g(1, 0, 1, GameResult::White),
            g(2, 1, 2, GameResult::White),
            g(3, 2, 0, GameResult::White),
            g(4, 0, 3, GameResult::White),
            g(5, 1, 3, GameResult::White),
            g(6, 3, 2, GameResult::White),
        ];
        let rows = standings(4, &[], &games);
        // 0: beats 1 and 3, loses to 2 => 2.0; 1: beats 2 and 3, loses to 0 => 2.0;
        // 2: beats 0, loses to 1 and 3 => 1.0; 3: beats 2 => 1.0.
        let by = |p: usize| rows.iter().find(|r| r.participant == p).expect("row");
        assert_eq!(by(0).half_points, 4);
        assert_eq!(by(1).half_points, 4);
        // Equal head to head, SB decides: 0 beat 1 (4 pts) and 3 (2 pts);
        // 1 beat 2 (2 pts) and 3 (2 pts).
        assert!(by(0).sonneborn_berger > by(1).sonneborn_berger);
        assert_eq!(rows[0].participant, 0);
        assert_eq!(rows[1].participant, 1);
        assert_eq!(
            rows.iter().map(|r| r.rank).collect::<Vec<_>>(),
            [1, 2, 3, 4]
        );
    }

    #[test]
    fn remaining_ties_follow_wins_black_wins_and_registration_order() {
        // 0 and 1 draw with everybody: same points, SB, wins; the order of
        // registration decides.
        let games = [g(1, 0, 1, GameResult::Draw)];
        let rows = standings(2, &[], &games);
        assert_eq!(order(&rows), [0, 1]);
        // More wins beat fewer with equal points and SB: 0 has a win and a
        // loss against 2, 1 has two draws against 2.
        let games = [
            g(1, 0, 2, GameResult::White),
            g(2, 2, 0, GameResult::White),
            g(3, 1, 2, GameResult::Draw),
            g(4, 2, 1, GameResult::Draw),
        ];
        let rows = standings(3, &[], &games);
        assert_eq!(order(&rows), [2, 0, 1], "0 and 1 tie on points and SB");
        assert_eq!(rows[1].half_points, rows[2].half_points);
        assert_eq!(rows[1].sonneborn_berger, rows[2].sonneborn_berger);
    }

    #[test]
    fn several_winners_are_listed_in_table_order() {
        let games = [g(1, 0, 1, GameResult::Draw), g(2, 2, 3, GameResult::Draw)];
        let rows = standings(4, &[], &games);
        assert_eq!(winners(&rows), [0, 1, 2, 3]);
        let games = [g(1, 0, 1, GameResult::White)];
        assert_eq!(winners(&standings(2, &[], &games)), [0]);
    }

    #[test]
    fn marks_withdrawn_participants_and_keeps_their_points() {
        let rows = standings(2, &[false, true], &[g(1, 0, 1, GameResult::Black)]);
        let one = rows.iter().find(|r| r.participant == 1).expect("row");
        assert!(one.withdrawn);
        assert_eq!(one.half_points, 2);
    }

    #[test]
    fn the_crosstable_lists_the_games_of_each_pair() {
        let games = [
            g(1, 0, 1, GameResult::White),
            g(2, 1, 0, GameResult::Draw),
            g(3, 0, 2, GameResult::Unknown),
        ];
        let cells = cross_table(3, &games);
        assert_eq!(cells[0][1], [(1, GameScore::Win), (2, GameScore::Draw)]);
        assert_eq!(cells[1][0], [(1, GameScore::Loss), (2, GameScore::Draw)]);
        assert!(cells[0][2].is_empty(), "unfinished games do not count");
        assert!(cells[0][0].is_empty());
        assert_eq!(GameScore::Draw.symbol(), "\u{bd}");
    }

    #[test]
    fn ignores_games_with_unknown_participants() {
        let rows = standings(2, &[], &[g(1, 0, 9, GameResult::White)]);
        assert_eq!(rows.iter().map(|r| r.played).sum::<u32>(), 1);
    }
}
