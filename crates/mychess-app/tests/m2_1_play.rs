//! Spec m2-1, rules 32-43: playing a move, the decision table of rule 38,
//! the replace prompt, promotion, null moves and armed variations.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::assert_is_empty)]

mod m2_1_common;

use m2_1_common::*;
use mychess_app::{
    EditSettings, GameMessage, GameSession, Nav, Notice, PendingMove, PlayOptions, Prompt,
    ReplaceChoice, VariationTarget,
};
use rstest::rstest;

/// Session on `1.e4 e5 2.Nf3 Nc6` with the cursor after 1.e4 (a branching
/// point once the new move d7d5 / c7c5 is played).
fn at_e4() -> (GameSession, NodeId, NodeId) {
    let g = line(&["e2e4", "e7e5", "g1f3", "b8c6"]);
    let m1 = g.children(g.root()).unwrap()[0];
    let m2 = g.children(m1).unwrap()[0];
    let mut s = GameSession::new(g);
    goto(&mut s, m1);
    (s, m1, m2)
}

fn settings(f: impl FnOnce(&mut EditSettings)) -> EditSettings {
    let mut e = EditSettings::default();
    f(&mut e);
    e
}

fn snapshot_of(s: &GameSession) -> (Game, NodeId, usize, bool) {
    (s.game().clone(), s.cursor(), s.undo_depth(), s.is_dirty())
}

fn assert_unchanged(s: &GameSession, before: &(Game, NodeId, usize, bool)) {
    assert!(s.game().same_content(&before.0), "game changed");
    assert_eq!(s.cursor(), before.1, "cursor moved");
    assert_eq!(s.undo_depth(), before.2, "undo point created");
    assert_eq!(s.is_dirty(), before.3, "dirty flag changed");
}

// ---- legality (rule 32) ----

#[test]
fn illegal_move_is_refused_with_a_notice_and_nothing_changes() {
    let mut s = GameSession::new(Game::new());
    // A Black move, taken from a position where Black is to move.
    let after_e4 = line(&["e2e4"]);
    let leaf = after_e4.mainline().last().unwrap();
    let black_move = mv_at(&after_e4, leaf, "e7e5");
    let before = snapshot_of(&s);
    s.update(GameMessage::PlayMove {
        mv: black_move,
        options: PlayOptions::default(),
    });
    assert_eq!(s.notice(), Some(&Notice::IllegalMove));
    assert_unchanged(&s, &before);
}

#[test]
fn null_move_in_check_is_refused() {
    let mut s = session_fen("4k3/8/8/8/8/8/4r3/4K3 w - - 0 1");
    let before = snapshot_of(&s);
    s.update(GameMessage::PlayNullMove);
    assert_eq!(s.notice(), Some(&Notice::NullMoveInCheck));
    assert_unchanged(&s, &before);
}

#[test]
fn null_move_outside_check_is_appended_at_a_leaf() {
    let mut s = GameSession::new(Game::new());
    s.update(GameMessage::PlayNullMove);
    assert_eq!(s.game().node_count(), 2);
    assert!(s.game().mv(s.cursor()).unwrap().is_null());
    assert_eq!(s.undo_depth(), 1);
    assert!(s.is_dirty());
}

// ---- PlaySquares (rule 33) ----

#[test]
fn play_squares_plays_a_normal_move() {
    let mut s = GameSession::new(Game::new());
    s.update(GameMessage::PlaySquares {
        from: sq("e2"),
        to: sq("e4"),
        promotion: None,
        options: PlayOptions::default(),
    });
    assert_eq!(s.game().ply(s.cursor()).unwrap(), 1);
    assert!(s.prompt().is_none());
}

#[rstest]
#[case("e2", "e5")]
#[case("e7", "e5")]
#[case("a1", "a8")]
#[case("e4", "e5")]
fn play_squares_without_a_legal_move_is_illegal(#[case] from: &str, #[case] to: &str) {
    let mut s = GameSession::new(Game::new());
    s.update(GameMessage::PlaySquares {
        from: sq(from),
        to: sq(to),
        promotion: None,
        options: PlayOptions::default(),
    });
    assert_eq!(s.notice(), Some(&Notice::IllegalMove));
    assert_eq!(s.game().node_count(), 1);
}

const PROMO_FEN: &str = "7k/P7/8/8/8/8/8/K7 w - - 0 1";

fn promo_squares(s: &mut GameSession, promotion: Option<Role>) {
    s.update(GameMessage::PlaySquares {
        from: sq("a7"),
        to: sq("a8"),
        promotion,
        options: PlayOptions::default(),
    });
}

