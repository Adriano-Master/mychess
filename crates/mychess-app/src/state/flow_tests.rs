//! Tests of the controller: the flows of the M2-2 slice, driven message by
//! message with the effects checked as data.

use std::path::PathBuf;

use mychess_core::{GameResult, Position, Tags};

use super::*;
use crate::chooser::ChooserMessage;
use crate::clipboard::{ClipboardError, ClipboardPurpose};
use crate::error::{FileError, FileErrorKind};
use crate::files::{GameOrigin, PgnLoad, PgnSource, SaveMode, ScanLimits, ScanOutcome, scan_pgn};
use crate::flow::{AppNotice, Command, DiscardChoice};
use crate::game_info::{GameInfoMessage, TagField};
use crate::host::PickKind;
use crate::import::{ImportIssue, ImportMessage};
use crate::prefs::{PrefChange, PrefsMessage, ThemePref, WindowGeometry};
use crate::session::{GameMessage, Nav};
use crate::setup::SetupMessage;
use crate::shortcuts::Action;

const TWO_GAMES: &str = "[White \"A\"]\n[Black \"B\"]\n\n1. e4 e5 *\n\n[White \"C\"]\n\n1. d4 *\n";

fn send(state: &mut AppState, message: impl Into<Message>) -> Vec<Effect> {
    update(state, message.into())
}

fn play(state: &mut AppState, uci: &str) {
    let mv = state.session.position().parse_uci(uci).expect("uci");
    send(
        state,
        GameMessage::PlayMove {
            mv,
            options: crate::session::PlayOptions::default(),
        },
    );
}

fn scanned(text: &str, path: Option<&str>) -> ScanOutcome {
    let (load, notices) = scan_pgn(
        PgnSource::Text(text.to_owned()),
        &ScanLimits::default(),
        &mychess_pgn::read::ReadOptions::default(),
    )
    .expect("scan");
    ScanOutcome {
        load,
        notices,
        path: path.map(PathBuf::from),
    }
}

fn request_of(effects: &[Effect]) -> RequestId {
    effects
        .iter()
        .find_map(|e| match e {
            Effect::ScanPgn { request, .. }
            | Effect::ParseCollectionGame { request, .. }
            | Effect::SavePgn { request, .. }
            | Effect::QueryToday { request }
            | Effect::ParseMovetext { request, .. } => Some(*request),
            Effect::PickFile(spec) => Some(spec.request),
            _ => None,
        })
        .expect("an effect with a request")
}

#[test]
fn copy_fen_writes_the_clipboard() {
    let mut state = AppState::default();
    play(&mut state, "e2e4");
    let effects = send(&mut state, Command::CopyFen);
    assert_eq!(
        effects,
        [Effect::WriteClipboard(state.session.position().fen())]
    );
}

#[test]
fn copy_pgn_is_one_unwrapped_text() {
    let mut state = AppState::default();
    for uci in ["e2e4", "e7e5", "g1f3"] {
        play(&mut state, uci);
    }
    let effects = send(&mut state, Command::CopyPgn);
    let [Effect::WriteClipboard(text)] = effects.as_slice() else {
        panic!("one clipboard write expected: {effects:?}");
    };
    assert!(text.contains("1. e4 e5 2. Nf3"), "{text}");
    assert!(!text.contains('\r'));
}

#[test]
fn paste_fen_replaces_a_clean_game_and_keeps_it_clean() {
    let mut state = AppState::default();
    let effects = send(&mut state, Command::PasteFen);
    assert_eq!(effects, [Effect::ReadClipboard(ClipboardPurpose::Fen)]);
    send(
        &mut state,
        Message::ClipboardRead {
            purpose: ClipboardPurpose::Fen,
            result: Ok("FEN: 8/8/8/8/8/8/8/K6k b".to_owned()),
        },
    );
    assert_eq!(
        state.session.position().fen(),
        "8/8/8/8/8/8/8/K6k b - - 0 1"
    );
    assert!(!state.is_dirty());
    assert_eq!(state.origin, GameOrigin::New);
}

#[test]
fn bad_clipboard_never_touches_the_game() {
    let mut state = AppState::default();
    play(&mut state, "e2e4");
    let before = state.session.game().clone();
    for result in [
        Ok("garbage".to_owned()),
        Ok("   ".to_owned()),
        Err(ClipboardError::Unavailable),
        Ok("x".repeat(crate::clipboard::MAX_CLIPBOARD_BYTES + 1)),
    ] {
        let effects = send(
            &mut state,
            Message::ClipboardRead {
                purpose: ClipboardPurpose::Fen,
                result,
            },
        );
        assert!(effects.is_empty());
        assert!(state.discard_prompt.is_none());
        assert!(state.notice.is_some());
        assert!(state.session.game().same_content(&before));
    }
    assert!(matches!(
        state.notice,
        Some(AppNotice::Clipboard(ClipboardError::TooLarge { .. }))
    ));
}

