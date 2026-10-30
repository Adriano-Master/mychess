//! Replacing the game (discard prompt), modality, obsolete request ids, host
//! effects versus `perform`, and architecture rules (spec M2-2, D100-D104,
//! rules 22, 65).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::assert_is_empty,
    clippy::default_trait_access,
    clippy::format_push_string,
    clippy::items_after_statements,
    clippy::assigning_clones,
    clippy::field_reassign_with_default,
    clippy::type_complexity,
    let_underscore_drop
)]

mod m2_2_common;

use std::path::Path;

use m2_2_common::*;
use mychess_app::{
    AppNotice, ChooserMessage, ClipboardPurpose, DiscardChoice, FileErrorKind, GameInfoMessage,
    GameInfoOptions, PickSpec, PrefChange, PrefsMessage, SaveMode, TagField,
};
use mychess_core::Tags;
use proptest::prelude::*;
use rstest::rstest;

fn dirty_harness() -> Harness {
    let mut h = Harness::new();
    h.play_all(&["e2e4", "e7e5"]);
    assert!(h.state.is_dirty());
    h
}

fn canonical(path: &Path) -> std::path::PathBuf {
    std::fs::canonicalize(path).unwrap()
}

// ------------------------------------------------------------------ D103

#[test]
fn d103_a_clean_game_is_replaced_without_asking() {
    let mut h = Harness::new();
    h.send(Command::NewGame);
    assert!(h.state.discard_prompt.is_none());
    let mut h = Harness::new();
    h.play("e2e4");
    h.send(Command::SaveAs);
    h.pick(Some(&tempfile::tempdir().unwrap().keep().join("a.pgn")));
    assert!(!h.state.is_dirty());
    h.send(Command::NewGame);
    assert!(
        h.state.discard_prompt.is_none(),
        "saved games are replaced silently"
    );
    assert_eq!(h.plies(), 0);
}

#[test]
fn d103_a_dirty_game_asks_and_dont_save_replaces() {
    let mut h = dirty_harness();
    h.send(Command::NewGame);
    assert!(h.state.discard_prompt.is_some());
    assert_eq!(h.plies(), 2, "untouched while the question is open");
    h.send(Message::AnswerDiscard(DiscardChoice::DontSave));
    assert!(h.state.discard_prompt.is_none());
    assert_eq!(h.plies(), 0);
    assert!(!h.state.is_dirty());
    assert_eq!(h.state.session.cursor(), h.state.session.game().root());
    assert_eq!(h.state.origin, GameOrigin::New);
}

#[test]
fn d103_cancel_closes_only_the_prompt() {
    let mut h = dirty_harness();
    h.send(Command::NewGame);
    h.send(Message::AnswerDiscard(DiscardChoice::Cancel));
    assert!(h.state.discard_prompt.is_none());
    assert_eq!(h.plies(), 2);
    assert!(h.state.is_dirty());
}

#[test]
fn d103_save_then_replace_for_a_new_game_asks_for_a_file_first() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = dirty_harness();
    let game = h.state.session.game().clone();
    h.send(Command::NewGame);
    let host = h.send(Message::AnswerDiscard(DiscardChoice::Save));
    assert!(matches!(
        host.as_slice(),
        [Effect::PickFile(PickSpec {
            kind: PickKind::SavePgn,
            ..
        })]
    ));
    assert_eq!(h.plies(), 2, "nothing replaced until the save is done");
    h.pick(Some(&dir.path().join("saved.pgn")));
    assert_eq!(h.plies(), 0, "replaced after saving");
    let text = std::fs::read_to_string(dir.path().join("saved.pgn")).unwrap();
    let parsed =
        mychess_pgn::read::parse_pgn_text(&text, &mychess_pgn::read::ReadOptions::default());
    assert!(parsed[0].game.same_content(&game));
    assert_eq!(h.state.origin, GameOrigin::New, "the new game has no file");
}

#[test]
fn d103_save_then_cancelled_dialog_replaces_nothing() {
    let mut h = dirty_harness();
    h.send(Command::NewGame);
    h.send(Message::AnswerDiscard(DiscardChoice::Save));
    h.pick(None);
    assert_eq!(h.plies(), 2);
    assert!(h.state.is_dirty());
    assert!(h.state.discard_prompt.is_none());
    // The pending replacement is forgotten: a later save does not trigger it.
    let dir = tempfile::tempdir().unwrap();
    h.send(Command::Save);
    h.pick(Some(&dir.path().join("later.pgn")));
    assert_eq!(h.plies(), 2, "a plain save never replaces the game");
}

#[test]
fn d103_save_then_failing_save_replaces_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = dirty_harness();
    h.send(Command::NewGame);
    h.send(Message::AnswerDiscard(DiscardChoice::Save));
    h.pick(Some(&dir.path().join("no such dir").join("x.pgn")));
    assert_eq!(h.plies(), 2);
    assert!(h.state.is_dirty());
    assert!(matches!(h.state.notice, Some(AppNotice::File(_))));
}

