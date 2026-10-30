//! `RequestStrip` / `AnswerStrip` (D190, rules 46-48): counts, prompt,
//! direct action, cancel, blocking and undo.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::assert_is_empty,
    clippy::unreadable_literal,
    clippy::assigning_clones,
    clippy::manual_assert_eq,
    clippy::format_collect,
    clippy::unchecked_time_subtraction,
    clippy::field_reassign_with_default,
    clippy::cast_possible_truncation,
    clippy::items_after_statements,
    clippy::too_many_lines,
    clippy::similar_names,
    clippy::needless_pass_by_value
)]

mod m2_4_common;

use m2_4_common::*;
use rstest::rstest;

/// `{i} 1.e4 {c1} e5 (1...c5 {c2} 2.Nf3 (2.Nc3)) 2.Nf3` with `{b}` before c5.
struct Fixture {
    session: GameSession,
    e4: NodeId,
    e5: NodeId,
    c5: NodeId,
    nf3: NodeId,
    nf3_var: NodeId,
    nc3: NodeId,
}

fn fixture() -> Fixture {
    let mut g = Game::new();
    let root = g.root();
    let e4 = san(&mut g, root, "e4");
    let e5 = san(&mut g, e4, "e5");
    let c5 = san(&mut g, e4, "c5");
    let nf3 = san(&mut g, e5, "Nf3");
    let nf3_var = san(&mut g, c5, "Nf3");
    let nc3 = san(&mut g, c5, "Nc3");
    g.set_comment(e4, "c1").unwrap();
    g.set_comment(c5, "c2").unwrap();
    g.set_comment_before(c5, "b").unwrap();
    g.set_initial_comment("i");
    g.tags_mut().white = "Keep".to_owned();
    Fixture {
        session: GameSession::new(g),
        e4,
        e5,
        c5,
        nf3,
        nf3_var,
        nc3,
    }
}

fn request(s: &mut GameSession, kind: StripKind, confirm: bool) {
    s.update(GameMessage::RequestStrip { kind, confirm });
}

fn text(s: &GameSession) -> String {
    build_notation(s.game(), &NotationOptions::default(), 0).to_plain()
}

#[test]
fn counts_per_kind() {
    let f = fixture();
    let s = &f.session;
    let root = s.game().root();
    assert_eq!(
        s.strip_count(StripKind::Comments),
        Some(4),
        "i, c1, c2 and the one before c5"
    );
    assert_eq!(s.strip_count(StripKind::Variations), Some(2), "c5 and Nc3");
    assert_eq!(s.strip_count(StripKind::TruncateAfter(f.e4)), Some(5));
    assert_eq!(s.strip_count(StripKind::TruncateAfter(f.nf3)), Some(0));
    assert_eq!(s.strip_count(StripKind::TruncateFrom(f.e5)), Some(2));
    assert_eq!(s.strip_count(StripKind::TruncateFrom(f.c5)), Some(3));
    assert_eq!(s.strip_count(StripKind::TruncateFrom(root)), Some(0));
    assert_eq!(s.strip_count(StripKind::TruncateBefore(f.e5)), Some(2));
    assert_eq!(s.strip_count(StripKind::TruncateBefore(f.nf3_var)), Some(3));
    assert_eq!(s.strip_count(StripKind::TruncateBefore(root)), Some(0));
}

#[test]
fn counts_of_a_plain_game_are_zero() {
    let mut g = Game::new();
    main_line(&mut g, "e4 e5");
    let s = GameSession::new(g);
    assert_eq!(s.strip_count(StripKind::Comments), Some(0));
    assert_eq!(s.strip_count(StripKind::Variations), Some(0));
}