#[test]
fn replacing_an_altered_game_asks_first() {
    let mut state = AppState::default();
    play(&mut state, "e2e4");
    send(&mut state, Command::NewGame);
    assert!(state.discard_prompt.is_some());
    assert_eq!(state.session.game().mainline().count(), 1);
    // Cancel keeps everything.
    send(&mut state, Message::AnswerDiscard(DiscardChoice::Cancel));
    assert!(state.discard_prompt.is_none());
    assert_eq!(state.session.game().mainline().count(), 1);
    send(&mut state, Command::NewGame);
    send(&mut state, Message::AnswerDiscard(DiscardChoice::DontSave));
    assert_eq!(state.session.game().mainline().count(), 0);
    assert!(!state.is_dirty());
    // An empty game never asks.
    send(&mut state, Command::NewGame);
    assert!(state.discard_prompt.is_none());
}

#[test]
fn trial_mode_skips_the_question() {
    let mut state = AppState::default();
    play(&mut state, "e2e4");
    send(&mut state, GameMessage::EnterTrial { null_move: false });
    send(&mut state, Command::NewGame);
    assert!(state.discard_prompt.is_none() || !state.needs_confirmation());
}

#[test]
fn save_answer_saves_first_and_replaces_only_after_success() {
    let mut state = AppState::default();
    play(&mut state, "e2e4");
    send(&mut state, Command::NewGame);
    let effects = send(&mut state, Message::AnswerDiscard(DiscardChoice::Save));
    let [Effect::PickFile(spec)] = effects.as_slice() else {
        panic!("a file dialog expected: {effects:?}");
    };
    assert_eq!(spec.kind, PickKind::SavePgn);
    // Cancelling the dialog replaces nothing.
    send(&mut state, Message::FilePicked(spec.request, None));
    assert_eq!(state.session.game().mainline().count(), 1);

    send(&mut state, Command::NewGame);
    let effects = send(&mut state, Message::AnswerDiscard(DiscardChoice::Save));
    let request = request_of(&effects);
    let effects = send(
        &mut state,
        Message::FilePicked(request, Some(PathBuf::from("out dir/g"))),
    );
    let [
        Effect::SavePgn {
            path,
            mode,
            request,
            ..
        },
    ] = effects.as_slice()
    else {
        panic!("a save expected: {effects:?}");
    };
    assert_eq!(path, &PathBuf::from("out dir/g.pgn"));
    assert_eq!(*mode, SaveMode::Overwrite);
    // A failed save replaces nothing.
    let failed = send(
        &mut state,
        Message::PgnSaved {
            request: *request,
            mode: SaveMode::Overwrite,
            result: Err(FileError::new(FileErrorKind::PermissionDenied, None)),
        },
    );
    assert!(failed.is_empty());
    assert_eq!(state.session.game().mainline().count(), 1);
    assert!(matches!(state.notice, Some(AppNotice::File(_))));

    send(&mut state, Command::NewGame);
    let effects = send(&mut state, Message::AnswerDiscard(DiscardChoice::Save));
    let effects = send(
        &mut state,
        Message::FilePicked(request_of(&effects), Some(PathBuf::from("a.pgn"))),
    );
    let request = request_of(&effects);
    send(
        &mut state,
        Message::PgnSaved {
            request,
            mode: SaveMode::Overwrite,
            result: Ok(PathBuf::from("/abs/a.pgn")),
        },
    );
    assert_eq!(
        state.session.game().mainline().count(),
        0,
        "replaced after saving"
    );
    assert_eq!(state.origin, GameOrigin::New);
    assert_eq!(
        state.prefs.files.recent.get(0),
        Some(&PathBuf::from("/abs/a.pgn"))
    );
}

#[test]
fn save_marks_clean_and_ctrl_s_reuses_the_file() {
    let mut state = AppState::default();
    play(&mut state, "e2e4");
    let effects = send(&mut state, Action::Save);
    let spec = match effects.as_slice() {
        [Effect::PickFile(spec)] => spec.clone(),
        other => panic!("a file dialog expected: {other:?}"),
    };
    assert_eq!(spec.file_name.as_deref(), Some("game.pgn"));
    let effects = send(
        &mut state,
        Message::FilePicked(spec.request, Some(PathBuf::from("x.pgn"))),
    );
    let request = request_of(&effects);
    assert!(state.is_dirty());
    send(
        &mut state,
        Message::PgnSaved {
            request,
            mode: SaveMode::Overwrite,
            result: Ok(PathBuf::from("/abs/x.pgn")),
        },
    );
    assert!(!state.is_dirty());
    assert_eq!(
        state.origin,
        GameOrigin::SavedFile {
            path: PathBuf::from("/abs/x.pgn")
        }
    );
    let effects = send(&mut state, Command::Save);
    assert!(
        matches!(effects.as_slice(), [Effect::SavePgn { path, .. }] if path == &PathBuf::from("/abs/x.pgn"))
    );
}

#[test]
fn a_stale_save_answer_is_dropped() {
    let mut state = AppState::default();
    play(&mut state, "e2e4");
    let effects = send(&mut state, Command::SaveAs);
    let effects = send(
        &mut state,
        Message::FilePicked(request_of(&effects), Some(PathBuf::from("x.pgn"))),
    );
    let request = request_of(&effects);
    let wrong = RequestId(request.0 + 100);
    send(
        &mut state,
        Message::PgnSaved {
            request: wrong,
            mode: SaveMode::Overwrite,
            result: Ok(PathBuf::from("/abs/x.pgn")),
        },
    );
    assert!(state.is_dirty());
    // Editing while the save runs leaves the game altered.
    play(&mut state, "e7e5");
    send(
        &mut state,
        Message::PgnSaved {
            request,
            mode: SaveMode::Overwrite,
            result: Ok(PathBuf::from("/abs/x.pgn")),
        },
    );
    assert!(state.is_dirty());
}