#[test]
fn promotion_without_piece_opens_the_prompt_and_does_not_play() {
    let mut s = session_fen(PROMO_FEN);
    promo_squares(&mut s, None);
    match s.prompt() {
        Some(Prompt::ChoosePromotion { from, to, .. }) => {
            assert_eq!((*from, *to), (sq("a7"), sq("a8")));
        }
        other => panic!("expected promotion prompt, got {other:?}"),
    }
    assert_eq!(s.game().node_count(), 1);
    assert!(!s.is_dirty());
    assert_eq!(s.undo_depth(), 0);
}

#[rstest]
#[case(Role::Queen)]
#[case(Role::Rook)]
#[case(Role::Bishop)]
#[case(Role::Knight)]
fn answering_the_promotion_prompt_plays_that_piece(#[case] role: Role) {
    let mut s = session_fen(PROMO_FEN);
    promo_squares(&mut s, None);
    s.update(GameMessage::AnswerPromotion(Some(role)));
    assert!(s.prompt().is_none());
    match s.game().mv(s.cursor()).unwrap() {
        Move::Normal { promotion, .. } => assert_eq!(promotion, Some(role)),
        other => panic!("expected promotion, got {other:?}"),
    }
    assert_eq!(s.undo_depth(), 1);
}

#[rstest]
#[case(Role::Queen)]
#[case(Role::Rook)]
#[case(Role::Bishop)]
#[case(Role::Knight)]
fn promotion_piece_given_directly_plays_without_prompt(#[case] role: Role) {
    let mut s = session_fen(PROMO_FEN);
    promo_squares(&mut s, Some(role));
    assert!(s.prompt().is_none());
    assert_eq!(s.game().node_count(), 2);
}

#[rstest]
#[case(Role::King)]
#[case(Role::Pawn)]
fn promotion_to_king_or_pawn_is_illegal(#[case] role: Role) {
    let mut s = session_fen(PROMO_FEN);
    promo_squares(&mut s, Some(role));
    assert_eq!(s.notice(), Some(&Notice::IllegalMove));
    assert_eq!(s.game().node_count(), 1);

    let mut s = session_fen(PROMO_FEN);
    promo_squares(&mut s, None);
    s.update(GameMessage::AnswerPromotion(Some(role)));
    assert_eq!(s.notice(), Some(&Notice::IllegalMove));
    assert_eq!(s.game().node_count(), 1, "nothing played");
}

#[test]
fn promotion_prompt_cancelled_by_none_or_dismiss() {
    for cancel in [
        GameMessage::AnswerPromotion(None),
        GameMessage::DismissPrompt,
    ] {
        let mut s = session_fen(PROMO_FEN);
        promo_squares(&mut s, None);
        s.update(cancel);
        assert!(s.prompt().is_none());
        assert_eq!(s.game().node_count(), 1);
        assert!(!s.is_dirty());
    }
}

#[test]
fn answer_promotion_without_prompt_changes_nothing() {
    let mut s = session_fen(PROMO_FEN);
    s.update(GameMessage::AnswerPromotion(Some(Role::Queen)));
    assert_eq!(s.game().node_count(), 1);
    assert!(s.prompt().is_none());
}

#[test]
fn promotion_by_capture_also_asks() {
    let mut s = session_fen("1r5k/P7/8/8/8/8/8/K7 w - - 0 1");
    s.update(GameMessage::PlaySquares {
        from: sq("a7"),
        to: sq("b8"),
        promotion: None,
        options: PlayOptions::default(),
    });
    assert!(matches!(s.prompt(), Some(Prompt::ChoosePromotion { .. })));
}

#[test]
fn king_takes_king_is_the_null_move() {
    let mut s = GameSession::new(Game::new());
    s.update(GameMessage::PlaySquares {
        from: sq("e1"),
        to: sq("e8"),
        promotion: None,
        options: PlayOptions::default(),
    });
    assert!(s.game().mv(s.cursor()).unwrap().is_null());
}

#[test]
fn king_takes_king_in_check_is_refused_as_null_move_in_check() {
    let mut s = session_fen("4k3/8/8/8/8/8/4r3/4K3 w - - 0 1");
    let before = snapshot_of(&s);
    s.update(GameMessage::PlaySquares {
        from: sq("e1"),
        to: sq("e8"),
        promotion: None,
        options: PlayOptions::default(),
    });
    assert_eq!(s.notice(), Some(&Notice::NullMoveInCheck));
    assert_unchanged(&s, &before);
}