#[rstest]
#[case(StripKind::Comments, 4)]
#[case(StripKind::Variations, 2)]
fn request_with_confirm_opens_the_prompt_with_the_count_and_changes_nothing(
    #[case] kind: StripKind,
    #[case] count: usize,
) {
    let mut f = fixture();
    let before = text(&f.session);
    request(&mut f.session, kind, true);
    assert_eq!(
        f.session.prompt(),
        Some(&Prompt::ConfirmStrip { kind, count })
    );
    assert_eq!(text(&f.session), before);
    assert!(!f.session.is_dirty());
}

#[test]
fn truncate_prompts_carry_their_counts() {
    let mut f = fixture();
    let cases = [
        (StripKind::TruncateAfter(f.e4), 5),
        (StripKind::TruncateFrom(f.e5), 2),
        (StripKind::TruncateBefore(f.e5), 2),
    ];
    for (kind, count) in cases {
        request(&mut f.session, kind, true);
        assert_eq!(
            f.session.prompt(),
            Some(&Prompt::ConfirmStrip { kind, count })
        );
        f.session.update(GameMessage::DismissPrompt);
    }
}

#[test]
fn answer_yes_strips_comments_and_keeps_moves_and_variations() {
    let mut f = fixture();
    let nodes = f.session.game().node_count();
    request(&mut f.session, StripKind::Comments, true);
    f.session.update(GameMessage::AnswerStrip(true));
    assert!(f.session.prompt().is_none());
    assert_eq!(f.session.game().node_count(), nodes);
    assert!(f.session.game().initial_comment().is_none());
    for n in [f.e4, f.c5] {
        assert!(f.session.game().comment(n).unwrap().is_none());
    }
    assert!(f.session.game().comment_before(f.c5).unwrap().is_none());
    assert!(f.session.is_dirty());
}

#[test]
fn answer_yes_strips_variations_at_every_level() {
    let mut f = fixture();
    request(&mut f.session, StripKind::Variations, true);
    f.session.update(GameMessage::AnswerStrip(true));
    let g = f.session.game();
    assert_eq!(g.node_count(), 4, "root, e4, e5, Nf3");
    assert!(g.children(f.e4).unwrap().len() == 1);
    assert!(g.contains(f.session.cursor()));
}

#[test]
fn truncate_after_keeps_the_node_and_removes_everything_below() {
    let mut f = fixture();
    request(&mut f.session, StripKind::TruncateAfter(f.e4), true);
    f.session.update(GameMessage::AnswerStrip(true));
    assert_eq!(f.session.game().node_count(), 2);
    assert!(f.session.game().contains(f.e4));
    assert!(f.session.game().contains(f.session.cursor()));
}

#[test]
fn truncate_from_removes_the_node_and_what_follows_but_not_siblings() {
    let mut f = fixture();
    request(&mut f.session, StripKind::TruncateFrom(f.e5), true);
    f.session.update(GameMessage::AnswerStrip(true));
    let g = f.session.game();
    assert_eq!(g.node_count(), 5);
    assert!(!g.contains(f.e5) && !g.contains(f.nf3));
    assert!(g.contains(f.c5) && g.contains(f.nc3));
}

#[test]
fn truncate_before_starts_a_new_game_at_the_node_and_keeps_the_tags() {
    let mut f = fixture();
    request(&mut f.session, StripKind::TruncateBefore(f.e5), true);
    f.session.update(GameMessage::AnswerStrip(true));
    let g = f.session.game();
    assert_eq!(f.session.cursor(), g.root());
    assert_eq!(g.tags().white, "Keep");
    assert_eq!(g.start_position().fullmove_number(), 2);
    assert_eq!(g.node_count(), 2, "only Nf3 remains");
}

#[test]
fn answer_no_and_dismiss_cancel() {
    for cancel in [GameMessage::AnswerStrip(false), GameMessage::DismissPrompt] {
        let mut f = fixture();
        let before = text(&f.session);
        request(&mut f.session, StripKind::Variations, true);
        f.session.update(cancel);
        assert!(f.session.prompt().is_none());
        assert_eq!(text(&f.session), before);
        assert!(!f.session.is_dirty());
        assert!(!f.session.can_undo());
    }
}

