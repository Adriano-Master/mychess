//! M5-2a contract tests of the engine errors (D972) and of unreadable games
//! in the panel.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::assert_is_empty)]

mod m5_2_common;

use m5_2_common::*;
use mychess_core::Position;
use tempfile::tempdir;

/// 218 legal moves for White.
const MANY_MOVES_FEN: &str = "R6R/3Q4/1Q4Q1/4Q3/2Q4Q/Q4Q2/pp1Q4/kBNN1KB1 w - - 0 1";

/// `n` games from `MANY_MOVES_FEN`, each with a different first move (one
/// ply), as a PGN text.
fn many_first_moves(n: usize) -> String {
    let pos = Position::from_fen(MANY_MOVES_FEN).unwrap();
    let mut text = String::new();
    for mv in pos.legal_moves().into_iter().take(n) {
        let san = pos.san(mv).unwrap();
        text.push_str(&game_pgn(
            &format!("1. {san}"),
            "1-0",
            0,
            0,
            "2000.01.01",
            Some(MANY_MOVES_FEN),
        ));
    }
    text
}

fn open_many(dir: &std::path::Path, n: usize) -> Rig {
    let mut rig = Rig::new();
    let slot = rig.open_si4(dir, "many", &many_first_moves(n));
    assert_eq!(
        rig.state.bases.slot(slot).unwrap().total(),
        u32::try_from(n).unwrap()
    );
    rig.send(BasesMessage::LoadGame { slot, number: 1 });
    assert_eq!(
        rig.state.session.position().fen(),
        MANY_MOVES_FEN,
        "precondition: the game starts from the FEN"
    );
    rig
}

#[test]
fn more_than_sixty_distinct_moves_is_a_message_in_the_panel() {
    let dir = tempdir().unwrap();
    let mut rig = open_many(dir.path(), 61);
    rig.open_tree();
    let view = rig.view();
    assert_eq!(view.body, TreeBody::Issue(TreeIssue::TooManyMoves));
    assert!(view.rows.is_empty() && view.total.is_none());
    assert!(!view.searching);
}

#[test]
fn sixty_distinct_moves_are_still_a_table() {
    let dir = tempdir().unwrap();
    let mut rig = open_many(dir.path(), 60);
    rig.open_tree();
    let view = rig.view();
    assert_eq!(view.body, TreeBody::Rows);
    assert_eq!(view.rows.len(), 60);
    assert_eq!(view.total.expect("total").games, 60);
}

#[test]
fn the_issue_goes_away_with_the_next_good_search() {
    let dir = tempdir().unwrap();
    let mut rig = open_many(dir.path(), 61);
    rig.open_tree();
    assert!(matches!(rig.view().body, TreeBody::Issue(_)));
    // Play the first move of game 1: the position after it has an end row.
    let first = Position::from_fen(MANY_MOVES_FEN)
        .unwrap()
        .legal_moves()
        .into_iter()
        .next()
        .unwrap();
    rig.send(GameMessage::PlayMove {
        mv: first,
        options: PlayOptions::default(),
    });
    let view = rig.view();
    assert_eq!(view.body, TreeBody::Rows);
    assert_eq!(rig.sans(), ["[end]"]);
}

#[test]
fn an_issue_is_not_searched_again_for_the_same_position() {
    let dir = tempdir().unwrap();
    let mut rig = open_many(dir.path(), 61);
    rig.open_tree();
    let started = rig.started();
    let effects = rig.raw(TreeMessage::SetShort(false));
    assert!(effects.is_empty());
    assert_eq!(rig.started(), started);
    assert!(matches!(rig.view().body, TreeBody::Issue(_)));
}

#[test]
fn closing_and_reopening_retries_after_an_issue() {
    let dir = tempdir().unwrap();
    let mut rig = open_many(dir.path(), 61);
    rig.open_tree();
    rig.send(TreeMessage::Close);
    assert_eq!(rig.view().body, TreeBody::Closed);
    let started = rig.started();
    rig.open_tree();
    assert_eq!(rig.started(), started + 1);
    assert_eq!(rig.view().body, TreeBody::Issue(TreeIssue::TooManyMoves));
}

#[test]
fn clicks_on_an_issue_do_nothing() {
    let dir = tempdir().unwrap();
    let mut rig = open_many(dir.path(), 61);
    rig.open_tree();
    let nodes = rig.nodes();
    let effects = rig.raw(TreeMessage::RowClicked(0));
    assert!(effects.is_empty());
    assert_eq!(rig.nodes(), nodes);
}

// ---- unreadable games ----

const HEADER_LEN: usize = 182;
const ENTRY_LEN: usize = 47;

/// Points the record of game `n` far beyond the end of the `.sg4`.
fn break_offset(si4: &std::path::Path, n: usize) {
    let mut bytes = std::fs::read(si4).unwrap();
    let at = HEADER_LEN + ENTRY_LEN * (n - 1);
    bytes[at..at + 4].copy_from_slice(&0x7FFF_FF00_u32.to_be_bytes());
    std::fs::write(si4, bytes).unwrap();
}

fn three_e4() -> String {
    let mut text = String::new();
    for _ in 0..3 {
        text.push_str(&game_pgn("1. e4 e5", "1-0", 0, 0, "2000.01.01", None));
    }
    text
}

#[test]
fn an_unreadable_game_is_counted_and_the_others_still_make_the_table() {
    let dir = tempdir().unwrap();
    let si4 = make_si4(dir.path(), "bad", &three_e4());
    break_offset(&si4, 2);
    let mut rig = Rig::new();
    rig.send(BasesMessage::Open(si4));
    assert_eq!(rig.state.bases.current_slot().unwrap().total(), 3);
    rig.open_tree();
    let view = rig.view();
    assert_eq!(view.body, TreeBody::Rows, "{view:?}");
    assert_eq!(view.unreadable, 1);
    assert_eq!(view.rows.len(), 1);
    assert_eq!(view.rows[0].games, 2, "game 2 is not in the tree");
}

#[test]
fn a_healthy_base_reports_no_unreadable_games() {
    let dir = tempdir().unwrap();
    let rig = Rig::std_with_tree(dir.path());
    assert_eq!(rig.view().unreadable, 0);
}

#[test]
fn unreadable_games_do_not_enter_the_adjusted_filter() {
    let dir = tempdir().unwrap();
    let si4 = make_si4(dir.path(), "bad", &three_e4());
    break_offset(&si4, 2);
    let mut rig = Rig::new();
    rig.send(BasesMessage::Open(si4));
    let slot = rig.state.bases.current();
    rig.open_tree();
    rig.send(TreeMessage::SetAdjust(true));
    assert_eq!(rig.filter_numbers(slot), [1, 3]);
}

#[test]
fn a_base_whose_games_are_all_unreadable_is_an_empty_tree_not_an_issue() {
    let dir = tempdir().unwrap();
    let si4 = make_si4(dir.path(), "bad", &three_e4());
    for n in 1..=3 {
        break_offset(&si4, n);
    }
    let mut rig = Rig::new();
    rig.send(BasesMessage::Open(si4));
    rig.open_tree();
    let view = rig.view();
    assert_eq!(view.body, TreeBody::Empty, "{view:?}");
    assert_eq!(view.unreadable, 3);
}
