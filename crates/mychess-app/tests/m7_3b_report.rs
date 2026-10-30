//! Contract tests of the flow of the player report (spec
//! `m7-3-relatorio-de-jogador.md`, criteria 11, 12, 13 and 16): the job, the
//! answers, the filter, the clicks, the dialog, the history and the Clipbase.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::assert_is_empty)]
mod m7_3b_common;

use m7_3b_common::*;
use mychess_app::player_report::NameMatching;
use mychess_app::{ListColumn, ListMessage};
use mychess_core::Game;
use mychess_db::GameSource as _;

use PlayerColor::{Black, White};
use PlayerStart::{Beginning, Position};

fn gen_msg(name: &str, color: PlayerColor, start: PlayerStart) -> PlayerReportMessage {
    PlayerReportMessage::Generate(Box::new(request(name, color, start)))
}

// ---- the job (criterion 11) ----

#[test]
fn generate_asks_for_one_effect_and_update_does_not_block() {
    let (mut rig, _slot, _dir) = sample();
    let effects = rig.raw(gen_msg("Alfa, A", White, Beginning));
    assert!(Rig::compute_of(&effects).is_some());
    let others: Vec<_> = effects
        .iter()
        .filter(|e| !matches!(e, Effect::PlayerReport(_)))
        .collect();
    // Only preference writes may come with it (remembering the choices).
    assert!(
        others
            .iter()
            .all(|e| matches!(e, Effect::SavePreferences { .. })),
        "{others:?}"
    );
    let view = rig.state.player_report_view();
    assert!(view.open);
    assert!(matches!(view.phase, PlayerPhase::Generating(_)));
    assert!(rig.state.player_report().is_running());
    assert!(view.report.is_none(), "nothing is computed inside update");
}

#[test]
fn generate_makes_the_report_of_the_player() {
    let (mut rig, _slot, _dir) = sample();
    rig.report("Alfa, A", White, Beginning);
    let view = rig.state.player_report_view();
    assert_eq!(view.phase, PlayerPhase::Ready);
    assert!(!view.stale);
    assert_eq!(view.database_name, Some("report.pgn"));
    let report = view.report.unwrap();
    assert_eq!(report.game_count(), 3);
    assert_eq!(report.name, "Alfa, A");
    assert_eq!(rig.lines(), vec![1, 4, 6]);
    let opponents: Vec<&str> = report.opponents.iter().map(|o| o.name.as_str()).collect();
    assert_eq!(opponents, ["Beta, B", "Gama, G", "Delta, D"]);
}

#[test]
fn black_and_the_current_position_are_passed_to_the_engine() {
    let (mut rig, _slot, _dir) = sample();
    rig.report("Alfa, A", Black, Beginning);
    assert_eq!(rig.lines(), vec![2, 7]);
    rig.report("Alfa, A", White, Position);
    assert_eq!(rig.lines(), vec![1, 4, 6]);
    let report = rig.state.player_report_view().report.unwrap();
    assert!(report.position.is_some());
    assert_eq!(report.stem.join(" "), "e4 e5 Nf3");
}

#[test]
fn the_matching_comes_from_the_request() {
    let (mut rig, _slot, _dir) = sample();
    let mut r = request("Alfa", White, Beginning);
    r.matching = NameMatching::Prefix;
    rig.send(PlayerReportMessage::Generate(Box::new(r)));
    assert_eq!(rig.lines(), vec![1, 4, 6, 8]);
    rig.report("Alfa", White, Beginning);
    assert_eq!(rig.lines(), Vec::<u32>::new(), "exact: no such name");
    assert_eq!(
        rig.state.player_report_view().report.unwrap().game_count(),
        0
    );
}