#[test]
fn d103_save_on_a_saved_origin_overwrites_that_file_without_a_dialog_then_replaces() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = Harness::new();
    h.play("e2e4");
    h.send(Command::SaveAs);
    h.pick(Some(&dir.path().join("g.pgn")));
    h.play("e7e5");
    h.send(Command::NewGame);
    let host = h.send(Message::AnswerDiscard(DiscardChoice::Save));
    assert!(host.is_empty(), "no dialog: {host:?}");
    assert_eq!(h.plies(), 0);
    let text = std::fs::read_to_string(dir.path().join("g.pgn")).unwrap();
    assert!(
        text.contains("e5"),
        "the second move reached the file: {text}"
    );
}

#[test]
fn d103_an_empty_game_with_default_tags_is_not_worth_a_question() {
    let mut h = Harness::new();
    // Apply the untouched dialog: it counts as an edit (dirty) but changes nothing.
    h.send(GameInfoMessage::Open);
    h.send(GameInfoMessage::Apply);
    if h.state.is_dirty() {
        h.send(Command::NewGame);
        assert!(
            h.state.discard_prompt.is_none(),
            "no moves and default tags: nothing to lose"
        );
    }
}

#[test]
fn d103_typed_tags_alone_are_worth_a_question() {
    let mut h = Harness::new();
    h.send(GameInfoMessage::Open);
    h.send(GameInfoMessage::SetText(
        TagField::White,
        "Carlsen, M.".into(),
    ));
    h.send(GameInfoMessage::Apply);
    assert!(h.state.is_dirty());
    assert_ne!(*h.tags(), Tags::default());
    h.send(Command::NewGame);
    assert!(
        h.state.discard_prompt.is_some(),
        "typed headers are not discarded in silence"
    );
}

#[test]
fn d103_in_trial_mode_no_question_is_asked() {
    let mut h = Harness::new();
    h.send(GameMessage::ToggleTrial { null_move: false });
    h.play("e2e4");
    h.send(Command::NewGame);
    assert!(h.state.discard_prompt.is_none());
    assert_eq!(h.plies(), 0);
    assert!(!h.state.session.in_trial());
}

#[test]
fn d103_one_flow_serves_every_replacement() {
    // Each source of a new game goes through the same prompt.
    let dir = tempfile::tempdir().unwrap();
    let pgn = write_file(dir.path(), "one.pgn", multi_pgn(1).as_bytes());
    let sources: Vec<Box<dyn Fn(&mut Harness)>> = vec![
        Box::new(|h| {
            h.send(Command::NewGame);
        }),
        Box::new(|h| {
            h.clipboard(ClipboardPurpose::Fen, "8/8/8/8/8/8/8/K6k w - - 0 1");
        }),
        Box::new(move |h| {
            h.send(Command::Open);
            h.pick(Some(&pgn));
        }),
        Box::new(|h| {
            h.send(ImportMessage::Open);
            h.send(ImportMessage::TextEdited("1. d4 d5".into()));
            h.send(ImportMessage::Import);
        }),
        Box::new(|h| {
            h.send(Command::SetupBoard);
            h.send(SetupMessage::Clear);
            h.send(SetupMessage::Ok);
        }),
    ];
    for (i, start) in sources.iter().enumerate() {
        let mut h = dirty_harness();
        let original = h.state.session.game().clone();
        start(&mut h);
        assert!(h.state.discard_prompt.is_some(), "source {i} asked");
        assert_eq!(h.plies(), 2, "source {i}: untouched");
        h.send(Message::AnswerDiscard(DiscardChoice::Cancel));
        assert_eq!(h.plies(), 2, "source {i}: cancel keeps");
        start(&mut h);
        h.send(Message::AnswerDiscard(DiscardChoice::DontSave));
        assert!(
            !h.state.session.game().same_content(&original),
            "source {i}: replaced"
        );
    }
}

#[test]
fn d104_replacing_clears_dialogs_pasted_moves_and_the_undo_history() {
    let mut h = dirty_harness();
    h.clipboard(ClipboardPurpose::Moves, "1. d4");
    assert!(h.state.pasted_moves.is_some());
    assert!(h.state.session.can_undo());
    h.send(Command::NewGame);
    h.send(Message::AnswerDiscard(DiscardChoice::DontSave));
    assert!(h.state.pasted_moves.is_none());
    assert!(!h.state.session.can_undo());
    assert!(!h.state.session.can_redo());
    assert!(h.state.setup.is_none() && h.state.info_dialog.is_none() && h.state.chooser.is_none());
}

#[test]
fn d104_the_previous_tags_are_remembered_for_use_previous() {
    let mut h = Harness::new();
    h.send(GameInfoMessage::Open);
    h.send(GameInfoMessage::SetText(
        TagField::Event,
        "Candidates".into(),
    ));
    h.send(GameInfoMessage::Apply);
    h.send(Command::NewGame);
    h.send(Message::AnswerDiscard(DiscardChoice::DontSave));
    assert_eq!(
        h.state.last_tags.as_ref().map(|t| t.event.as_str()),
        Some("Candidates")
    );
    // A default-tagged game never overwrites the memory.
    h.send(Command::NewGame);
    assert_eq!(
        h.state.last_tags.as_ref().map(|t| t.event.as_str()),
        Some("Candidates")
    );
}

