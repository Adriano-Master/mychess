//! Basic unit tests of the session. The extensive suite is derived from the
//! specification by the tester.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::assert_is_empty)]

use std::time::Duration;

use mychess_core::{Game, Move, Nag, NodeId, Position, Role, Square};

use super::*;

/// A game with the main line `ucis`, from the standard start.
fn line(ucis: &[&str]) -> Game {
    let mut game = Game::new();
    let mut cur = game.root();
    for uci in ucis {
        let mv = game.position_at(cur).unwrap().parse_uci(uci).unwrap();
        cur = game.add_move(cur, mv).unwrap();
    }
    game
}

fn uci_at(game: &Game, node: NodeId, uci: &str) -> Move {
    game.position_at(node).unwrap().parse_uci(uci).unwrap()
}

fn send(session: &mut GameSession, message: GameMessage) -> Vec<Effect> {
    session.update(message)
}

fn nav(session: &mut GameSession, nav: Nav) {
    session.update(GameMessage::Navigate(nav));
}

fn play(session: &mut GameSession, uci: &str) {
    let mv = uci_at(session.game(), session.cursor(), uci);
    session.update(GameMessage::PlayMove {
        mv,
        options: PlayOptions::default(),
    });
}

/// 1.e4 e5 2.Nf3 (2.Bc4 Nf6 (2...Nc6) ) 2...Nc6 : variations on two levels.
fn nested() -> (GameSession, [NodeId; 3]) {
    let mut s = GameSession::new(line(&["e2e4", "e7e5", "g1f3", "b8c6"]));
    nav(&mut s, Nav::GoToMainlinePly(2));
    play(&mut s, "f1c4"); // variation 2.Bc4 (asks nothing? ask is on)
    // The prompt is open: answer with a variation.
    s.update(GameMessage::AnswerReplace(ReplaceChoice::AddVariation));
    let bc4 = s.cursor();
    play(&mut s, "g8f6");
    let nf6 = s.cursor();
    nav(&mut s, Nav::Back(1));
    play(&mut s, "b8c6");
    s.update(GameMessage::AnswerReplace(ReplaceChoice::AddVariation));
    let nc6 = s.cursor();
    (s, [bc4, nf6, nc6])
}

#[test]
fn new_session_starts_clean() {
    let s = GameSession::new(Game::new());
    assert_eq!(s.cursor(), s.game().root());
    assert!(!s.is_dirty());
    assert!(!s.can_undo() && !s.can_redo() && !s.in_trial());
    assert!(s.prompt().is_none() && s.chooser().is_none());
    assert!(s.move_entry().text().is_empty());
    assert!(!s.autoplay().running);
    assert_eq!(s.autoplay().delay, Duration::from_secs(5));
}

#[test]
fn back_stops_at_head_of_variation() {
    let (mut s, [bc4, nf6, _]) = nested();
    let origin = s.game().parent(bc4).unwrap().unwrap();
    nav(&mut s, Nav::GoToNode(nf6));
    nav(&mut s, Nav::Back(5));
    assert_eq!(s.cursor(), origin);
    nav(&mut s, Nav::Back(5));
    assert_eq!(s.game().ply(s.cursor()).unwrap(), 0);
}

#[test]
fn home_and_end_work_in_two_steps() {
    let (mut s, [bc4, nf6, _]) = nested();
    nav(&mut s, Nav::GoToNode(bc4));
    nav(&mut s, Nav::End);
    assert_eq!(s.cursor(), nf6);
    nav(&mut s, Nav::End);
    let leaf = s.game().mainline().last().unwrap();
    assert_eq!(s.cursor(), leaf);
    nav(&mut s, Nav::End);
    assert_eq!(s.cursor(), leaf);
    nav(&mut s, Nav::GoToNode(nf6));
    nav(&mut s, Nav::Home);
    assert_eq!(s.game().ply(s.cursor()).unwrap(), 2);
    assert!(!s.game().is_variation(s.cursor()).unwrap());
}