#[test]
fn append_does_not_change_the_origin() {
    let mut state = AppState::default();
    play(&mut state, "e2e4");
    let effects = send(&mut state, Command::Append);
    let effects = send(
        &mut state,
        Message::FilePicked(request_of(&effects), Some(PathBuf::from("all.PGN"))),
    );
    let [
        Effect::SavePgn {
            path,
            mode,
            request,
            ..
        },
    ] = effects.as_slice()
    else {
        panic!("a save expected");
    };
    assert_eq!(
        (path.clone(), *mode),
        (PathBuf::from("all.PGN"), SaveMode::Append)
    );
    send(
        &mut state,
        Message::PgnSaved {
            request: *request,
            mode: SaveMode::Append,
            result: Ok(PathBuf::from("/abs/all.PGN")),
        },
    );
    assert!(!state.is_dirty());
    assert_eq!(state.origin, GameOrigin::New);
}

/// Opens a real two-game file; returns its canonical path and the directory.
fn open_two_games(state: &mut AppState) -> (PathBuf, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("multi.pgn");
    std::fs::write(&path, TWO_GAMES).expect("write");
    let effects = send(state, Command::Open);
    let effects = send(
        state,
        Message::FilePicked(request_of(&effects), Some(path.clone())),
    );
    let request = request_of(&effects);
    let (load, notices) = scan_pgn(
        PgnSource::File(path.clone()),
        &ScanLimits::default(),
        &mychess_pgn::read::ReadOptions::default(),
    )
    .expect("scan");
    let canonical = std::fs::canonicalize(&path).expect("canonical");
    send(
        state,
        Message::PgnScanned {
            request,
            path: Some(path),
            result: Ok(ScanOutcome {
                load,
                notices,
                path: Some(canonical.clone()),
            }),
        },
    );
    (canonical, dir)
}

#[test]
fn opening_a_file_with_one_game_loads_it() {
    let mut state = AppState::default();
    let effects = send(&mut state, Command::Open);
    let [Effect::PickFile(spec)] = effects.as_slice() else {
        panic!("dialog expected");
    };
    assert_eq!(spec.kind, PickKind::OpenPgn);
    let effects = send(
        &mut state,
        Message::FilePicked(spec.request, Some(PathBuf::from("g.pgn"))),
    );
    let [
        Effect::ScanPgn {
            request,
            source: PgnSource::File(path),
        },
    ] = effects.as_slice()
    else {
        panic!("scan expected: {effects:?}");
    };
    assert_eq!(path, &PathBuf::from("g.pgn"));
    send(
        &mut state,
        Message::PgnScanned {
            request: *request,
            path: Some(PathBuf::from("g.pgn")),
            result: Ok(scanned("[White \"Solo\"]\n\n1. e4 *\n", Some("/abs/g.pgn"))),
        },
    );
    assert_eq!(state.session.game().tags().white, "Solo");
    assert_eq!(
        state.origin,
        GameOrigin::PgnFile {
            path: PathBuf::from("/abs/g.pgn"),
            index: 0,
            total: 1,
            partial: false,
        }
    );
    assert!(!state.is_dirty());
    assert_eq!(
        state.prefs.files.recent.get(0),
        Some(&PathBuf::from("/abs/g.pgn"))
    );
    assert!(state.prefs_dirty);
    assert_eq!(
        state
            .names
            .matches(crate::game_info::NameKind::Player, "so", 9)
            .len(),
        1
    );
}

#[test]
fn a_failed_open_forgets_the_recent_entry_and_keeps_the_game() {
    let mut state = AppState::default();
    state.prefs.files.recent.push(PathBuf::from("gone.pgn"));
    play(&mut state, "e2e4");
    let effects = send(&mut state, Command::OpenRecent(0));
    let request = request_of(&effects);
    send(
        &mut state,
        Message::PgnScanned {
            request,
            path: Some(PathBuf::from("gone.pgn")),
            result: Err(FileError::new(
                FileErrorKind::NotFound,
                Some(PathBuf::from("gone.pgn")),
            )),
        },
    );
    assert!(state.prefs.files.recent.is_empty());
    assert_eq!(state.session.game().mainline().count(), 1);
    assert!(matches!(state.notice, Some(AppNotice::File(_))));
}

