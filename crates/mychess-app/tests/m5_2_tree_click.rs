//! M5-2a contract tests of the click on a row (D965) and of the highlight of
//! the next move (D966).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::assert_is_empty)]

mod m5_2_common;

use m5_2_common::*;
use mychess_app::Prompt;
use tempfile::tempdir;

fn index_of(rig: &Rig, san: &str) -> usize {
    rig.row(san).index
}

fn flagged(rig: &Rig) -> Vec<String> {
    rig.view()
        .rows
        .iter()
        .filter(|r| r.is_next_move)
        .map(|r| r.san.clone())
        .collect()
}

#[test]
fn clicking_adds_the_move_at_the_end_of_the_game() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::std_with_tree(dir.path());
    let nodes = rig.nodes();
    rig.send(TreeMessage::RowClicked(index_of(&rig, "e4")));
    assert_eq!(rig.ply(), 1);
    assert_eq!(rig.nodes(), nodes + 1, "a new node");
    assert_eq!(rig.sorted_sans(), ["[end]", "c5", "e5"], "the tree follows");
}

#[test]
fn clicking_the_next_move_of_the_line_only_advances() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::std_with_tree(dir.path());
    rig.play("e2e4");
    rig.play("e7e5");
    rig.nav(Nav::Start);
    let nodes = rig.nodes();
    rig.send(TreeMessage::RowClicked(index_of(&rig, "e4")));
    assert_eq!(rig.ply(), 1);
    assert_eq!(rig.nodes(), nodes, "no node was added");
    assert!(rig.state.session.prompt().is_none());
}

#[test]
fn clicking_a_move_that_is_already_a_variation_enters_it() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::std_with_tree(dir.path());
    rig.play("e2e4");
    rig.nav(Nav::Start);
    // 1.d4 as a variation of 1.e4.
    let mv = rig.state.session.position().parse_uci("d2d4").unwrap();
    rig.send(GameMessage::PlayMove {
        mv,
        options: PlayOptions {
            force_variation: true,
        },
    });
    rig.nav(Nav::Start);
    let nodes = rig.nodes();
    rig.send(TreeMessage::RowClicked(index_of(&rig, "d4")));
    assert_eq!(rig.nodes(), nodes, "the existing node is reused");
    assert_eq!(rig.ply(), 1);
    let game = rig.state.session.game();
    let kids = game.children(game.root()).unwrap();
    assert_eq!(kids.len(), 2);
    assert_eq!(rig.state.session.cursor(), kids[1], "inside the variation");
    assert!(rig.state.session.prompt().is_none());
}

#[test]
fn clicking_another_move_in_the_middle_of_a_line_asks_what_to_do() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::std_with_tree(dir.path());
    rig.play("e2e4");
    rig.play("e7e5");
    rig.nav(Nav::Start);
    let nodes = rig.nodes();
    rig.send(TreeMessage::RowClicked(index_of(&rig, "d4")));
    assert!(
        matches!(rig.state.session.prompt(), Some(Prompt::ReplaceMove(_))),
        "{:?}",
        rig.state.session.prompt()
    );
    assert_eq!(rig.nodes(), nodes, "nothing changes before the answer");
    assert_eq!(rig.ply(), 0);
}

#[test]
fn the_answer_to_the_question_follows_the_normal_flow_and_the_tree_follows_the_game() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::std_with_tree(dir.path());
    rig.play("e2e4");
    rig.nav(Nav::Start);
    rig.send(TreeMessage::RowClicked(index_of(&rig, "d4")));
    rig.send(GameMessage::AnswerReplace(
        mychess_app::ReplaceChoice::AddVariation,
    ));
    assert_eq!(rig.ply(), 1);
    // Position after 1.d4: one game, replied d5.
    assert_eq!(rig.sans(), ["d5"]);
}

#[test]
fn clicking_the_end_row_does_nothing() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::std_with_tree(dir.path());
    rig.play("e2e4");
    let nodes = rig.nodes();
    let started = rig.started();
    let effects = rig.raw(TreeMessage::RowClicked(index_of(&rig, "[end]")));
    assert!(effects.is_empty());
    assert_eq!(rig.nodes(), nodes);
    assert_eq!(rig.ply(), 1);
    assert_eq!(rig.started(), started);
}

#[test]
fn clicking_the_total_line_or_past_the_table_does_nothing() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::std_with_tree(dir.path());
    let rows = rig.view().rows.len();
    let nodes = rig.nodes();
    for index in [rows, rows + 1, 60, usize::MAX] {
        let effects = rig.raw(TreeMessage::RowClicked(index));
        assert!(effects.is_empty(), "{index}");
    }
    assert_eq!(rig.nodes(), nodes);
    assert_eq!(rig.ply(), 0);
}

