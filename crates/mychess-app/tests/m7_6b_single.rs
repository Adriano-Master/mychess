//! Contract tests of the merge of one game (spec `m7-6-mesclar-partida.md`,
//! slice b: criteria 11 to 13 and 15, D3850-D3868).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::too_many_lines)]
mod m7_6b_common;

use m7_6b_common::*;
use mychess_app::online::MarkerInfo;
use mychess_app::{MergePrefs, PrefChange, PrefsMessage};

const ENGLISH_B: &str = "Alfa, A -- Beta, B 1-0\nAberto, 2020";

#[test]
fn open_only_asks_for_the_games_and_does_not_open_the_dialog() {
    let (mut rig, slot, _dir) = sample(A);
    let effects = rig.raw(MergeMessage::Open { slot, number: B });
    assert!(
        matches!(effects.as_slice(), [Effect::LoadGamesForMerge { numbers, .. }] if *numbers == vec![B]),
        "{effects:?}"
    );
    assert!(rig.state.merge_view().is_none());
}

#[test]
fn the_dialog_previews_the_variation_and_apply_merges_in_one_undo_step() {
    let (mut rig, slot, _dir) = sample(A);
    let before = rig.state.session.game().clone();
    let depth = rig.state.session.undo_depth();
    rig.open(slot, B);
    {
        let view = rig.state.merge_view().expect("dialog");
        assert!(!view.batch);
        assert_eq!(view.end_ply, Some(80));
        let report = view.report.expect("report");
        assert_eq!((report.match_ply, report.other_ply), (4, 4));
        assert_eq!((report.added, report.reused), (4, 0));
        assert!(!report.extended_mainline && !report.truncated);
        assert!(view.is_new(report.first));
        assert!(!view.is_new(view.original.root()));
        assert_eq!(view.english_comment().as_deref(), Some(ENGLISH_B));
    }
    // The game of the editor is untouched while the dialog is open.
    assert!(rig.state.session.game().same_content(&before));
    rig.apply();
    assert!(rig.state.merge_view().is_none());
    assert_eq!(rig.nodes(), 1 + 6 + 4);
    assert!(rig.state.session.is_dirty());
    let game = rig.state.session.game();
    let nc6 = game.mainline().nth(3).unwrap();
    let variant = game.children(nc6).unwrap().len();
    assert_eq!(variant, 2, "3.Bb5 and 3.Bc4 share the parent");
    // One step of the undo.
    assert_eq!(rig.state.session.undo_depth(), depth + 1);
    rig.undo();
    assert!(rig.state.session.game().same_content(&before));
}

#[test]
fn the_comment_goes_before_the_first_merged_move_and_can_be_translated() {
    let (mut rig, slot, _dir) = sample(A);
    rig.open(slot, B);
    rig.send(MergeMessage::Apply {
        comment: Some("Alfa, A -- Beta, B 1-0 (4 lances)".to_owned()),
    });
    let game = rig.state.session.game();
    let first = game
        .mainline()
        .nth(3)
        .and_then(|n| game.children(n).ok())
        .and_then(|c| c.get(1).copied())
        .expect("variation");
    assert_eq!(
        game.comment_before(first).unwrap(),
        Some("Alfa, A -- Beta, B 1-0 (4 lances)")
    );
}

#[test]
fn the_neutral_english_comment_is_used_without_a_translation() {
    let (mut rig, slot, _dir) = sample(A);
    rig.open(slot, B);
    rig.apply();
    let game = rig.state.session.game();
    let first = game
        .mainline()
        .nth(3)
        .and_then(|n| game.children(n).ok())
        .and_then(|c| c.get(1).copied())
        .expect("variation");
    assert_eq!(game.comment_before(first).unwrap(), Some(ENGLISH_B));
}