#[test]
fn go_to_move_number_standard_and_black_to_move_fen() {
    let mut s = GameSession::new(line(&["e2e4", "e7e5", "g1f3", "b8c6"]));
    nav(&mut s, Nav::GoToMoveNumber(2));
    assert_eq!(s.game().ply(s.cursor()).unwrap(), 3);
    nav(&mut s, Nav::GoToBlackMoveNumber(1));
    assert_eq!(s.game().ply(s.cursor()).unwrap(), 2);
    nav(&mut s, Nav::GoToMoveNumber(99));
    assert_eq!(s.game().ply(s.cursor()).unwrap(), 4);

    let pos = Position::from_fen("4k3/8/8/8/8/8/4P3/4K3 b - - 0 7").unwrap();
    let mut game = Game::from_position(pos);
    let mv = uci_at(&game, game.root(), "e8e7");
    game.add_move(game.root(), mv).unwrap();
    let mut s = GameSession::new(game);
    let first = s.game().mainline().next().unwrap();
    nav(&mut s, Nav::GoToNode(first));
    nav(&mut s, Nav::GoToMoveNumber(7));
    assert_eq!(s.cursor(), s.game().root());
}

#[test]
fn chooser_selects_and_confirms() {
    let (mut s, [bc4, ..]) = nested();
    nav(&mut s, Nav::GoToMainlinePly(2));
    send(&mut s, GameMessage::OpenVariationChooser);
    assert_eq!(s.chooser().unwrap().selected, 0);
    send(&mut s, GameMessage::ChooserMove(-1));
    assert_eq!(s.chooser().unwrap().selected, 0);
    send(&mut s, GameMessage::ChooserMove(5));
    assert_eq!(s.chooser().unwrap().selected, 1);
    let choices = s.variation_choices();
    assert_eq!(choices.len(), 2);
    assert_eq!(choices[1].san, vec!["Bc4", "Nf6"]);
    send(&mut s, GameMessage::ChooserConfirm(None));
    assert_eq!(s.cursor(), bc4);
    assert!(s.chooser().is_none());
}

#[test]
fn forward_opens_chooser_only_with_show_var_popup() {
    let (mut s, _) = nested();
    nav(&mut s, Nav::GoToMainlinePly(2));
    nav(&mut s, Nav::Forward(1));
    assert!(s.chooser().is_none());
    nav(&mut s, Nav::Back(1));
    let settings = EditSettings {
        show_var_popup: true,
        ..EditSettings::default()
    };
    send(&mut s, GameMessage::ApplySettings(settings));
    nav(&mut s, Nav::GoToMainlinePly(2));
    let at = s.cursor();
    nav(&mut s, Nav::Forward(1));
    assert!(s.chooser().is_some());
    assert_eq!(s.cursor(), at);
}

#[test]
fn autoplay_steps_schedules_and_stops_at_leaf() {
    let mut s = GameSession::new(line(&["e2e4", "e7e5"]));
    let fx = send(&mut s, GameMessage::StartAutoplay);
    assert_eq!(s.game().ply(s.cursor()).unwrap(), 1);
    let generation = s.autoplay().generation;
    assert_eq!(
        fx,
        vec![Effect::ScheduleAutoplayTick {
            generation,
            delay: Duration::from_secs(5)
        }]
    );
    // A stale tick is ignored.
    assert!(
        send(
            &mut s,
            GameMessage::AutoplayTick {
                generation: generation - 1
            }
        )
        .is_empty()
    );
    assert_eq!(s.game().ply(s.cursor()).unwrap(), 1);
    let fx = send(&mut s, GameMessage::AutoplayTick { generation });
    assert!(fx.is_empty());
    assert!(!s.autoplay().running);
    assert_eq!(s.game().ply(s.cursor()).unwrap(), 2);
}

#[test]
fn autoplay_delay_is_clamped() {
    let mut s = GameSession::default();
    send(
        &mut s,
        GameMessage::SetAutoplayDelay(Duration::from_millis(1)),
    );
    assert_eq!(s.autoplay().delay, MIN_AUTOPLAY_DELAY);
    send(
        &mut s,
        GameMessage::SetAutoplayDelay(Duration::from_secs(9999)),
    );
    assert_eq!(s.autoplay().delay, MAX_AUTOPLAY_DELAY);
}

#[test]
fn playing_at_leaf_appends_and_marks_dirty() {
    let mut s = GameSession::default();
    play(&mut s, "e2e4");
    assert!(s.is_dirty());
    assert_eq!(s.undo_depth(), 1);
    assert_eq!(s.game().ply(s.cursor()).unwrap(), 1);
}