#[test]
fn many_games_open_the_chooser_and_navigation_works() {
    let mut state = AppState::default();
    let (canonical, _dir) = open_two_games(&mut state);
    let chooser = state.chooser.as_ref().expect("chooser");
    assert_eq!(chooser.visible, [0, 1]);
    send(&mut state, ChooserMessage::Filter("c".to_owned()));
    assert_eq!(
        state.chooser.as_ref().map(|c| c.visible.clone()),
        Some(vec![1])
    );
    let effects = send(&mut state, ChooserMessage::Confirm);
    let request = request_of(&effects);
    let Some(Effect::ParseCollectionGame {
        collection, index, ..
    }) = effects.first().cloned()
    else {
        panic!("parse expected");
    };
    assert_eq!(index, 1);
    let parsed = crate::effect::perform_parse_for_tests(&collection, index);
    send(
        &mut state,
        Message::CollectionGameParsed {
            request,
            result: Ok(parsed),
        },
    );
    assert_eq!(state.session.game().tags().white, "C");
    assert!(state.chooser.is_none());
    assert_eq!(
        state.origin,
        GameOrigin::PgnFile {
            path: canonical.clone(),
            index: 1,
            total: 2,
            partial: false,
        }
    );
    // At the last game nothing happens; previous goes back.
    assert!(send(&mut state, Command::NextGame).is_empty());
    assert!(send(&mut state, Command::LastGame).is_empty());
    let effects = send(&mut state, Command::PrevGame);
    assert!(matches!(
        effects.as_slice(),
        [Effect::ParseCollectionGame { index: 0, .. }]
    ));
    let effects = send(&mut state, Command::FirstGame);
    assert!(matches!(
        effects.as_slice(),
        [Effect::ParseCollectionGame { index: 0, .. }]
    ));
}

#[test]
fn saving_over_a_multi_game_file_asks_for_confirmation() {
    let mut state = AppState::default();
    let (canonical, _dir) = open_two_games(&mut state);
    send(&mut state, ChooserMessage::Open(0));
    let request = state.flow.parse.as_ref().expect("parse").request;
    let collection = state.flow.parse.as_ref().expect("parse").collection.clone();
    let parsed = crate::effect::perform_parse_for_tests(&collection, 0);
    send(
        &mut state,
        Message::CollectionGameParsed {
            request,
            result: Ok(parsed),
        },
    );
    let effects = send(&mut state, Command::SaveAs);
    let effects = send(
        &mut state,
        Message::FilePicked(request_of(&effects), Some(canonical.clone())),
    );
    assert!(effects.is_empty());
    let prompt = state.overwrite_prompt.as_ref().expect("prompt");
    assert_eq!(prompt.games, 2);
    assert!(send(&mut state, Message::AnswerOverwrite(false)).is_empty());
    assert!(state.overwrite_prompt.is_none());
    let effects = send(&mut state, Command::SaveAs);
    send(
        &mut state,
        Message::FilePicked(request_of(&effects), Some(canonical)),
    );
    let effects = send(&mut state, Message::AnswerOverwrite(true));
    assert!(matches!(
        effects.as_slice(),
        [Effect::SavePgn {
            mode: SaveMode::Overwrite,
            ..
        }]
    ));
}

#[test]
fn import_never_parses_in_update() {
    let mut state = AppState::default();
    let effects = send(&mut state, ImportMessage::Open);
    assert_eq!(
        effects,
        [Effect::ReadClipboard(ClipboardPurpose::ImportPrefill)]
    );
    send(
        &mut state,
        Message::ClipboardRead {
            purpose: ClipboardPurpose::ImportPrefill,
            result: Ok("1. e4 e5".to_owned()),
        },
    );
    assert_eq!(
        state.import.as_ref().map(|d| d.text.as_str()),
        Some("1. e4 e5")
    );
    let huge = "1. e4 e5 ".repeat(1 << 20);
    send(&mut state, ImportMessage::TextEdited(huge.clone()));
    let effects = send(&mut state, ImportMessage::Import);
    assert_eq!(
        effects,
        [Effect::ScanPgn {
            request: RequestId(1),
            source: PgnSource::Text(huge),
        }]
    );
    assert_eq!(state.session.game().mainline().count(), 0);
}

#[test]
fn import_loads_the_game_and_closes_only_when_clean() {
    let mut state = AppState::default();
    send(&mut state, ImportMessage::Open);
    send(
        &mut state,
        ImportMessage::TextEdited("[Event \"x\"]\n\n1. e4 e5 2. Ke4 *".to_owned()),
    );
    let request = request_of(&send(&mut state, ImportMessage::Import));
    // A stale answer is ignored.
    let stale = send(
        &mut state,
        Message::PgnScanned {
            request: RequestId(request.0 + 7),
            path: None,
            result: Ok(scanned("1. d4", None)),
        },
    );
    assert!(stale.is_empty());
    assert_eq!(state.session.game().mainline().count(), 0);
    send(
        &mut state,
        Message::PgnScanned {
            request,
            path: None,
            result: Ok(scanned("[Event \"x\"]\n\n1. e4 e5 2. Ke4 *", None)),
        },
    );
    assert_eq!(state.session.game().mainline().count(), 2);
    assert!(state.is_dirty(), "pasted games count as unsaved");
    assert_eq!(state.origin, GameOrigin::Clipboard);
    let dialog = state
        .import
        .as_ref()
        .expect("dialog stays open with errors");
    assert!(
        dialog
            .report
            .as_ref()
            .is_some_and(ImportReport::has_problems)
    );
    send(&mut state, ImportMessage::Close);
    assert!(state.import.is_none());

    // A clean import closes the dialog.
    send(&mut state, ImportMessage::Open);
    send(
        &mut state,
        ImportMessage::TextEdited("[Event \"y\"]\n\n1. d4 *".to_owned()),
    );
    let request = request_of(&send(&mut state, ImportMessage::Import));
    send(
        &mut state,
        Message::PgnScanned {
            request,
            path: None,
            result: Ok(scanned("[Event \"y\"]\n\n1. d4 *", None)),
        },
    );
    // The altered game asks first.
    assert!(state.discard_prompt.is_some());
    assert!(state.import.is_some());
    send(&mut state, Message::AnswerDiscard(DiscardChoice::Cancel));
    assert!(state.import.is_some(), "cancel keeps the dialog");
    send(
        &mut state,
        Message::PgnScanned {
            request,
            path: None,
            result: Ok(scanned("[Event \"y\"]\n\n1. d4 *", None)),
        },
    );
}

