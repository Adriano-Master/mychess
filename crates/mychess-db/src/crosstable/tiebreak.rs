// Derived from Scid vs PC 4.27, src/crosstab.cpp (`Crosstable::Tiebreaks`,
// `comparePlayerData`). Copyright (c) 2001 Shane Hudson; Scid vs PC
// contributors. Scid is distributed under the GNU General Public License
// (see its COPYING); this file is part of MyChess, GPL-3.0-or-later
// (docs/adr/0004-licenca.md).

//! Tie-break scores and the order of the rows (rules T11 to T15, T20, T21).

use std::cmp::Ordering;

use super::table::Player;
use super::{CrosstableOptions, CrosstableSort, Outcome, TableMode};

/// Fills `tiebreak`, `won`, `lost`, `drawn` and `head_to_head` of every
/// player from the final scores. Knockout tables have none of them.
pub(super) fn compute(players: &mut [Player], mode: TableMode) {
    let scores: Vec<u64> = players.iter().map(|p| p.score).collect();
    for player in players.iter_mut() {
        player.tiebreak = 0;
        player.won = 0;
        player.lost = 0;
        player.drawn = 0;
        player.head_to_head = 0;
        if mode == TableMode::Knockout {
            continue;
        }
        let mut tiebreak = 0u64;
        for clash in &player.clashes {
            let opponent_score = scores.get(clash.opponent).copied().unwrap_or(0);
            match mode {
                // Buchholz: the score of the opponent, whatever the result.
                TableMode::Swiss => tiebreak = tiebreak.saturating_add(opponent_score),
                // Sonneborn-Berger, in quarter points.
                _ => match clash.outcome {
                    Outcome::Win => {
                        tiebreak = tiebreak.saturating_add(opponent_score.saturating_mul(2));
                    }
                    Outcome::Draw => tiebreak = tiebreak.saturating_add(opponent_score),
                    Outcome::Loss | Outcome::Unknown => {}
                },
            }
            match clash.outcome {
                Outcome::Win => player.won = player.won.saturating_add(1),
                Outcome::Loss => player.lost = player.lost.saturating_add(1),
                Outcome::Draw => player.drawn = player.drawn.saturating_add(1),
                Outcome::Unknown => {}
            }
            if player.score == opponent_score {
                match clash.outcome {
                    Outcome::Win => player.head_to_head = player.head_to_head.saturating_add(2),
                    Outcome::Draw => player.head_to_head = player.head_to_head.saturating_add(1),
                    Outcome::Loss | Outcome::Unknown => {}
                }
            }
        }
        player.tiebreak = tiebreak;
    }
}

/// `comparePlayerData`: `Less` puts `a` first. Equal means the order of first
/// appearance decides (the caller sorts stably, D3360).
pub(super) fn compare(options: &CrosstableOptions, a: &Player, b: &Player) -> Ordering {
    match options.sort {
        CrosstableSort::Score => b
            .score
            .cmp(&a.score)
            .then_with(|| a.game_count.cmp(&b.game_count))
            .then_with(|| {
                if options.tiebreak_head_to_head {
                    b.head_to_head.cmp(&a.head_to_head)
                } else {
                    Ordering::Equal
                }
            })
            .then_with(|| {
                if options.tiebreak_wins {
                    b.won.cmp(&a.won)
                } else {
                    Ordering::Equal
                }
            })
            .then_with(|| b.tiebreak.cmp(&a.tiebreak)),
        CrosstableSort::Name => a.name.as_bytes().cmp(b.name.as_bytes()),
        CrosstableSort::Country => a.country.as_bytes().cmp(b.country.as_bytes()),
        CrosstableSort::Rating => b.elo.cmp(&a.elo),
    }
}
