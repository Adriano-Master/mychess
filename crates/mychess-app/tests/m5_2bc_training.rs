//! M5-2c contract tests, app side: the training of the opening tree
//! (spec `m5-2-janela-da-arvore.md`, D1620-D1626, D974; Scid `tree.tcl`
//! `toggleTraining`, `refreshTraining`, `doTraining`).
//!
//! The generator of the training is seeded from the clock and the seed cannot
//! be fixed from outside the crate, so the tests use positions where the
//! answer is forced or check the set of possible answers.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::assert_is_empty)]

mod m5_2_common;

use m5_2_common::*;
use mychess_app::{BrowseMessage, TreeFastMode};
use rstest::rstest;
use tempfile::tempdir;

fn training(rig: &mut Rig, on: bool) {
    rig.send(TreeMessage::SetTraining(on));
}

fn on(dir: &std::path::Path, pgn: &str) -> Rig {
    let mut rig = Rig::new();
    rig.open_pgn(dir, "t.pgn", pgn);
    rig.open_tree();
    training(&mut rig, true);
    rig
}

fn std_training(dir: &std::path::Path) -> Rig {
    on(dir, STD)
}

fn fen(rig: &Rig) -> String {
    rig.state.session.position().fen()
}

/// Many games that all play the same line, so the answer is forced.
fn forced() -> String {
    (0..3)
        .map(|_| game_pgn("1. d4 d5 2. c4 e6", "1-0", 0, 0, "2001.01.01", None))
        .collect()
}

// ---- turning it on (D1620, D1621) ----

#[test]
fn turning_it_on_enters_the_trial_mode_and_hides_the_moves() {
    let dir = tempdir().unwrap();
    let rig = std_training(dir.path());
    assert!(rig.state.tree().training());
    assert!(rig.state.session.in_trial());
    let view = rig.view();
    assert!(view.training);
    assert!(!view.rows.is_empty());
    for row in &view.rows {
        assert!(row.hidden, "{} is hidden", row.san);
        assert!(!row.clickable, "{} is not clickable", row.san);
    }
    // The numbers stay: only the text of the move and the ECO are hidden.
    assert_eq!(view.total.map(|t| t.games), Some(5));
}

#[test]
fn rows_are_not_hidden_without_training() {
    let dir = tempdir().unwrap();
    let rig = Rig::std_with_tree(dir.path());
    assert!(rig.view().rows.iter().all(|r| !r.hidden));
    assert!(!rig.view().training);
}

#[test]
fn turning_it_on_does_not_play_for_the_tree() {
    let dir = tempdir().unwrap();
    let rig = std_training(dir.path());
    assert_eq!(rig.ply(), 0, "the tree answers the first move of the user");
}

#[test]
fn turning_it_on_again_changes_nothing() {
    let dir = tempdir().unwrap();
    let mut rig = std_training(dir.path());
    let started = rig.started();
    training(&mut rig, true);
    assert!(rig.state.tree().training());
    assert_eq!(rig.started(), started);
}

#[test]
fn clicking_a_row_in_training_does_nothing() {
    let dir = tempdir().unwrap();
    let mut rig = std_training(dir.path());
    let nodes = rig.nodes();
    rig.send(TreeMessage::RowClicked(0));
    assert_eq!(rig.nodes(), nodes);
    assert_eq!(rig.ply(), 0);
}

#[test]
fn the_view_says_whether_training_can_be_turned_on() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::std_with_tree(dir.path());
    assert!(rig.view().training_available);
    rig.send(TreeMessage::ToggleAutoRefresh);
    assert!(!rig.view().training_available);
}

#[test]
fn it_stays_available_while_on_so_it_can_be_turned_off() {
    let dir = tempdir().unwrap();
    let rig = std_training(dir.path());
    assert!(rig.view().training_available);
}