#[test]
fn import_of_nothing_reports_and_keeps_the_game() {
    let mut state = AppState::default();
    play(&mut state, "e2e4");
    send(&mut state, ImportMessage::Open);
    let request = request_of(&send(&mut state, ImportMessage::Import));
    send(
        &mut state,
        Message::PgnScanned {
            request,
            path: None,
            result: Ok(scanned("   ", None)),
        },
    );
    let report = state
        .import
        .as_ref()
        .and_then(|d| d.report.clone())
        .expect("report");
    assert!(
        report
            .notices
            .iter()
            .any(|n| n.issue == ImportIssue::NothingToImport)
    );
    assert_eq!(state.session.game().mainline().count(), 1);
}

#[test]
fn multi_game_import_opens_the_chooser() {
    let mut state = AppState::default();
    send(&mut state, ImportMessage::Open);
    send(&mut state, ImportMessage::TextEdited(TWO_GAMES.to_owned()));
    let request = request_of(&send(&mut state, ImportMessage::Import));
    send(
        &mut state,
        Message::PgnScanned {
            request,
            path: None,
            result: Ok(scanned(TWO_GAMES, None)),
        },
    );
    assert!(state.chooser.is_some());
    assert_eq!(state.session.game().mainline().count(), 0);
}

#[test]
fn paste_current_game_inserts_pgn() {
    let mut state = AppState::default();
    play(&mut state, "e2e4");
    send(&mut state, ImportMessage::Open);
    send(&mut state, ImportMessage::PasteCurrentGame);
    let text = state
        .import
        .as_ref()
        .map(|d| d.text.clone())
        .expect("dialog");
    assert!(text.contains("1. e4"), "{text}");
    send(&mut state, ImportMessage::Clear);
    assert_eq!(state.import, Some(ImportDialog::default()));
}

#[test]
fn setup_dialog_edits_a_draft_and_ok_replaces_the_game() {
    let mut state = AppState::default();
    play(&mut state, "e2e4");
    send(&mut state, Command::SetupBoard);
    assert!(state.setup.is_some());
    // The draft starts from the cursor position.
    assert_eq!(
        state
            .setup
            .as_ref()
            .and_then(|s| s.analysis.position())
            .map(Position::fen),
        Some(state.session.position().fen())
    );
    send(&mut state, SetupMessage::Clear);
    assert_eq!(
        state.session.game().mainline().count(),
        1,
        "the game is untouched"
    );
    send(&mut state, SetupMessage::Cancel);
    assert!(state.setup.is_none());
    assert_eq!(state.session.game().mainline().count(), 1);

    send(&mut state, Command::SetupBoard);
    send(&mut state, SetupMessage::Clear);
    send(&mut state, SetupMessage::Ok);
    // Altered game: the question comes first, the dialog stays.
    assert!(state.discard_prompt.is_some() && state.setup.is_some());
    send(&mut state, Message::AnswerDiscard(DiscardChoice::DontSave));
    assert!(state.setup.is_none());
    assert_eq!(
        state.session.position().fen(),
        "8/1k6/8/8/8/8/6K1/8 w - - 0 1"
    );
    assert!(!state.is_dirty());
    assert_eq!(state.setup_history.len(), 1);
}

#[test]
fn invalid_setup_cannot_be_accepted() {
    let mut state = AppState::default();
    send(&mut state, Command::SetupBoard);
    send(
        &mut state,
        SetupMessage::RightClickSquare(mychess_core::Square::E1),
    );
    assert!(send(&mut state, SetupMessage::Ok).is_empty());
    assert!(state.setup.is_some());
    assert!(state.discard_prompt.is_none());
}

#[test]
fn game_info_apply_is_one_undo_step() {
    let mut state = AppState::default();
    send(&mut state, GameInfoMessage::Open);
    send(
        &mut state,
        GameInfoMessage::SetText(TagField::White, "Carlsen, M.".to_owned()),
    );
    send(&mut state, GameInfoMessage::SetResult(GameResult::White));
    send(&mut state, GameInfoMessage::Apply);
    assert!(state.info_dialog.is_none());
    assert_eq!(state.session.game().tags().white, "Carlsen, M.");
    assert!(state.is_dirty());
    assert_eq!(state.session.undo_depth(), 1);
    assert_eq!(
        state
            .names
            .matches(crate::game_info::NameKind::Player, "car", 9)
            .len(),
        1
    );
    send(&mut state, GameMessage::Undo);
    assert_eq!(*state.session.game().tags(), Tags::default());
    assert!(!state.is_dirty());
}