#[test]
fn existing_move_only_navigates() {
    let mut s = GameSession::new(line(&["e2e4", "e7e5"]));
    play(&mut s, "e2e4");
    assert_eq!(s.game().ply(s.cursor()).unwrap(), 1);
    assert!(!s.is_dirty());
    assert_eq!(s.undo_depth(), 0);
}

#[test]
fn illegal_move_sets_notice_and_changes_nothing() {
    let mut s = GameSession::default();
    let mv = Move::Normal {
        role: Role::Pawn,
        from: Square::E2,
        to: Square::E5,
        capture: None,
        promotion: None,
    };
    send(
        &mut s,
        GameMessage::PlayMove {
            mv,
            options: PlayOptions::default(),
        },
    );
    assert_eq!(s.notice(), Some(&Notice::IllegalMove));
    assert!(!s.is_dirty());
}

#[test]
fn new_move_with_continuation_asks_and_replace_discards() {
    let mut s = GameSession::new(line(&["e2e4", "e7e5", "g1f3"]));
    nav(&mut s, Nav::GoToMainlinePly(1));
    play(&mut s, "e7e6");
    assert!(matches!(s.prompt(), Some(Prompt::ReplaceMove(_))));
    assert!(!s.is_dirty());
    // While the prompt is open, other messages are refused.
    send(&mut s, GameMessage::Navigate(Nav::Start));
    assert_eq!(s.notice(), Some(&Notice::PromptPending));
    send(&mut s, GameMessage::AnswerReplace(ReplaceChoice::Replace));
    assert!(s.prompt().is_none());
    let parent = s.game().parent(s.cursor()).unwrap().unwrap();
    assert_eq!(s.game().children(parent).unwrap(), &[s.cursor()]);
    assert_eq!(s.game().node_count(), 3);
    assert_eq!(s.undo_depth(), 1);
}

#[test]
fn new_main_line_demotes_old_move() {
    let mut s = GameSession::new(line(&["e2e4", "e7e5"]));
    nav(&mut s, Nav::GoToMainlinePly(1));
    let old = s.game().children(s.cursor()).unwrap()[0];
    play(&mut s, "c7c5");
    send(
        &mut s,
        GameMessage::AnswerReplace(ReplaceChoice::NewMainLine),
    );
    let parent = s.game().parent(s.cursor()).unwrap().unwrap();
    assert_eq!(s.game().children(parent).unwrap(), &[s.cursor(), old]);
}

#[test]
fn cancel_leaves_everything_untouched() {
    let mut s = GameSession::new(line(&["e2e4", "e7e5"]));
    nav(&mut s, Nav::GoToMainlinePly(1));
    play(&mut s, "c7c5");
    send(&mut s, GameMessage::AnswerReplace(ReplaceChoice::Cancel));
    assert!(s.prompt().is_none());
    assert!(!s.is_dirty());
    assert_eq!(s.undo_depth(), 0);
}

#[test]
fn start_variation_at_leaf_backs_up_and_next_move_is_variation() {
    let mut s = GameSession::new(line(&["e2e4", "e7e5"]));
    nav(&mut s, Nav::GameEnd);
    send(&mut s, GameMessage::StartVariation);
    assert_eq!(s.game().ply(s.cursor()).unwrap(), 1);
    assert_eq!(s.pending_variation(), Some(s.cursor()));
    play(&mut s, "c7c5");
    assert!(s.prompt().is_none());
    assert!(s.pending_variation().is_none());
    assert!(s.game().is_variation(s.cursor()).unwrap());
}

#[test]
fn promotion_prompt_and_answer() {
    let pos = Position::from_fen("8/P3k3/8/8/8/8/8/4K3 w - - 0 1").unwrap();
    let mut s = GameSession::new(Game::from_position(pos));
    send(
        &mut s,
        GameMessage::PlaySquares {
            from: Square::A7,
            to: Square::A8,
            promotion: None,
            options: PlayOptions::default(),
        },
    );
    assert!(matches!(s.prompt(), Some(Prompt::ChoosePromotion { .. })));
    send(&mut s, GameMessage::AnswerPromotion(Some(Role::Knight)));
    assert!(s.prompt().is_none());
    let mv = s.game().mv(s.cursor()).unwrap();
    assert_eq!(mv.promotion(), Some(Role::Knight));
}