#[test]
fn it_needs_the_panel_open() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_pgn(dir.path(), "t.pgn", STD);
    training(&mut rig, true);
    assert!(!rig.state.tree().training());
    assert!(!rig.state.session.in_trial());
}

#[test]
fn it_needs_refresh_on() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::std_with_tree(dir.path());
    rig.send(TreeMessage::ToggleAutoRefresh);
    training(&mut rig, true);
    assert!(!rig.state.tree().training());
    assert!(!rig.state.session.in_trial());
}

#[test]
fn it_needs_a_usable_base_with_games() {
    let mut rig = Rig::new();
    rig.open_tree();
    rig.send(BasesMessage::Select(SlotId::CLIPBASE));
    assert!(!rig.view().training_available);
    training(&mut rig, true);
    assert!(!rig.state.tree().training());
    assert!(!rig.state.session.in_trial());
}

#[test]
fn it_is_refused_while_a_modal_window_is_open() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::std_with_tree(dir.path());
    let slot = rig.state.bases.current();
    rig.send(BasesMessage::Browse(BrowseMessage::Open {
        slot,
        numbers: vec![1, 2],
    }));
    assert!(rig.state.browser.is_some(), "precondition");
    training(&mut rig, true);
    assert!(!rig.state.tree().training());
    assert!(!rig.state.session.in_trial());
}

#[test]
fn it_does_not_enter_the_trial_again_if_the_user_already_is_in_it() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::std_with_tree(dir.path());
    rig.send(GameMessage::EnterTrial { null_move: false });
    assert!(rig.state.session.in_trial());
    training(&mut rig, true);
    assert!(rig.state.tree().training());
    // The trial is the user's: turning the training off leaves it on.
    training(&mut rig, false);
    assert!(!rig.state.tree().training());
    assert!(rig.state.session.in_trial());
}

// ---- the answer (D1622, D1623, D1626) ----

#[test]
fn the_tree_answers_the_move_of_the_user_with_a_move_of_the_tree() {
    let dir = tempdir().unwrap();
    let mut rig = std_training(dir.path());
    rig.play("e2e4");
    assert_eq!(rig.ply(), 2, "the move of the user and the answer");
    let f = fen(&rig);
    assert!(
        f.contains("/4p3/4P3/") || f.contains("/2p5/4P3/"),
        "e5 or c5, the moves of the games: {f}"
    );
}

#[test]
fn the_answer_is_the_only_move_when_the_tree_has_just_one() {
    let dir = tempdir().unwrap();
    let mut rig = on(dir.path(), &forced());
    rig.play("d2d4");
    assert_eq!(rig.ply(), 2);
    assert!(fen(&rig).contains("/3p4/3P4/"), "{}", fen(&rig));
    // The next move of the user gets the next move of the games.
    rig.play("c2c4");
    assert_eq!(rig.ply(), 4);
    assert!(
        fen(&rig).contains("4p3/2PP4/") || fen(&rig).contains("/4p3/"),
        "{}",
        fen(&rig)
    );
}

#[test]
fn the_tree_never_answers_its_own_answer() {
    let dir = tempdir().unwrap();
    let mut rig = on(dir.path(), &forced());
    rig.play("d2d4");
    // One user move, one answer: exactly two plies, whatever follows.
    assert_eq!(rig.ply(), 2);
    assert_eq!(rig.nodes(), 3, "root, d4 and d5");
}

#[test]
fn the_answer_does_not_depend_on_the_fast_mode_and_comes_once() {
    for mode in TreeFastMode::ALL {
        let dir = tempdir().unwrap();
        let mut rig = Rig::new();
        rig.open_pgn(dir.path(), "t.pgn", &forced());
        rig.open_tree();
        rig.send(TreeMessage::SetFastMode(mode));
        training(&mut rig, true);
        rig.play("d2d4");
        assert_eq!(rig.ply(), 2, "{mode:?}");
        assert_eq!(rig.nodes(), 3, "{mode:?}");
    }
}