#[test]
fn game_info_with_blocking_problem_changes_nothing() {
    let mut state = AppState::default();
    send(&mut state, GameInfoMessage::Open);
    send(
        &mut state,
        GameInfoMessage::SetDate(
            crate::game_info::DateTarget::Date,
            crate::game_info::DatePart::Year,
            "2048".to_owned(),
        ),
    );
    send(&mut state, GameInfoMessage::Apply);
    let dialog = state.info_dialog.as_ref().expect("still open");
    assert!(!dialog.issues.is_empty());
    assert!(!state.is_dirty());
    // The game is not editable while the dialog is open.
    let before = state.session.cursor();
    send(&mut state, GameMessage::Navigate(Nav::GameEnd));
    assert_eq!(state.session.cursor(), before);
    send(&mut state, GameInfoMessage::Cancel);
    assert!(state.info_dialog.is_none());
}

#[test]
fn today_fills_the_date() {
    let mut state = AppState::default();
    send(&mut state, GameInfoMessage::Open);
    let effects = send(
        &mut state,
        GameInfoMessage::Today(crate::game_info::DateTarget::Date),
    );
    let request = request_of(&effects);
    let date = "2024.03.09".parse().expect("date");
    send(&mut state, Message::Today(RequestId(request.0 + 1), date));
    assert_eq!(
        state
            .info_dialog
            .as_ref()
            .map(|d| d.draft.date.year.clone()),
        Some(String::new())
    );
    send(&mut state, Message::Today(request, date));
    let draft = &state.info_dialog.as_ref().expect("dialog").draft;
    assert_eq!(
        (
            draft.date.year.as_str(),
            draft.date.month.as_str(),
            draft.date.day.as_str()
        ),
        ("2024", "03", "09")
    );
}

#[test]
fn classify_without_a_book_reports_it() {
    let mut state = AppState::default();
    send(&mut state, GameInfoMessage::Open);
    send(&mut state, GameInfoMessage::ClassifyEco);
    let dialog = state.info_dialog.as_ref().expect("dialog");
    assert!(
        dialog
            .issues
            .iter()
            .any(|i| i.kind == crate::game_info::FieldIssueKind::EcoBookNotLoaded)
    );
    assert!(dialog.draft.eco.is_empty());
}

#[test]
fn classify_with_a_book_fills_the_field() {
    let mut state = AppState::default();
    let book = mychess_core::EcoBook::parse(
        "B90a \"Sicilian, Najdorf\" 1.e4 c5 2.Nf3 d6 3.d4 cxd4 4.Nxd4 Nf6 5.Nc3 a6 *\n",
    )
    .expect("book");
    state.eco_book = Some(SharedEco::new(book));
    for uci in [
        "e2e4", "c7c5", "g1f3", "d7d6", "d2d4", "c5d4", "f3d4", "g8f6", "b1c3", "a7a6",
    ] {
        play(&mut state, uci);
    }
    send(&mut state, GameInfoMessage::Open);
    send(&mut state, GameInfoMessage::ClassifyEco);
    let eco = state
        .info_dialog
        .as_ref()
        .map(|d| d.draft.eco.clone())
        .expect("dialog");
    assert!(eco.starts_with("B90"), "{eco}");
    let view = state.game_info();
    assert_eq!(
        view.opening.map(|o| o.name),
        Some("Sicilian, Najdorf".to_owned())
    );
}

#[test]
fn preferences_save_at_once_but_never_concurrently() {
    let mut state =
        AppState::with_preferences_path(Some(PathBuf::from("cfg dir/preferences.toml")));
    let effects = send(
        &mut state,
        PrefsMessage::Set(PrefChange::Theme(ThemePref::Dark)),
    );
    assert!(matches!(
        effects.as_slice(),
        [Effect::SavePreferences { .. }]
    ));
    // While the first save runs, two more changes make one new save.
    assert!(
        send(
            &mut state,
            PrefsMessage::Set(PrefChange::Theme(ThemePref::Light))
        )
        .is_empty()
    );
    assert!(
        send(
            &mut state,
            PrefsMessage::Set(PrefChange::BoardTheme("wood".to_owned()))
        )
        .is_empty()
    );
    let effects = send(&mut state, Message::PreferencesSaved(Ok(())));
    let [Effect::SavePreferences { prefs, .. }] = effects.as_slice() else {
        panic!("one resave expected: {effects:?}");
    };
    assert_eq!(prefs.general.theme, ThemePref::Light);
    assert_eq!(prefs.board.theme, "wood");
    assert!(send(&mut state, Message::PreferencesSaved(Ok(()))).is_empty());
}

#[test]
fn choosing_an_accent_updates_the_preference_and_saves() {
    use crate::prefs::AccentPref;
    let mut state =
        AppState::with_preferences_path(Some(PathBuf::from("cfg dir/preferences.toml")));
    let effects = send(
        &mut state,
        PrefsMessage::Set(PrefChange::Accent(AccentPref::Blue)),
    );
    assert_eq!(state.prefs.general.accent, AccentPref::Blue);
    let [Effect::SavePreferences { prefs, .. }] = effects.as_slice() else {
        panic!("one save expected: {effects:?}");
    };
    assert_eq!(prefs.general.accent, AccentPref::Blue);
}

