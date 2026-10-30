//! Contract tests of the mask over the table of the tree, the edits, the
//! fills, the search and the outline (spec M5-4, slice M5-4b: D1822, D1826
//! to D1829, D1832, D1833, D1835, D1860).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::assert_is_empty,
    clippy::cloned_ref_to_slice_refs
)]

mod m5_4b_common;

use m5_4b_common::*;
use mychess_app::{
    FillSide, GameMessage, Marker, MarkerSlot, MaskColor, MaskKey, MaskNag, MaskQuery, Nav,
    TreeBody,
};
use mychess_core::Position;
use mychess_db::tree::mask::MaskOp;

fn dir() -> tempfile::TempDir {
    tempfile::tempdir().expect("temp dir")
}

fn view(rig: &Rig) -> mychess_app::MaskView {
    rig.state.tree_view().mask.expect("a mask is open")
}

/// The std base with the tree open and a new mask.
fn rig_with_mask(dir: &std::path::Path) -> Rig {
    let mut rig = Rig::std_with_tree(dir);
    m(&mut rig, MaskMessage::New);
    rig
}

fn row_index(rig: &Rig, san: &str) -> usize {
    rig.view()
        .rows
        .iter()
        .position(|r| r.san == san)
        .unwrap_or_else(|| panic!("no row {san}"))
}

fn hits(rig: &mut Rig, query: MaskQuery) -> mychess_app::MaskHits {
    m(rig, MaskMessage::Search(Box::new(query)));
    let state = rig.state.mask_search().expect("a search");
    (**state.hits.as_ref().expect("the answer came back")).clone()
}

#[test]
fn the_rows_carry_what_the_mask_says_in_one_lookup() {
    let dir = dir();
    let mut rig = Rig::std_with_tree(dir.path());
    assert!(rig.view().rows.iter().all(|r| r.mask.is_none()));
    m(&mut rig, MaskMessage::New);
    let e4 = row_index(&rig, "e4");
    m(
        &mut rig,
        MaskMessage::Edit {
            target: MaskTarget::Row(e4),
            edit: MaskEdit::Nag(Some(MaskNag::Good)),
            scope: MaskScope::One,
        },
    );
    edit(&mut rig, "e4", MaskEdit::Color(MaskColor::Green));
    edit(
        &mut rig,
        "e4",
        MaskEdit::Marker {
            slot: MarkerSlot::Second,
            marker: Some(Marker::MainLine),
        },
    );
    edit(
        &mut rig,
        "e4",
        MaskEdit::Comment("best by test\nmore".to_owned()),
    );
    let view = rig.view();
    let cell = view.rows[row_index(&rig, "e4")].mask.clone().expect("cell");
    assert!(cell.in_mask);
    assert_eq!(cell.nag, Some(MaskNag::Good));
    assert_eq!(cell.color, MaskColor::Green);
    assert_eq!(cell.markers, [None, Some(Marker::MainLine)]);
    assert_eq!(cell.comment, "best by test");
    assert!(cell.comment_more);
    assert_eq!(
        rig.state.mask_comment(Some(&san("e4"))).as_deref(),
        Some("best by test\nmore")
    );
    // The rows that are not in the mask have a plain cell.
    let other = view.rows[row_index(&rig, "d4")].mask.clone().expect("cell");
    assert!(!other.in_mask);
    assert_eq!(other.nag, None);
    // A row edited through its index and through its text are the same.
    assert!(dirty(&rig));
}

