//! M5-6d review fixes, GUI side (D2170-D2175): the export controls fit a
//! narrow panel in both languages, the stop button is off while the export
//! stops, the plies stepper never lands on odd extremes, and quitting stops
//! an export and waits for it (with a limit). Spec
//! `docs/specs/m5-6-janela-do-livro.md`.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod m5_2_gui_common;

use iced::advanced::widget::Id;
use iced::{Settings, Size};
use iced_test::Simulator;
use m5_2_gui_common::*;
use mychess_app::{BookMessage, BookSide, LanguagePref};
use mychess_core::Game;
use mychess_gui::windows::book_panel::{BookUiMessage, EXPORT_STOP_ID, side_id};

fn book(message: BookMessage) -> AppMessage {
    AppMessage::Book(message)
}

fn rig_in(language: LanguagePref) -> TreeRig {
    let mut rig = TreeRig::with_language(language);
    let mut game = Game::new();
    let root = game.root();
    common::line(&mut game, root, &["e2e4", "e7e5"]);
    rig.rig.set_game(game);
    rig.nav(Nav::Start);
    rig.ui(UiMessage::WindowResized(Size::new(1280.0, 800.0)));
    rig.key(command_shift(Key::Char('k')));
    for side in BookSide::ALL {
        rig.app_msg(book(BookMessage::New { side }));
        rig.app_msg(book(BookMessage::AddGameMove { side }));
    }
    rig.msg(Message::Book(BookUiMessage::ToggleTools));
    rig
}

fn panel(rig: &TreeRig, size: Size) -> Simulator<'_, Message> {
    Simulator::with_size(
        Settings::default(),
        size,
        mychess_gui::windows::book_panel::view(&rig.rig.shell),
    )
}

fn export_ids() -> Vec<String> {
    let mut ids = vec![EXPORT_STOP_ID.to_owned()];
    for name in ["plies", "lines", "weight"] {
        for way in ["down", "up"] {
            ids.push(format!("book-export-{name}-{way}"));
        }
    }
    for side in BookSide::ALL {
        ids.push(side_id(side, "export"));
    }
    ids
}

fn assert_inside(rig: &TreeRig, language: LanguagePref, note: &str) {
    for width in [280.0_f32, 240.0] {
        let mut s = panel(rig, Size::new(width, 1800.0));
        for id in export_ids() {
            let bounds = s
                .find(Id::from(id.clone()))
                .unwrap_or_else(|e| panic!("{id}: {e:?}"))
                .bounds();
            assert!(
                bounds.x >= 0.0 && bounds.x + bounds.width <= width + 0.5,
                "{id} at {width} ({language:?}, {note}): {bounds:?}"
            );
        }
    }
}

#[test]
fn the_export_controls_are_inside_a_narrow_panel_in_both_languages() {
    for language in [LanguagePref::EnUs, LanguagePref::PtBr] {
        let rig = rig_in(language);
        assert_inside(&rig, language, "idle");
    }
}

#[test]
fn the_export_controls_are_inside_a_narrow_panel_while_an_export_runs() {
    let dir = tempfile::tempdir().unwrap();
    for language in [LanguagePref::EnUs, LanguagePref::PtBr] {
        let mut rig = rig_in(language);
        // The job is asked for but its effect is not run, so it stays on.
        drop(
            rig.rig
                .shell
                .update(Message::App(book(BookMessage::ExportBranch {
                    side: BookSide::First,
                    path: dir.path().join("a.pgn"),
                }))),
        );
        assert!(rig.app().book_view().export_progress.is_some());
        assert_inside(&rig, language, "running");
    }
}

#[test]
fn the_stop_button_is_off_while_the_export_is_stopping() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = rig_in(LanguagePref::EnUs);
    drop(
        rig.rig
            .shell
            .update(Message::App(book(BookMessage::ExportBranch {
                side: BookSide::First,
                path: dir.path().join("a.pgn"),
            }))),
    );
    let stop = |rig: &TreeRig| {
        let mut s = panel(rig, SIZE);
        s.click(Id::from(EXPORT_STOP_ID.to_owned())).ok();
        s.into_messages().collect::<Vec<_>>()
    };
    let messages = stop(&rig);
    assert_eq!(messages.len(), 1, "enabled while it runs");
    for message in messages {
        rig.msg(message);
    }
    assert!(rig.app().book_view().export_stopping);
    assert!(stop(&rig).is_empty(), "disabled while it stops");
    assert!(panel(&rig, SIZE).find("Stopping the export…").is_ok());
}