#[test]
fn ordinary_king_move_is_not_a_null_move() {
    let mut s = GameSession::new(Game::new());
    play(&mut s, "e2e4");
    play(&mut s, "e7e5");
    s.update(GameMessage::PlaySquares {
        from: sq("e1"),
        to: sq("e2"),
        promotion: None,
        options: PlayOptions::default(),
    });
    assert!(!s.game().mv(s.cursor()).unwrap().is_null());
}

// ---- already existing move (rule 35) ----

#[test]
fn existing_main_move_only_navigates() {
    let (mut s, m1, m2) = at_e4();
    let before = snapshot_of(&s);
    play(&mut s, "e7e5");
    assert_eq!(s.cursor(), m2);
    assert_eq!(s.undo_depth(), before.2);
    assert_eq!(s.is_dirty(), before.3);
    assert!(s.game().same_content(&before.0));
    assert!(s.prompt().is_none());
    assert_eq!(m1, s.game().parent(m2).unwrap().unwrap());
}

#[test]
fn existing_variation_move_only_navigates_even_when_forced() {
    let (mut s, n) = nested_session();
    goto(&mut s, n.m1);
    let before = snapshot_of(&s);
    play_forced(&mut s, "c7c5");
    assert_eq!(s.cursor(), n.a1);
    assert!(s.game().same_content(&before.0));
    assert_eq!(s.undo_depth(), 0);
    assert!(!s.is_dirty());
    assert!(s.prompt().is_none());
}

#[test]
fn existing_null_move_only_navigates() {
    let mut s = GameSession::new(line(&["e2e4"]));
    nav(&mut s, Nav::GameEnd);
    s.update(GameMessage::PlayNullMove);
    let null_node = s.cursor();
    let count = s.game().node_count();
    nav(&mut s, Nav::Back(1));
    s.update(GameMessage::PlayNullMove);
    assert_eq!(s.cursor(), null_node);
    assert_eq!(s.game().node_count(), count);
    assert_eq!(s.undo_depth(), 1, "only the first null move made a point");
}

// ---- leaf (rule 36) ----

#[test]
fn move_at_a_leaf_is_appended_without_asking() {
    let mut s = GameSession::new(line(&["e2e4"]));
    nav(&mut s, Nav::GameEnd);
    let leaf = s.cursor();
    play(&mut s, "e7e5");
    assert!(s.prompt().is_none());
    assert_eq!(children(&s, leaf), vec![s.cursor()]);
    assert_eq!(s.undo_depth(), 1);
    assert!(s.is_dirty());
}

#[test]
fn playing_from_the_root_of_an_empty_game() {
    let mut s = GameSession::new(Game::new());
    play(&mut s, "d2d4");
    assert_eq!(s.game().ply(s.cursor()).unwrap(), 1);
    assert_eq!(s.position().fen(), {
        let mut p = Position::new();
        p.play(mv_at(s.game(), s.game().root(), "d2d4")).unwrap();
        p.fen()
    });
}

// ---- decision table of rule 38 ----

#[test]
fn rule38_row1_force_variation_adds_a_variation_without_prompt() {
    let (mut s, m1, m2) = at_e4();
    play_forced(&mut s, "c7c5");
    assert!(s.prompt().is_none());
    let kids = children(&s, m1);
    assert_eq!(kids.len(), 2);
    assert_eq!(kids[0], m2, "main continuation unchanged");
    assert_eq!(s.cursor(), kids[1]);
    assert_eq!((s.undo_depth(), s.is_dirty()), (1, true));
}

#[test]
fn rule38_row1_force_variation_beats_ask_off_and_trial() {
    let (mut s, m1, m2) = at_e4();
    s.update(GameMessage::ApplySettings(settings(|e| {
        e.ask_to_replace_moves = false;
    })));
    s.update(GameMessage::EnterTrial { null_move: false });
    play_forced(&mut s, "c7c5");
    let kids = children(&s, m1);
    assert_eq!(kids.len(), 2, "variation, not replace");
    assert_eq!(kids[0], m2);
}

#[test]
fn rule38_row2_ask_off_replaces() {
    let (mut s, m1, m2) = at_e4();
    s.update(GameMessage::ApplySettings(settings(|e| {
        e.ask_to_replace_moves = false;
    })));
    play(&mut s, "c7c5");
    assert!(s.prompt().is_none());
    assert_eq!(children(&s, m1), vec![s.cursor()]);
    assert!(!s.game().contains(m2), "old continuation dropped");
    assert_eq!(s.game().node_count(), 3, "root, e4, c5");
    assert_eq!((s.undo_depth(), s.is_dirty()), (1, true));
}