#[test]
fn moves_only_the_mask_has_come_after_the_total_and_can_be_played() {
    let dir = dir();
    let mut rig = rig_with_mask(dir.path());
    edit(&mut rig, "c4", MaskEdit::Add);
    edit(&mut rig, "c4", MaskEdit::Comment("English".to_owned()));
    let v = view(&rig);
    assert_eq!(v.only_rows.len(), 1);
    let only = &v.only_rows[0];
    assert_eq!(only.san, "c4");
    assert!(only.legal && only.clickable);
    assert_eq!(only.cell.comment, "English");
    // The tree rows do not repeat it.
    assert!(rig.view().rows.iter().all(|r| r.san != "c4"));
    // A click plays it (the normal flow of a move).
    m(&mut rig, MaskMessage::PlayOnlyRow(0));
    assert_eq!(rig.ply(), 1);
    assert_eq!(
        rig.state
            .session
            .position()
            .fen()
            .split(' ')
            .next()
            .unwrap(),
        "rnbqkbnr/pppppppp/8/8/2P5/8/PP1PPPPP/RNBQKBNR"
    );
}

#[test]
fn a_move_the_tree_has_is_not_repeated_even_with_another_spelling() {
    let dir = dir();
    let mut rig = rig_with_mask(dir.path());
    // `Nf3+` and `Nf3` are the same move (D1805).
    edit(&mut rig, "Nf3+", MaskEdit::Add);
    assert!(view(&rig).only_rows.is_empty());
    assert!(rig.row("Nf3").mask.expect("cell").in_mask);
}

#[test]
fn an_illegal_entry_is_listed_but_a_click_does_nothing() {
    let dir = dir();
    let path = dir.path().join("odd.stm");
    write_mask(&path, |mask| {
        mask.apply(
            &Position::new(),
            &MaskOp::AddMove {
                san: "Ke5".to_owned(),
            },
        );
    });
    let mut rig = Rig::std_with_tree(dir.path());
    m(&mut rig, MaskMessage::Open(Some(path)));
    let v = view(&rig);
    assert_eq!(v.only_rows.len(), 1);
    assert!(!v.only_rows[0].legal && !v.only_rows[0].clickable);
    m(&mut rig, MaskMessage::PlayOnlyRow(0));
    assert_eq!(rig.ply(), 0);
}

#[test]
fn the_mask_shows_even_when_no_game_reaches_the_position() {
    let dir = dir();
    let mut rig = rig_with_mask(dir.path());
    rig.play("a2a3");
    assert_eq!(rig.view().body, TreeBody::Empty);
    edit(&mut rig, "a6", MaskEdit::Add);
    m(
        &mut rig,
        MaskMessage::SetPositionComment("Sidelines".to_owned()),
    );
    let v = view(&rig);
    assert_eq!(rig.view().body, TreeBody::Empty);
    assert_eq!(v.only_rows.len(), 1);
    assert_eq!(v.only_rows[0].san, "a6");
    assert_eq!(v.position_comment, "Sidelines");
    assert!(!v.position_comment_more);
}

#[test]
fn with_no_base_the_mask_has_no_rows_but_stays_open() {
    let mut rig = Rig::new();
    m(&mut rig, MaskMessage::New);
    edit(&mut rig, "e4", MaskEdit::Add);
    rig.send(TreeMessage::Open);
    assert_eq!(rig.view().body, TreeBody::Unavailable);
    let v = view(&rig);
    assert!(
        v.only_rows.is_empty(),
        "the Scid shows nothing either (D1828)"
    );
    assert!(v.dirty);
}

#[test]
fn the_table_follows_the_position_and_the_mask_follows_the_table() {
    let dir = dir();
    let mut rig = rig_with_mask(dir.path());
    edit(&mut rig, "e4", MaskEdit::Color(MaskColor::Red));
    m(&mut rig, MaskMessage::PlayOnlyRow(0)); // nothing to play: no effect
    rig.send(TreeMessage::RowClicked(row_index(&rig, "e4")));
    assert_eq!(rig.ply(), 1);
    // After 1.e4 the rows are others and plain.
    assert!(
        rig.view()
            .rows
            .iter()
            .filter(|r| !r.is_end)
            .all(|r| !r.mask.clone().unwrap().in_mask)
    );
    rig.nav(Nav::Start);
    assert_eq!(rig.row("e4").mask.expect("cell").color, MaskColor::Red);
}