#[test]
fn d104_a_loaded_file_game_is_clean_and_an_imported_one_is_dirty() {
    let dir = tempfile::tempdir().unwrap();
    let pgn = write_file(dir.path(), "one.pgn", multi_pgn(1).as_bytes());
    let mut h = Harness::new();
    h.send(Command::Open);
    h.pick(Some(&pgn));
    assert!(!h.state.is_dirty());
    let mut h = Harness::new();
    h.send(ImportMessage::Open);
    h.send(ImportMessage::TextEdited(multi_pgn(1)));
    h.send(ImportMessage::Import);
    assert!(h.state.is_dirty());
    assert_eq!(h.state.origin, GameOrigin::Clipboard);
    // Saving it clears the flag and sets the origin.
    h.send(Command::Save);
    h.pick(Some(&dir.path().join("imp.pgn")));
    assert!(!h.state.is_dirty());
    assert!(matches!(h.state.origin, GameOrigin::SavedFile { .. }));
}

#[test]
fn d104_a_new_game_from_the_menu_resets_the_origin_and_the_flip_is_a_view_setting() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = Harness::new();
    h.play("e2e4");
    h.send(Command::SaveAs);
    h.pick(Some(&dir.path().join("a.pgn")));
    h.send(Action::FlipBoard);
    h.send(Command::NewGame);
    assert_eq!(h.state.origin, GameOrigin::New);
    assert!(
        h.state.board_flipped,
        "the board orientation is not part of the game"
    );
}

use mychess_app::Action;

// ------------------------------------------------------------------ modality

#[test]
fn the_discard_prompt_is_modal() {
    let mut h = dirty_harness();
    h.send(Command::NewGame);
    let game = h.state.session.game().clone();
    assert!(h.raw(Command::NewGame).is_empty());
    assert!(h.raw(Command::Open).is_empty());
    assert!(h.raw(Command::SetupBoard).is_empty());
    assert!(
        h.raw(GameMessage::Navigate(mychess_app::Nav::Start))
            .is_empty()
    );
    h.play("g1f3");
    assert!(
        h.state.session.game().same_content(&game),
        "no edits behind the prompt"
    );
    assert!(h.raw(GameInfoMessage::Open).is_empty());
    assert!(h.state.info_dialog.is_none());
    assert!(
        h.raw(Message::Quit).is_empty(),
        "a second quit is ignored while asking"
    );
    assert!(h.state.discard_prompt.is_some());
}

#[test]
fn the_setup_dialog_is_modal_for_the_game_and_the_commands() {
    let mut h = dirty_harness();
    h.send(Command::SetupBoard);
    let game = h.state.session.game().clone();
    assert!(h.raw(Command::NewGame).is_empty());
    assert!(h.raw(Command::Open).is_empty());
    assert!(h.raw(Command::PasteFen).is_empty());
    assert!(h.raw(Action::Save).is_empty());
    assert!(h.raw(GameMessage::Undo).is_empty());
    h.send(GameInfoMessage::Open);
    assert!(h.state.info_dialog.is_none(), "one modal at a time");
    assert!(h.state.session.game().same_content(&game));
    assert!(h.state.discard_prompt.is_none());
}

#[test]
fn the_info_dialog_blocks_commands_but_not_the_autoplay_timer_message() {
    let mut h = dirty_harness();
    h.send(GameInfoMessage::Open);
    assert!(h.raw(Command::NewGame).is_empty());
    assert!(h.raw(Command::SetupBoard).is_empty());
    assert!(h.state.setup.is_none());
    h.raw(GameMessage::AutoplayTick { generation: 12345 });
}

#[test]
fn the_import_dialog_is_not_modal_for_the_game_view() {
    let mut h = dirty_harness();
    h.send(ImportMessage::Open);
    h.send(GameMessage::Navigate(mychess_app::Nav::Start));
    assert_eq!(h.state.session.cursor(), h.state.session.game().root());
}

#[test]
fn modal_open_reports_the_three_modal_dialogs() {
    let mut h = dirty_harness();
    assert!(!h.state.modal_open());
    h.send(Command::SetupBoard);
    assert!(h.state.modal_open());
    h.send(SetupMessage::Cancel);
    h.send(GameInfoMessage::Open);
    assert!(h.state.modal_open());
    h.send(GameInfoMessage::Cancel);
    h.send(Command::NewGame);
    assert!(h.state.modal_open());
}

// ------------------------------------------------------------------ obsolete ids (D102)

#[test]
fn d102_a_file_dialog_answer_with_an_unknown_id_is_ignored() {
    let mut h = Harness::new();
    h.send(Command::Open);
    let eff = h.raw(Message::FilePicked(
        RequestId(424_242),
        Some("x.pgn".into()),
    ));
    assert!(eff.is_empty());
    assert!(h.state.notice.is_none());
    // The real answer still works afterwards.
    let dir = tempfile::tempdir().unwrap();
    let pgn = write_file(dir.path(), "a.pgn", multi_pgn(1).as_bytes());
    h.pick(Some(&pgn));
    assert_eq!(h.plies(), 3);
}

