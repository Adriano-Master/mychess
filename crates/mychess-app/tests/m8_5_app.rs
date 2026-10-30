//! Contract tests of the opening trainer in the application (spec M8-5,
//! D2229-D2237): the real effects, the editor, the other modes and the
//! store.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod m3_4a_common;

use std::path::{Path, PathBuf};

use m3_4a_common::*;
use mychess_app::online::{OnlinePhase, PlayingState};
use mychess_app::opening_trainer::{
    OpeningConfig, OpeningMessage, OpeningNotice, OpeningPhase, RepSourceKind, STORE_FILE,
    TrainSide, load_file,
};
use mychess_app::tactics::{TacticsConfig, TacticsMessage, TacticsPhase};
use mychess_app::{Command, PlayOptions};
use mychess_core::Square;

const PGN: &str = "[Event \"a\"]\n[Result \"*\"]\n\n1. e4 e5 2. Nf3 Nc6 3. Bb5 *\n";

fn typed_base(dir: &Path, name: &str, base_type: u32, pgn: &str) -> PathBuf {
    use mychess_db::si4::Si4Writer;
    use mychess_db::{CancelToken, ImportOptions, NoProgress, import_pgn};
    let base = dir.join(name);
    let mut writer = Si4Writer::create(&base, base_type).expect("create si4");
    import_pgn(
        pgn.as_bytes(),
        &mut writer,
        &ImportOptions::default(),
        None,
        &mut NoProgress,
        &CancelToken::new(),
    )
    .expect("import");
    drop(writer);
    base.with_extension("si4")
}

fn typed(side: TrainSide) -> OpeningConfig {
    OpeningConfig {
        side,
        source: RepSourceKind::TypedBase,
        ..OpeningConfig::default()
    }
}

fn file_config(path: &Path) -> OpeningConfig {
    OpeningConfig {
        source: RepSourceKind::File,
        file: Some(path.to_path_buf()),
        ..OpeningConfig::default()
    }
}

fn start(rig: &mut Rig, config: OpeningConfig) {
    rig.send(Message::OpeningTrainer(OpeningMessage::Start {
        config: Box::new(config),
        seed: 7,
        today: 100,
    }));
}

fn opening(rig: &mut Rig, message: OpeningMessage) {
    rig.send(Message::OpeningTrainer(message));
}

fn user_move(rig: &mut Rig, from: &str, to: &str) {
    let (from, to): (Square, Square) = (from.parse().unwrap(), to.parse().unwrap());
    rig.send(GameMessage::PlaySquares {
        from,
        to,
        promotion: None,
        options: PlayOptions::default(),
    });
}

fn event(rig: &Rig) -> String {
    rig.state.session.game().tags().event.clone()
}

fn phase(rig: &Rig) -> OpeningPhase {
    rig.state.opening_phase()
}

#[test]
fn the_typed_base_is_the_repertoire_and_the_line_runs_in_the_editor() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open(&write(dir.path(), "other.pgn", TWO));
    rig.open(&typed_base(dir.path(), "white", 17, PGN));
    start(&mut rig, typed(TrainSide::White));
    assert_eq!(phase(&rig), OpeningPhase::UserTurn);
    assert_eq!(event(&rig), "Opening Trainer");
    assert!(!rig.state.board_flipped);

    user_move(&mut rig, "e2", "e4");
    assert_eq!(rig.plies(), 2, "e4 and the reply e5");
    assert_eq!(phase(&rig), OpeningPhase::UserTurn);
    // A move out of the repertoire does not enter the game.
    user_move(&mut rig, "a2", "a3");
    assert_eq!(rig.plies(), 2);
    assert_eq!(
        rig.state.opening_view().notice,
        Some(OpeningNotice::OutOfRepertoire)
    );
    user_move(&mut rig, "g1", "f3");
    user_move(&mut rig, "f1", "b5");
    assert!(matches!(phase(&rig), OpeningPhase::LineDone(_)));
    assert_eq!(rig.plies(), 5);
}

#[test]
fn black_looks_for_the_black_base_and_turns_the_board() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open(&typed_base(dir.path(), "white", 17, PGN));
    start(&mut rig, typed(TrainSide::Black));
    assert_eq!(
        phase(&rig),
        OpeningPhase::Closed,
        "there is no base for Black"
    );
    assert_eq!(
        rig.state.opening_view().notice,
        Some(OpeningNotice::NoRepertoire)
    );

    rig.open(&typed_base(dir.path(), "black", 18, PGN));
    start(&mut rig, typed(TrainSide::Black));
    assert_eq!(phase(&rig), OpeningPhase::UserTurn);
    assert!(rig.state.board_flipped);
    assert_eq!(rig.plies(), 1, "the machine opened with e4");
}