#[test]
fn an_answer_of_an_old_request_is_ignored() {
    let (mut rig, _slot, _dir) = sample();
    let first = rig.raw(gen_msg("Alfa, A", White, Beginning));
    let second = rig.raw(PlayerReportMessage::Refresh);
    let old = rig.perform(Rig::compute_of(&first).unwrap());
    rig.raw(old);
    assert!(
        matches!(
            rig.state.player_report_view().phase,
            PlayerPhase::Generating(_)
        ),
        "the old answer changes nothing"
    );
    assert!(rig.state.player_report_view().report.is_none());
    let new = rig.perform(Rig::compute_of(&second).unwrap());
    rig.raw(new);
    assert_eq!(rig.state.player_report_view().phase, PlayerPhase::Ready);
}

#[test]
fn quit_in_the_middle_stops_the_job_and_drops_the_answer() {
    let (mut rig, _slot, _dir) = sample();
    let effects = rig.raw(gen_msg("Alfa, A", White, Beginning));
    let compute = Rig::compute_of(&effects).unwrap();
    rig.raw(Message::Quit);
    assert!(!rig.state.player_report().is_running());
    let late = rig.perform(compute);
    rig.raw(late);
    assert!(rig.state.player_report_view().report.is_none());
}

#[test]
fn closing_the_slot_in_the_middle_cancels_the_job() {
    let (mut rig, slot, _dir) = sample();
    let effects = rig.raw(gen_msg("Alfa, A", White, Beginning));
    let compute = Rig::compute_of(&effects).unwrap();
    rig.raw(BasesMessage::Close(slot));
    assert!(!rig.state.player_report().is_running());
    let late = rig.perform(compute);
    rig.raw(late);
    assert!(rig.state.player_report_view().report.is_none());
}

#[test]
fn switching_the_slot_in_the_middle_cancels_the_job() {
    let (mut rig, _slot, _dir) = sample();
    let effects = rig.raw(gen_msg("Alfa, A", White, Beginning));
    let compute = Rig::compute_of(&effects).unwrap();
    rig.raw(BasesMessage::Select(SlotId::CLIPBASE));
    assert!(!rig.state.player_report().is_running());
    let late = rig.perform(compute);
    rig.raw(late);
    assert!(rig.state.player_report_view().report.is_none());
}

#[test]
fn close_stops_the_job_too() {
    let (mut rig, _slot, _dir) = sample();
    let effects = rig.raw(gen_msg("Alfa, A", White, Beginning));
    rig.raw(PlayerReportMessage::Close);
    assert!(!rig.state.player_report().is_running());
    assert!(!rig.state.player_report().is_open());
    let late = rig.perform(Rig::compute_of(&effects).unwrap());
    rig.raw(late);
    assert!(rig.state.player_report_view().report.is_none());
}

#[test]
fn stop_gives_an_interrupted_report_with_the_notice() {
    let (mut rig, _slot, _dir) = sample();
    let effects = rig.raw(gen_msg("Alfa, A", White, Beginning));
    rig.raw(PlayerReportMessage::Stop);
    let answer = rig.perform(Rig::compute_of(&effects).unwrap());
    rig.raw(answer);
    let view = rig.state.player_report_view();
    assert!(view.open);
    assert_eq!(view.phase, PlayerPhase::Ready);
    let report = view.report.unwrap();
    assert!(report.core.interrupted, "marked as interrupted");
    assert_eq!(report.game_count(), 0, "stopped before the selection ended");
    assert_eq!(rig.notice(), Some(PlayerReportNotice::Interrupted));
    // A new report is made afterwards.
    rig.send(PlayerReportMessage::Refresh);
    let view = rig.state.player_report_view();
    assert!(!view.report.unwrap().core.interrupted);
    assert_eq!(rig.lines(), vec![1, 4, 6]);
}