#[test]
fn geometry_waits_for_the_flush() {
    let mut state = AppState::with_preferences_path(Some(PathBuf::from("p.toml")));
    let effects = send(
        &mut state,
        Message::WindowGeometryChanged(WindowGeometry {
            width: 1000,
            height: 700,
            x: Some(10),
            y: None,
            maximized: false,
        }),
    );
    assert!(effects.is_empty());
    assert!(state.prefs_dirty);
    assert_eq!(state.prefs.window.width, Some(1000));
    assert!(matches!(
        send(&mut state, Message::FlushPreferences).as_slice(),
        [Effect::SavePreferences { .. }]
    ));
    assert!(send(&mut state, Message::FlushPreferences).is_empty());
    let layout = PrefChange::Layout(Some(crate::LayoutNode::default_layout()));
    assert!(send(&mut state, PrefsMessage::Set(layout)).is_empty());
}

#[test]
fn quit_saves_preferences_then_exits() {
    let mut state = AppState::with_preferences_path(Some(PathBuf::from("p.toml")));
    let effects = send(&mut state, Message::Quit);
    assert!(matches!(
        effects.as_slice(),
        [Effect::SavePreferences { .. }]
    ));
    let effects = send(&mut state, Message::PreferencesSaved(Ok(())));
    assert_eq!(effects, [Effect::Exit]);

    let mut state = AppState::default();
    assert_eq!(send(&mut state, Message::Quit), [Effect::Exit]);
}

#[test]
fn quit_asks_when_the_game_is_altered() {
    let mut state = AppState::default();
    play(&mut state, "e2e4");
    assert!(send(&mut state, Message::Quit).is_empty());
    assert!(state.discard_prompt.is_some());
    send(&mut state, Message::AnswerDiscard(DiscardChoice::Cancel));
    assert!(send(&mut state, Message::Quit).is_empty());
    assert_eq!(
        send(&mut state, Message::AnswerDiscard(DiscardChoice::DontSave)),
        [Effect::Exit]
    );
}

#[test]
fn startup_loads_preferences_and_applies_them() {
    let mut state = AppState::with_preferences_path(Some(PathBuf::from("p.toml")));
    let effects = send(&mut state, Message::Startup);
    assert_eq!(
        effects,
        [
            Effect::LoadBookmarks {
                path: PathBuf::from("bookmarks.toml")
            },
            Effect::LoadEngines {
                path: PathBuf::from("engines.toml")
            },
            Effect::LoadPreferences {
                path: PathBuf::from("p.toml")
            }
        ]
    );
    let mut loaded = crate::load_preferences(
        "[shortcuts]\nFlipBoard = \"Command+Shift+F\"\nNope = \"X\"\n[files]\neco_file = \"scid.eco\"\n",
    );
    loaded.prefs.playback.autoplay_delay_ms = 2000;
    let effects = send(&mut state, Message::PreferencesLoaded(loaded));
    assert_eq!(
        effects,
        [Effect::LoadEcoBook {
            path: PathBuf::from("scid.eco")
        }]
    );
    assert_eq!(
        state.session.autoplay().delay,
        std::time::Duration::from_secs(2)
    );
    assert!(
        state
            .prefs_issues
            .iter()
            .any(|i| matches!(i, PrefsIssue::Shortcut(_)))
    );
    assert_eq!(
        state
            .shortcuts
            .bindings(Action::FlipBoard)
            .first()
            .map(ToString::to_string),
        Some("Command+Shift+F".to_owned())
    );

    let mut bare = AppState::default();
    assert!(send(&mut bare, Message::Startup).is_empty());
    assert_eq!(bare.prefs_issues, [PrefsIssue::NoConfigDir]);
    send(&mut bare, Message::Startup);
    assert_eq!(bare.prefs_issues.len(), 1, "warned once");
}

#[test]
fn actions_map_to_the_scid_navigation() {
    let mut state = AppState::default();
    for uci in ["e2e4", "e7e5", "g1f3"] {
        play(&mut state, uci);
    }
    send(&mut state, Action::GoStart);
    assert_eq!(state.session.cursor(), state.session.game().root());
    send(&mut state, Action::ForwardFast);
    assert_eq!(
        state.session.cursor(),
        state.session.game().mainline().last().expect("leaf")
    );
    send(&mut state, Action::BackFast);
    assert_eq!(state.session.cursor(), state.session.game().root());
    send(&mut state, Action::FlipBoard);
    assert!(state.board_flipped);
    assert!(send(&mut state, Action::Help).is_empty());
    let effects = send(&mut state, Action::CopyFen);
    assert!(matches!(effects.as_slice(), [Effect::WriteClipboard(_)]));
    send(&mut state, Action::SetupBoard);
    assert!(state.setup.is_some());
}

#[test]
fn paste_moves_parses_through_an_effect() {
    let mut state = AppState::default();
    play(&mut state, "e2e4");
    let effects = send(&mut state, Command::PasteMoves);
    assert_eq!(effects, [Effect::ReadClipboard(ClipboardPurpose::Moves)]);
    let effects = send(
        &mut state,
        Message::ClipboardRead {
            purpose: ClipboardPurpose::Moves,
            result: Ok("1... e5 2. Nf3".to_owned()),
        },
    );
    let [
        Effect::ParseMovetext {
            request,
            start,
            text,
        },
    ] = effects.as_slice()
    else {
        panic!("parse expected");
    };
    assert_eq!(start, state.session.position());
    assert_eq!(text, "1... e5 2. Nf3");
    let outcome = MovetextOutcome {
        game: crate::shared::SharedGame::new(mychess_core::Game::new()),
        report: ImportReport::default(),
    };
    send(
        &mut state,
        Message::MovetextParsed {
            request: RequestId(request.0 + 1),
            outcome: Box::new(outcome.clone()),
        },
    );
    assert!(state.pasted_moves.is_none());
    send(
        &mut state,
        Message::MovetextParsed {
            request: *request,
            outcome: Box::new(outcome),
        },
    );
    assert!(state.pasted_moves.is_some());
}