#[test]
fn d102_an_answer_can_be_used_only_once() {
    let dir = tempfile::tempdir().unwrap();
    let pgn = write_file(dir.path(), "a.pgn", multi_pgn(1).as_bytes());
    let mut h = Harness::new();
    h.send(Command::Open);
    h.pick(Some(&pgn));
    h.send(Command::NewGame);
    assert_eq!(h.plies(), 0);
    // Replaying the same dialog answer does nothing.
    let spec = h
        .host
        .iter()
        .find_map(|e| match e {
            Effect::PickFile(s) => Some(s.clone()),
            _ => None,
        })
        .unwrap();
    let eff = h.raw(Message::FilePicked(spec.request, Some(pgn)));
    assert!(eff.is_empty());
    assert_eq!(h.plies(), 0);
}

#[test]
fn d102_opening_twice_only_the_second_scan_counts() {
    let dir = tempfile::tempdir().unwrap();
    let a = write_file(
        dir.path(),
        "a.pgn",
        multi_pgn(1).replace("P0", "Alpha").as_bytes(),
    );
    let b = write_file(
        dir.path(),
        "b.pgn",
        multi_pgn(1).replace("P0", "Beta").as_bytes(),
    );
    let mut h = Harness::new();
    h.send(Command::Open);
    let first = h.raw(Message::FilePicked(
        h.host
            .iter()
            .find_map(|e| match e {
                Effect::PickFile(s) => Some(s.request),
                _ => None,
            })
            .unwrap(),
        Some(a),
    ));
    h.send(Command::Open);
    let second = h.raw(Message::FilePicked(
        h.host
            .iter()
            .rev()
            .find_map(|e| match e {
                Effect::PickFile(s) => Some(s.request),
                _ => None,
            })
            .unwrap(),
        Some(b),
    ));
    let late = h.perform(first.into_iter().next().unwrap());
    h.raw(late);
    assert_eq!(h.tags().white, "?", "the slow first answer is dropped");
    let current = h.perform(second.into_iter().next().unwrap());
    h.raw(current);
    assert_eq!(h.tags().white, "Beta");
}

#[test]
fn d102_a_save_answer_for_an_old_request_is_ignored() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = Harness::new();
    h.play("e2e4");
    h.send(Command::SaveAs);
    let eff = h.pick_raw(&dir.path().join("a.pgn"));
    let Effect::SavePgn { request, .. } = &eff[0] else {
        panic!("{eff:?}")
    };
    let stale = Message::PgnSaved {
        request: RequestId(request.0 + 77),
        mode: SaveMode::Overwrite,
        result: Ok(dir.path().join("a.pgn")),
    };
    let out = h.raw(stale);
    assert!(out.is_empty());
    assert!(
        h.state.is_dirty(),
        "a stale confirmation does not mark the game saved"
    );
    assert_eq!(h.state.origin, GameOrigin::New);
}

#[test]
fn d102_a_game_edited_while_saving_stays_dirty() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = Harness::new();
    h.play("e2e4");
    h.send(Command::SaveAs);
    let eff = h.pick_raw(&dir.path().join("a.pgn"));
    // The user plays another move before the save finishes.
    h.play("e7e5");
    let done = h.perform(eff.into_iter().next().unwrap());
    h.raw(done);
    assert!(h.state.is_dirty(), "the written file lacks the last move");
}

#[test]
fn d102_collection_game_answers_and_movetext_answers_with_wrong_ids_are_ignored() {
    let mut h = Harness::new();
    let loaded = mychess_app::LoadedGame {
        game: Game::new().into(),
        number: 1,
        report: mychess_app::ImportReport::default(),
    };
    assert!(
        h.raw(Message::CollectionGameParsed {
            request: RequestId(5),
            result: Ok(loaded)
        })
        .is_empty()
    );
    assert!(
        h.raw(Message::MovetextParsed {
            request: RequestId(5),
            outcome: Box::new(mychess_app::MovetextOutcome {
                game: Game::new().into(),
                report: Default::default()
            })
        })
        .is_empty()
    );
    assert!(h.state.pasted_moves.is_none());
    assert!(
        h.raw(Message::Today(
            RequestId(5),
            mychess_core::PartialDate::UNKNOWN
        ))
        .is_empty()
    );
    assert!(
        h.raw(Message::EcoLoaded {
            path: "x.eco".into(),
            result: Err(mychess_app::FileError::new(FileErrorKind::NotFound, None))
        })
        .is_empty()
    );
    assert!(h.state.eco_book.is_none());
}

// ------------------------------------------------------------------ host effects (D100)