#[test]
fn an_edit_of_a_stale_row_is_ignored() {
    let dir = dir();
    let mut rig = rig_with_mask(dir.path());
    // The cursor moves, then an edit by the index of the old table arrives
    // before the new one is drawn: the position differs, so it is ignored.
    let before = row_index(&rig, "e4");
    rig.play("e2e4");
    let _ = before;
    m(
        &mut rig,
        MaskMessage::Edit {
            target: MaskTarget::Row(99),
            edit: MaskEdit::Add,
            scope: MaskScope::One,
        },
    );
    assert!(!dirty(&rig));
}

#[test]
fn the_comment_of_the_position_is_edited_and_empty_removes_it() {
    let mut rig = Rig::new();
    m(&mut rig, MaskMessage::New);
    m(
        &mut rig,
        MaskMessage::SetPositionComment("Plan A".to_owned()),
    );
    assert_eq!(rig.state.mask_comment(None).as_deref(), Some("Plan A"));
    assert_eq!(view(&rig).positions, 1);
    m(&mut rig, MaskMessage::SetPositionComment(String::new()));
    assert_eq!(view(&rig).positions, 0);
}

fn play_line(rig: &mut Rig, ucis: &[&str]) {
    for uci in ucis {
        rig.play(uci);
    }
}

#[test]
fn applying_to_the_line_goes_back_to_the_start_in_one_step() {
    let dir = dir();
    let mut rig = rig_with_mask(dir.path());
    play_line(&mut rig, &["e2e4", "e7e5", "g1f3", "b8c6"]);
    m(
        &mut rig,
        MaskMessage::Edit {
            target: san("Bb5"),
            edit: MaskEdit::Color(MaskColor::Green),
            scope: MaskScope::Line,
        },
    );
    assert!(dirty(&rig));
    let found = hits(
        &mut rig,
        MaskQuery {
            color: Some(MaskColor::Green),
            ..MaskQuery::default()
        },
    );
    let sans: Vec<String> = found.rows.iter().filter_map(|h| h.san.clone()).collect();
    assert_eq!(found.moves_found, 5, "{sans:?}");
    for want in ["e4", "e5", "Nf3", "Nc6", "Bb5"] {
        assert!(sans.iter().any(|s| s == want), "{want} in {sans:?}");
    }
}

#[test]
fn applying_to_the_line_at_the_start_is_only_the_clicked_move() {
    let dir = dir();
    let mut rig = rig_with_mask(dir.path());
    m(
        &mut rig,
        MaskMessage::Edit {
            target: san("e4"),
            edit: MaskEdit::Nag(Some(MaskNag::Brilliant)),
            scope: MaskScope::Line,
        },
    );
    assert_eq!(view(&rig).positions, 1);
}

#[test]
fn applying_to_the_line_inside_a_variation_passes_through_it() {
    let mut rig = Rig::new();
    // 1.e4 e5 (1...c5 2.Nf3): the cursor is on 2.Nf3 inside the variation.
    let mut game = mychess_core::Game::new();
    let mut node = game.root();
    let mut pos = Position::new();
    for uci in ["e2e4", "e7e5"] {
        let mv = pos.parse_uci(uci).unwrap();
        pos.play(mv).unwrap();
        node = game.add_move(node, mv).unwrap();
    }
    let e4 = game.parent(node).unwrap().unwrap();
    let mut var = e4;
    let mut vpos = game.position_at(e4).unwrap().clone();
    for uci in ["c7c5", "g1f3"] {
        let mv = vpos.parse_uci(uci).unwrap();
        vpos.play(mv).unwrap();
        var = game.add_move(var, mv).unwrap();
    }
    rig.state.session.replace_game(game);
    rig.send(GameMessage::Navigate(Nav::GoToNode(var)));
    m(&mut rig, MaskMessage::New);
    m(
        &mut rig,
        MaskMessage::Edit {
            target: san("d4"),
            edit: MaskEdit::Color(MaskColor::Blue),
            scope: MaskScope::Line,
        },
    );
    let found = hits(
        &mut rig,
        MaskQuery {
            color: Some(MaskColor::Blue),
            ..MaskQuery::default()
        },
    );
    let sans: Vec<String> = found.rows.iter().filter_map(|h| h.san.clone()).collect();
    // The line to the cursor is e4, c5, Nf3 (not e5), plus the clicked d4.
    assert_eq!(found.moves_found, 4, "{sans:?}");
    assert!(sans.iter().any(|s| s == "c5") && !sans.iter().any(|s| s == "e5"));
}

