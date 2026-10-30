//! M4-4 contract tests of editing the game with an engine line (D440-D443,
//! D791-D793; rules 5-8, 14): Add move, Add score, Add line, Add all lines,
//! one undo point each, the cursor, the end-of-line rules and the notices.
#![allow(
    clippy::assert_is_empty,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::format_push_string
)]

mod m4_4_common;

use m4_4_common::*;
use mychess_app::engines::RunId;
use mychess_app::{
    AllLinesMode, AnalysisMessage, AnalysisNotice, AppState, EngineLineToAdd, GameMessage, Nav,
    Notice,
};
use mychess_core::{Game, NodeId, Position};

/// A game: `1.e4 e5 2.Nf3 Nc6` with the variation `1...c5` (a sibling of e5).
struct Fixture {
    game: Game,
    e4: NodeId,
    e5: NodeId,
    nf3: NodeId,
    nc6: NodeId,
    c5: NodeId,
}

fn extend(game: &mut Game, from: NodeId, ucis: &[&str]) -> Vec<NodeId> {
    let mut node = from;
    let mut out = Vec::new();
    for u in ucis {
        let mv = game.position_at(node).unwrap().parse_uci(u).unwrap();
        node = game.add_move(node, mv).unwrap();
        out.push(node);
    }
    out
}

fn fixture() -> Fixture {
    let mut game = Game::new();
    let root = game.root();
    let ids = extend(&mut game, root, &["e2e4", "e7e5", "g1f3", "b8c6"]);
    let c5 = extend(&mut game, ids[0], &["c7c5"])[0];
    Fixture {
        game,
        e4: ids[0],
        e5: ids[1],
        nf3: ids[2],
        nc6: ids[3],
        c5,
    }
}