#[test]
fn without_confirmation_it_acts_at_once() {
    let mut f = fixture();
    request(&mut f.session, StripKind::Comments, false);
    assert!(f.session.prompt().is_none());
    assert_eq!(f.session.strip_count(StripKind::Comments), Some(0));
    assert!(f.session.is_dirty());
}

#[rstest]
#[case(false)]
#[case(true)]
fn zero_count_is_a_notice_with_no_prompt_and_no_edit(#[case] confirm: bool) {
    let mut g = Game::new();
    let ns = main_line(&mut g, "e4 e5");
    let root = g.root();
    let mut s = GameSession::new(g);
    let kinds = [
        StripKind::Comments,
        StripKind::Variations,
        StripKind::TruncateAfter(ns[1]),
        StripKind::TruncateBefore(root),
    ];
    for kind in kinds {
        request(&mut s, kind, confirm);
        assert!(s.prompt().is_none(), "{kind:?}");
        assert_eq!(s.notice(), Some(&Notice::NothingToStrip), "{kind:?}");
        assert!(!s.is_dirty() && !s.can_undo(), "{kind:?}");
    }
}

#[test]
fn truncate_from_the_root_is_no_move_here() {
    let mut f = fixture();
    let root = f.session.game().root();
    request(&mut f.session, StripKind::TruncateFrom(root), true);
    assert_eq!(f.session.notice(), Some(&Notice::NoMoveHere));
    assert!(f.session.prompt().is_none());
}

#[test]
fn an_open_prompt_blocks_the_other_game_messages() {
    let mut f = fixture();
    request(&mut f.session, StripKind::Comments, true);
    let before = text(&f.session);
    let cursor = f.session.cursor();
    let blocked = [
        GameMessage::Navigate(Nav::GameEnd),
        GameMessage::AddNag(nag(1)),
        GameMessage::StripVariations,
        GameMessage::Undo,
        GameMessage::PlayNullMove,
        GameMessage::MoveEntryChar('e'),
        GameMessage::SetComment {
            node: f.e4,
            text: "x".into(),
        },
        GameMessage::EnterTrial { null_move: false },
        GameMessage::DeleteVariation(VariationTarget::Node(f.c5)),
    ];
    for m in blocked {
        let label = format!("{m:?}");
        f.session.update(m);
        assert_eq!(f.session.notice(), Some(&Notice::PromptPending), "{label}");
        assert!(f.session.prompt().is_some(), "{label}");
        assert_eq!(text(&f.session), before, "{label}");
        assert_eq!(f.session.cursor(), cursor, "{label}");
    }
}

#[test]
fn a_second_request_does_not_replace_the_open_prompt() {
    let mut f = fixture();
    request(&mut f.session, StripKind::Comments, true);
    request(&mut f.session, StripKind::Variations, true);
    assert!(matches!(
        f.session.prompt(),
        Some(Prompt::ConfirmStrip {
            kind: StripKind::Comments,
            ..
        })
    ));
}

#[test]
fn answer_without_a_prompt_is_harmless() {
    let mut f = fixture();
    let before = text(&f.session);
    f.session.update(GameMessage::AnswerStrip(true));
    assert_eq!(text(&f.session), before);
    assert!(!f.session.is_dirty());
}

#[test]
fn the_answer_to_another_kind_of_prompt_is_not_a_strip_answer() {
    let mut f = fixture();
    // A replace-move prompt is open: AnswerStrip must not execute anything.
    let mv = f
        .session
        .game()
        .position_at(f.session.game().root())
        .unwrap()
        .parse_uci("d2d4")
        .unwrap();
    f.session.update(GameMessage::PlayMove {
        mv,
        options: PlayOptions::default(),
    });
    if matches!(f.session.prompt(), Some(Prompt::ReplaceMove(_))) {
        let before = text(&f.session);
        f.session.update(GameMessage::AnswerStrip(true));
        assert_eq!(text(&f.session), before);
    }
}

