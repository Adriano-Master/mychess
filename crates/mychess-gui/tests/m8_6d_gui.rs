//! M8-6d (GUI, spec D2370, D2372): the history rows with their review
//! button, the origin line of the panel and the button of the tactics
//! panel. The menu entry of the game list is tested in the module itself.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::too_many_lines)]

use mychess_app::calvar::{
    Assessment, CalvarMessage, CalvarNotice, CalvarPhase, CalvarStore, Eval, HistoryEntry,
    LineRecord, LoadedStore, ReviewRecord,
};
use mychess_app::engines::EngineDraft;
use mychess_app::tactics::{TacticsConfig, TacticsMessage};
use mychess_app::{Effect, Message as AppMessage, update};
use mychess_gui::i18n::{I18n, Language};
use mychess_gui::windows::calvar_dialog::CalvarUiMessage;
use mychess_gui::windows::calvar_text::{date_text, history_text, notice_text};
use mychess_gui::{Message, Shell};

fn entry(with_detail: bool) -> HistoryEntry {
    HistoryEntry {
        when: 1_700_000_000,
        score: 64,
        found: 1,
        good: 2,
        lines: 1,
        crucial: false,
        white_to_move: true,
        fen: "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1".to_owned(),
        detail: with_detail.then(|| {
            Box::new(ReviewRecord {
                good: Vec::new(),
                lines: vec![LineRecord {
                    moves: vec!["e2e4".to_owned(), "e7e5".to_owned()],
                    assessment: Assessment::WhiteSlightly,
                    first: Some(Eval::centipawns(28)),
                    end: Some(Eval::centipawns(35)),
                    engine: Some(Assessment::WhiteSlightly),
                    good_candidate: true,
                    agree_plies: 2,
                    accuracy: 1000,
                    sound: true,
                    depth_credit: 400,
                    quality: 400,
                }],
                candidates: 500,
                accuracy: 1000,
                quality: 400,
                penalty: 0,
            })
        }),
    }
}

/// A shell in English whatever the machine says (`System` would depend on it).
fn english_shell() -> Shell {
    use mychess_app::{General, LanguagePref, LoadedPreferences, Preferences};
    let mut shell = Shell::new(None);
    let prefs = Preferences {
        general: General {
            language: LanguagePref::EnUs,
            ..General::default()
        },
        ..Preferences::default()
    };
    drop(shell.update(Message::App(AppMessage::PreferencesLoaded(
        LoadedPreferences {
            prefs,
            issues: Vec::new(),
        },
    ))));
    shell
}

fn shell_with_history() -> Shell {
    let mut shell = english_shell();
    shell
        .app
        .engines
        .add(EngineDraft::new("Fake", "/fake"))
        .expect("engine");
    let mut store = CalvarStore::default();
    store.record(entry(false));
    store.record(entry(true));
    drop(update(
        &mut shell.app,
        AppMessage::Calvar(CalvarMessage::StoreLoaded(Box::new(LoadedStore {
            store,
            ..LoadedStore::default()
        }))),
    ));
    shell
}

fn render(shell: &Shell) -> iced_test::Simulator<'_, Message> {
    let element = mychess_gui::windows::main_window::view(shell);
    iced_test::Simulator::with_size(
        iced::Settings::default(),
        iced::Size::new(1300.0, 900.0),
        element,
    )
}

#[test]
fn the_dialog_lists_the_last_exercises_with_a_review_button() {
    let mut shell = shell_with_history();
    drop(shell.update(Message::App(AppMessage::Calvar(CalvarMessage::Open))));
    let view = shell.app.calvar_view();
    assert_eq!(view.history.len(), 2);
    assert_eq!(view.history[0].index, 1, "newest first");
    assert!(view.history[0].reviewable && !view.history[1].reviewable);
    let i18n = I18n::new(Language::EnUs);
    assert_eq!(
        history_text(&view.history[0], &i18n),
        "2023-11-14: 64 points, 1 of 2 good moves, 1 lines"
    );
    let mut sim = render(&shell);
    assert!(sim.find("Past exercises").is_ok());
    assert!(sim.find("Review").is_ok());
}