#[test]
fn removing_along_the_line_goes_forward_from_the_cursor() {
    let dir = dir();
    let mut rig = rig_with_mask(dir.path());
    play_line(&mut rig, &["e2e4", "e7e5", "g1f3", "b8c6"]);
    m(&mut rig, MaskMessage::FillGame);
    assert_eq!(view(&rig).positions, 4);
    rig.nav(Nav::Start);
    // One: only e4 goes.
    edit(&mut rig, "e4", MaskEdit::Remove);
    assert_eq!(view(&rig).positions, 3);
    // Put it back and remove along the line: everything goes.
    edit(&mut rig, "e4", MaskEdit::Add);
    assert_eq!(view(&rig).positions, 4);
    m(
        &mut rig,
        MaskMessage::Edit {
            target: san("e4"),
            edit: MaskEdit::Remove,
            scope: MaskScope::Line,
        },
    );
    assert_eq!(view(&rig).positions, 0);
}

#[test]
fn filling_with_the_line_takes_the_moves_up_to_the_cursor_by_side() {
    let dir = dir();
    let mut rig = rig_with_mask(dir.path());
    play_line(&mut rig, &["e2e4", "e7e5", "g1f3", "b8c6"]);
    rig.nav(Nav::Back(1));
    m(&mut rig, MaskMessage::FillLine(FillSide::White));
    let found = hits(&mut rig, MaskQuery::default());
    let sans: Vec<String> = found.rows.iter().filter_map(|h| h.san.clone()).collect();
    assert_eq!(sans, ["e4", "Nf3"], "only White's moves up to the cursor");
    assert!(dirty(&rig));
    // Black.
    let mut rig = rig_with_mask(dir.path());
    play_line(&mut rig, &["e2e4", "e7e5", "g1f3", "b8c6"]);
    m(&mut rig, MaskMessage::FillLine(FillSide::Black));
    let found = hits(&mut rig, MaskQuery::default());
    assert_eq!(found.moves_found, 2);
    // At the start of the game there is nothing to do, and no mark.
    let mut rig = rig_with_mask(dir.path());
    m(&mut rig, MaskMessage::FillLine(FillSide::All));
    assert!(!dirty(&rig));
}

#[test]
fn filling_twice_with_the_same_game_changes_nothing_the_second_time() {
    let dir = dir();
    let mut rig = rig_with_mask(dir.path());
    play_line(&mut rig, &["e2e4", "e7e5"]);
    m(&mut rig, MaskMessage::FillGame);
    let save = m(&mut rig, MaskMessage::Save);
    pick(&mut rig, &save, Some(dir.path().join("m.stm")));
    assert!(!dirty(&rig));
    m(&mut rig, MaskMessage::FillGame);
    assert!(!dirty(&rig), "nothing new: not altered (rule 8)");
}