#[rstest]
#[case::comments(0)]
#[case::variations(1)]
#[case::after(2)]
#[case::from(3)]
#[case::before(4)]
fn undo_restores_the_game_after_a_confirmed_removal(#[case] which: usize) {
    let mut f = fixture();
    let kind = [
        StripKind::Comments,
        StripKind::Variations,
        StripKind::TruncateAfter(f.e4),
        StripKind::TruncateFrom(f.e5),
        StripKind::TruncateBefore(f.e5),
    ][which];
    let before = text(&f.session);
    let nodes = f.session.game().node_count();
    request(&mut f.session, kind, true);
    f.session.update(GameMessage::AnswerStrip(true));
    assert_ne!(text(&f.session), before, "{kind:?} did something");
    f.session.update(GameMessage::Undo);
    assert_eq!(text(&f.session), before, "{kind:?}");
    assert_eq!(f.session.game().node_count(), nodes);
    assert!(f.session.game().contains(f.session.cursor()));
    // Redo does it again.
    f.session.update(GameMessage::Redo);
    assert_ne!(text(&f.session), before);
}

#[test]
fn a_cancelled_removal_leaves_no_undo_step() {
    let mut f = fixture();
    request(&mut f.session, StripKind::Variations, true);
    f.session.update(GameMessage::AnswerStrip(false));
    f.session.update(GameMessage::Undo);
    assert!(f.session.game().contains(f.c5));
}

#[test]
fn a_removed_node_id_is_stale_and_never_panics() {
    let mut f = fixture();
    f.session
        .update(GameMessage::DeleteVariation(VariationTarget::Node(f.c5)));
    for kind in [
        StripKind::TruncateAfter(f.c5),
        StripKind::TruncateFrom(f.c5),
        StripKind::TruncateBefore(f.nc3),
    ] {
        request(&mut f.session, kind, true);
        assert_eq!(f.session.notice(), Some(&Notice::StaleNode), "{kind:?}");
        assert!(f.session.prompt().is_none());
        assert_eq!(f.session.strip_count(kind), None);
    }
}

#[test]
fn a_node_id_from_a_bigger_game_is_stale() {
    let mut big = Game::new();
    let ns = main_line(&mut big, "e4 e5 Nf3 Nc6 Bb5 a6 Ba4 Nf6");
    let foreign = *ns.last().unwrap();
    let mut s = GameSession::default();
    for confirm in [true, false] {
        request(&mut s, StripKind::TruncateAfter(foreign), confirm);
        assert_eq!(s.notice(), Some(&Notice::StaleNode));
        assert!(s.prompt().is_none());
    }
}

#[test]
fn removal_through_app_state_refreshes_the_notation() {
    let f = fixture();
    let mut state = AppState::default();
    state.session = f.session;
    send(
        &mut state,
        GameMessage::RequestStrip {
            kind: StripKind::Variations,
            confirm: true,
        },
    );
    assert!(
        state.notation().flat_text().contains('('),
        "prompt does not change the text"
    );
    send(&mut state, GameMessage::AnswerStrip(true));
    assert!(!state.notation().flat_text().contains('('));
}

#[test]
fn removal_inside_a_variation_keeps_a_valid_cursor() {
    let mut f = fixture();
    f.session
        .update(GameMessage::Navigate(Nav::GoToNode(f.nc3)));
    assert_eq!(f.session.cursor(), f.nc3);
    request(&mut f.session, StripKind::Variations, false);
    assert!(f.session.game().contains(f.session.cursor()));
    let mut f = fixture();
    f.session
        .update(GameMessage::Navigate(Nav::GoToNode(f.nf3_var)));
    request(&mut f.session, StripKind::TruncateFrom(f.c5), false);
    assert!(f.session.game().contains(f.session.cursor()));
}