#[test]
fn rule38_row3_trial_replaces_without_prompt_and_without_undo_point() {
    let (mut s, m1, m2) = at_e4();
    s.update(GameMessage::EnterTrial { null_move: false });
    play(&mut s, "c7c5");
    assert!(s.prompt().is_none());
    assert_eq!(children(&s, m1), vec![s.cursor()]);
    assert!(!s.game().contains(m2));
    assert_eq!(s.undo_depth(), 0);
    assert!(!s.is_dirty(), "dirty shows the value from before the trial");
}

#[test]
fn rule38_row4_prompt_is_opened_and_nothing_changes() {
    let (mut s, m1, _) = at_e4();
    let before = snapshot_of(&s);
    let mv = mv_at(s.game(), m1, "c7c5");
    s.update(GameMessage::PlayMove {
        mv,
        options: PlayOptions::default(),
    });
    assert_eq!(
        s.prompt(),
        Some(&Prompt::ReplaceMove(PendingMove { parent: m1, mv }))
    );
    assert_unchanged(&s, &before);
}

#[test]
fn rule38_prompt_stops_a_running_autoplay() {
    let (mut s, _, m2) = at_e4();
    s.update(GameMessage::StartAutoplay);
    assert_eq!(s.cursor(), m2, "immediate step");
    assert!(s.autoplay().running);
    play(&mut s, "b1c3"); // new move at a node that has Nf3
    assert!(matches!(s.prompt(), Some(Prompt::ReplaceMove(_))));
    assert!(!s.autoplay().running);
}

// ---- AnswerReplace (rules 39-41) ----

fn open_prompt_at_m1() -> (GameSession, Nested) {
    let (mut s, n) = nested_session();
    goto(&mut s, n.m1); // children [m2, a1]
    let mv = mv_at(s.game(), n.m1, "d7d5");
    s.update(GameMessage::PlayMove {
        mv,
        options: PlayOptions::default(),
    });
    assert!(s.prompt().is_some());
    (s, n)
}

#[test]
fn answer_replace_discards_following_moves_and_their_variations() {
    let (mut s, n) = open_prompt_at_m1();
    let revision = s.revision();
    answer(&mut s, ReplaceChoice::Replace);
    assert!(s.prompt().is_none());
    assert_eq!(children(&s, n.m1), vec![s.cursor()]);
    for gone in [n.m2, n.a1, n.m6, n.a3, n.c2] {
        assert!(!s.game().contains(gone));
    }
    assert_eq!(s.game().node_count(), 3, "root, e4, d5");
    assert_eq!((s.undo_depth(), s.is_dirty()), (1, true));
    assert_ne!(s.revision(), revision);
}

#[test]
fn answer_new_main_line_promotes_the_new_move_and_keeps_the_old_as_variation() {
    let (mut s, n) = open_prompt_at_m1();
    let count = s.game().node_count();
    answer(&mut s, ReplaceChoice::NewMainLine);
    let kids = children(&s, n.m1);
    assert_eq!(kids.len(), 3);
    assert_eq!(kids[0], s.cursor(), "new move is children[0]");
    assert_eq!(kids[1], n.m2, "old main line is the first variation");
    assert_eq!(kids[2], n.a1);
    assert_eq!(s.game().node_count(), count + 1, "nothing lost");
    assert_eq!((s.undo_depth(), s.is_dirty()), (1, true));
}

#[test]
fn answer_add_variation_appends_at_the_end() {
    let (mut s, n) = open_prompt_at_m1();
    answer(&mut s, ReplaceChoice::AddVariation);
    let kids = children(&s, n.m1);
    assert_eq!(kids[..2], [n.m2, n.a1]);
    assert_eq!(kids[2], s.cursor());
    assert_eq!((s.undo_depth(), s.is_dirty()), (1, true));
}

#[test]
fn answer_trial_enters_trial_mode_and_replaces() {
    let (mut s, n) = open_prompt_at_m1();
    answer(&mut s, ReplaceChoice::Trial);
    assert!(s.in_trial());
    assert!(s.prompt().is_none());
    assert_eq!(children(&s, n.m1), vec![s.cursor()]);
    assert_eq!(s.undo_depth(), 0);
    assert!(!s.is_dirty());
    s.update(GameMessage::ExitTrial);
    assert!(s.game().contains(n.m2), "exit restores the original");
}