#[test]
fn the_progress_starts_at_zero_and_ends_complete() {
    let (mut rig, _slot, _dir) = sample();
    let effects = rig.raw(gen_msg("Alfa, A", White, Beginning));
    let PlayerPhase::Generating(before) = rig.state.player_report_view().phase else {
        panic!("generating");
    };
    assert_eq!(before.0, 0);
    let answer = rig.perform(Rig::compute_of(&effects).unwrap());
    // The numbers are read from the control of the job, which the state keeps
    // until the answer is taken.
    let PlayerPhase::Generating((done, total)) = rig.state.player_report_view().phase else {
        panic!("still generating");
    };
    assert_eq!(done, total);
    assert!(total > 0);
    rig.raw(answer);
}

#[test]
fn an_unusable_name_asks_for_nothing() {
    let (mut rig, _slot, _dir) = sample();
    for name in ["", "?", " , ", "  "] {
        let effects = rig.raw(gen_msg(name, White, Beginning));
        assert!(effects.is_empty(), "{name:?}: {effects:?}");
        assert_eq!(
            rig.notice(),
            Some(PlayerReportNotice::InvalidName),
            "{name:?}"
        );
        assert!(!rig.state.player_report().is_running());
    }
}

#[test]
fn an_empty_database_has_no_report() {
    let mut rig = Rig::new();
    let effects = rig.raw(gen_msg("Alfa, A", White, Beginning));
    assert!(effects.is_empty());
    assert_eq!(rig.notice(), Some(PlayerReportNotice::NoGames));
    rig.raw(PlayerReportMessage::OpenDialog { player: None });
    assert!(rig.state.player_report_dialog().is_none());
    assert_eq!(rig.notice(), Some(PlayerReportNotice::NoGames));
}

// ---- the filter (criterion 11) ----

#[test]
fn the_filter_of_the_slot_is_the_same_before_and_after() {
    let (mut rig, slot, _dir) = sample();
    let before = rig.state.bases.slot(slot).unwrap().filter().clone();
    let rev = rig.state.bases.slot(slot).unwrap().filter_rev();
    rig.report("Alfa, A", White, Position);
    rig.report("Alfa", Black, Beginning);
    let after = rig.state.bases.slot(slot).unwrap();
    assert_eq!(*after.filter(), before);
    assert_eq!(after.filter_rev(), rev);
    assert_eq!(after.filtered(), 8);
}

#[test]
fn a_click_sets_the_filter_with_the_ply_of_the_report() {
    let (mut rig, slot, _dir) = sample();
    // From the beginning of the games the ply is 0 (filter value 1).
    rig.report("Alfa, A", White, Beginning);
    rig.send(PlayerReportMessage::Select(
        mychess_app::opening_report::ReportSelector::All,
    ));
    {
        let s = rig.state.bases.slot(slot).unwrap();
        let games: Vec<u32> = s.filter().iter_games().collect();
        assert_eq!(games, vec![1, 4, 6]);
        assert!(games.iter().all(|g| s.filter().get(*g) == 1));
        assert_eq!(s.list.first, 1);
    }
    assert_eq!(rig.notice(), Some(PlayerReportNotice::FilterSet));
    // From the position of the board (ply 3) the value is 4.
    rig.report("Alfa, A", White, Position);
    rig.send(PlayerReportMessage::Select(
        mychess_app::opening_report::ReportSelector::Endgame(7),
    ));
    let s = rig.state.bases.slot(slot).unwrap();
    let games: Vec<u32> = s.filter().iter_games().collect();
    assert_eq!(games, vec![1, 4, 6]);
    assert!(games.iter().all(|g| s.filter().get(*g) == 4));
}

#[test]
fn a_game_reference_loads_the_game_at_the_position() {
    let (mut rig, slot, _dir) = sample();
    rig.report("Alfa, A", White, Position);
    rig.send(PlayerReportMessage::LoadGame(1));
    assert_eq!(rig.state.bases.slot(slot).unwrap().current_game, Some(1));
    assert_eq!(
        rig.state
            .session
            .game()
            .path_to(rig.state.session.cursor())
            .map_or(0, |p| p.len()),
        3
    );
}