#[test]
fn an_either_colour_base_serves_both_sides() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open(&typed_base(dir.path(), "either", 19, PGN));
    start(&mut rig, typed(TrainSide::Both));
    assert_eq!(phase(&rig), OpeningPhase::UserTurn);
}

#[test]
fn without_a_typed_base_nothing_starts_and_nothing_changes() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open(&write(dir.path(), "other.pgn", TWO));
    rig.send(GameMessage::Navigate(Nav::GameEnd));
    let before = rig.plies();
    start(&mut rig, typed(TrainSide::White));
    assert_eq!(phase(&rig), OpeningPhase::Closed);
    assert_eq!(
        rig.state.opening_view().notice,
        Some(OpeningNotice::NoRepertoire)
    );
    assert_ne!(event(&rig), "Opening Trainer");
    assert_eq!(rig.plies(), before);
}

#[test]
fn the_game_of_the_user_is_parked_and_comes_back_with_its_changes() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open(&write(dir.path(), "a.pgn", TWO));
    rig.send(BasesMessage::LoadGame {
        slot: sel(1),
        number: 1,
    });
    rig.play("g1f3");
    assert!(rig.state.is_dirty());
    let plies = rig.plies();
    let repertoire = write(dir.path(), "rep.pgn", PGN);
    start(&mut rig, file_config(&repertoire));
    // The game has changes: the user is asked first, "don't save" goes on.
    assert_eq!(phase(&rig), OpeningPhase::Closed, "waiting for the answer");
    rig.send(Message::AnswerDiscard(DiscardChoice::DontSave));
    assert_eq!(phase(&rig), OpeningPhase::UserTurn);
    assert_eq!(event(&rig), "Opening Trainer");

    opening(&mut rig, OpeningMessage::Close);
    assert_eq!(phase(&rig), OpeningPhase::Closed);
    assert_eq!(event(&rig), "E1");
    assert_eq!(rig.plies(), plies);
    assert!(
        rig.state.is_dirty(),
        "nothing was lost, it is still unsaved"
    );
}

#[test]
fn cancelling_the_question_changes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open(&write(dir.path(), "a.pgn", TWO));
    rig.send(BasesMessage::LoadGame {
        slot: sel(1),
        number: 1,
    });
    rig.play("g1f3");
    let repertoire = write(dir.path(), "rep.pgn", PGN);
    start(&mut rig, file_config(&repertoire));
    rig.send(Message::AnswerDiscard(DiscardChoice::Cancel));
    assert_eq!(phase(&rig), OpeningPhase::Closed);
    assert_eq!(event(&rig), "E1");
    assert!(rig.state.is_dirty());
}

#[test]
fn replacing_the_game_ends_the_session_and_the_question_protects_the_user_game() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open(&write(dir.path(), "a.pgn", TWO));
    rig.send(BasesMessage::LoadGame {
        slot: sel(1),
        number: 1,
    });
    rig.play("g1f3");
    let repertoire = write(dir.path(), "rep.pgn", PGN);
    start(&mut rig, file_config(&repertoire));
    rig.send(Message::AnswerDiscard(DiscardChoice::DontSave));
    assert_eq!(phase(&rig), OpeningPhase::UserTurn);

    rig.send(Command::NewGame);
    assert_eq!(phase(&rig), OpeningPhase::Closed);
    // The game of the user came back marked as unsaved, so the normal
    // question is asked before it is replaced.
    assert!(rig.state.discard_prompt.is_some());
    rig.send(Message::AnswerDiscard(DiscardChoice::Cancel));
    assert_eq!(event(&rig), "E1");
    assert!(rig.state.is_dirty());
}

#[test]
fn switching_slots_ends_the_session_and_gives_the_game_back() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open(&write(dir.path(), "a.pgn", TWO));
    rig.open(&write(dir.path(), "b.pgn", TWO));
    rig.select(1);
    rig.send(BasesMessage::LoadGame {
        slot: sel(1),
        number: 1,
    });
    let repertoire = write(dir.path(), "rep.pgn", PGN);
    start(&mut rig, file_config(&repertoire));
    assert_eq!(phase(&rig), OpeningPhase::UserTurn);
    rig.select(2);
    assert_eq!(phase(&rig), OpeningPhase::Closed);
    rig.select(1);
    assert_eq!(event(&rig), "E1");
}