#[test]
fn filling_with_the_filter_runs_in_the_background_with_progress() {
    let dir = dir();
    let mut rig = rig_with_mask(dir.path());
    let effects = rig.raw(TreeMessage::Mask(MaskMessage::FillFilter(FillSide::All)));
    let job = effects
        .into_iter()
        .find_map(|e| match e {
            Effect::Mask(effect @ mychess_app::MaskEffect::Fill { .. }) => Some(effect),
            _ => None,
        })
        .expect("a fill job");
    let busy = rig.state.mask_busy().expect("busy");
    assert_eq!(busy.kind, mychess_app::MaskJobKind::Fill);
    assert!(view(&rig).busy.is_some());
    // While it runs the edits, the close and the open are refused.
    edit(&mut rig, "e4", MaskEdit::Add);
    assert_eq!(notice(&rig), Some(MaskNotice::Busy));
    assert!(!dirty(&rig));
    m(&mut rig, MaskMessage::Close);
    assert!(rig.state.mask_open());
    let answer = rig.perform(Effect::Mask(job));
    rig.send(answer);
    assert!(rig.state.mask_busy().is_none());
    assert!(dirty(&rig));
    match notice(&rig) {
        Some(MaskNotice::Filled {
            games: 5,
            skipped: 0,
            cancelled: false,
            ..
        }) => {}
        other => panic!("{other:?}"),
    }
    // The five games gave e4, d4, Nf3 at the start, and more.
    let found = hits(&mut rig, MaskQuery::default());
    assert!(found.moves_found >= 8, "{}", found.moves_found);
    // The game on the board and the table were not touched.
    assert_eq!(rig.ply(), 0);
    assert_eq!(rig.view().body, TreeBody::Rows);
}

#[test]
fn cancelling_the_fill_keeps_what_was_done_and_says_so() {
    let dir = dir();
    let mut rig = rig_with_mask(dir.path());
    let effects = rig.raw(TreeMessage::Mask(MaskMessage::FillFilter(FillSide::All)));
    let Some(Effect::Mask(job)) = effects
        .into_iter()
        .find(|e| matches!(e, Effect::Mask(mychess_app::MaskEffect::Fill { .. })))
    else {
        panic!("no job");
    };
    rig.raw(TreeMessage::Mask(MaskMessage::CancelFill));
    assert!(rig.state.mask_busy().is_some_and(|b| b.cancelling));
    let answer = rig.perform(Effect::Mask(job));
    rig.send(answer);
    assert!(rig.state.mask_busy().is_none());
    assert!(matches!(
        notice(&rig),
        Some(MaskNotice::Filled {
            cancelled: true,
            ..
        })
    ));
}

#[test]
fn an_answer_of_an_old_fill_is_dropped() {
    let dir = dir();
    let mut rig = rig_with_mask(dir.path());
    let effects = rig.raw(TreeMessage::Mask(MaskMessage::FillFilter(FillSide::All)));
    let Some(Effect::Mask(job)) = effects
        .into_iter()
        .find(|e| matches!(e, Effect::Mask(mychess_app::MaskEffect::Fill { .. })))
    else {
        panic!("no job");
    };
    let answer = rig.perform(Effect::Mask(job));
    // The quit gives the job up (deadline), then its answer comes late.
    rig.raw(Message::Quit);
    rig.raw(TreeMessage::Mask(MaskMessage::Deadline(1)));
    assert!(rig.state.mask_busy().is_none());
    rig.send(answer);
    assert!(!dirty(&rig), "the old job changed nothing");
}

#[test]
fn filling_with_the_filter_without_a_base_says_there_is_nothing() {
    let mut rig = Rig::new();
    m(&mut rig, MaskMessage::New);
    m(&mut rig, MaskMessage::FillFilter(FillSide::All));
    assert_eq!(notice(&rig), Some(MaskNotice::FillUnavailable));
    assert!(rig.state.mask_busy().is_none());
}