#[test]
fn an_opponent_click_opens_his_information() {
    let (mut rig, _slot, _dir) = sample();
    rig.report("Alfa, A", White, Beginning);
    let effects = rig.raw(PlayerReportMessage::OpenOpponent("Beta, B".to_owned()));
    drop(effects);
    assert!(rig.state.player_info().is_open());
    assert_eq!(rig.state.player_info().name(), "Beta, B");
}

// ---- stale (criterion 16) ----

#[test]
fn the_report_is_stale_after_the_slot_or_the_games_change() {
    let (mut rig, slot, _dir) = sample();
    rig.report("Alfa, A", White, Beginning);
    assert!(!rig.state.player_report_view().stale);
    rig.send(BasesMessage::Select(SlotId::CLIPBASE));
    assert!(rig.state.player_report_view().stale);
    rig.send(PlayerReportMessage::Select(
        mychess_app::opening_report::ReportSelector::All,
    ));
    assert_eq!(rig.notice(), Some(PlayerReportNotice::Stale));
    rig.send(BasesMessage::Select(slot));
    assert!(!rig.state.player_report_view().stale);
    // Sorting the list is a view: nothing is stale.
    rig.send(BasesMessage::List(ListMessage::SortBy {
        slot,
        column: ListColumn::Date,
    }));
    assert!(!rig.state.player_report_view().stale);
    // The games of the Clipbase change: a report over it goes stale and
    // "Update" makes it again.
    rig.send(BasesMessage::Select(SlotId::CLIPBASE));
    rig.send(PlayerReportMessage::Refresh);
    assert!(!rig.state.player_report_view().stale);
    assert_eq!(
        rig.state.player_report_view().report.unwrap().game_count(),
        0
    );
    rig.send(BasesMessage::Select(slot));
    rig.send(BasesMessage::ClipbaseCopy);
    rig.send(BasesMessage::Select(SlotId::CLIPBASE));
    assert!(rig.state.player_report_view().stale);
    rig.send(PlayerReportMessage::Refresh);
    assert!(!rig.state.player_report_view().stale);
}

#[test]
fn update_repeats_the_same_choices() {
    let (mut rig, _slot, _dir) = sample();
    rig.report("Alfa, A", Black, Position);
    let first = rig.lines();
    rig.send(PlayerReportMessage::Refresh);
    assert_eq!(rig.lines(), first);
    let report = rig.state.player_report_view().report.unwrap();
    assert_eq!(
        (report.name.as_str(), report.color),
        ("Alfa, A", mychess_core::Color::Black)
    );
    assert!(report.position.is_some());
}

#[test]
fn update_without_a_report_says_so() {
    let (mut rig, _slot, _dir) = sample();
    let effects = rig.raw(PlayerReportMessage::Refresh);
    assert!(effects.is_empty());
    assert_eq!(rig.notice(), Some(PlayerReportNotice::NoReport));
}

// ---- the dialog and the history (criteria 12, 15) ----

#[test]
fn the_dialog_is_filled_from_the_player_and_the_game() {
    let (mut rig, _slot, _dir) = sample();
    // Game 5 is on the board: White "Delta, D", Black "Beta, B".
    rig.raw(PlayerReportMessage::OpenDialog {
        player: Some("Beta, B".to_owned()),
    });
    let dialog = rig.state.player_report_dialog().unwrap();
    assert_eq!(dialog.request.name, "Beta, B");
    assert_eq!(
        dialog.request.color, Black,
        "Black in the game on the board"
    );
    assert_eq!(dialog.white_name.as_deref(), Some("Delta, D"));
    assert_eq!(dialog.black_name.as_deref(), Some("Beta, B"));
    assert!(dialog.clipbase_enabled);
    rig.raw(PlayerReportMessage::OpenDialog {
        player: Some("Delta, D".to_owned()),
    });
    assert_eq!(
        rig.state.player_report_dialog().unwrap().request.color,
        White
    );
    // Somebody else: the last colour used.
    rig.raw(PlayerReportMessage::OpenDialog {
        player: Some("Alfa, A".to_owned()),
    });
    let dialog = rig.state.player_report_dialog().unwrap();
    assert_eq!(dialog.request.name, "Alfa, A");
    assert_eq!(
        dialog.request.color,
        PlayerReportPrefs::default().last_color
    );
    rig.raw(PlayerReportMessage::CloseDialog);
    assert!(rig.state.player_report_dialog().is_none());
}