#[test]
fn king_takes_king_is_a_null_move_and_refused_in_check() {
    let pos = Position::from_fen("8/8/8/8/8/3k4/8/3K4 w - - 0 1").unwrap();
    let mut s = GameSession::new(Game::from_position(pos));
    send(
        &mut s,
        GameMessage::PlaySquares {
            from: Square::D1,
            to: Square::D3,
            promotion: None,
            options: PlayOptions::default(),
        },
    );
    assert_eq!(s.game().mv(s.cursor()).unwrap(), Move::Null);

    let pos = Position::from_fen("4k3/8/8/8/8/8/4r3/4K3 w - - 0 1").unwrap();
    let mut s = GameSession::new(Game::from_position(pos));
    send(&mut s, GameMessage::PlayNullMove);
    assert_eq!(s.notice(), Some(&Notice::NullMoveInCheck));
    assert!(!s.is_dirty());
}

fn texts(s: &GameSession) -> Vec<String> {
    let mut v: Vec<String> = s
        .move_entry()
        .candidates()
        .iter()
        .map(|c| c.text.clone())
        .collect();
    v.sort();
    v
}

fn type_str(s: &mut GameSession, text: &str) {
    for ch in text.chars() {
        s.update(GameMessage::MoveEntryChar(ch));
    }
}

#[test]
fn keyboard_entry_candidates_in_start_position() {
    let mut s = GameSession::default();
    type_str(&mut s, "e");
    assert_eq!(texts(&s), vec!["e2e3", "e2e4", "e3", "e4"]);
    type_str(&mut s, "x-+9");
    assert_eq!(s.move_entry().text(), "e");
    type_str(&mut s, "O");
    assert_eq!(s.move_entry().text(), "e");
    s.update(GameMessage::MoveEntryClear);
    type_str(&mut s, "O");
    assert!(s.move_entry().candidates().is_empty());
}

#[test]
fn keyboard_entry_plays_exact_match_and_auto_expand() {
    let mut s = GameSession::default();
    type_str(&mut s, "e4");
    assert_eq!(s.game().ply(s.cursor()).unwrap(), 1);
    assert!(s.move_entry().text().is_empty());

    let mut s = GameSession::default();
    let settings = EditSettings {
        move_entry: MoveEntrySettings {
            auto_expand: true,
            ..MoveEntrySettings::default()
        },
        ..EditSettings::default()
    };
    s.update(GameMessage::ApplySettings(settings));
    type_str(&mut s, "nf");
    assert_eq!(s.game().ply(s.cursor()).unwrap(), 1);
}

#[test]
fn keyboard_entry_backspace_commit_and_clear() {
    let mut s = GameSession::default();
    type_str(&mut s, "e2");
    s.update(GameMessage::MoveEntryBackspace);
    assert_eq!(s.move_entry().text(), "e");
    s.update(GameMessage::MoveEntryCommit); // 4 candidates: nothing
    assert_eq!(s.game().ply(s.cursor()).unwrap(), 0);
    type_str(&mut s, "4");
    assert_eq!(s.game().ply(s.cursor()).unwrap(), 1);
}

#[test]
fn castling_candidates() {
    let pos = Position::from_fen("4k3/8/8/8/8/8/8/R3K2R w KQ - 0 1").unwrap();
    let c = entry::candidates_for(&pos, "O", false);
    let mut t: Vec<_> = c.iter().map(|c| c.text.as_str()).collect();
    t.sort_unstable();
    assert_eq!(t, vec!["OK", "OO", "OQ"]);

    // Only long castling is legal: it also answers to `OO`.
    let pos = Position::from_fen("4k3/8/8/8/8/8/8/R3K3 w Q - 0 1").unwrap();
    let c = entry::candidates_for(&pos, "O", false);
    let mut t: Vec<_> = c.iter().map(|c| c.text.as_str()).collect();
    t.sort_unstable();
    assert_eq!(t, vec!["OO", "OQ"]);
}