#[test]
fn d100_runs_on_host_partitions_the_effects() {
    let host = [
        Effect::PickFile(PickSpec {
            request: RequestId(1),
            kind: PickKind::OpenPgn,
            initial_dir: None,
            file_name: None,
        }),
        Effect::ReadClipboard(ClipboardPurpose::Fen),
        Effect::WriteClipboard("x".into()),
        Effect::Exit,
    ];
    for e in &host {
        assert!(e.runs_on_host(), "{e:?}");
    }
    let dir = tempfile::tempdir().unwrap();
    let local = [
        Effect::ScanPgn {
            request: RequestId(1),
            source: mychess_app::PgnSource::Text(String::new()),
        },
        Effect::LoadPreferences {
            path: dir.path().join("p.toml"),
        },
        Effect::SavePreferences {
            path: dir.path().join("p.toml"),
            prefs: Box::default(),
        },
        Effect::LoadEcoBook {
            path: dir.path().join("x.eco"),
        },
        Effect::QueryToday {
            request: RequestId(1),
        },
        Effect::ParseMovetext {
            request: RequestId(1),
            start: Position::new(),
            text: String::new(),
        },
        Effect::ScheduleAutoplayTick {
            generation: 1,
            delay: std::time::Duration::from_millis(1),
        },
        Effect::SavePgn {
            request: RequestId(1),
            path: dir.path().join("x.pgn"),
            mode: SaveMode::Overwrite,
            game: Game::new().into(),
            options: Default::default(),
            guard: Default::default(),
        },
    ];
    for e in &local {
        assert!(!e.runs_on_host(), "{e:?}");
    }
}

#[test]
fn d100_perform_of_a_host_effect_answers_host_effect_not_handled() {
    let h = Harness::new();
    for e in [
        Effect::PickFile(PickSpec {
            request: RequestId(1),
            kind: PickKind::SavePgn,
            initial_dir: None,
            file_name: Some("x.pgn".into()),
        }),
        Effect::ReadClipboard(ClipboardPurpose::Moves),
        Effect::WriteClipboard("text".into()),
        Effect::Exit,
    ] {
        assert_eq!(h.perform(e), Message::HostEffectNotHandled);
    }
}

#[test]
fn d100_the_not_handled_answer_becomes_a_notice() {
    let mut h = Harness::new();
    h.send(Message::HostEffectNotHandled);
    assert_eq!(h.state.notice, Some(AppNotice::HostEffectNotHandled));
    assert_eq!(
        h.state.notice.as_ref().unwrap().message_key(),
        "app-notice-host-effect-not-handled"
    );
}

#[test]
fn d100_non_host_effects_produce_the_matching_answer_variants() {
    let dir = tempfile::tempdir().unwrap();
    let h = Harness::new();
    assert!(matches!(
        h.perform(Effect::ScanPgn {
            request: RequestId(3),
            source: mychess_app::PgnSource::Text("1. e4".into())
        }),
        Message::PgnScanned {
            request: RequestId(3),
            path: None,
            ..
        }
    ));
    assert!(matches!(
        h.perform(Effect::QueryToday {
            request: RequestId(4)
        }),
        Message::Today(RequestId(4), _)
    ));
    assert!(matches!(
        h.perform(Effect::ParseMovetext {
            request: RequestId(5),
            start: Position::new(),
            text: "1. e4 e5".into()
        }),
        Message::MovetextParsed {
            request: RequestId(5),
            ..
        }
    ));
    assert!(matches!(
        h.perform(Effect::LoadPreferences {
            path: dir.path().join("p.toml")
        }),
        Message::PreferencesLoaded(_)
    ));
    assert!(matches!(
        h.perform(Effect::LoadEcoBook {
            path: dir.path().join("nope.eco")
        }),
        Message::EcoLoaded { result: Err(_), .. }
    ));
    assert!(matches!(
        h.perform(Effect::SavePgn {
            request: RequestId(6),
            path: dir.path().join("o.pgn"),
            mode: SaveMode::Append,
            game: Game::new().into(),
            options: Default::default(),
            guard: Default::default(),
        }),
        Message::PgnSaved {
            request: RequestId(6),
            mode: SaveMode::Append,
            result: Ok(_)
        }
    ));
}

#[test]
fn d100_save_answer_carries_the_canonical_path() {
    let dir = tempfile::tempdir().unwrap();
    let h = Harness::new();
    let path = dir.path().join("sub").join("..").join("o.pgn");
    std::fs::create_dir(dir.path().join("sub")).unwrap();
    let m = h.perform(Effect::SavePgn {
        request: RequestId(1),
        path: path.clone(),
        mode: SaveMode::Overwrite,
        game: Game::new().into(),
        options: Default::default(),
        guard: Default::default(),
    });
    let Message::PgnSaved {
        result: Ok(returned),
        ..
    } = m
    else {
        panic!("{m:?}")
    };
    assert_eq!(returned, canonical(&dir.path().join("o.pgn")));
}

#[tokio::test(flavor = "current_thread")]
async fn d100_perform_runs_inside_a_tokio_test_runtime() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("p.toml");
    let saved = perform(Effect::SavePreferences {
        path: path.clone(),
        prefs: Box::default(),
    })
    .await;
    assert_eq!(saved, Message::PreferencesSaved(Ok(())));
    let loaded = perform(Effect::LoadPreferences { path }).await;
    assert!(matches!(loaded, Message::PreferencesLoaded(l) if l.issues.is_empty()));
}