#[test]
fn the_name_buttons_are_off_for_empty_or_unknown_names() {
    let (mut rig, _slot, _dir) = sample();
    for (white, black, expect) in [
        ("", "?", (None, None)),
        ("  ", "Beta, B", (None, Some("Beta, B"))),
        ("?", "", (None, None)),
        ("Alfa, A", "?", (Some("Alfa, A"), None)),
    ] {
        let mut game = Game::new();
        game.tags_mut().white = white.to_owned();
        game.tags_mut().black = black.to_owned();
        rig.state.session.replace_game(game);
        rig.raw(PlayerReportMessage::OpenDialog { player: None });
        let dialog = rig.state.player_report_dialog().unwrap();
        assert_eq!(
            (dialog.white_name.as_deref(), dialog.black_name.as_deref()),
            expect,
            "{white:?} x {black:?}"
        );
    }
}

#[test]
fn the_history_keeps_ten_names_without_repeats_and_is_shared_with_player_info() {
    let (mut rig, _slot, _dir) = sample();
    for i in 0..12 {
        rig.raw(gen_msg(&format!("Player {i}"), White, Beginning));
    }
    rig.raw(gen_msg("Player 5", White, Beginning));
    rig.raw(PlayerReportMessage::OpenDialog { player: None });
    let dialog = rig.state.player_report_dialog().unwrap();
    assert_eq!(dialog.history.len(), 10);
    assert_eq!(dialog.history[0], "Player 5");
    let mut unique = dialog.history.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(unique.len(), 10, "{:?}", dialog.history);
    assert_eq!(dialog.history[1], "Player 11");
    // The same list is the one of the player information.
    assert_eq!(rig.state.prefs.player_tools.history[0], "Player 5");
    // And the dialog starts with the last name used.
    assert_eq!(dialog.request.name, "Player 5");
}

#[test]
fn the_choices_are_remembered_in_the_preferences() {
    let (mut rig, _slot, _dir) = sample();
    let mut r = request("Alfa, A", Black, Position);
    r.matching = NameMatching::Contains;
    rig.raw(PlayerReportMessage::Generate(Box::new(r)));
    let p = &rig.state.prefs.player_report;
    assert_eq!(
        (p.last_color, p.last_start, p.matching),
        (Black, Position, NameMatching::Contains)
    );
    rig.raw(PlayerReportMessage::OpenDialog { player: None });
    let dialog = rig.state.player_report_dialog().unwrap();
    assert_eq!(
        (
            dialog.request.color,
            dialog.request.start,
            dialog.request.matching
        ),
        (Black, Position, NameMatching::Contains)
    );
}

// ---- the Clipbase (criterion 13) ----

fn clipbase_games(rig: &Rig) -> u32 {
    rig.state.bases.slot(SlotId::CLIPBASE).unwrap().total()
}

#[test]
fn the_clipbase_option_copies_exactly_the_games_found() {
    let (mut rig, slot, _dir) = sample();
    assert_eq!(clipbase_games(&rig), 0);
    let mut r = request("Alfa, A", White, Beginning);
    r.copy_to_clipbase = true;
    rig.send(PlayerReportMessage::Generate(Box::new(r)));
    assert_eq!(rig.lines(), vec![1, 4, 6]);
    assert_eq!(clipbase_games(&rig), 3);
    let clip = rig.state.bases.slot(SlotId::CLIPBASE).unwrap();
    let mut whites: Vec<String> = (1..=3)
        .map(|n| clip.source.lock().game(n).unwrap().tags().white.clone())
        .collect();
    whites.sort();
    assert_eq!(whites, ["Alfa, A"; 3]);
    // The slot of the report is still the current one and its filter intact.
    assert_eq!(rig.state.bases.current(), slot);
    assert_eq!(rig.state.bases.slot(slot).unwrap().filtered(), 8);
    assert!(!rig.state.player_report_view().stale);
}