#[test]
fn clicking_review_asks_the_application_for_that_entry() {
    let mut shell = shell_with_history();
    drop(shell.update(Message::App(AppMessage::Calvar(CalvarMessage::Open))));
    let mut sim = render(&shell);
    sim.click("Review").expect("click");
    let messages: Vec<Message> = sim.into_messages().collect();
    assert!(
        messages.iter().any(|m| matches!(
            m,
            Message::App(AppMessage::Calvar(CalvarMessage::Review(1)))
        )),
        "{messages:?}"
    );
}

#[test]
fn the_panel_of_a_review_names_its_origin_and_offers_no_adding() {
    let mut shell = shell_with_history();
    drop(shell.update(Message::App(AppMessage::Calvar(CalvarMessage::Review(1)))));
    assert!(matches!(
        shell.app.calvar_phase(),
        CalvarPhase::Reported(64)
    ));
    assert!(shell.app.calvar_is_review());
    let mut sim = render(&shell);
    assert!(
        sim.find(iced::widget::Id::new("calvar-bar")).is_ok(),
        "the panel"
    );
    assert!(
        sim.find("Review of the exercise of 2023-11-14 (read only)")
            .is_ok()
    );
    assert!(!shell.app.calvar_can_add_to_game());
}

#[test]
fn the_tactics_panel_offers_the_exercise_from_its_problem() {
    let mut shell = english_shell();
    shell
        .app
        .engines
        .add(EngineDraft::new("Fake", "/fake"))
        .expect("engine");
    let dir = tempfile::tempdir().expect("dir");
    let file = dir.path().join("p.pgn");
    std::fs::write(
        &file,
        "[Event \"p\"]\n[SetUp \"1\"]\n[FEN \"6k1/5ppp/8/8/8/8/8/R5K1 w - - 0 1\"]\n\n*\n\n",
    )
    .unwrap();
    let effects = update(
        &mut shell.app,
        AppMessage::Tactics(TacticsMessage::Start {
            config: Box::new(TacticsConfig {
                file: Some(file),
                ..TacticsConfig::default()
            }),
            seed: 1,
        }),
    );
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let mut queue: std::collections::VecDeque<Effect> = effects.into();
    while let Some(effect) = queue.pop_front() {
        if matches!(effect, Effect::Tactics(_)) {
            let answer = runtime.block_on(mychess_app::perform(effect));
            queue.extend(update(&mut shell.app, answer));
        }
    }
    assert!(shell.app.calvar_can_start_from_tactics());
    let mut sim = render(&shell);
    sim.click("Calculate variants").expect("click");
    let messages: Vec<Message> = sim.into_messages().collect();
    // The button sends the GUI message; the shell reads the clock in its
    // `update` and dispatches `CalvarMessage::StartFromTactics` (D2375).
    assert!(
        messages
            .iter()
            .any(|m| matches!(m, Message::CalvarUi(CalvarUiMessage::FromTactics))),
        "{messages:?}"
    );
}

#[test]
fn the_new_texts_exist_in_both_languages_and_portuguese_says_variante() {
    for language in [Language::EnUs, Language::PtBr] {
        let i18n = I18n::new(language);
        for notice in [
            CalvarNotice::GameUnreadable,
            CalvarNotice::NoProblem,
            CalvarNotice::NoReview,
        ] {
            let text = notice_text(notice, &i18n);
            assert_ne!(text, notice.message_key(), "{language:?} {notice:?}");
        }
        for key in [
            "calvar-history-title",
            "calvar-history-review",
            "calvar-history-review-tip",
            "calvar-history-old",
            "calvar-from-tactics",
            "calvar-from-tactics-tip",
            "calvar-reading-game",
            "games-menu-calvar",
        ] {
            let text = i18n.tr(key);
            assert_ne!(text, key, "{language:?} {key}");
            assert!(!text.to_lowercase().contains("variaç"), "{key}: {text}");
        }
    }
    let pt = I18n::new(Language::PtBr);
    assert!(pt.tr("calvar-from-tactics").contains("variantes"));
    assert!(pt.tr("games-menu-calvar").contains("variantes"));
}

#[test]
fn dates_in_the_rows_are_utc_civil_dates() {
    assert_eq!(date_text(0), "1970-01-01");
    assert_eq!(date_text(1_700_000_000), "2023-11-14");
}