#[test]
fn b_pawn_and_bishop_capture_are_told_apart_by_case() {
    let fen = "4k3/8/8/8/2p5/1P6/8/4KB2 w - - 0 1";
    let mut s = GameSession::new(Game::from_position(Position::from_fen(fen).unwrap()));
    type_str(&mut s, "bc");
    assert_eq!(s.move_entry().candidates().len(), 2);
    type_str(&mut s, "4");
    let mv = s.game().mv(s.cursor()).unwrap();
    assert_eq!(mv.role(), Some(Role::Pawn));

    let mut s = GameSession::new(Game::from_position(Position::from_fen(fen).unwrap()));
    type_str(&mut s, "Bc4");
    let mv = s.game().mv(s.cursor()).unwrap();
    assert_eq!(mv.role(), Some(Role::Bishop));
}

#[test]
fn enter_move_text_reads_san_uci_and_rejects_garbage() {
    let mut s = GameSession::default();
    send(&mut s, GameMessage::EnterMoveText("e4".into()));
    send(&mut s, GameMessage::EnterMoveText("e7e5".into()));
    send(&mut s, GameMessage::EnterMoveText("Nf3+".into()));
    assert_eq!(s.game().ply(s.cursor()).unwrap(), 3);
    send(&mut s, GameMessage::EnterMoveText("zzz".into()));
    assert!(matches!(s.notice(), Some(Notice::UnreadableMove(_))));
    send(
        &mut s,
        GameMessage::EnterMoveText("\u{0}\u{202e}é".repeat(100)),
    );
    assert!(matches!(s.notice(), Some(Notice::UnreadableMove(_))));
}

#[test]
fn variation_operations() {
    let (mut s, [bc4, nf6, _]) = nested();
    nav(&mut s, Nav::GoToNode(nf6));
    send(
        &mut s,
        GameMessage::PromoteVariation(VariationTarget::Node(nf6)),
    );
    assert_eq!(s.notice(), Some(&Notice::NotAVariation));
    send(
        &mut s,
        GameMessage::PromoteVariation(VariationTarget::Current),
    );
    let parent = s.game().parent(bc4).unwrap().unwrap();
    assert_eq!(s.game().children(parent).unwrap()[0], bc4);
    send(
        &mut s,
        GameMessage::DeleteVariation(VariationTarget::Current),
    );
    assert_eq!(s.notice(), Some(&Notice::NotInVariation));
}

#[test]
fn delete_variation_current_moves_cursor_out() {
    let (mut s, [bc4, nf6, _]) = nested();
    nav(&mut s, Nav::GoToNode(nf6));
    send(
        &mut s,
        GameMessage::DeleteVariation(VariationTarget::Current),
    );
    assert!(!s.game().contains(bc4));
    assert_eq!(s.game().ply(s.cursor()).unwrap(), 2);
}

#[test]
fn truncate_end_start_and_from() {
    let mut s = GameSession::new(line(&["e2e4", "e7e5", "g1f3"]));
    nav(&mut s, Nav::GoToMainlinePly(2));
    send(&mut s, GameMessage::TruncateEnd);
    assert_eq!(s.game().node_count(), 3);
    send(&mut s, GameMessage::TruncateStart);
    assert_eq!(s.cursor(), s.game().root());
    assert_eq!(s.game().node_count(), 1);
    send(&mut s, GameMessage::Undo);
    assert_eq!(s.game().node_count(), 3);
    assert_eq!(s.game().ply(s.cursor()).unwrap(), 2);
    send(&mut s, GameMessage::TruncateFrom);
    assert_eq!(s.game().node_count(), 2);
    assert_eq!(s.game().ply(s.cursor()).unwrap(), 1);
}

#[test]
fn strip_variations_moves_cursor_to_nearest_live_ancestor() {
    let (mut s, [bc4, nf6, _]) = nested();
    nav(&mut s, Nav::GoToNode(nf6));
    send(&mut s, GameMessage::StripVariations);
    assert!(!s.game().contains(bc4));
    assert_eq!(s.game().ply(s.cursor()).unwrap(), 2);
}