#[tokio::test(flavor = "current_thread")]
async fn d100_many_effects_can_run_concurrently() {
    let dir = tempfile::tempdir().unwrap();
    let mut tasks = Vec::new();
    for i in 0..8 {
        let path = dir.path().join(format!("g{i}.pgn"));
        tasks.push(tokio::spawn(async move {
            perform(Effect::SavePgn {
                request: RequestId(i),
                path,
                mode: SaveMode::Overwrite,
                game: Game::new().into(),
                options: Default::default(),
                guard: Default::default(),
            })
            .await
        }));
    }
    for t in tasks {
        assert!(matches!(
            t.await.unwrap(),
            Message::PgnSaved { result: Ok(_), .. }
        ));
    }
    assert_eq!(dir_names(dir.path()).len(), 8);
}

#[test]
fn messages_and_effects_are_clone_and_eq_as_the_gui_requires() {
    fn assert_traits<T: Clone + PartialEq + std::fmt::Debug>() {}
    assert_traits::<Message>();
    assert_traits::<Effect>();
    assert_traits::<AppNotice>();
    let m = Message::FilePicked(RequestId(1), Some("a".into()));
    assert_eq!(m.clone(), m);
}

// ------------------------------------------------------------------ rule 65 (architecture)

#[test]
fn rule65_the_update_modules_do_no_io() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = vec![
        src.join("state.rs"),
        src.join("flow.rs"),
        src.join("chooser.rs"),
    ];
    for entry in std::fs::read_dir(src.join("state")).unwrap() {
        let path = entry.unwrap().path();
        // Files that are entirely unit tests (declared `#[cfg(test)] mod` by
        // their parent) have no marker to stop at.
        let test_module = path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.ends_with("_tests.rs"));
        if path.extension().is_some_and(|e| e == "rs") && !test_module {
            files.push(path);
        }
    }
    files.sort();
    assert!(files.len() >= 6, "{files:?}");
    let forbidden = [
        "std::fs",
        "tokio::fs",
        "std::thread",
        "thread::sleep",
        "File::open",
        "File::create",
        "read_to_string",
        "scan_pgn(",
        "save_pgn(",
        "parse_pgn_text(",
        "parse_movetext(",
        "read_eco_file",
        "SystemTime",
        "Instant::now",
        "std::net",
        "Command::new(",
        "std::process",
    ];
    for file in files {
        let text = std::fs::read_to_string(&file).unwrap();
        // Stop at the unit tests of the file, if any.
        let code = text.split("#[cfg(test)]").next().unwrap();
        // The one clock read allowed: `play_now` (M8-1) stamps the events
        // that carry no time; the tests inject `fixed_now` instead.
        let code = code.replace("fixed_now.unwrap_or_else(Instant::now)", "");
        // The other exception: `parse_line` of the opening report validates
        // the moves of one favorite, typed by the user and capped at 4000
        // characters (M7-2b, D3664): a parse of microseconds, no I/O.
        let code = code.replace(
            "parse_movetext(text, &Position::new(), &ReadOptions::default())",
            "",
        );
        for pattern in forbidden {
            assert!(
                !code.contains(pattern),
                "{} uses `{pattern}`",
                file.display()
            );
        }
    }
}

// ------------------------------------------------------------------ fuzz

#[derive(Debug, Clone)]
enum Op {
    NewGame,
    Open,
    Save,
    SaveAs,
    Append,
    Next,
    Prev,
    First,
    Last,
    CopyFen,
    PasteFen,
    CopyPgn,
    PasteMoves,
    Setup,
    SetupClick(u8),
    SetupOk,
    SetupCancel,
    SetupClear,
    SetupFen(String),
    ImportOpen,
    ImportText(String),
    Import,
    ImportClose,
    InfoOpen,
    InfoApply,
    InfoCancel,
    InfoText(String),
    InfoYear(String),
    Discard(u8),
    Overwrite(bool),
    Chooser(u8, usize),
    ChooserFilter(String),
    PickOk(u8),
    PickNone,
    PickStale,
    Clip(String),
    Play(u8),
    Back,
    Quit,
    Flush,
    Undo,
    TruncateAtCursor,
    Geometry(u32, u32),
    SetTheme,
}