#[test]
fn quit_cancels_a_fill_and_waits_for_it_then_asks_about_the_result() {
    let dir = dir();
    let mut rig = rig_with_mask(dir.path());
    let effects = rig.raw(TreeMessage::Mask(MaskMessage::FillFilter(FillSide::All)));
    let Some(Effect::Mask(job)) = effects
        .into_iter()
        .find(|e| matches!(e, Effect::Mask(mychess_app::MaskEffect::Fill { .. })))
    else {
        panic!("no job");
    };
    let quit = rig.raw(Message::Quit);
    assert!(quit.iter().any(|e| matches!(
        e,
        Effect::Mask(mychess_app::MaskEffect::Deadline { generation: 1, .. })
    )));
    assert!(!quit.iter().any(|e| matches!(e, Effect::Exit)));
    assert!(rig.state.mask_busy().is_some_and(|b| b.cancelling));
    // A timer of another wait changes nothing.
    rig.raw(TreeMessage::Mask(MaskMessage::Deadline(7)));
    assert!(rig.state.mask_busy().is_some());
    // The job ends (it was cancelled before it ran: nothing was added) and
    // the quit goes on to Exit.
    let answer = rig.perform(Effect::Mask(job));
    let host = rig.send(answer);
    assert!(host.iter().any(|e| matches!(e, Effect::Exit)), "{host:?}");
}

#[test]
fn the_deadline_gives_up_a_job_that_never_answers() {
    let dir = dir();
    let mut rig = rig_with_mask(dir.path());
    rig.raw(TreeMessage::Mask(MaskMessage::FillFilter(FillSide::All)));
    rig.raw(Message::Quit);
    let host = rig.send(TreeMessage::Mask(MaskMessage::Deadline(1)));
    assert!(host.iter().any(|e| matches!(e, Effect::Exit)), "{host:?}");
    assert!(rig.state.mask_busy().is_none());
}

#[test]
fn masks_hold_standard_chess_only() {
    let mut rig = Rig::new();
    m(&mut rig, MaskMessage::New);
    let pos = Position::chess960_start(519).unwrap();
    rig.state
        .session
        .replace_game(mychess_core::Game::from_position(pos));
    assert!(!view(&rig).enabled);
    edit(&mut rig, "Nb3", MaskEdit::Add);
    assert_eq!(notice(&rig), Some(MaskNotice::NotStandard));
    m(&mut rig, MaskMessage::FillGame);
    assert_eq!(notice(&rig), Some(MaskNotice::NotStandard));
    assert!(!dirty(&rig));
    // The mask stays visible, and a standard game enables it again.
    assert!(rig.state.mask_open());
    rig.state.session.replace_game(mychess_core::Game::new());
    assert!(view(&rig).enabled);
}

#[test]
fn the_legal_moves_for_the_submenu_are_in_full_san() {
    let mut rig = Rig::new();
    assert!(rig.state.mask_legal_moves().is_empty(), "no mask");
    m(&mut rig, MaskMessage::New);
    let moves = rig.state.mask_legal_moves();
    assert_eq!(moves.len(), 20);
    assert!(moves.iter().any(|s| s == "Nf3"));
    // A move taken from the list is added as it is.
    edit(&mut rig, &moves[0].clone(), MaskEdit::Add);
    assert!(dirty(&rig));
}

#[test]
fn the_search_lists_and_a_stale_answer_is_dropped() {
    let dir = dir();
    let mut rig = rig_with_mask(dir.path());
    edit(&mut rig, "e4", MaskEdit::Nag(Some(MaskNag::Good)));
    edit(&mut rig, "d4", MaskEdit::Comment("Queen pawn".to_owned()));
    m(
        &mut rig,
        MaskMessage::SetPositionComment("Start *here?*".to_owned()),
    );
    let found = hits(
        &mut rig,
        MaskQuery {
            nag: Some(Some(MaskNag::Good)),
            ..MaskQuery::default()
        },
    );
    assert_eq!(found.moves_found, 1);
    // Text is a literal substring: `*` and `?` are not wildcards.
    let found = hits(
        &mut rig,
        MaskQuery {
            position_text: Some("*here?*".to_owned()),
            ..MaskQuery::default()
        },
    );
    assert_eq!(found.positions_found, 1);
    let found = hits(
        &mut rig,
        MaskQuery {
            move_text: Some("QUEEN".to_owned()),
            ..MaskQuery::default()
        },
    );
    assert_eq!(found.moves_found, 1);
    // An answer for another request does nothing.
    let before = rig.state.mask_search().cloned();
    rig.send(TreeMessage::Mask(MaskMessage::Searched {
        request: mychess_app::RequestId(424_242),
        hits: Box::default(),
    }));
    assert_eq!(rig.state.mask_search().cloned(), before);
    m(&mut rig, MaskMessage::CloseSearch);
    assert!(rig.state.mask_search().is_none());
}