#[test]
fn an_empty_comment_adds_none() {
    let (mut rig, slot, _dir) = sample(A);
    rig.open(slot, B);
    rig.send(MergeMessage::Apply {
        comment: Some(String::new()),
    });
    let game = rig.state.session.game();
    assert!(
        game.descendants(game.root())
            .unwrap()
            .all(|n| game.comment_before(n).unwrap().is_none())
    );
}

#[test]
fn set_end_ply_recomputes_the_preview_without_effects_and_is_remembered() {
    let (mut rig, slot, _dir) = sample(A);
    rig.open(slot, B);
    let effects = rig.raw(MergeMessage::SetEndPly(Some(6)));
    assert!(
        effects
            .iter()
            .all(|e| !matches!(e, Effect::LoadGamesForMerge { .. }))
    );
    let view = rig.state.merge_view().expect("dialog");
    let report = view.report.unwrap();
    assert_eq!(report.added, 2);
    assert!(report.truncated && view.truncated());
    assert_eq!(view.end_ply, Some(6));
    assert_eq!(
        view.english_comment().as_deref(),
        Some("Alfa, A -- Beta, B 1-0 (4 moves)\nAberto, 2020")
    );
    assert_eq!(rig.state.prefs.merge.end_ply, 6);
    // "All".
    rig.raw(MergeMessage::SetEndPly(None));
    let view = rig.state.merge_view().expect("dialog");
    assert_eq!(view.report.unwrap().added, 4);
    assert_eq!(rig.state.prefs.merge.end_ply, 0);
}

#[test]
fn the_dialog_starts_with_the_remembered_end_ply() {
    let (mut rig, slot, _dir) = sample(A);
    rig.send(PrefsMessage::Set(PrefChange::Merge(MergePrefs {
        end_ply: 0,
        confirm_batch: true,
    })));
    rig.open(slot, B);
    assert_eq!(rig.state.merge_view().unwrap().end_ply, None);
}

#[test]
fn cancel_closes_the_dialog_and_changes_nothing() {
    let (mut rig, slot, _dir) = sample(A);
    let before = rig.state.session.game().clone();
    rig.open(slot, B);
    rig.send(MergeMessage::Cancel);
    assert!(rig.state.merge_view().is_none());
    assert!(rig.state.session.game().same_content(&before));
    assert!(!rig.state.session.is_dirty());
}

#[test]
fn cancel_while_reading_drops_the_answer() {
    let (mut rig, slot, _dir) = sample(A);
    let effects = rig.raw(MergeMessage::Open { slot, number: B });
    rig.raw(MergeMessage::Cancel);
    let answer = rig.perform(effects.into_iter().next().unwrap());
    rig.raw(answer);
    assert!(rig.state.merge_view().is_none());
}

#[test]
fn an_old_answer_is_dropped_by_its_request() {
    let (mut rig, slot, _dir) = sample(A);
    let first = rig.raw(MergeMessage::Open { slot, number: B });
    let second = rig.raw(MergeMessage::Open { slot, number: D });
    let old = rig.perform(first.into_iter().next().unwrap());
    rig.raw(old);
    assert!(rig.state.merge_view().is_none(), "the old answer was used");
    let new = rig.perform(second.into_iter().next().unwrap());
    rig.raw(new);
    let view = rig.state.merge_view().expect("dialog");
    assert_eq!(view.numbers.iter().collect::<Vec<_>>(), vec![D]);
}

#[test]
fn quit_cancels_the_reading() {
    let (mut rig, slot, _dir) = sample(A);
    let effects = rig.raw(MergeMessage::Open { slot, number: B });
    rig.raw(Message::Quit);
    let answer = rig.perform(effects.into_iter().next().unwrap());
    rig.raw(answer);
    assert!(rig.state.merge_view().is_none());
}

/// Two bases: the one with the source games is opened first, so the editor
/// ends up with game 1 of the second one. Returns `(rig, source, slot)`.
fn two_bases() -> (Rig, SlotId, SlotId, [tempfile::TempDir; 2]) {
    let first = tempfile::tempdir().unwrap();
    let second = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    let source = rig.open_sample(first.path());
    let slot = rig.open_sample(second.path());
    assert_ne!(source, slot);
    rig.load(slot, A);
    (rig, source, slot, [first, second])
}