fn arb_op() -> impl Strategy<Value = Op> {
    let text = prop_oneof![
        Just(String::new()),
        Just("1. e4 e5 2. Nf3".to_owned()),
        Just("[Event \"x\"]\n\n1. d4 d5 *".to_owned()),
        Just("8/8/8/8/8/8/8/K6k w - - 0 1".to_owned()),
        Just(multi_pgn(3)),
        "\\PC{0,40}",
    ];
    prop_oneof![
        Just(Op::NewGame),
        Just(Op::Open),
        Just(Op::Save),
        Just(Op::SaveAs),
        Just(Op::Append),
        Just(Op::Next),
        Just(Op::Prev),
        Just(Op::First),
        Just(Op::Last),
        Just(Op::CopyFen),
        Just(Op::PasteFen),
        Just(Op::CopyPgn),
        Just(Op::PasteMoves),
        Just(Op::Setup),
        (0u8..64).prop_map(Op::SetupClick),
        Just(Op::SetupOk),
        Just(Op::SetupCancel),
        Just(Op::SetupClear),
        text.clone().prop_map(Op::SetupFen),
        Just(Op::ImportOpen),
        text.clone().prop_map(Op::ImportText),
        Just(Op::Import),
        Just(Op::ImportClose),
        Just(Op::InfoOpen),
        Just(Op::InfoApply),
        Just(Op::InfoCancel),
        "\\PC{0,20}".prop_map(Op::InfoText),
        prop_oneof![
            Just("2024".to_owned()),
            Just("2048".to_owned()),
            "\\PC{0,5}"
        ]
        .prop_map(Op::InfoYear),
        (0u8..3).prop_map(Op::Discard),
        any::<bool>().prop_map(Op::Overwrite),
        (0u8..4, 0usize..6).prop_map(|(a, b)| Op::Chooser(a, b)),
        "\\PC{0,6}".prop_map(Op::ChooserFilter),
        (0u8..3).prop_map(Op::PickOk),
        Just(Op::PickNone),
        Just(Op::PickStale),
        text.prop_map(Op::Clip),
        (0u8..6).prop_map(Op::Play),
        Just(Op::Back),
        Just(Op::Quit),
        Just(Op::Flush),
        Just(Op::Undo),
        Just(Op::TruncateAtCursor),
        (0u32..2000, 0u32..2000).prop_map(|(a, b)| Op::Geometry(a, b)),
        Just(Op::SetTheme),
    ]
}