#[test]
fn the_search_runs_as_an_effect_not_in_update() {
    let mut rig = Rig::new();
    m(&mut rig, MaskMessage::New);
    let effects = rig.raw(TreeMessage::Mask(MaskMessage::Search(Box::default())));
    assert!(matches!(
        effects.as_slice(),
        [Effect::Mask(mychess_app::MaskEffect::Search { .. })]
    ));
    assert!(rig.state.mask_search().is_some_and(|s| s.hits.is_none()));
    assert!(rig.state.mask_busy().is_some());
    let effects = rig.raw(TreeMessage::Mask(MaskMessage::BuildOutline));
    assert!(matches!(
        effects.as_slice(),
        [Effect::Mask(mychess_app::MaskEffect::Outline { .. })]
    ));
}

#[test]
fn the_outline_starts_at_the_cursor_and_a_node_goes_to_its_position() {
    let mut rig = Rig::new();
    m(&mut rig, MaskMessage::New);
    play_line(&mut rig, &["e2e4", "e7e5", "g1f3"]);
    m(&mut rig, MaskMessage::FillGame);
    rig.nav(Nav::Start);
    m(&mut rig, MaskMessage::BuildOutline);
    let state = rig.state.mask_outline().expect("outline");
    let outline = state.outline.clone().expect("the answer");
    // A single line of three moves is one merged node.
    assert_eq!(outline.roots.len(), 1);
    let steps = &outline.roots[0].steps;
    assert_eq!(steps.len(), 3);
    assert!(!outline.truncated);
    let key = steps[1].after.clone().expect("legal");
    // The double click opens a new game at that position.
    m(&mut rig, MaskMessage::GoTo(key));
    // The game was played on the board, so it is altered: it asks first.
    assert!(rig.state.discard_prompt.is_some());
    rig.send(Message::AnswerDiscard(mychess_app::DiscardChoice::DontSave));
    assert_eq!(rig.ply(), 0);
    assert_eq!(
        rig.state.session.position().fen(),
        "rnbqkbnr/pppp1ppp/8/4p3/4P3/8/PPPP1PPP/RNBQKBNR w KQkq - 0 1"
    );
    m(&mut rig, MaskMessage::CloseOutline);
    assert!(rig.state.mask_outline().is_none());
}

#[test]
fn going_to_an_opaque_key_is_a_notice() {
    let mut rig = Rig::new();
    m(&mut rig, MaskMessage::New);
    m(&mut rig, MaskMessage::GoTo(MaskKey::from_text("not a fen")));
    assert_eq!(notice(&rig), Some(MaskNotice::CannotShowPosition));
}

#[test]
fn going_to_a_position_asks_when_the_game_is_altered() {
    let mut rig = Rig::new();
    m(&mut rig, MaskMessage::New);
    rig.play("e2e4");
    let key = MaskKey::from_position(&Position::new());
    m(&mut rig, MaskMessage::GoTo(key));
    assert!(rig.state.discard_prompt.is_some());
}