#[test]
fn host_answers_become_notices() {
    let mut state = AppState::default();
    send(&mut state, Message::ClipboardWriteFailed);
    assert_eq!(state.notice, Some(AppNotice::ClipboardWriteFailed));
    send(&mut state, Message::HostEffectNotHandled);
    assert_eq!(state.notice, Some(AppNotice::HostEffectNotHandled));
}

#[test]
fn loaded_scan_helper_matches_the_scan_types() {
    let outcome = scanned("1. e4", None);
    assert!(matches!(outcome.load, PgnLoad::Single(_)));
}

#[test]
fn the_ask_to_replace_preference_reaches_the_session_and_keeps_the_autoplay_delay() {
    let mut state = AppState::default();
    let delay = state.session.autoplay().delay;
    assert!(state.session.settings().ask_to_replace_moves);
    send(
        &mut state,
        PrefsMessage::Set(PrefChange::AskToReplaceMoves(false)),
    );
    assert!(!state.prefs.editing.ask_to_replace_moves);
    assert!(!state.session.settings().ask_to_replace_moves);
    assert_eq!(state.session.autoplay().delay, delay, "untouched");
    send(
        &mut state,
        PrefsMessage::Set(PrefChange::ConfirmDestructive(false)),
    );
    assert!(!state.prefs.editing.confirm_destructive);
    send(
        &mut state,
        PrefsMessage::Set(PrefChange::AskToReplaceMoves(true)),
    );
    assert!(state.session.settings().ask_to_replace_moves);
}

#[test]
fn the_header_form_keeps_its_problems_up_to_date_on_every_edit() {
    use crate::game_info::{DatePart, DateTarget};
    let mut state = AppState::default();
    send(&mut state, GameInfoMessage::Open);
    let issues = |state: &AppState| state.info_dialog.as_ref().map(|d| d.issues.len());
    assert_eq!(issues(&state), Some(0));
    send(
        &mut state,
        GameInfoMessage::SetDate(DateTarget::EventDate, DatePart::Year, "2024".to_owned()),
    );
    assert_eq!(issues(&state), Some(1), "event date without a game date");
    send(&mut state, GameInfoMessage::CopyDateToEventDate);
    send(
        &mut state,
        GameInfoMessage::SetDate(DateTarget::Date, DatePart::Year, "2024".to_owned()),
    );
    assert_eq!(issues(&state), Some(0), "fixed by the next edit");
}

// ------------------------------------------------------------ M9-6: Chess960

#[test]
fn a_new_chess960_game_starts_from_the_numbered_position() {
    let mut state = AppState::default();
    send(&mut state, Command::NewChess960(0));
    let game = state.session.game();
    assert_eq!(game.variant(), mychess_core::Variant::Chess960);
    assert_eq!(game.start_position().chess960_number(), Some(0));
    assert_eq!(
        state.session.position().fen().split(' ').next(),
        Some("bbqnnrkr/pppppppp/8/8/8/8/PPPPPPPP/BBQNNRKR")
    );
    assert_eq!(state.origin, GameOrigin::New);
}

#[test]
fn chess960_position_518_keeps_the_chess960_variant() {
    let mut state = AppState::default();
    send(&mut state, Command::NewChess960(518));
    let game = state.session.game();
    assert_eq!(game.variant(), mychess_core::Variant::Chess960);
    assert_eq!(game.start_position().chess960_number(), Some(518));
}

#[test]
fn a_chess960_number_out_of_range_changes_nothing() {
    let mut state = AppState::default();
    play(&mut state, "e2e4");
    send(&mut state, Command::NewChess960(960));
    assert!(state.discard_prompt.is_none());
    assert_eq!(state.session.game().mainline().count(), 1);
}

#[test]
fn a_new_chess960_game_asks_before_discarding_an_altered_game() {
    let mut state = AppState::default();
    play(&mut state, "e2e4");
    send(&mut state, Command::NewChess960(5));
    assert!(state.discard_prompt.is_some());
    send(&mut state, Message::AnswerDiscard(DiscardChoice::DontSave));
    assert_eq!(
        state.session.game().start_position().chess960_number(),
        Some(5)
    );
}

#[test]
fn chess960_castling_plays_in_the_new_game() {
    let mut state = AppState::default();
    send(&mut state, Command::NewChess960(518));
    for uci in ["g1f3", "g8f6", "e2e4", "e7e5", "f1e2", "f8e7"] {
        play(&mut state, uci);
    }
    // Standard arrangement in Chess960 mode: castling is "king takes rook".
    let pos = state.session.position().clone();
    let castle = pos
        .legal_moves()
        .into_iter()
        .find(|m| m.is_castle())
        .expect("castling is legal");
    assert_eq!(castle.to_uci(mychess_core::CastlingMode::Chess960), "e1h1");
}