#[test]
fn closing_the_source_base_drops_the_reading() {
    let (mut rig, source, _slot, _dirs) = two_bases();
    let effects = rig.raw(MergeMessage::Open {
        slot: source,
        number: B,
    });
    rig.raw(BasesMessage::Close(source));
    let answer = rig.perform(effects.into_iter().next().unwrap());
    rig.raw(answer);
    assert!(rig.state.merge_view().is_none());
    assert!(!rig.state.session.is_dirty());
}

#[test]
fn a_dialog_open_keeps_its_base_from_closing() {
    // The dialog is modal: the base cannot be closed under it.
    let (mut rig, source, _slot, _dirs) = two_bases();
    rig.open(source, B);
    assert!(rig.state.merge_view().is_some());
    rig.send(BasesMessage::Close(source));
    assert!(rig.state.bases.slot(source).is_some());
    assert!(rig.state.merge_view().is_some());
    rig.send(MergeMessage::Cancel);
    rig.send(BasesMessage::Close(source));
    assert!(rig.state.bases.slot(source).is_none());
}

#[test]
fn a_game_of_another_base_with_the_same_number_is_not_the_same_game() {
    let (mut rig, source, _slot, _dirs) = two_bases();
    rig.open(source, A);
    assert!(rig.state.merge_view().is_none());
    assert_eq!(rig.notice(), Some(MergeNotice::NothingToAdd));
}

#[test]
fn refusals_name_the_reason_and_open_nothing() {
    let (mut rig, slot, _dir) = sample(A);
    // Number out of the database: no effect at all.
    for number in [0, 99] {
        let effects = rig.raw(MergeMessage::Open { slot, number });
        assert_eq!(effects, vec![]);
        assert_eq!(rig.notice(), Some(MergeNotice::InvalidNumber));
    }
    // A slot that is not open.
    let closed = SlotId::new(5).unwrap();
    let effects = rig.raw(MergeMessage::Open {
        slot: closed,
        number: 1,
    });
    assert_eq!(effects, vec![]);
    assert_eq!(rig.notice(), Some(MergeNotice::Closed));
    // The game of the editor itself.
    let effects = rig.raw(MergeMessage::Open { slot, number: A });
    assert_eq!(effects, vec![]);
    assert_eq!(rig.notice(), Some(MergeNotice::SameGame));
    assert!(rig.state.merge_view().is_none());
}

#[test]
fn a_dirty_game_is_still_the_same_game() {
    let (mut rig, slot, _dir) = sample(A);
    rig.open(slot, B);
    rig.apply();
    assert!(rig.state.session.is_dirty());
    rig.raw(MergeMessage::Open { slot, number: A });
    assert_eq!(rig.notice(), Some(MergeNotice::SameGame));
}

#[test]
fn a_copy_of_the_game_is_not_the_same_game_but_has_nothing_to_add() {
    let (mut rig, slot, _dir) = sample(A);
    let before = rig.state.session.game().clone();
    rig.open(slot, A_COPY);
    assert_eq!(rig.notice(), Some(MergeNotice::NothingToAdd));
    assert!(rig.state.merge_view().is_none());
    assert!(rig.state.session.game().same_content(&before));
}

#[test]
fn a_game_with_no_common_position_is_refused_after_the_reading() {
    let (mut rig, slot, _dir) = sample(A);
    rig.open(slot, OTHER_START);
    assert_eq!(rig.notice(), Some(MergeNotice::NoMatch));
    assert!(rig.state.merge_view().is_none());
}

#[test]
fn the_current_game_without_moves_is_refused_before_reading() {
    let (mut rig, slot, _dir) = sample(A);
    rig.send(mychess_app::Command::NewGame);
    let effects = rig.raw(MergeMessage::Open { slot, number: B });
    assert_eq!(effects, vec![]);
    assert_eq!(rig.notice(), Some(MergeNotice::EmptyGame));
}