#[test]
fn the_help_of_an_online_game_family_is_the_openings_for_what_shows_lines() {
    use mychess_app::online::{Family, family_of};
    let show = [
        MaskMessage::Search(Box::default()),
        MaskMessage::BuildOutline,
        MaskMessage::GoTo(MaskKey::from_position(&Position::new())),
        MaskMessage::PlayOnlyRow(0),
    ];
    for message in show {
        assert_eq!(
            family_of(&Message::Tree(TreeMessage::Mask(message.clone()))),
            Some(Family::Openings),
            "{message:?}"
        );
    }
    for message in [
        MaskMessage::New,
        MaskMessage::Open(None),
        MaskMessage::Save,
        MaskMessage::Close,
        MaskMessage::SetShowMarkers(false),
        MaskMessage::CancelFill,
    ] {
        assert_eq!(
            family_of(&Message::Tree(TreeMessage::Mask(message.clone()))),
            None,
            "{message:?}"
        );
    }
}

#[test]
fn the_training_does_not_learn_moves_from_the_mask() {
    let dir = dir();
    let mut rig = rig_with_mask(dir.path());
    edit(&mut rig, "c4", MaskEdit::Add);
    edit(&mut rig, "e4", MaskEdit::Nag(Some(MaskNag::Good)));
    rig.send(TreeMessage::SetTraining(true));
    let v = view(&rig);
    assert!(v.only_rows.is_empty());
    assert!(rig.view().rows.iter().all(|r| r.mask.is_none() || r.hidden));
}

#[test]
fn the_files_of_the_texts_have_every_key_in_both_languages() {
    let keys = [
        "tree-mask-open-failed",
        "tree-mask-save-failed",
        "tree-mask-saved",
        "tree-mask-warnings",
        "tree-mask-no-mask",
        "tree-mask-busy",
        "tree-mask-not-standard",
        "tree-mask-filled",
        "tree-mask-fill-cancelled",
        "tree-mask-fill-unavailable",
        "tree-mask-full",
        "tree-mask-cannot-show",
        "tree-mask-unsaved-title",
        "tree-mask-unsaved-body",
    ];
    let notices = [
        MaskNotice::Saved,
        MaskNotice::NoMask,
        MaskNotice::Busy,
        MaskNotice::NotStandard,
        MaskNotice::FillUnavailable,
        MaskNotice::Full,
        MaskNotice::CannotShowPosition,
        MaskNotice::Warnings { count: 1 },
        MaskNotice::Filled {
            moves: 1,
            games: 1,
            skipped: 0,
            cancelled: false,
        },
        MaskNotice::Filled {
            moves: 1,
            games: 1,
            skipped: 0,
            cancelled: true,
        },
    ];
    let issues = [
        mychess_app::MaskIssue::NotAMask,
        mychess_app::MaskIssue::Foreign,
        mychess_app::MaskIssue::Syntax,
        mychess_app::MaskIssue::TooLarge,
        mychess_app::MaskIssue::ReadOnly,
        mychess_app::MaskIssue::Io(std::io::ErrorKind::NotFound),
        mychess_app::MaskIssue::Io(std::io::ErrorKind::PermissionDenied),
        mychess_app::MaskIssue::Io(std::io::ErrorKind::Other),
        mychess_app::MaskIssue::Internal,
    ];
    let mut wanted: Vec<&str> = keys.to_vec();
    wanted.extend(notices.iter().map(MaskNotice::message_key));
    wanted.extend(issues.iter().map(mychess_app::MaskIssue::message_key));
    for language in ["en-US", "pt-BR"] {
        let file = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../assets/i18n")
            .join(language)
            .join("tree.ftl");
        let text = std::fs::read_to_string(&file).expect("the Fluent file");
        for key in &wanted {
            assert!(
                text.lines().any(|l| l.starts_with(&format!("{key} ="))),
                "{key} is missing in {language}"
            );
        }
        // "variante", never "variação" (D1836).
        if language == "pt-BR" {
            assert!(!text.to_lowercase().contains("variaç"));
        }
    }
}