type Line<'a> = (u8, u32, i32, &'a [&'a str]);

/// An analysing run with `count` lines, over `game`, with the cursor at
/// `node` and `lines` reported for it.
fn setup(game: Game, node: NodeId, count: u8, lines: &[Line<'_>]) -> (AppState, RunId) {
    let (mut state, run, _) = analysing(true, true);
    msg(&mut state, AnalysisMessage::SetMultiPv(run, count));
    state.session.replace_game(game);
    state.prefs.analysis.name_in_comments = false;
    goto(&mut state, run, node, 1, lines);
    (state, run)
}

fn goto(state: &mut AppState, run: RunId, node: NodeId, search: u64, lines: &[Line<'_>]) {
    nav(state, Nav::GoToNode(node));
    started(state, run, search);
    let pos: Position = state.session.position().clone();
    for (n, depth, cp, pv) in lines {
        info(state, run, report(&pos, search, *n, *depth, *cp, pv));
    }
}

fn children(state: &AppState, node: NodeId) -> Vec<NodeId> {
    state.session.game().children(node).unwrap().to_vec()
}

fn nodes(state: &AppState) -> usize {
    state.session.game().node_count()
}

fn before(state: &AppState, node: NodeId) -> Option<String> {
    state
        .session
        .game()
        .comment_before(node)
        .unwrap()
        .map(str::to_owned)
}

/// Asserts that exactly one undo restores `original` and nothing is left to
/// undo (each action is one undo point; the session was fresh before it).
fn assert_single_undo_point(state: &mut AppState, original: &Game) {
    assert_eq!(state.session.undo_depth(), 1, "one undo point per action");
    game(state, GameMessage::Undo);
    assert!(
        state.session.game().same_content(original),
        "undo must restore the game"
    );
    assert!(
        state.session.game().contains(state.session.cursor()),
        "stale cursor after undo"
    );
    assert_eq!(state.session.undo_depth(), 0);
    game(state, GameMessage::Redo);
    assert!(state.session.game().contains(state.session.cursor()));
    assert!(
        !state.session.game().same_content(original),
        "redo must reapply"
    );
}

// ---- Add move (rule 5, D791) ----

#[test]
fn add_move_at_the_end_of_the_line_appends_the_first_move_of_line_one() {
    let f = fixture();
    let original = f.game.clone();
    let (mut state, run) = setup(f.game, f.nc6, 1, &[(1, 12, 30, &["f1b5", "a7a6"])]);
    msg(&mut state, AnalysisMessage::AddMove(run));
    let tail = mainline_ucis(&state);
    assert_eq!(tail.last().map(String::as_str), Some("f1b5"));
    assert_eq!(tail.len(), 5, "only the first move is added");
    assert_single_undo_point(&mut state, &original);
}

#[test]
fn add_move_equal_to_the_next_move_only_advances() {
    let f = fixture();
    let (mut state, run) = setup(f.game, f.e4, 1, &[(1, 12, 30, &["e7e5", "g1f3"])]);
    let count = nodes(&state);
    msg(&mut state, AnalysisMessage::AddMove(run));
    assert_eq!(state.session.cursor(), f.e5);
    assert_eq!(nodes(&state), count, "nothing is edited");
    assert!(state.session.prompt().is_none());
}

#[test]
fn add_move_different_from_the_next_in_the_middle_asks_what_to_do() {
    let f = fixture();
    let (mut state, run) = setup(f.game, f.e4, 1, &[(1, 12, 30, &["e7e6"])]);
    let count = nodes(&state);
    msg(&mut state, AnalysisMessage::AddMove(run));
    assert!(
        state.session.prompt().is_some(),
        "M2-1 replace dialog expected"
    );
    assert_eq!(nodes(&state), count, "the game waits for the answer");
}

#[test]
fn add_move_does_nothing_when_the_lines_do_not_belong_to_the_cursor_position() {
    let f = fixture();
    let (mut state, run) = setup(f.game, f.nc6, 1, &[(1, 12, 30, &["f1b5"])]);
    // The user moves the cursor and the engine has not answered yet.
    nav(&mut state, Nav::Back(1));
    let count = nodes(&state);
    msg(&mut state, AnalysisMessage::AddMove(run));
    msg(&mut state, AnalysisMessage::AddScore(run));
    msg(&mut state, AnalysisMessage::AddLine(run));
    msg(
        &mut state,
        AnalysisMessage::AddAllLines(run, AllLinesMode::All),
    );
    assert_eq!(nodes(&state), count);
    assert_eq!(state.session.undo_depth(), 0);
}

#[test]
fn add_move_needs_the_analysis_on() {
    let f = fixture();
    let (mut state, run) = setup(f.game, f.nc6, 1, &[(1, 12, 30, &["f1b5"])]);
    msg(&mut state, AnalysisMessage::Toggle(run));
    let count = nodes(&state);
    msg(&mut state, AnalysisMessage::AddMove(run));
    assert_eq!(nodes(&state), count);
}

#[test]
fn locked_analysis_refuses_every_edit_with_a_notice() {
    let f = fixture();
    let (mut state, run) = setup(f.game, f.nc6, 1, &[(1, 12, 30, &["f1b5"])]);
    msg(&mut state, AnalysisMessage::ToggleLock(run));
    let count = nodes(&state);
    for m in [
        AnalysisMessage::AddMove(run),
        AnalysisMessage::AddScore(run),
        AnalysisMessage::AddLine(run),
        AnalysisMessage::AddAllLines(run, AllLinesMode::All),
    ] {
        state.notice = None;
        msg(&mut state, m.clone());
        assert_eq!(
            analysis_notice(&state),
            Some(&AnalysisNotice::Locked),
            "{m:?}"
        );
    }
    assert_eq!(nodes(&state), count);
}

// ---- Add score (rule 6, D441) ----

#[test]
fn add_score_puts_depth_and_score_in_front_of_the_current_comment() {
    let mut f = fixture();
    f.game.set_comment(f.e5, "solid").unwrap();
    let original = f.game.clone();
    let (mut state, run) = setup(f.game, f.e5, 1, &[(1, 18, -25, &["g1f3"])]);
    msg(&mut state, AnalysisMessage::AddScore(run));
    assert_eq!(
        state.session.game().comment(f.e5).unwrap(),
        Some("18:-0.25 solid")
    );
    assert_eq!(state.session.cursor(), f.e5, "the cursor does not move");
    assert_single_undo_point(&mut state, &original);
}

#[test]
fn add_score_includes_the_engine_name_by_default() {
    let f = fixture();
    let (mut state, run) = setup(f.game, f.e5, 1, &[(1, 18, 35, &["g1f3"])]);
    state.prefs.analysis.name_in_comments = true;
    msg(&mut state, AnalysisMessage::AddScore(run));
    let comment = state
        .session
        .game()
        .comment(f.e5)
        .unwrap()
        .unwrap()
        .to_owned();
    assert!(comment.starts_with("[Fake] 18:+0.35"), "got {comment:?}");
}

#[test]
fn add_score_at_the_root_comments_the_initial_position() {
    let (mut state, run) = setup(Game::new(), Game::new().root(), 1, &[(1, 9, 20, &["e2e4"])]);
    let root = state.session.game().root();
    msg(&mut state, AnalysisMessage::AddScore(run));
    let text = state.session.game().comment(root).unwrap();
    assert!(
        text.is_some_and(|t| t.starts_with("9:+0.20")),
        "got {text:?}"
    );
}

#[test]
fn add_score_of_a_mate_line_writes_m3_without_plus() {
    let f = fixture();
    let (mut state, run) = setup(f.game, f.e5, 1, &[]);
    let pos = state.session.position().clone();
    let mut r = report(&pos, 1, 1, 22, 0, &["g1f3"]);
    r.score = Some((mychess_engine::Score::Mate(3), mychess_engine::Bound::Exact));
    info(&mut state, run, r);
    msg(&mut state, AnalysisMessage::AddScore(run));
    assert_eq!(state.session.game().comment(f.e5).unwrap(), Some("22:M3"));
}

// ---- Add line (rule 7, D442, D443) ----

#[test]
fn add_line_in_the_middle_of_the_main_line_creates_a_variation_with_the_comment_before_it() {
    let f = fixture();
    let original = f.game.clone();
    let (mut state, run) = setup(f.game, f.e5, 1, &[(1, 15, 12, &["f1c4", "g8f6", "d2d3"])]);
    msg(&mut state, AnalysisMessage::AddLine(run));
    let kids = children(&state, f.e5);
    assert_eq!(kids.len(), 2, "Nf3 and the new Bc4");
    assert_eq!(kids[0], f.nf3, "the main move stays first");
    let head = kids[1];
    assert_eq!(before(&state, head).as_deref(), Some("15:+0.12"));
    let depth = children(&state, head);
    assert_eq!(depth.len(), 1);
    assert_eq!(
        children(&state, depth[0]).len(),
        1,
        "three moves in the variation"
    );
    assert_eq!(
        state.session.cursor(),
        f.e5,
        "D443: the cursor does not move"
    );
    assert_single_undo_point(&mut state, &original);
}

#[test]
fn add_line_whose_first_move_exists_follows_it_and_adds_only_what_is_missing() {
    let f = fixture();
    let (mut state, run) = setup(f.game, f.e5, 1, &[(1, 15, 12, &["g1f3", "b8c6", "f1b5"])]);
    let count = nodes(&state);
    msg(&mut state, AnalysisMessage::AddLine(run));
    assert_eq!(nodes(&state), count + 1, "only Bb5 is new");
    assert_eq!(children(&state, f.e5), [f.nf3], "no duplicated sibling");
    let bb5 = children(&state, f.nc6);
    assert_eq!(bb5.len(), 1);
    assert_eq!(before(&state, bb5[0]).as_deref(), Some("15:+0.12"));
    assert_eq!(
        before(&state, f.nf3),
        None,
        "the comment goes to the first NEW node"
    );
}

#[test]
fn add_line_that_is_already_in_the_game_changes_nothing() {
    let f = fixture();
    let original = f.game.clone();
    let (mut state, run) = setup(f.game, f.e5, 1, &[(1, 15, 12, &["g1f3", "b8c6"])]);
    msg(&mut state, AnalysisMessage::AddLine(run));
    assert!(state.session.game().same_content(&original));
}

#[test]
fn add_line_at_the_end_of_a_variation_extends_it() {
    let f = fixture();
    let (mut state, run) = setup(f.game, f.c5, 1, &[(1, 14, 5, &["g1f3", "d7d6"])]);
    let mainline = mainline_ucis(&state);
    msg(&mut state, AnalysisMessage::AddLine(run));
    let nf3 = children(&state, f.c5);
    assert_eq!(nf3.len(), 1);
    assert_eq!(children(&state, nf3[0]).len(), 1);
    assert_eq!(
        mainline_ucis(&state),
        mainline,
        "the main line is untouched"
    );
    assert_eq!(before(&state, nf3[0]).as_deref(), Some("14:+0.05"));
    assert_eq!(state.session.cursor(), f.c5);
}

#[test]
fn add_line_at_the_end_of_the_main_line_is_refused_with_a_notice() {
    let f = fixture();
    let original = f.game.clone();
    let (mut state, run) = setup(
        f.game,
        f.nc6,
        3,
        &[(1, 15, 12, &["f1b5"]), (2, 15, 10, &["f1c4"])],
    );
    msg(&mut state, AnalysisMessage::AddLine(run));
    assert_eq!(analysis_notice(&state), Some(&AnalysisNotice::AtGameEnd));
    assert!(state.session.game().same_content(&original));
    assert_eq!(state.session.undo_depth(), 0);
    state.notice = None;
    msg(
        &mut state,
        AnalysisMessage::AddAllLines(run, AllLinesMode::All),
    );
    assert_eq!(analysis_notice(&state), Some(&AnalysisNotice::AtGameEnd));
    assert!(state.session.game().same_content(&original));
}

#[test]
fn add_line_on_an_empty_game_makes_the_main_line() {
    let (mut state, run) = setup(
        Game::new(),
        Game::new().root(),
        1,
        &[(1, 10, 20, &["e2e4", "e7e5"])],
    );
    msg(&mut state, AnalysisMessage::AddLine(run));
    assert_eq!(mainline_ucis(&state), ["e2e4", "e7e5"]);
    let first = children(&state, state.session.game().root())[0];
    assert!(before(&state, first).is_some());
}

#[test]
fn add_line_number_adds_that_line_and_a_missing_number_does_nothing() {
    let f = fixture();
    let (mut state, run) = setup(
        f.game,
        f.e5,
        3,
        &[
            (1, 15, 12, &["g1f3"]),
            (2, 15, 10, &["f1c4"]),
            (3, 15, 8, &["d2d3"]),
        ],
    );
    let count = nodes(&state);
    msg(&mut state, AnalysisMessage::AddLineNumber(run, 2));
    assert_eq!(nodes(&state), count + 1);
    let kids = children(&state, f.e5);
    let mv = state.session.game().mv(kids[1]).unwrap();
    assert_eq!(
        state.session.game().position_at(f.e5).unwrap().uci(mv),
        "f1c4"
    );
    msg(&mut state, AnalysisMessage::AddLineNumber(run, 5));
    msg(&mut state, AnalysisMessage::AddLineNumber(run, 0));
    assert_eq!(nodes(&state), count + 1);
}

#[test]
fn add_line_with_no_lines_yet_does_nothing() {
    let f = fixture();
    let (mut state, run) = setup(f.game, f.e5, 1, &[]);
    let count = nodes(&state);
    msg(&mut state, AnalysisMessage::AddLine(run));
    msg(
        &mut state,
        AnalysisMessage::AddAllLines(run, AllLinesMode::All),
    );
    msg(&mut state, AnalysisMessage::AddScore(run));
    assert_eq!(nodes(&state), count);
    assert_eq!(state.session.undo_depth(), 0);
}

// ---- Add all (rule 8) ----

fn three_alternatives() -> [Line<'static>; 3] {
    [
        (1, 20, 30, &["f1c4", "g8f6"]),
        (2, 19, -10, &["d2d3"]),
        (3, 18, 5, &["b1c3", "g8f6", "f1c4"]),
    ]
}

#[test]
fn add_all_lines_adds_one_variation_per_line_with_its_own_comment_in_one_undo_point() {
    let f = fixture();
    let original = f.game.clone();
    let lines = three_alternatives();
    let (mut state, run) = setup(f.game, f.e5, 3, &lines);
    state.prefs.analysis.name_in_comments = true;
    msg(
        &mut state,
        AnalysisMessage::AddAllLines(run, AllLinesMode::All),
    );
    let kids = children(&state, f.e5);
    assert_eq!(kids.len(), 4, "Nf3 + three lines");
    assert_eq!(before(&state, kids[1]).as_deref(), Some("[Fake] 20:+0.30"));
    assert_eq!(before(&state, kids[2]).as_deref(), Some("[Fake] 19:-0.10"));
    assert_eq!(before(&state, kids[3]).as_deref(), Some("[Fake] 18:+0.05"));
    assert_eq!(state.session.cursor(), f.e5);
    assert_single_undo_point(&mut state, &original);
}

#[test]
fn add_all_lines_second_only_adds_just_line_two() {
    let f = fixture();
    let lines = three_alternatives();
    let (mut state, run) = setup(f.game, f.e5, 3, &lines);
    msg(
        &mut state,
        AnalysisMessage::AddAllLines(run, AllLinesMode::SecondOnly),
    );
    let kids = children(&state, f.e5);
    assert_eq!(kids.len(), 2);
    assert_eq!(before(&state, kids[1]).as_deref(), Some("19:-0.10"));
}

#[test]
fn add_all_lines_second_only_needs_two_lines() {
    let f = fixture();
    let (mut state, run) = setup(f.game, f.e5, 3, &[(1, 20, 30, &["f1c4"])]);
    let count = nodes(&state);
    msg(
        &mut state,
        AnalysisMessage::AddAllLines(run, AllLinesMode::SecondOnly),
    );
    assert_eq!(nodes(&state), count);
}

#[test]
fn add_all_lines_first_moves_only_cuts_each_line_to_one_move() {
    let f = fixture();
    let lines = three_alternatives();
    let (mut state, run) = setup(f.game, f.e5, 3, &lines);
    let count = nodes(&state);
    msg(
        &mut state,
        AnalysisMessage::AddAllLines(run, AllLinesMode::FirstMovesOnly),
    );
    assert_eq!(nodes(&state), count + 3, "three single-move variations");
    for kid in children(&state, f.e5).into_iter().skip(1) {
        assert!(children(&state, kid).is_empty());
    }
}

#[test]
fn add_all_lines_with_a_shared_first_move_does_not_duplicate_siblings() {
    let f = fixture();
    let (mut state, run) = setup(
        f.game,
        f.e5,
        2,
        &[
            (1, 20, 30, &["f1c4", "g8f6"]),
            (2, 19, 20, &["f1c4", "f8c5"]),
        ],
    );
    msg(
        &mut state,
        AnalysisMessage::AddAllLines(run, AllLinesMode::All),
    );
    let kids = children(&state, f.e5);
    assert_eq!(kids.len(), 2, "one Bc4 variation, shared");
    assert_eq!(children(&state, kids[1]).len(), 2, "Nf6 and Bc5 under Bc4");
}

#[test]
fn add_all_at_the_end_of_a_variation_extends_with_only_the_first_line() {
    let f = fixture();
    let (mut state, run) = setup(
        f.game,
        f.c5,
        2,
        &[(1, 14, 5, &["g1f3", "d7d6"]), (2, 14, 3, &["b1c3"])],
    );
    let count = nodes(&state);
    msg(
        &mut state,
        AnalysisMessage::AddAllLines(run, AllLinesMode::All),
    );
    assert_eq!(nodes(&state), count + 2, "only the two moves of line 1");
    assert_eq!(children(&state, f.c5).len(), 1);
}

#[test]
fn add_all_writes_the_wdl_of_each_line() {
    let f = fixture();
    let (mut state, run) = setup(f.game, f.e5, 1, &[]);
    let pos = state.session.position().clone();
    let mut r = report(&pos, 1, 1, 20, 30, &["f1c4"]);
    r.wdl = Some(mychess_engine::Wdl {
        win: 450,
        draw: 300,
        loss: 250,
    });
    info(&mut state, run, r);
    msg(
        &mut state,
        AnalysisMessage::AddAllLines(run, AllLinesMode::All),
    );
    let kid = children(&state, f.e5)[1];
    assert_eq!(
        before(&state, kid).as_deref(),
        Some("20:+0.30 W: 45% D: 30% B: 25%")
    );
}

// ---- GameSession messages directly (D792, rule 14) ----

fn mv_in(state: &AppState, node: NodeId, uci: &str) -> mychess_core::Move {
    state
        .session
        .game()
        .position_at(node)
        .unwrap()
        .parse_uci(uci)
        .unwrap()
}

#[test]
fn a_pv_with_an_illegal_move_keeps_the_legal_prefix_and_reports_how_many() {
    let f = fixture();
    let original = f.game.clone();
    let (mut state, _) = setup(f.game, f.e5, 1, &[]);
    let pv = vec![
        mv_in(&state, f.e5, "f1c4"),
        mv_in(&state, f.e5, "f1c4"), // not playable after Bc4 (the bishop moved)
    ];
    game(
        &mut state,
        GameMessage::AddEngineLine {
            node: f.e5,
            pv,
            comment: None,
        },
    );
    assert_eq!(
        analysis_notice_session(&state),
        Some(AnalysisNotice::LineTruncated(1))
    );
    assert_eq!(children(&state, f.e5).len(), 2, "the valid move stays");
    assert_single_undo_point(&mut state, &original);
}

#[test]
fn a_pv_that_fails_at_its_first_move_changes_nothing_and_leaves_no_undo_point() {
    let f = fixture();
    let original = f.game.clone();
    let (mut state, _) = setup(f.game, f.e5, 1, &[]);
    // A move that is legal for Black after 1.e4, not at e5 (White to move).
    let wrong = mv_in(&state, f.e4, "d7d6");
    game(
        &mut state,
        GameMessage::AddEngineLine {
            node: f.e5,
            pv: vec![wrong],
            comment: Some("x".into()),
        },
    );
    assert_eq!(
        analysis_notice_session(&state),
        Some(AnalysisNotice::LineTruncated(0))
    );
    assert!(state.session.game().same_content(&original));
    assert_eq!(
        state.session.undo_depth(),
        0,
        "no undo point if nothing was added"
    );
}

#[test]
fn engine_line_messages_with_a_vanished_node_say_stale_node() {
    let f = fixture();
    let stale = f.nc6;
    let (mut state, _) = setup(f.game, f.e5, 1, &[]);
    state.session.replace_game(Game::new());
    let mv = Position::new().parse_uci("e2e4").unwrap();
    for m in [
        GameMessage::AddEngineLine {
            node: stale,
            pv: vec![mv],
            comment: None,
        },
        GameMessage::AddEngineLines {
            node: stale,
            lines: vec![EngineLineToAdd {
                pv: vec![mv],
                comment: None,
            }],
        },
        GameMessage::PrependComment {
            node: stale,
            text: "x".into(),
        },
    ] {
        state.session.update(m.clone());
        assert_eq!(state.session.notice(), Some(&Notice::StaleNode), "{m:?}");
        assert_eq!(state.session.game().node_count(), 1);
        assert_eq!(state.session.undo_depth(), 0);
    }
}

#[test]
fn engine_lines_with_empty_pvs_do_nothing() {
    let f = fixture();
    let original = f.game.clone();
    let (mut state, _) = setup(f.game, f.e5, 1, &[]);
    game(
        &mut state,
        GameMessage::AddEngineLine {
            node: f.e5,
            pv: vec![],
            comment: Some("x".into()),
        },
    );
    game(
        &mut state,
        GameMessage::AddEngineLines {
            node: f.e5,
            lines: vec![],
        },
    );
    game(
        &mut state,
        GameMessage::PrependComment {
            node: f.e5,
            text: "  ".into(),
        },
    );
    assert!(state.session.game().same_content(&original));
    assert_eq!(state.session.undo_depth(), 0);
}

fn analysis_notice_session(state: &AppState) -> Option<AnalysisNotice> {
    state.session.analysis_notice().cloned()
}

#[test]
fn the_analysis_notice_reaches_the_application_notice() {
    let f = fixture();
    let (mut state, run) = setup(f.game, f.nc6, 1, &[(1, 12, 30, &["f1b5"])]);
    msg(&mut state, AnalysisMessage::AddLine(run));
    assert_eq!(analysis_notice(&state), Some(&AnalysisNotice::AtGameEnd));
}

#[test]
fn a_new_session_message_clears_the_previous_analysis_notice() {
    let f = fixture();
    let (mut state, run) = setup(f.game, f.nc6, 1, &[(1, 12, 30, &["f1b5"])]);
    msg(&mut state, AnalysisMessage::AddLine(run));
    assert!(state.session.analysis_notice().is_some());
    nav(&mut state, Nav::Start);
    assert!(state.session.analysis_notice().is_none());
}

// ---- random sequences: one undo point per action, no stale ids ----

mod random {
    use super::*;
    use proptest::prelude::*;

    #[derive(Debug, Clone)]
    enum Op {
        AddMove,
        AddScore,
        AddLine,
        AddAll(u8),
        Undo,
        Redo,
        Back,
        Forward,
        Start,
        End,
    }

    fn op() -> impl Strategy<Value = Op> {
        prop_oneof![
            Just(Op::AddMove),
            Just(Op::AddScore),
            Just(Op::AddLine),
            (0u8..3).prop_map(Op::AddAll),
            Just(Op::Undo),
            Just(Op::Redo),
            Just(Op::Back),
            Just(Op::Forward),
            Just(Op::Start),
            Just(Op::End),
        ]
    }

    /// Reports the (up to) three first legal moves of the cursor position as
    /// lines 1-3 of a new search.
    fn refresh_lines(state: &mut AppState, run: RunId, search: u64) {
        started(state, run, search);
        let pos = state.session.position().clone();
        let legal = pos.legal_moves();
        for (i, mv) in legal.iter().take(3).enumerate() {
            let n = u8::try_from(i + 1).unwrap();
            let uci = pos.uci(*mv);
            info(
                state,
                run,
                report(&pos, search, n, 10, 10 * i32::from(n), &[&uci]),
            );
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(48))]
        #[test]
        fn edits_keep_the_cursor_valid_and_cost_at_most_one_undo_point(
            ops in proptest::collection::vec(op(), 1..25)
        ) {
            let f = fixture();
            let (mut state, run) = setup(f.game, f.e5, 3, &[]);
            let mut search = 1;
            refresh_lines(&mut state, run, search);
            for op in ops {
                let depth = state.session.undo_depth();
                let is_edit = matches!(op, Op::AddMove | Op::AddScore | Op::AddLine | Op::AddAll(_));
                match op {
                    Op::AddMove => { msg(&mut state, AnalysisMessage::AddMove(run)); }
                    Op::AddScore => { msg(&mut state, AnalysisMessage::AddScore(run)); }
                    Op::AddLine => { msg(&mut state, AnalysisMessage::AddLine(run)); }
                    Op::AddAll(m) => {
                        let mode = [AllLinesMode::All, AllLinesMode::SecondOnly, AllLinesMode::FirstMovesOnly][usize::from(m)];
                        msg(&mut state, AnalysisMessage::AddAllLines(run, mode));
                    }
                    Op::Undo => { game(&mut state, GameMessage::Undo); }
                    Op::Redo => { game(&mut state, GameMessage::Redo); }
                    Op::Back => { nav(&mut state, Nav::Back(1)); }
                    Op::Forward => { nav(&mut state, Nav::Forward(1)); }
                    Op::Start => { nav(&mut state, Nav::Start); }
                    Op::End => { nav(&mut state, Nav::GameEnd); }
                }
                prop_assert!(state.session.game().contains(state.session.cursor()), "stale cursor after {op:?}");
                if is_edit && state.session.prompt().is_none() {
                    prop_assert!(
                        state.session.undo_depth() <= depth + 1,
                        "{op:?} cost {} undo points", state.session.undo_depth() - depth
                    );
                }
                if state.session.prompt().is_some() {
                    game(&mut state, GameMessage::AnswerReplace(mychess_app::ReplaceChoice::Cancel));
                }
                search += 1;
                refresh_lines(&mut state, run, search);
            }
        }
    }
}