#[test]
fn comments_and_nags() {
    let mut s = GameSession::new(line(&["e2e4"]));
    nav(&mut s, Nav::Forward(1));
    let node = s.cursor();
    send(
        &mut s,
        GameMessage::SetComment {
            node,
            text: "good [%clk 1]".into(),
        },
    );
    assert_eq!(s.game().comment(node).unwrap(), Some("good [%clk 1]"));
    let depth = s.undo_depth();
    send(
        &mut s,
        GameMessage::SetComment {
            node,
            text: "good [%clk 1]".into(),
        },
    );
    assert_eq!(s.undo_depth(), depth);
    let root = s.game().root();
    send(
        &mut s,
        GameMessage::SetCommentBefore {
            node: root,
            text: "x".into(),
        },
    );
    assert_eq!(s.notice(), Some(&Notice::NoMoveHere));

    let n = |v| Nag::new(v).unwrap();
    send(&mut s, GameMessage::AddNag(n(1)));
    send(&mut s, GameMessage::AddNag(n(4)));
    assert_eq!(s.game().nags(node).unwrap(), &[n(4)]);
    send(&mut s, GameMessage::ToggleNag(n(4)));
    assert!(s.game().nags(node).unwrap().is_empty());
    send(&mut s, GameMessage::SetNags(vec![n(1), n(2), n(1)]));
    assert_eq!(s.game().nags(node).unwrap(), &[n(1), n(2)]);

    let mut s2 = GameSession::new(line(&["e2e4"]));
    nav(&mut s2, Nav::Forward(1));
    let settings = EditSettings {
        exclusive_nags: false,
        ..EditSettings::default()
    };
    send(&mut s2, GameMessage::ApplySettings(settings));
    send(&mut s2, GameMessage::AddNag(n(1)));
    send(&mut s2, GameMessage::AddNag(n(4)));
    assert_eq!(s2.game().nags(s2.cursor()).unwrap().len(), 2);
}

#[test]
fn nag_shortcuts_match_parser_and_list_parsing() {
    for (symbol, number) in NAG_SHORTCUTS {
        assert_eq!(
            Nag::parse(symbol).map(Nag::value),
            Some(*number),
            "{symbol}"
        );
    }
    let values: Vec<u8> = parse_nag_list("! $14 xx +-")
        .iter()
        .map(|n| n.value())
        .collect();
    assert_eq!(values, vec![1, 14, 18]);
}

#[test]
fn undo_redo_round_trip_and_dirty_flag() {
    let mut s = GameSession::default();
    play(&mut s, "e2e4");
    play(&mut s, "e7e5");
    let end = s.game().clone();
    send(&mut s, GameMessage::Undo);
    assert_eq!(s.game().ply(s.cursor()).unwrap(), 1);
    send(&mut s, GameMessage::Undo);
    assert!(!s.is_dirty());
    assert!(s.can_redo());
    send(&mut s, GameMessage::Redo);
    send(&mut s, GameMessage::Redo);
    assert!(s.game().same_content(&end));
    assert!(s.is_dirty());
    s.mark_saved();
    assert!(!s.is_dirty());
    send(&mut s, GameMessage::Undo);
    assert!(s.is_dirty());
    send(&mut s, GameMessage::Redo);
    assert!(!s.is_dirty());
}

#[test]
fn undo_depth_is_capped() {
    let mut s = GameSession::default();
    for i in 0..30 {
        let node = s.cursor();
        send(
            &mut s,
            GameMessage::SetComment {
                node,
                text: format!("c{i}"),
            },
        );
    }
    assert_eq!(s.undo_depth(), UNDO_LIMIT);
}

#[test]
fn trial_mode_reverts_and_disables_undo() {
    let mut s = GameSession::new(line(&["e2e4", "e7e5"]));
    let before = s.game().clone();
    send(&mut s, GameMessage::EnterTrial { null_move: false });
    assert!(s.in_trial());
    nav(&mut s, Nav::GoToMainlinePly(1));
    play(&mut s, "c7c5"); // replaces without asking
    assert!(s.prompt().is_none());
    assert_eq!(s.undo_depth(), 0);
    assert!(!s.is_dirty());
    send(&mut s, GameMessage::Undo);
    assert_eq!(s.notice(), Some(&Notice::UndoDisabledInTrial));
    send(&mut s, GameMessage::ExitTrial);
    assert!(!s.in_trial());
    assert!(s.game().same_content(&before));
}

