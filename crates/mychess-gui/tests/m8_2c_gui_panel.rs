//! M8-2c (GUI): the field of games at once with the advice of the machine,
//! the extra destination, the tabs of Elo and time, and the selector of the
//! game shown when several are on (spec
//! `docs/specs/m8-2-torneios-entre-engines.md`, "M8-2c", D2500-D2516).
//! Headless: no engine process is started.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::too_many_lines)]

#[path = "m2_4_common/mod.rs"]
mod common;

use std::time::Instant;

use common::Rig;
use iced::Size;
use mychess_app::engines::EngineDraft;
use mychess_app::tournament::{
    Adjudication, ExtraDestination, Loaded, Participant, SearchOutcome, SideReady, SystemResources,
    TimeControl, TournamentConfig, TournamentMessage,
};
use mychess_app::{Effect, Message as AppMessage};
use mychess_core::{GameResult, PartialDate};
use mychess_gui::windows::tournament_draft::{DraftEdit, TField};
use mychess_gui::windows::tournament_ui::{TournamentTab, TournamentUiMessage};
use mychess_gui::{Message, UiMessage};

const SIZE: Size = Size::new(1400.0, 900.0);

fn rig_with(names: &[&str]) -> Rig {
    let mut rig = Rig::new();
    for name in names {
        rig.shell
            .app
            .engines
            .add(EngineDraft::new(*name, "/fake/engine"))
            .expect("engine added");
    }
    rig
}

fn has(rig: &Rig, label: &str) -> bool {
    rig.sim(SIZE).find(label).is_ok()
}

fn app(rig: &mut Rig, message: TournamentMessage) -> Vec<Effect> {
    let effects = mychess_app::update(&mut rig.shell.app, AppMessage::Tournament(message));
    rig.sync();
    effects
}

fn machine(rig: &mut Rig, cores: u32) {
    app(
        rig,
        TournamentMessage::Resources(SystemResources {
            cores,
            ram_available_mb: Some(15_000),
        }),
    );
}

fn config(rig: &Rig, lanes: u32) -> TournamentConfig {
    TournamentConfig {
        participants: rig
            .shell
            .app
            .engines
            .engines()
            .iter()
            .map(|e| Participant::new(e.id.0, &e.name))
            .collect(),
        time: TimeControl::Depth(1),
        adjudication: Adjudication {
            max_plies: Some(6),
            ..Adjudication::default()
        },
        concurrency: lanes,
        ..TournamentConfig::default()
    }
}

/// Starts a tournament without running the effects of the engines; returns
/// the effects of the start.
fn start(rig: &mut Rig, dir: &std::path::Path, lanes: u32) -> Vec<Effect> {
    let config = config(rig, lanes);
    let effects = app(
        rig,
        TournamentMessage::Start {
            config: Box::new(config),
            at: Instant::now(),
        },
    );
    let request = effects
        .iter()
        .find_map(|e| match e {
            Effect::TournamentLoad { request, .. } => Some(*request),
            _ => None,
        })
        .expect("the start asks for the files");
    let probed = effects.iter().any(|e| matches!(e, Effect::TournamentProbe));
    let loaded = Loaded {
        book: None,
        book_unreadable: false,
        suite: None,
        slug: "test-cup".to_owned(),
        dir: dir.to_path_buf(),
        today: PartialDate::new(Some(2026), Some(10), Some(24)).expect("date"),
        seed: 1,
        tablebase: None,
    };
    let effects = app(
        rig,
        TournamentMessage::Loaded {
            request,
            result: Box::new(Ok(loaded)),
            at: Instant::now(),
        },
    );
    if probed {
        // The unknown machine is read first (D2501): the answer releases the
        // load that waits for it.
        return app(
            rig,
            TournamentMessage::Resources(SystemResources {
                cores: 16,
                ram_available_mb: Some(15_000),
            }),
        );
    }
    effects
}

fn prepares(effects: &[Effect]) -> Vec<(u64, [usize; 2], usize)> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::TournamentPrepare { epoch, plan, .. } => Some((*epoch, plan.sides, plan.lane)),
            _ => None,
        })
        .collect()
}

fn ready(sides: [usize; 2]) -> [SideReady; 2] {
    sides.map(|participant| SideReady {
        participant,
        engine_name: None,
        can_ponder: false,
    })
}