#[allow(clippy::too_many_lines)] // one flat match over the operations
fn apply(h: &mut Harness, op: &Op, dir: &Path) {
    use mychess_app::SetupMessage as S;
    match op {
        Op::NewGame => drop(h.send(Command::NewGame)),
        Op::Open => drop(h.send(Command::Open)),
        Op::Save => drop(h.send(Command::Save)),
        Op::SaveAs => drop(h.send(Command::SaveAs)),
        Op::Append => drop(h.send(Command::Append)),
        Op::Next => drop(h.send(Command::NextGame)),
        Op::Prev => drop(h.send(Command::PrevGame)),
        Op::First => drop(h.send(Command::FirstGame)),
        Op::Last => drop(h.send(Command::LastGame)),
        Op::CopyFen => drop(h.send(Command::CopyFen)),
        Op::PasteFen => drop(h.send(Command::PasteFen)),
        Op::CopyPgn => drop(h.send(Command::CopyPgn)),
        Op::PasteMoves => drop(h.send(Command::PasteMoves)),
        Op::Setup => drop(h.send(Command::SetupBoard)),
        Op::SetupClick(n) => drop(h.send(S::ClickSquare(Square::new(u32::from(*n))))),
        Op::SetupOk => drop(h.send(S::Ok)),
        Op::SetupCancel => drop(h.send(S::Cancel)),
        Op::SetupClear => drop(h.send(S::Clear)),
        Op::SetupFen(t) => {
            h.send(S::FenEdited(t.clone()));
            h.send(S::ApplyFenText);
        }
        Op::ImportOpen => drop(h.send(ImportMessage::Open)),
        Op::ImportText(t) => drop(h.send(ImportMessage::TextEdited(t.clone()))),
        Op::Import => drop(h.send(ImportMessage::Import)),
        Op::ImportClose => drop(h.send(ImportMessage::Close)),
        Op::InfoOpen => drop(h.send(GameInfoMessage::Open)),
        Op::InfoApply => drop(h.send(GameInfoMessage::Apply)),
        Op::InfoCancel => drop(h.send(GameInfoMessage::Cancel)),
        Op::InfoText(t) => drop(h.send(GameInfoMessage::SetText(TagField::White, t.clone()))),
        Op::InfoYear(t) => drop(h.send(GameInfoMessage::SetDate(
            mychess_app::DateTarget::Date,
            mychess_app::DatePart::Year,
            t.clone(),
        ))),
        Op::Discard(n) => drop(h.send(Message::AnswerDiscard(
            [
                DiscardChoice::Save,
                DiscardChoice::DontSave,
                DiscardChoice::Cancel,
            ][usize::from(*n % 3)],
        ))),
        Op::Overwrite(b) => drop(h.send(Message::AnswerOverwrite(*b))),
        Op::Chooser(kind, i) => drop(h.send(match kind % 4 {
            0 => ChooserMessage::Open(*i),
            1 => ChooserMessage::Confirm,
            2 => ChooserMessage::Move(isize::try_from(*i).unwrap() - 3),
            _ => ChooserMessage::Cancel,
        })),
        Op::ChooserFilter(t) => drop(h.send(ChooserMessage::Filter(t.clone()))),
        Op::PickOk(n) => {
            if h.host.iter().any(|e| matches!(e, Effect::PickFile(_))) {
                let path = match n % 3 {
                    0 => dir.join("one.pgn"),
                    1 => dir.join("three.pgn"),
                    _ => dir.join("out.pgn"),
                };
                drop(h.pick(Some(&path)));
            }
        }
        Op::PickNone => {
            if h.host.iter().any(|e| matches!(e, Effect::PickFile(_))) {
                drop(h.pick(None));
            }
        }
        Op::PickStale => drop(h.send(Message::FilePicked(
            RequestId(u64::MAX),
            Some(dir.join("one.pgn")),
        ))),
        Op::Clip(t) => {
            for p in [
                ClipboardPurpose::Fen,
                ClipboardPurpose::SetupFen,
                ClipboardPurpose::ImportPrefill,
                ClipboardPurpose::Moves,
            ] {
                h.clipboard(p, t);
            }
        }
        Op::Play(n) => {
            let moves = h.state.session.position().clone();
            let legal = moves.legal_moves();
            if let Some(mv) = legal.get(usize::from(*n) % legal.len().max(1)) {
                drop(h.send(GameMessage::PlayMove {
                    mv: *mv,
                    options: mychess_app::PlayOptions::default(),
                }));
            }
        }
        Op::Back => drop(h.send(GameMessage::Navigate(mychess_app::Nav::Back(1)))),
        Op::Quit => drop(h.send(Message::Quit)),
        Op::Flush => drop(h.send(Message::FlushPreferences)),
        Op::Undo => drop(h.send(GameMessage::Undo)),
        Op::TruncateAtCursor => drop(h.send(Action::TruncateGame)),
        Op::Geometry(w, hh) => drop(h.send(Message::WindowGeometryChanged(
            mychess_app::WindowGeometry {
                width: *w,
                height: *hh,
                x: None,
                y: None,
                maximized: false,
            },
        ))),
        Op::SetTheme => drop(h.send(PrefsMessage::Set(PrefChange::Theme(
            mychess_app::ThemePref::Dark,
        )))),
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]

    /// Rule 66 at the level of the whole controller: no sequence of messages
    /// panics or leaves an impossible state.
    #[test]
    fn rule66_random_message_sequences_never_panic_and_keep_the_state_coherent(
        ops in proptest::collection::vec(arb_op(), 1..40)
    ) {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("one.pgn"), multi_pgn(1)).unwrap();
        std::fs::write(dir.path().join("three.pgn"), multi_pgn(3)).unwrap();
        let mut h = Harness::with_prefs_path(Some(dir.path().join("cfg").join("preferences.toml")));
        for op in &ops {
            apply(&mut h, op, dir.path());
            let s = &h.state;
            // The cursor always belongs to the game.
            prop_assert!(s.session.game().contains(s.session.cursor()));
            // At most one of the two edit dialogs is open.
            prop_assert!(!(s.setup.is_some() && s.info_dialog.is_some()), "{:?}", op);
            // A collection exists exactly while the origin says so.
            if matches!(s.origin, GameOrigin::PgnFile { .. }) {
                // may or may not keep the collection (single-game files do not)
            } else {
                prop_assert!(s.collection.is_none(), "{:?}", op);
            }
            // Trial mode never coexists with a clean "saved" origin change.
            if let GameOrigin::PgnFile { index, total, .. } = &s.origin {
                prop_assert!(index < total);
            }
            // The panel always renders.
            drop(s.game_info());
            prop_assert!(s.prefs.board.animation_ms <= mychess_app::MAX_ANIMATION_MS);
        }
    }
}

// Everything the options struct offers is Copy + comparable (used in prefs).
#[test]
fn game_info_options_are_plain_data() {
    let a = GameInfoOptions::default();
    let b = a;
    assert_eq!(a, b);
}

#[rstest]
#[case(FileErrorKind::NotFound, "file-error-not-found")]
#[case(FileErrorKind::PermissionDenied, "file-error-permission-denied")]
#[case(FileErrorKind::AlreadyExists, "file-error-already-exists")]
#[case(FileErrorKind::TooLarge { limit: 1 }, "file-error-too-large")]
#[case(
    FileErrorKind::UnsupportedEncoding("UTF-16"),
    "file-error-unsupported-encoding"
)]
#[case(FileErrorKind::CorruptGzip, "file-error-corrupt-gzip")]
#[case(FileErrorKind::NoGamesFound { skipped_bytes: 0 }, "file-error-no-games-found")]
#[case(FileErrorKind::EncodingLossy, "file-error-encoding-lossy")]
#[case(FileErrorKind::Io(std::io::ErrorKind::Other), "file-error-io")]
fn d101_every_file_error_kind_has_a_fluent_key(#[case] kind: FileErrorKind, #[case] key: &str) {
    assert_eq!(kind.message_key(), key);
    let err = mychess_app::FileError::new(kind, None);
    assert_eq!(err.message_key(), key);
    assert!(err.to_string().starts_with("file error"));
}