#[test]
fn both_moves_of_the_tree_are_eventually_drawn() {
    let dir = tempdir().unwrap();
    let mut rig = std_training(dir.path());
    let (mut e5, mut c5) = (0_u32, 0_u32);
    for _ in 0..60 {
        rig.nav(Nav::Start);
        rig.play("e2e4");
        let f = fen(&rig);
        assert_eq!(rig.ply(), 2);
        if f.contains("/4p3/4P3/") {
            e5 += 1;
        } else if f.contains("/2p5/4P3/") {
            c5 += 1;
        } else {
            panic!("neither e5 nor c5: {f}");
        }
        assert!(rig.state.tree().training());
    }
    // Each has a share of one half; missing one in 60 draws is 2^-59.
    assert!(e5 > 0 && c5 > 0, "e5 {e5}, c5 {c5}");
}

#[test]
fn the_answer_is_never_the_end_row() {
    // After 1.e4 the only game that continues is game 3 of this base; the
    // other ends there. The `[end]` row has weight but is not a move.
    let dir = tempdir().unwrap();
    let pgn = format!(
        "{}{}",
        game_pgn("1. e4", "*", 0, 0, "2001.01.01", None),
        game_pgn("1. e4 e5", "1-0", 0, 0, "2001.01.01", None)
    );
    let mut rig = on(dir.path(), &pgn);
    for _ in 0..20 {
        rig.nav(Nav::Start);
        rig.play("e2e4");
        let ply = rig.ply();
        assert!(ply == 1 || ply == 2, "ply {ply}");
        if ply == 2 {
            assert!(fen(&rig).contains("/4p3/4P3/"));
        }
    }
}

#[test]
fn no_answer_when_the_position_has_no_moves_in_the_tree() {
    let dir = tempdir().unwrap();
    let mut rig = std_training(dir.path());
    rig.play("a2a3");
    assert_eq!(rig.ply(), 1);
    assert_eq!(rig.view().body, TreeBody::Empty);
    assert!(rig.state.tree().training(), "and the training goes on");
}

#[test]
fn no_answer_when_the_tree_has_only_the_end_row() {
    let dir = tempdir().unwrap();
    let mut rig = on(
        dir.path(),
        &game_pgn("1. e4", "*", 0, 0, "2001.01.01", None),
    );
    rig.play("e2e4");
    assert_eq!(rig.ply(), 1);
    // Going elsewhere and coming back does not call the answer late.
    rig.nav(Nav::Back(1));
    rig.nav(Nav::Forward(1));
    assert_eq!(rig.ply(), 1);
    assert_eq!(rig.nodes(), 2);
}

#[test]
fn navigation_is_not_a_move_of_the_user() {
    let dir = tempdir().unwrap();
    let mut rig = on(dir.path(), &forced());
    rig.play("d2d4");
    let nodes = rig.nodes();
    rig.nav(Nav::Start);
    rig.nav(Nav::Forward(1));
    rig.nav(Nav::GameEnd);
    rig.nav(Nav::Back(2));
    assert_eq!(rig.nodes(), nodes);
}

#[test]
fn an_answer_that_arrives_after_the_cursor_left_is_discarded() {
    let dir = tempdir().unwrap();
    let mut rig = on(dir.path(), &forced());
    let mv = rig.state.session.position().parse_uci("d2d4").unwrap();
    // The search of the new position flies; the user goes back meanwhile.
    let mut sent = jobs(rig.raw(GameMessage::PlayMove {
        mv,
        options: PlayOptions::default(),
    }));
    assert_eq!(sent.len(), 1);
    let flight = sent.remove(0);
    rig.raw(GameMessage::Navigate(Nav::Back(1)));
    let answer = rig.run_job(flight);
    let mut queue = rig.raw(answer);
    while let Some(effect) = queue.pop() {
        if effect.runs_on_host() {
            continue;
        }
        let message = rig.perform(effect);
        queue.extend(rig.raw(message));
    }
    assert_eq!(rig.ply(), 0);
    assert_eq!(rig.nodes(), 2, "root and d4 only: the tree did not play");
}