#[test]
fn an_unreadable_game_is_refused() {
    let (mut rig, slot, _dir) = sample(A);
    let effects = rig.raw(MergeMessage::Open { slot, number: B });
    let Some(Effect::LoadGamesForMerge { request, .. }) = effects.into_iter().next() else {
        panic!("no effect");
    };
    rig.raw(MergeMessage::Loaded {
        request,
        games: vec![(B, Err(MergeLoadError::Decode))],
    });
    assert_eq!(rig.notice(), Some(MergeNotice::Decode));
    assert!(rig.state.merge_view().is_none());
}

#[test]
fn trial_mode_refuses() {
    let (mut rig, slot, _dir) = sample(A);
    rig.send(GameMessage::EnterTrial { null_move: false });
    assert!(rig.state.session.in_trial());
    let effects = rig.raw(MergeMessage::Open { slot, number: B });
    assert_eq!(effects, vec![]);
    assert_eq!(rig.notice(), Some(MergeNotice::Trial));
}

#[test]
fn a_locked_game_refuses_the_merge() {
    let (mut rig, slot, _dir) = sample(A);
    rig.state.online.marker = Some(MarkerInfo {
        game_id: "g".to_owned(),
        date: "2026-01-01".to_owned(),
    });
    assert!(rig.state.assist_locked());
    let effects = rig.raw(MergeMessage::Open { slot, number: B });
    assert_eq!(effects, vec![]);
    assert!(matches!(
        rig.state.notice,
        Some(AppNotice::Online(
            mychess_app::online::OnlineNotice::AssistanceDisabled
        ))
    ));
}

#[test]
fn the_end_of_the_game_continues_the_main_line() {
    let (mut rig, slot, _dir) = sample(E);
    rig.open(slot, B);
    let view = rig.state.merge_view().expect("dialog");
    assert!(view.extends_mainline());
    assert_eq!(view.report.unwrap().added, 6);
    rig.apply();
    assert_eq!(rig.nodes(), 1 + 8);
}

#[test]
fn the_other_game_may_come_from_the_clipbase() {
    let (mut rig, slot, _dir) = sample(B);
    rig.send(BasesMessage::ClipbaseCopy);
    rig.load(slot, A);
    rig.open(SlotId::CLIPBASE, 1);
    let view = rig.state.merge_view().expect("dialog");
    assert_eq!(view.slot, SlotId::CLIPBASE);
    assert_eq!(view.report.unwrap().match_ply, 4);
    rig.apply();
    assert_eq!(rig.nodes(), 1 + 6 + 4);
}

#[test]
fn the_action_merges_the_game_selected_in_the_list() {
    let (mut rig, slot, _dir) = sample(A);
    rig.send(mychess_app::Action::MergeGame);
    // The list has no selection of its own yet or selects the loaded game:
    // either way nothing is merged.
    assert!(rig.state.merge_view().is_none());
    rig.send(BasesMessage::SelectRow {
        slot,
        number: Some(B),
    });
    rig.send(mychess_app::Action::MergeGame);
    assert_eq!(rig.state.merge_view().unwrap().numbers.len(), 1);
}

#[test]
fn the_action_without_a_selection_says_so() {
    let (mut rig, slot, _dir) = sample(A);
    rig.send(BasesMessage::SelectRow { slot, number: None });
    rig.send(mychess_app::Action::MergeGame);
    assert_eq!(rig.notice(), Some(MergeNotice::NoGame));
}

#[test]
fn the_action_has_no_default_key() {
    let state = AppState::default();
    assert_eq!(
        mychess_app::Action::MergeGame.message_key(),
        "action-merge-game"
    );
    assert_eq!(
        state
            .shortcuts
            .bindings(mychess_app::Action::MergeGame)
            .len(),
        0
    );
}
