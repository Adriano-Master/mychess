//! M5-2a property test: whatever sequence of changes and answers happens,
//! at most one search is in flight (D976) and, once everything is answered,
//! the table is the tree of the position of the cursor (checked against an
//! oracle that replays the games).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::format_push_string)]

mod m5_2_common;

use std::collections::{BTreeMap, VecDeque};

use m5_2_common::*;
use mychess_core::Position;
use proptest::prelude::*;
use tempfile::tempdir;

const GAMES: &[&[&str]] = &[
    &["e2e4", "e7e5", "g1f3", "b8c6"],
    &["d2d4", "d7d5", "c2c4"],
    &["e2e4", "c7c5"],
    &["g1f3", "d7d5", "d2d4"],
    &["d2d4", "d7d5", "g1f3"],
    &["e2e4", "e7e5", "g1f3"],
    &["g1f3", "g8f6", "g2g3"],
    &["d2d4", "g8f6", "g1f3", "d7d5"],
];

fn pgn() -> String {
    let mut text = String::new();
    for game in GAMES {
        let mut pos = Position::new();
        let mut moves = String::new();
        for (i, uci) in game.iter().enumerate() {
            let mv = pos.parse_uci(uci).unwrap();
            let san = pos.san(mv).unwrap();
            if i % 2 == 0 {
                moves.push_str(&format!("{}. ", i / 2 + 1));
            }
            moves.push_str(&san);
            moves.push(' ');
            pos.play(mv).unwrap();
        }
        text.push_str(&game_pgn(moves.trim_end(), "*", 0, 0, "2000.01.01", None));
    }
    text
}

fn board_key(pos: &Position) -> String {
    pos.fen()
        .split_whitespace()
        .take(2)
        .collect::<Vec<_>>()
        .join(" ")
}

/// The expected tree: games reaching the position and, per game, the move
/// played from its first occurrence (`[end]` when it ends there).
fn oracle(target: &Position) -> (u32, BTreeMap<String, u32>) {
    let want = board_key(target);
    let mut total = 0;
    let mut rows: BTreeMap<String, u32> = BTreeMap::new();
    for game in GAMES {
        let mut pos = Position::new();
        for ply in 0..=game.len() {
            if board_key(&pos) == want {
                total += 1;
                let san = match game.get(ply) {
                    Some(uci) => {
                        let mv = target.parse_uci(uci).unwrap();
                        target.san(mv).unwrap()
                    }
                    None => "[end]".to_owned(),
                };
                *rows.entry(san).or_default() += 1;
                break;
            }
            if let Some(uci) = game.get(ply) {
                let mv = pos.parse_uci(uci).unwrap();
                pos.play(mv).unwrap();
            }
        }
    }
    (total, rows)
}

fn check_table(rig: &Rig) -> Result<(), TestCaseError> {
    let (total, rows) = oracle(rig.state.session.position());
    let view = rig.view();
    prop_assert!(!view.searching);
    if total == 0 {
        prop_assert_eq!(view.body, TreeBody::Empty);
        prop_assert!(view.rows.is_empty() && view.total.is_none());
        return Ok(());
    }
    prop_assert_eq!(view.body.clone(), TreeBody::Rows);
    prop_assert_eq!(view.total.as_ref().map(|t| t.games), Some(total));
    let got: BTreeMap<String, u32> = view.rows.iter().map(|r| (r.san.clone(), r.games)).collect();
    prop_assert_eq!(got, rows);
    prop_assert_eq!(view.rows.iter().map(|r| r.games).sum::<u32>(), total);
    Ok(())
}

fn pump(pending: &mut VecDeque<Box<TreeJob>>, effects: Vec<Effect>) {
    pending.extend(jobs(effects));
}

fn drain(rig: &mut Rig, pending: &mut VecDeque<Box<TreeJob>>) {
    let mut guard = 0;
    while let Some(job) = pending.pop_front() {
        let answer = rig.run_job(job);
        let effects = rig.raw(answer);
        pump(pending, effects);
        guard += 1;
        assert!(guard < 50, "the searches never settle");
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(40))]

    #[test]
    fn at_most_one_search_in_flight_and_the_table_ends_up_right(
        ops in prop::collection::vec((0u8..9, 0usize..40), 1..25),
        fast_then_slow in any::<bool>(),
    ) {
        let dir = tempdir().unwrap();
        let mut rig = Rig::new();
        rig.open_pgn(dir.path(), "g.pgn", &pgn());
        if fast_then_slow {
            rig.send(TreeMessage::SetFastMode(TreeFastMode::FastThenSlow));
        }
        let mut pending: VecDeque<Box<TreeJob>> = VecDeque::new();
        let effects = rig.raw(TreeMessage::Open);
        pump(&mut pending, effects);
        for (kind, arg) in ops {
            let effects = match kind {
                0..=2 => {
                    let moves = rig.state.session.position().legal_moves();
                    if moves.is_empty() {
                        continue;
                    }
                    let mv = moves[arg % moves.len()];
                    let mut effects = rig.raw(GameMessage::PlayMove { mv, options: PlayOptions::default() });
                    if rig.state.session.prompt().is_some() {
                        effects.extend(rig.raw(GameMessage::AnswerReplace(mychess_app::ReplaceChoice::AddVariation)));
                    }
                    effects
                }
                3 => rig.raw(GameMessage::Navigate(Nav::Back(1))),
                4 => rig.raw(GameMessage::Navigate(Nav::Start)),
                5 => rig.raw(GameMessage::Navigate(Nav::GameEnd)),
                6 => rig.raw(TreeMessage::SetSort(TreeSort::ALL[arg % 4])),
                7 => {
                    // Deliver the oldest answer.
                    match pending.pop_front() {
                        Some(job) => {
                            let answer = rig.run_job(job);
                            rig.raw(answer)
                        }
                        None => Vec::new(),
                    }
                }
                _ => rig.raw(TreeMessage::SetShort(arg % 2 == 0)),
            };
            pump(&mut pending, effects);
            prop_assert!(pending.len() <= 1, "{} searches in flight", pending.len());
        }
        drain(&mut rig, &mut pending);
        check_table(&rig)?;
    }
}