#[test]
fn the_form_asks_the_machine_and_advises_the_games_at_once() {
    let mut rig = rig_with(&["Alpha", "Beta", "Gamma"]);
    let outcome = rig
        .shell
        .ui
        .dialogs
        .tournament
        .update(TournamentUiMessage::Open, &rig.shell.app);
    assert!(
        outcome
            .messages
            .iter()
            .any(|m| matches!(m, AppMessage::Tournament(TournamentMessage::ProbeResources))),
        "opening the form probes the machine (D2502)"
    );
    rig.msg(Message::Tournament(TournamentUiMessage::Open));
    machine(&mut rig, 16);
    assert!(has(&rig, "Games at once"));
    // Threads and Hash unset: 64 MB assumed, 14 usable cores.
    assert!(
        has(&rig, "advised 7, at most 14 on this machine"),
        "the hint follows the machine"
    );
}

#[test]
fn the_field_of_games_at_once_edits_the_draft_and_the_estimate() {
    let mut rig = rig_with(&["Alpha", "Beta", "Gamma"]);
    rig.msg(Message::Tournament(TournamentUiMessage::Open));
    rig.msg(Message::Tournament(TournamentUiMessage::Edit(
        DraftEdit::Number(TField::Concurrency, "3".to_owned()),
    )));
    let draft = rig
        .shell
        .ui
        .dialogs
        .tournament
        .draft
        .as_ref()
        .expect("form");
    assert_eq!(draft.to_config().concurrency, 3);
    assert_eq!(draft.estimate().lanes, 3);
    // A value of zero is refused with its own message.
    rig.msg(Message::Tournament(TournamentUiMessage::Edit(
        DraftEdit::Number(TField::Concurrency, "0".to_owned()),
    )));
    assert!(has(&rig, "Games at once must be between 1 and 16."));
}

#[test]
fn the_extra_destination_is_chosen_in_the_form() {
    let mut rig = rig_with(&["Alpha", "Beta"]);
    rig.msg(Message::Tournament(TournamentUiMessage::Open));
    for label in ["Only the PGN", "Current database", "Clipbase"] {
        assert!(has(&rig, label), "{label}");
    }
    rig.msg(Message::Tournament(TournamentUiMessage::Edit(
        DraftEdit::Destination(ExtraDestination::Clipbase),
    )));
    let draft = rig
        .shell
        .ui
        .dialogs
        .tournament
        .draft
        .as_ref()
        .expect("form");
    assert_eq!(draft.to_config().destination, ExtraDestination::Clipbase);
}

#[test]
fn the_form_speaks_portuguese_about_the_new_fields() {
    let mut rig = Rig::with_language(mychess_app::LanguagePref::PtBr);
    for name in ["Alpha", "Beta"] {
        rig.shell
            .app
            .engines
            .add(EngineDraft::new(name, "/fake/engine"))
            .expect("engine added");
    }
    rig.msg(Message::Tournament(TournamentUiMessage::Open));
    for label in [
        "Partidas ao mesmo tempo",
        "Copiar as partidas para",
        "Só o PGN",
    ] {
        assert!(has(&rig, label), "{label}");
    }
}

#[test]
fn the_tabs_of_elo_and_time_are_there_with_their_tables() {
    let mut rig = rig_with(&["Alpha", "Beta"]);
    let dir = tempfile::tempdir().expect("tempdir");
    start(&mut rig, dir.path(), 1);
    for (tab, probes) in [
        (
            TournamentTab::Ratings,
            vec!["Elo", "LOS %", "Performance", "Sample", "Export"],
        ),
        (
            TournamentTab::Time,
            vec!["Mean", "Overhead", "Verdict", "Export"],
        ),
    ] {
        rig.msg(Message::Tournament(TournamentUiMessage::Tab(tab)));
        for probe in probes {
            assert!(has(&rig, probe), "{tab:?} shows {probe:?}");
        }
    }
    // The tabs are in the row of choices.
    assert!(has(&rig, "Time") && has(&rig, "Elo"));
}