#[test]
fn the_plies_stepper_never_lands_on_an_odd_extreme_from_the_default() {
    let mut rig = rig_in(LanguagePref::EnUs);
    let press = |rig: &mut TreeRig, id: &str| {
        let messages = {
            let mut s = panel(rig, SIZE);
            s.click(Id::from(id.to_owned())).unwrap();
            s.into_messages().collect::<Vec<_>>()
        };
        for message in messages {
            rig.msg(message);
        }
    };
    let mut seen = Vec::new();
    for _ in 0..8 {
        press(&mut rig, "book-export-plies-down");
        seen.push(rig.app().book_view().export.max_plies);
    }
    assert_eq!(seen, [10, 8, 6, 4, 3, 2, 1, 1]);
    for _ in 0..3 {
        press(&mut rig, "book-export-plies-up");
    }
    assert_eq!(rig.app().book_view().export.max_plies, 4);
    press(&mut rig, "book-export-plies-up");
    assert_eq!(rig.app().book_view().export.max_plies, 6);
}

// ---- D2175: quitting ----

fn quit_started(rig: &TreeRig) -> bool {
    rig.app().prefs.window.width == Some(1280)
}

fn export_rig(dir: &std::path::Path) -> (TreeRig, iced::Task<Message>) {
    let mut rig = rig_in(LanguagePref::EnUs);
    let task = rig
        .rig
        .shell
        .update(Message::App(book(BookMessage::ExportBranch {
            side: BookSide::First,
            path: dir.join("out.pgn"),
        })));
    (rig, task)
}

#[test]
fn closing_during_an_export_stops_it_and_waits_for_the_answer() {
    let dir = tempfile::tempdir().unwrap();
    let (mut rig, task) = export_rig(dir.path());
    // The books are not saved: answer "Discard" first.
    rig.ui(UiMessage::CloseRequested);
    rig.msg(Message::Book(BookUiMessage::QuitAnswer(
        mychess_gui::windows::book_panel::QuitChoice::Discard,
    )));
    assert!(!quit_started(&rig), "waits for the job");
    assert!(rig.app().book_view().export_stopping);
    // The job runs, sees the flag and answers.
    rig.drain(task);
    assert!(rig.app().book_view().export_progress.is_none());
    rig.ui(UiMessage::BookQuitTick);
    assert!(quit_started(&rig));
    let left: Vec<_> = std::fs::read_dir(dir.path()).unwrap().collect();
    assert!(left.is_empty(), "no file and no temporary: {left:?}");
}

#[test]
fn the_wait_for_the_export_has_a_limit() {
    let dir = tempfile::tempdir().unwrap();
    let (mut rig, _never_run) = export_rig(dir.path());
    rig.ui(UiMessage::CloseRequested);
    rig.msg(Message::Book(BookUiMessage::QuitAnswer(
        mychess_gui::windows::book_panel::QuitChoice::Discard,
    )));
    for _ in 0..4 {
        rig.ui(UiMessage::BookQuitTick);
        assert!(!quit_started(&rig));
    }
    rig.ui(UiMessage::BookQuitTick);
    assert!(quit_started(&rig), "five seconds at most");
}

#[test]
fn a_second_close_request_while_waiting_is_ignored() {
    let dir = tempfile::tempdir().unwrap();
    let (mut rig, _task) = export_rig(dir.path());
    rig.ui(UiMessage::CloseRequested);
    rig.msg(Message::Book(BookUiMessage::QuitAnswer(
        mychess_gui::windows::book_panel::QuitChoice::Discard,
    )));
    rig.ui(UiMessage::CloseRequested);
    assert!(!quit_started(&rig));
}