#[test]
fn stale_node_never_panics() {
    let mut s = GameSession::new(line(&["e2e4"]));
    let node = s.game().mainline().next().unwrap();
    nav(&mut s, Nav::Forward(1));
    send(&mut s, GameMessage::TruncateFrom);
    assert!(!s.game().contains(node));
    nav(&mut s, Nav::GoToNode(node));
    assert_eq!(s.notice(), Some(&Notice::StaleNode));
    send(
        &mut s,
        GameMessage::SetComment {
            node,
            text: "x".into(),
        },
    );
    assert_eq!(s.notice(), Some(&Notice::StaleNode));
    send(
        &mut s,
        GameMessage::DeleteVariation(VariationTarget::Node(node)),
    );
    assert_eq!(s.notice(), Some(&Notice::StaleNode));
    assert!(s.game().contains(s.cursor()));
}

#[test]
fn replace_game_resets_session() {
    let mut s = GameSession::default();
    play(&mut s, "e2e4");
    let g = line(&["d2d4"]);
    s.replace_game(g);
    assert!(!s.is_dirty() && !s.can_undo());
    assert_eq!(s.cursor(), s.game().root());
}

#[test]
fn a_nag_toggled_at_a_node_leaves_the_cursor_alone() {
    let mut s = GameSession::new(line(&["e2e4", "e7e5", "g1f3"]));
    nav(&mut s, Nav::GameEnd);
    let cursor = s.cursor();
    let e4 = s.game().children(s.game().root()).unwrap()[0];
    let n = |v| Nag::new(v).unwrap();
    send(
        &mut s,
        GameMessage::ToggleNagAt {
            node: e4,
            nag: n(1),
        },
    );
    assert_eq!(s.game().nags(e4).unwrap(), &[n(1)]);
    assert!(s.game().nags(cursor).unwrap().is_empty());
    assert_eq!(s.cursor(), cursor, "the cursor did not move");
    // A group-mate replaces it, and the same NAG again removes it.
    send(
        &mut s,
        GameMessage::ToggleNagAt {
            node: e4,
            nag: n(2),
        },
    );
    assert_eq!(s.game().nags(e4).unwrap(), &[n(2)]);
    send(
        &mut s,
        GameMessage::ToggleNagAt {
            node: e4,
            nag: n(2),
        },
    );
    assert!(s.game().nags(e4).unwrap().is_empty());
}

#[test]
fn a_nag_toggled_at_a_dead_node_is_a_stale_node_notice() {
    let mut s = GameSession::new(line(&["e2e4", "e7e5"]));
    let other = line(&["e2e4", "e7e5", "g1f3", "b8c6", "f1b5"]);
    let far = other.mainline().last().unwrap();
    let changes = s.changes();
    send(
        &mut s,
        GameMessage::ToggleNagAt {
            node: far,
            nag: Nag::new(1).unwrap(),
        },
    );
    assert_eq!(s.notice(), Some(&Notice::StaleNode));
    assert_eq!(s.changes(), changes);
}

#[test]
fn a_typed_key_without_candidates_is_told_but_a_plain_char_is_silent() {
    let mut s = GameSession::new(Game::new());
    send(&mut s, GameMessage::MoveEntryChar('x'));
    assert!(s.notice().is_none(), "the field's keystroke stays silent");
    send(&mut s, GameMessage::MoveEntryTyped('x'));
    assert_eq!(s.notice(), Some(&Notice::MoveEntryNoMatch("x".to_owned())));
    assert_eq!(s.move_entry().text(), "");
    // With text in the buffer the notice names the whole attempt.
    send(&mut s, GameMessage::MoveEntryTyped('e'));
    assert_eq!(s.move_entry().text(), "e");
    send(&mut s, GameMessage::MoveEntryTyped('5'));
    assert_eq!(s.notice(), Some(&Notice::MoveEntryNoMatch("e5".to_owned())));
    assert_eq!(s.move_entry().text(), "e", "the buffer did not change");
    // A key that is accepted, or no key at all, tells nothing.
    let mut quiet = GameSession::new(Game::new());
    send(&mut quiet, GameMessage::MoveEntryTyped('e'));
    send(&mut quiet, GameMessage::MoveEntryTyped(' '));
    assert!(quiet.notice().is_none());
    // A move completed by the key is not a refusal either.
    send(&mut quiet, GameMessage::MoveEntryTyped('4'));
    assert_eq!(quiet.game().node_count(), 2);
    assert!(quiet.notice().is_none());
}