#[test]
fn a_clipbase_with_games_asks_before_it_is_emptied() {
    let (mut rig, slot, _dir) = sample();
    rig.send(BasesMessage::ClipbaseCopy);
    assert_eq!(clipbase_games(&rig), 1);
    let mut r = request("Alfa, A", White, Beginning);
    r.copy_to_clipbase = true;
    // The question comes first: no job, nothing changed.
    let effects = rig.raw(PlayerReportMessage::Generate(Box::new(r.clone())));
    assert!(Rig::compute_of(&effects).is_none());
    assert!(rig.state.player_report_view().confirm_clipbase);
    assert_eq!(clipbase_games(&rig), 1);
    // "No": still nothing.
    rig.send(PlayerReportMessage::ConfirmClipbase(false));
    assert!(!rig.state.player_report_view().confirm_clipbase);
    assert_eq!(clipbase_games(&rig), 1);
    assert!(!rig.state.player_report().is_running());
    // "Yes": the report is made and the Clipbase holds the three games.
    rig.raw(PlayerReportMessage::Generate(Box::new(r)));
    rig.send(PlayerReportMessage::ConfirmClipbase(true));
    assert_eq!(clipbase_games(&rig), 3);
    assert_eq!(rig.state.bases.current(), slot);
}

#[test]
fn an_empty_clipbase_is_not_asked_about_and_update_does_not_empty_it_again() {
    let (mut rig, _slot, _dir) = sample();
    let mut r = request("Alfa, A", White, Beginning);
    r.copy_to_clipbase = true;
    rig.send(PlayerReportMessage::Generate(Box::new(r)));
    assert!(!rig.state.player_report_view().confirm_clipbase);
    assert_eq!(clipbase_games(&rig), 3);
    rig.send(BasesMessage::ClipbaseCopy);
    assert_eq!(clipbase_games(&rig), 4);
    rig.send(PlayerReportMessage::Refresh);
    assert_eq!(clipbase_games(&rig), 4, "the Clipbase is left alone");
}

#[test]
fn the_clipbase_option_is_refused_on_the_clipbase_itself() {
    let (mut rig, _slot, _dir) = sample();
    rig.send(BasesMessage::ClipbaseCopy);
    rig.send(BasesMessage::Select(SlotId::CLIPBASE));
    rig.raw(PlayerReportMessage::OpenDialog { player: None });
    let dialog = rig.state.player_report_dialog().unwrap();
    assert!(!dialog.clipbase_enabled);
    assert!(!dialog.request.copy_to_clipbase);
    let mut r = request("Alfa, A", White, Beginning);
    r.copy_to_clipbase = true;
    let effects = rig.raw(PlayerReportMessage::Generate(Box::new(r)));
    assert!(effects.is_empty());
    assert_eq!(rig.notice(), Some(PlayerReportNotice::ClipbaseSelf));
    assert_eq!(clipbase_games(&rig), 1);
}

#[test]
fn an_interrupted_report_does_not_touch_the_clipbase() {
    let (mut rig, _slot, _dir) = sample();
    rig.send(BasesMessage::ClipbaseCopy);
    let mut r = request("Alfa, A", White, Beginning);
    r.copy_to_clipbase = true;
    rig.raw(PlayerReportMessage::Generate(Box::new(r)));
    let effects = rig.raw(PlayerReportMessage::ConfirmClipbase(true));
    rig.raw(PlayerReportMessage::Stop);
    let answer = rig.perform(Rig::compute_of(&effects).unwrap());
    rig.raw(answer);
    assert_eq!(clipbase_games(&rig), 1);
}