#[test]
fn a_finished_game_shows_in_the_table_of_elo() {
    let mut rig = rig_with(&["Alpha", "Beta"]);
    let dir = tempfile::tempdir().expect("tempdir");
    let effects = start(&mut rig, dir.path(), 1);
    let (epoch, sides, _) = prepares(&effects)[0];
    // The engines are ready: the first search is asked.
    let effects = app(
        &mut rig,
        TournamentMessage::Prepared {
            epoch,
            result: Ok(ready(sides)),
            at: Instant::now(),
        },
    );
    let job_epoch = effects
        .iter()
        .find_map(|e| match e {
            Effect::TournamentSearch { job, .. } => Some(job.epoch),
            _ => None,
        })
        .expect("a search");
    // The user decides the game: White wins.
    app(
        &mut rig,
        TournamentMessage::Adjudicate(GameResult::White, Instant::now()),
    );
    app(
        &mut rig,
        TournamentMessage::Searched {
            epoch: job_epoch,
            outcome: SearchOutcome::Cancelled,
            at: Instant::now(),
        },
    );
    assert_eq!(rig.shell.app.tournament().unwrap().progress().0, 1);
    rig.msg(Message::Tournament(TournamentUiMessage::Tab(
        TournamentTab::Ratings,
    )));
    // A single game: one side is at +inf, the other at -inf, and the note
    // about the few games is there.
    assert!(has(&rig, "+inf"));
    assert!(has(&rig, "-inf"));
    assert!(has(&rig, "very uncertain"));
    // The report of time counts no move (an adjudicated game has none).
    rig.msg(Message::Tournament(TournamentUiMessage::Tab(
        TournamentTab::Time,
    )));
    assert!(has(&rig, "no data"));
}

#[test]
fn several_games_at_once_offer_a_button_per_game_on_the_live_tab() {
    let mut rig = rig_with(&["Alpha", "Beta", "Gamma", "Delta"]);
    machine(&mut rig, 16);
    let dir = tempfile::tempdir().expect("tempdir");
    let effects = start(&mut rig, dir.path(), 2);
    let lanes = prepares(&effects);
    assert_eq!(lanes.len(), 2, "two games are prepared at once");
    assert_eq!(lanes.iter().map(|p| p.2).collect::<Vec<_>>(), [0, 1]);
    for (epoch, sides, _) in lanes {
        app(
            &mut rig,
            TournamentMessage::Prepared {
                epoch,
                result: Ok(ready(sides)),
                at: Instant::now(),
            },
        );
    }
    rig.msg(Message::Tournament(TournamentUiMessage::Tab(
        TournamentTab::Live,
    )));
    let lanes = rig.shell.app.tournament().unwrap().lanes();
    assert_eq!(lanes.len(), 2);
    assert!(lanes[0].watched && !lanes[1].watched);
    // One button per game, named by the number and the players.
    let names = ["Alpha", "Beta", "Gamma", "Delta"];
    let label = |l: &mychess_app::tournament::LaneView| {
        format!("#{} {} - {}", l.number, names[l.white], names[l.black])
    };
    assert!(has(&rig, &label(&lanes[0])), "first game");
    assert!(has(&rig, &label(&lanes[1])), "second game");
    let mut sim = rig.sim(SIZE);
    sim.click(label(&lanes[1])).expect("the button");
    let messages: Vec<Message> = sim.into_messages().collect();
    assert!(
        messages
            .iter()
            .any(|m| matches!(m, Message::Tournament(TournamentUiMessage::Watch(1)))),
        "{messages:?}"
    );
    // The choice reaches the application, which shows the other game.
    rig.msg(Message::Tournament(TournamentUiMessage::Watch(1)));
    let live = rig.shell.app.tournament().unwrap().live().expect("live");
    assert_eq!((live.lane, live.number), (1, lanes[1].number));
}

#[test]
fn one_game_at_a_time_shows_no_selector() {
    let mut rig = rig_with(&["Alpha", "Beta"]);
    let dir = tempfile::tempdir().expect("tempdir");
    let effects = start(&mut rig, dir.path(), 1);
    let (epoch, sides, _) = prepares(&effects)[0];
    app(
        &mut rig,
        TournamentMessage::Prepared {
            epoch,
            result: Ok(ready(sides)),
            at: Instant::now(),
        },
    );
    assert!(!has(&rig, "#1 Alpha - Beta"));
}

#[test]
fn closing_the_panel_with_games_at_once_keeps_the_tournament() {
    let mut rig = rig_with(&["Alpha", "Beta", "Gamma"]);
    machine(&mut rig, 16);
    let dir = tempfile::tempdir().expect("tempdir");
    start(&mut rig, dir.path(), 2);
    rig.msg(Message::Ui(UiMessage::TogglePane(
        mychess_gui::windows::layout::PaneKind::Tournament,
    )));
    assert!(rig.shell.app.tournament_live());
    assert_eq!(rig.shell.app.tournament().unwrap().lane_count(), 2);
}