#[test]
fn answer_cancel_changes_nothing_and_clears_the_entry() {
    let (mut s, n) = open_prompt_at_m1();
    let before = snapshot_of(&s);
    answer(&mut s, ReplaceChoice::Cancel);
    assert!(s.prompt().is_none());
    assert_unchanged(&s, &before);
    assert_eq!(s.cursor(), n.m1);
    assert!(s.move_entry().text().is_empty());
}

#[test]
fn dismiss_prompt_is_cancel() {
    let (mut s, _) = open_prompt_at_m1();
    let before = snapshot_of(&s);
    s.update(GameMessage::DismissPrompt);
    assert!(s.prompt().is_none());
    assert_unchanged(&s, &before);
}

#[test]
fn answer_replace_without_prompt_is_ignored() {
    let (mut s, _n) = nested_session();
    let before = snapshot_of(&s);
    for c in [
        ReplaceChoice::Replace,
        ReplaceChoice::NewMainLine,
        ReplaceChoice::AddVariation,
        ReplaceChoice::Trial,
        ReplaceChoice::Cancel,
    ] {
        answer(&mut s, c);
    }
    assert_unchanged(&s, &before);
    assert!(!s.in_trial());
}

#[rstest]
#[case(ReplaceChoice::Replace)]
#[case(ReplaceChoice::NewMainLine)]
#[case(ReplaceChoice::AddVariation)]
fn rule41_postcondition_position_is_before_after_move(#[case] choice: ReplaceChoice) {
    let (mut s, n) = open_prompt_at_m1();
    let expected = {
        let before = s.game().position_at(n.m1).unwrap().clone();
        let mv = mv_at(s.game(), n.m1, "d7d5");
        before.after(mv).unwrap()
    };
    answer(&mut s, choice);
    assert_eq!(s.position().fen(), expected.fen());
    assert_eq!(s.game().parent(s.cursor()).unwrap(), Some(n.m1));
}

#[test]
fn each_branch_is_undone_with_exactly_one_undo() {
    for choice in [
        ReplaceChoice::Replace,
        ReplaceChoice::NewMainLine,
        ReplaceChoice::AddVariation,
    ] {
        let (mut s, n) = open_prompt_at_m1();
        let original = s.game().clone();
        answer(&mut s, choice);
        s.update(GameMessage::Undo);
        assert!(s.game().same_content(&original), "{choice:?}");
        assert_eq!(s.cursor(), n.m1);
        assert!(!s.is_dirty());
    }
}

// ---- modal prompt (rules 7, D2-78) ----

#[rstest]
#[case(GameMessage::Navigate(Nav::Start))]
#[case(GameMessage::Undo)]
#[case(GameMessage::PlayNullMove)]
#[case(GameMessage::StartVariation)]
#[case(GameMessage::TruncateEnd)]
#[case(GameMessage::StripComments)]
#[case(GameMessage::MoveEntryChar('e'))]
#[case(GameMessage::OpenVariationChooser)]
#[case(GameMessage::ClearNags)]
#[case(GameMessage::EnterTrial { null_move: false })]
fn messages_other_than_answers_are_blocked_by_an_open_prompt(#[case] message: GameMessage) {
    let (mut s, _) = open_prompt_at_m1();
    let before = snapshot_of(&s);
    let prompt = s.prompt().cloned();
    s.update(message);
    assert_eq!(s.notice(), Some(&Notice::PromptPending));
    assert_unchanged(&s, &before);
    assert_eq!(s.prompt().cloned(), prompt);
    assert!(!s.in_trial());
}

#[test]
fn apply_settings_is_accepted_while_a_prompt_is_open() {
    let (mut s, _) = open_prompt_at_m1();
    s.update(GameMessage::ApplySettings(settings(|e| {
        e.exclusive_nags = false;
    })));
    assert!(!s.settings().exclusive_nags);
    assert!(s.prompt().is_some());
    assert_ne!(s.notice(), Some(&Notice::PromptPending));
}

#[test]
fn autoplay_tick_with_a_prompt_open_is_dropped_silently() {
    let (mut s, _) = open_prompt_at_m1();
    let effects = s.update(GameMessage::AutoplayTick {
        generation: s.autoplay().generation,
    });
    assert!(effects.is_empty());
    assert_ne!(s.notice(), Some(&Notice::PromptPending));
}

// ---- StartVariation (rules 37, 42, D2-80) ----