#[test]
fn the_editor_is_locked_while_it_is_the_turn_of_the_user() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    let repertoire = write(dir.path(), "rep.pgn", PGN);
    start(&mut rig, file_config(&repertoire));
    assert_eq!(phase(&rig), OpeningPhase::UserTurn);
    rig.send(GameMessage::Navigate(Nav::Start));
    assert_eq!(rig.state.opening_view().notice, Some(OpeningNotice::Locked));
    // At the end of the line the game is free again.
    for (from, to) in [("e2", "e4"), ("g1", "f3"), ("f1", "b5")] {
        user_move(&mut rig, from, to);
    }
    assert!(matches!(phase(&rig), OpeningPhase::LineDone(_)));
    rig.send(GameMessage::Navigate(Nav::Start));
    assert_eq!(rig.state.session.position().fullmove_number(), 1);
}

#[test]
fn the_trainer_and_the_tactics_training_exclude_each_other() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    let repertoire = write(dir.path(), "rep.pgn", PGN);
    let problems = write(
        dir.path(),
        "p.pgn",
        "[Event \"one\"]\n[SetUp \"1\"]\n[FEN \"6k1/5ppp/8/8/8/8/8/R5K1 w - - 0 1\"]\n\n*\n",
    );
    start(&mut rig, file_config(&repertoire));
    assert_eq!(phase(&rig), OpeningPhase::UserTurn);
    rig.send(Message::Tactics(TacticsMessage::Start {
        config: Box::new(TacticsConfig {
            file: Some(problems.clone()),
            ..TacticsConfig::default()
        }),
        seed: 1,
    }));
    assert_eq!(rig.state.tactics_phase(), TacticsPhase::Closed);
    assert_eq!(phase(&rig), OpeningPhase::UserTurn, "the first one stays");

    opening(&mut rig, OpeningMessage::Close);
    rig.send(Message::Tactics(TacticsMessage::Start {
        config: Box::new(TacticsConfig {
            file: Some(problems),
            ..TacticsConfig::default()
        }),
        seed: 1,
    }));
    assert_eq!(rig.state.tactics_phase(), TacticsPhase::Solving);
    start(&mut rig, file_config(&repertoire));
    assert_eq!(phase(&rig), OpeningPhase::Closed);
    assert_eq!(rig.state.opening_view().notice, Some(OpeningNotice::Busy));
}

#[test]
fn the_opening_family_is_locked_during_an_online_game() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    let repertoire = write(dir.path(), "rep.pgn", PGN);
    rig.state.online.phase = OnlinePhase::Playing(PlayingState::Live);
    assert!(rig.state.assist_locked());
    start(&mut rig, file_config(&repertoire));
    assert_eq!(phase(&rig), OpeningPhase::Closed);
    opening(&mut rig, OpeningMessage::Open);
    assert!(!rig.state.opening_dialog_open());
    assert!(matches!(
        rig.state.notice,
        Some(AppNotice::Online(
            mychess_app::online::OnlineNotice::AssistanceDisabled
        ))
    ));
}

#[test]
fn a_session_that_is_on_ends_when_the_online_lock_comes_on() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    let repertoire = write(dir.path(), "rep.pgn", PGN);
    start(&mut rig, file_config(&repertoire));
    assert_eq!(phase(&rig), OpeningPhase::UserTurn);
    rig.state.online.phase = OnlinePhase::Playing(PlayingState::Live);
    // Any message runs the guard.
    rig.send(GameMessage::DismissPrompt);
    assert_eq!(phase(&rig), OpeningPhase::Closed);
}

#[test]
fn the_score_is_written_beside_the_preferences_and_read_back() {
    let dir = tempfile::tempdir().unwrap();
    let prefs = dir.path().join("preferences.toml");
    let mut rig = Rig::new();
    rig.state = AppState::with_preferences_path(Some(prefs));
    let repertoire = write(dir.path(), "rep.pgn", PGN);
    start(&mut rig, file_config(&repertoire));
    user_move(&mut rig, "e2", "e4");
    opening(&mut rig, OpeningMessage::Close);

    let loaded = load_file(&dir.path().join(STORE_FILE));
    assert!(!loaded.unreadable && !loaded.read_only);
    assert_eq!(loaded.store.config.source, RepSourceKind::File);
    let key = format!("{}|w", mychess_app::tactics::source_key(&repertoire));
    assert!(
        loaded.store.recorded(&key) >= 1,
        "the position was recorded"
    );
    // A new session reads it back through the effect.
    let mut again = Rig::new();
    again.state = AppState::with_preferences_path(Some(dir.path().join("preferences.toml")));
    opening(&mut again, OpeningMessage::Open);
    assert_eq!(
        again.state.opening_view().config.source,
        RepSourceKind::File
    );
}