#[test]
fn the_user_may_play_a_move_that_the_games_never_played() {
    let dir = tempdir().unwrap();
    let mut rig = on(dir.path(), &forced());
    rig.play("e2e4");
    assert_eq!(rig.ply(), 1);
    // The user takes it back and plays a move of the tree: the training works.
    rig.nav(Nav::Back(1));
    rig.play("d2d4");
    assert_eq!(rig.ply(), 2);
}

// ---- turning it off (D1625) ----

#[test]
fn turning_it_off_leaves_the_trial_it_turned_on() {
    let dir = tempdir().unwrap();
    let mut rig = on(dir.path(), &forced());
    rig.play("d2d4");
    training(&mut rig, false);
    assert!(!rig.state.tree().training());
    assert!(!rig.state.session.in_trial());
    assert_eq!(rig.ply(), 0, "the trial gives the game back");
    assert!(rig.view().rows.iter().all(|r| !r.hidden));
}

#[test]
fn closing_the_panel_turns_it_off_and_leaves_the_trial() {
    let dir = tempdir().unwrap();
    let mut rig = std_training(dir.path());
    rig.send(TreeMessage::Close);
    assert!(!rig.state.tree().training());
    assert!(!rig.state.session.in_trial());
}

#[test]
fn leaving_the_trial_turns_the_training_off_and_nothing_else() {
    let dir = tempdir().unwrap();
    let mut rig = std_training(dir.path());
    rig.send(GameMessage::ExitTrial);
    assert!(!rig.state.tree().training());
    assert!(!rig.state.session.in_trial());
    assert!(rig.state.tree().is_open(), "the panel stays");
    assert!(rig.view().rows.iter().all(|r| !r.hidden));
    // And the tree does not answer any more.
    rig.play("d2d4");
    assert_eq!(rig.ply(), 1);
}

#[test]
fn toggling_the_trial_off_by_the_toggle_message_turns_it_off_too() {
    let dir = tempdir().unwrap();
    let mut rig = std_training(dir.path());
    rig.send(GameMessage::ToggleTrial { null_move: false });
    assert!(!rig.state.session.in_trial());
    assert!(!rig.state.tree().training());
}

#[test]
fn a_game_that_replaces_the_current_one_turns_it_off() {
    let dir = tempdir().unwrap();
    let mut rig = std_training(dir.path());
    let slot = rig.state.bases.current();
    rig.send(BasesMessage::LoadGame { slot, number: 2 });
    assert!(!rig.state.tree().training());
}

#[test]
fn closing_the_base_turns_it_off() {
    let dir = tempdir().unwrap();
    let mut rig = std_training(dir.path());
    let slot = rig.state.bases.current();
    rig.send(BasesMessage::Close(slot));
    assert!(!rig.state.tree().training());
}

#[test]
fn switching_to_a_base_without_games_turns_it_off() {
    let dir = tempdir().unwrap();
    let mut rig = std_training(dir.path());
    // The Clipbase is empty.
    rig.send(BasesMessage::Select(SlotId::CLIPBASE));
    assert_eq!(rig.state.bases.slot(SlotId::CLIPBASE).unwrap().total(), 0);
    assert!(!rig.state.tree().training());
    assert!(!rig.state.session.in_trial());
}

#[rstest]
#[case(true)]
#[case(false)]
fn the_training_is_not_saved_in_the_preferences(#[case] on_now: bool) {
    let dir = tempdir().unwrap();
    let mut rig = Rig::std_with_tree(dir.path());
    if on_now {
        training(&mut rig, true);
    }
    let text = format!("{:?}", rig.state.prefs.tree);
    assert!(!text.to_lowercase().contains("training"), "{text}");
}