#[test]
fn start_variation_in_the_middle_arms_the_cursor_without_touching_history() {
    let (mut s, m1, _) = at_e4();
    s.update(GameMessage::StartVariation);
    assert_eq!(s.pending_variation(), Some(m1));
    assert_eq!(s.cursor(), m1);
    assert_eq!(s.undo_depth(), 0);
    assert!(!s.is_dirty());
}

#[test]
fn start_variation_at_a_leaf_steps_back_and_arms_the_parent() {
    let mut s = GameSession::new(line(&["e2e4", "e7e5"]));
    nav(&mut s, Nav::GameEnd);
    let leaf = s.cursor();
    let parent = s.game().parent(leaf).unwrap().unwrap();
    s.update(GameMessage::StartVariation);
    assert_eq!(s.cursor(), parent);
    assert_eq!(s.pending_variation(), Some(parent));
    assert!(!s.is_dirty());
    play(&mut s, "c7c5");
    assert_eq!(children(&s, parent).len(), 2, "variation of the last move");
    assert_eq!(children(&s, parent)[0], leaf);
}

#[test]
fn start_variation_on_an_empty_game_reports_no_move_here() {
    let mut s = GameSession::new(Game::new());
    s.update(GameMessage::StartVariation);
    assert_eq!(s.notice(), Some(&Notice::NoMoveHere));
    assert_eq!(s.pending_variation(), None);
}

#[test]
fn armed_variation_makes_the_next_new_move_a_variation_without_prompt() {
    let (mut s, m1, m2) = at_e4();
    s.update(GameMessage::StartVariation);
    play(&mut s, "c7c5");
    assert!(s.prompt().is_none());
    assert_eq!(s.pending_variation(), None, "consumed");
    let kids = children(&s, m1);
    assert_eq!(kids.len(), 2);
    assert_eq!(kids[0], m2);
    assert_eq!(s.cursor(), kids[1]);
    assert_eq!((s.undo_depth(), s.is_dirty()), (1, true));
}

#[test]
fn armed_variation_with_an_existing_move_navigates_and_disarms() {
    let (mut s, m1, m2) = at_e4();
    s.update(GameMessage::StartVariation);
    play(&mut s, "e7e5");
    assert_eq!(s.cursor(), m2);
    assert_eq!(s.pending_variation(), None);
    assert_eq!(children(&s, m1), vec![m2]);
    assert_eq!(s.undo_depth(), 0);
}

#[test]
fn armed_variation_is_disarmed_by_navigation_undo_and_edit() {
    let (mut s, m1, _) = at_e4();
    s.update(GameMessage::StartVariation);
    nav(&mut s, Nav::Back(1));
    assert_eq!(s.pending_variation(), None);

    goto(&mut s, m1);
    s.update(GameMessage::SetComment {
        node: m1,
        text: "c".into(),
    });
    s.update(GameMessage::StartVariation);
    s.update(GameMessage::Undo);
    assert_eq!(s.pending_variation(), None);

    goto(&mut s, m1);
    s.update(GameMessage::StartVariation);
    s.update(GameMessage::SetComment {
        node: m1,
        text: "d".into(),
    });
    assert_eq!(s.pending_variation(), None);
}

#[test]
fn armed_variation_does_not_apply_after_it_was_disarmed() {
    let (mut s, m1, _) = at_e4();
    s.update(GameMessage::StartVariation);
    nav(&mut s, Nav::Forward(0));
    // Forward(0) is a no-op; the arm may survive. Use a real move:
    nav(&mut s, Nav::Start);
    nav(&mut s, Nav::GoToNode(m1));
    play(&mut s, "c7c5");
    assert!(
        matches!(s.prompt(), Some(Prompt::ReplaceMove(_))),
        "back to the normal table"
    );
}

// ---- no effects (rule 43) ----

#[test]
fn playing_moves_emits_no_effects() {
    let (mut s, m1, _) = at_e4();
    let mv = mv_at(s.game(), m1, "c7c5");
    let effects = s.update(GameMessage::PlayMove {
        mv,
        options: PlayOptions::default(),
    });
    assert!(effects.is_empty());
    let effects = s.update(GameMessage::AnswerReplace(ReplaceChoice::AddVariation));
    assert!(effects.is_empty());
}

#[test]
fn stale_variation_target_after_replace_is_reported() {
    let (mut s, n) = open_prompt_at_m1();
    answer(&mut s, ReplaceChoice::Replace);
    s.update(GameMessage::DeleteVariation(VariationTarget::Node(n.a1)));
    assert_eq!(s.notice(), Some(&Notice::StaleNode));
}