#[test]
fn a_store_of_a_newer_version_is_never_written_over() {
    let dir = tempfile::tempdir().unwrap();
    let store = dir.path().join(STORE_FILE);
    let original = "version = 99\n[config]\nside = \"black\"\n";
    std::fs::write(&store, original).unwrap();
    let mut rig = Rig::new();
    rig.state = AppState::with_preferences_path(Some(dir.path().join("preferences.toml")));
    let repertoire = write(dir.path(), "rep.pgn", PGN);
    opening(&mut rig, OpeningMessage::Open);
    assert_eq!(
        rig.state.opening_view().store_issue,
        Some(OpeningNotice::StoreNewer)
    );
    start(&mut rig, file_config(&repertoire));
    user_move(&mut rig, "e2", "e4");
    opening(&mut rig, OpeningMessage::Close);
    assert_eq!(std::fs::read_to_string(&store).unwrap(), original);
}

#[test]
fn a_book_and_a_mask_are_repertoires_too() {
    use mychess_book::Book;
    use mychess_core::Position;
    use mychess_db::tree::mask::{MaskKey, MaskMove, MaskPosition, TreeMask};
    let dir = tempfile::tempdir().unwrap();
    let start_pos = Position::new();
    let e4 = start_pos.parse_uci("e2e4").unwrap();
    let after = start_pos.after(e4).unwrap();
    let mut book = Book::new();
    book.set_position_moves(&start_pos, &[(e4, 100)]).unwrap();
    book.set_position_moves(&after, &[(after.parse_uci("e7e5").unwrap(), 100)])
        .unwrap();
    let bin = dir.path().join("rep.bin");
    book.save_as(&bin).unwrap();
    let mut rig = Rig::new();
    start(&mut rig, file_config(&bin));
    assert_eq!(phase(&rig), OpeningPhase::UserTurn);
    user_move(&mut rig, "e2", "e4");
    assert_eq!(rig.plies(), 2);
    opening(&mut rig, OpeningMessage::Close);

    let mut mask = TreeMask::new();
    mask.insert(
        MaskKey::from_position(&start_pos),
        MaskPosition {
            moves: vec![MaskMove::new("d4")],
            comment: String::new(),
        },
    );
    let stm = dir.path().join("rep.stm");
    mychess_db::tree::mask::stm::write(&stm, &mask).unwrap();
    start(&mut rig, file_config(&stm));
    assert_eq!(phase(&rig), OpeningPhase::UserTurn);
    user_move(&mut rig, "d2", "d4");
    assert!(matches!(phase(&rig), OpeningPhase::LineDone(_)));
}

#[test]
fn closing_the_base_that_is_being_read_ends_the_session() {
    // The build is asked for and its answer is held back, so the base can
    // go away in between (D2237).
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open(&typed_base(dir.path(), "white", 17, PGN));
    let effects = rig.raw(Message::OpeningTrainer(OpeningMessage::Start {
        config: Box::new(typed(TrainSide::White)),
        seed: 1,
        today: 1,
    }));
    assert!(
        effects
            .iter()
            .any(|e| matches!(e, Effect::OpeningTrainer(_)))
    );
    assert_eq!(phase(&rig), OpeningPhase::Building);
    // Closing a slot ends the training first, like the other modes (D2233);
    // the answer of the build, which comes late, is dropped.
    rig.send(BasesMessage::Close(sel(1)));
    assert_eq!(phase(&rig), OpeningPhase::Closed);
    assert!(!rig.state.opening_live());
}

#[test]
fn only_flagged_lines_keeps_the_games_with_the_opening_flag_of_the_colour() {
    use mychess_db::si4::{FlagOp, GameFlags, Si4Writer};
    let dir = tempfile::tempdir().unwrap();
    let pgn = "[Event \"a\"]\n[Result \"*\"]\n\n1. e4 e5 *\n\n[Event \"b\"]\n[Result \"*\"]\n\n1. d4 d5 *\n";
    let si4 = typed_base(dir.path(), "white", 17, pgn);
    {
        let mut writer = Si4Writer::open(&dir.path().join("white")).unwrap();
        writer
            .set_flag(2, GameFlags::WHITE_OPENING, FlagOp::Set)
            .unwrap();
    }
    let mut rig = Rig::new();
    rig.open(&si4);
    let config = OpeningConfig {
        only_flagged: true,
        ..typed(TrainSide::White)
    };
    start(&mut rig, config);
    assert_eq!(phase(&rig), OpeningPhase::UserTurn);
    user_move(&mut rig, "e2", "e4");
    assert_eq!(
        rig.plies(),
        0,
        "the unflagged game is not in the repertoire"
    );
    user_move(&mut rig, "d2", "d4");
    assert_eq!(rig.plies(), 2);
}