#[test]
fn clicks_on_a_closed_panel_do_nothing() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_pgn(dir.path(), "std.pgn", STD);
    let effects = rig.raw(TreeMessage::RowClicked(0));
    assert!(effects.is_empty());
    assert_eq!(rig.ply(), 0);
}

#[test]
fn clicks_while_a_search_runs_are_ignored() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::std_with_tree(dir.path());
    // A change starts a search that nobody answers yet.
    let mv = rig.state.session.position().parse_uci("e2e4").unwrap();
    let flight = rig.raw(GameMessage::PlayMove {
        mv,
        options: PlayOptions::default(),
    });
    assert_eq!(tree_searches(&flight), 1);
    let nodes = rig.nodes();
    let effects = rig.raw(TreeMessage::RowClicked(0));
    assert!(effects.is_empty());
    assert_eq!(rig.nodes(), nodes);
    assert!(rig.view().rows.iter().all(|r| !r.clickable));
    // The panel is usable again when the answer arrives.
    let answer = rig.run_job(jobs(flight).remove(0));
    rig.raw(answer);
    assert!(rig.row("e5").clickable);
}

#[test]
fn clicks_with_the_refresh_off_are_ignored() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::std_with_tree(dir.path());
    rig.send(TreeMessage::ToggleAutoRefresh);
    assert!(rig.view().rows.iter().all(|r| !r.clickable));
    let nodes = rig.nodes();
    rig.send(TreeMessage::RowClicked(0));
    assert_eq!(rig.nodes(), nodes);
    assert_eq!(rig.ply(), 0);
}

#[test]
fn a_table_that_is_not_for_the_cursor_position_ignores_clicks() {
    // Refresh off and the cursor moved: the rows are those of another
    // position and must not be played.
    let dir = tempdir().unwrap();
    let mut rig = Rig::std_with_tree(dir.path());
    rig.send(TreeMessage::ToggleAutoRefresh);
    rig.play("d2d4");
    let nodes = rig.nodes();
    rig.send(TreeMessage::RowClicked(index_of(&rig, "e4")));
    assert_eq!(rig.nodes(), nodes, "e4 must not be played after 1.d4");
}

#[test]
fn clicking_twice_in_a_row_plays_two_moves() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::std_with_tree(dir.path());
    rig.send(TreeMessage::RowClicked(index_of(&rig, "e4")));
    rig.send(TreeMessage::RowClicked(index_of(&rig, "e5")));
    assert_eq!(rig.ply(), 2);
    assert_eq!(rig.sans(), ["Nf3"]);
}

// ---- the next move (D966) ----

#[test]
fn no_row_is_the_next_move_in_an_empty_game() {
    let dir = tempdir().unwrap();
    let rig = Rig::std_with_tree(dir.path());
    assert!(flagged(&rig).is_empty());
}

#[test]
fn the_row_of_the_next_move_of_the_main_line_is_flagged() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::std_with_tree(dir.path());
    rig.play("e2e4");
    rig.play("e7e5");
    rig.nav(Nav::Start);
    assert_eq!(flagged(&rig), ["e4"]);
    rig.nav(Nav::Forward(1));
    assert_eq!(flagged(&rig), ["e5"]);
}

#[test]
fn nothing_is_flagged_at_the_end_of_the_line() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::std_with_tree(dir.path());
    rig.play("e2e4");
    rig.play("e7e5");
    assert!(flagged(&rig).is_empty());
}

#[test]
fn the_first_continuation_is_the_one_flagged_not_a_variation() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::std_with_tree(dir.path());
    rig.play("e2e4");
    rig.nav(Nav::Start);
    let mv = rig.state.session.position().parse_uci("d2d4").unwrap();
    rig.send(GameMessage::PlayMove {
        mv,
        options: PlayOptions {
            force_variation: true,
        },
    });
    rig.nav(Nav::Start);
    assert_eq!(flagged(&rig), ["e4"], "main line, not the variation d4");
}

#[test]
fn a_next_move_the_base_never_played_flags_nothing() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::std_with_tree(dir.path());
    rig.play("a2a3");
    rig.nav(Nav::Start);
    assert!(flagged(&rig).is_empty());
}

#[test]
fn only_one_row_is_ever_flagged() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::std_with_tree(dir.path());
    rig.play("e2e4");
    rig.nav(Nav::Start);
    assert!(flagged(&rig).len() <= 1);
}

#[test]
fn the_flag_is_dropped_for_a_table_of_another_position() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::std_with_tree(dir.path());
    rig.play("e2e4");
    rig.nav(Nav::Start);
    assert_eq!(flagged(&rig), ["e4"]);
    rig.send(TreeMessage::ToggleAutoRefresh);
    rig.nav(Nav::End);
    assert!(
        flagged(&rig).is_empty(),
        "the rows are of the start position, the cursor is not"
    );
}
