//! M6-4d extra (GUI): hostile names and big tables in the crosstable and the
//! rating graph, the states of the four panels not covered elsewhere, the
//! double click on a piece, the language switch with the panels open and the
//! Fluent keys of every notice (spec M6-4, D3430-D3439).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::too_many_lines)]

mod m6_4d_common;

/// Text shaping shares one font system: drawing tests run one at a time
/// (several simulators at once crash the process, see `m2_4_common`).
fn render_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

use std::fmt::Write as _;

use m6_4d_common::*;
use mychess_app::charts::{ChartsEffect, ChartsFailure, ChartsNotice};
use mychess_app::crosstable::{CrossWarning, CrosstableBody, CrosstableNotice, ExportFormat};
use mychess_app::piece_tracker::PieceTrackerNotice;
use mychess_core::Square;
use mychess_gui::i18n::{I18n, Language};

fn leak(text: String) -> &'static str {
    Box::leak(text.into_boxed_str())
}

fn hostile_games() -> Vec<Spec> {
    let long = leak("L".repeat(3000));
    let names: [&'static str; 6] = [
        "A",
        long,
        "\u{202E}Evil, E",
        "Nul\0Name, N",
        "Tab\tName, T",
        "Mix \u{202B}ed \u{2067}bidi\u{202C}, M",
    ];
    let mut games = Vec::new();
    for (n, black) in names.iter().enumerate().skip(1) {
        games.push((
            "Club",
            "Rio",
            "2020.05.01",
            leak(n.to_string()),
            names[0],
            *black,
            "1-0",
            2400,
            2300,
            "1. e4 e5 1-0",
        ));
    }
    games
}

#[test]
fn hostile_names_draw_in_the_crosstable_the_rating_graph_and_export() {
    let _render = render_lock();
    let mut rig = Rig::new();
    let slot = rig.open_games(&hostile_games());
    rig.load(slot, 1);
    for kind in [
        PaneKind::Crosstable,
        PaneKind::RatingGraph,
        PaneKind::FilterStats,
        PaneKind::PieceTracker,
    ] {
        rig.toggle(kind);
    }
    rig.app(CrosstableMessage::Refresh);
    rig.app(ChartsMessage::RefreshRating);
    rig.app(ChartsMessage::RefreshStats);
    for name in [
        "\u{202E}Evil, E",
        "Nul\0Name, N",
        "X".repeat(5000).as_str(),
        "",
    ] {
        rig.app(ChartsMessage::AddPlayer(name.to_owned()));
    }
    rig.app(ChartsMessage::RefreshRating);
    assert!(
        matches!(
            rig.shell.app.crosstable_view().body,
            CrosstableBody::Table(_) | CrosstableBody::Empty(_)
        ),
        "no crash and a defined state"
    );
    for theme in themes() {
        for (w, h) in [(300.0, 200.0), (640.0, 400.0), (1500.0, 900.0)] {
            assert!(rig.sim(w, h).snapshot(&theme).is_ok(), "{w}x{h}");
        }
    }
    for (format, ext) in [
        (ExportFormat::Text, "txt"),
        (ExportFormat::Html, "html"),
        (ExportFormat::Latex, "tex"),
    ] {
        let path = rig.dir.path().join(format!("hostile.{ext}"));
        rig.app(CrosstableMessage::Export {
            format,
            path: path.clone(),
        });
        if let Ok(text) = std::fs::read_to_string(&path)
            && format == ExportFormat::Html
        {
            assert!(
                !text.contains('\0'),
                "a NUL byte must not reach the document"
            );
        }
    }
    let host = rig.app(CrosstableMessage::Copy);
    drop(host);
}

#[test]
fn a_swiss_with_hundreds_of_players_draws_without_hanging() {
    let _render = render_lock();
    let mut text = String::new();
    for n in 0..300 {
        write!(
            text,
            "[Event \"Big\"]\n[Site \"Web\"]\n[Date \"2020.01.01\"]\n[Round \"1\"]\n\
             [White \"P{n:03}\"]\n[Black \"P{:03}\"]\n[Result \"1-0\"]\n[WhiteElo \"2000\"]\n\
             [BlackElo \"2000\"]\n\n1. e4 e5 1-0\n\n",
            (n + 1) % 300
        )
        .unwrap();
    }
    let mut rig = Rig::new();
    let path = rig.dir.path().join("big.pgn");
    std::fs::write(&path, text).unwrap();
    rig.app(BasesMessage::Open(path));
    let slot = rig.shell.app.bases.current();
    rig.load(slot, 1);
    rig.toggle(PaneKind::Crosstable);
    rig.app(CrosstableMessage::Refresh);
    assert!(matches!(
        rig.shell.app.crosstable_view().body,
        CrosstableBody::Table(_) | CrosstableBody::Empty(_)
    ));
    let started = std::time::Instant::now();
    for theme in themes() {
        assert!(rig.sim(640.0, 400.0).snapshot(&theme).is_ok());
    }
    assert!(
        started.elapsed() < std::time::Duration::from_secs(60),
        "{:?}",
        started.elapsed()
    );
}

fn rating_rig() -> Rig {
    let mut rig = Rig::new();
    let slot = rig.open_games(&GAMES);
    rig.load(slot, 1);
    rig.toggle(PaneKind::RatingGraph);
    rig
}

#[test]
fn the_rating_graph_says_computing_then_cancelled_when_stopped() {
    let _render = render_lock();
    let mut rig = rating_rig();
    // The computation of the toggle is not run: the panel is busy.
    let mut sim = rig.sim(1300.0, 900.0);
    assert!(
        sim.find(rig.tr("rating-stop").as_str()).is_ok() || rig.shell.app.charts().is_running()
    );
    drop(sim);
    rig.ui(Message::App(AppMessage::Charts(
        ChartsMessage::CancelRating,
    )));
    let view = rig.shell.app.rating_view();
    assert_eq!(view.issue, Some(ChartsNotice::Cancelled));
    let mut sim = rig.sim(1300.0, 900.0);
    assert!(sim.find(rig.tr("charts-notice-cancelled").as_str()).is_ok());
}

#[test]
fn the_rating_graph_tells_a_failed_computation() {
    let _render = render_lock();
    let mut rig = rating_rig();
    let request = update(&mut rig.shell.app, ChartsMessage::RefreshRating.into())
        .into_iter()
        .find_map(|e| match e {
            Effect::Charts(ChartsEffect::Rating { request, .. }) => Some(request),
            _ => None,
        })
        .expect("a computation starts");
    rig.app(ChartsMessage::RatingComputed {
        request,
        result: Box::new(Err(ChartsFailure::Internal)),
    });
    assert_eq!(
        rig.shell.app.rating_view().issue,
        Some(ChartsNotice::RatingFailed)
    );
    assert!(
        rig.sim(1300.0, 900.0)
            .find(rig.tr("charts-notice-rating-failed").as_str())
            .is_ok()
    );
    // A filter too big is a failure with a text too, never a blank panel.
    let request = update(&mut rig.shell.app, ChartsMessage::RefreshRating.into())
        .into_iter()
        .find_map(|e| match e {
            Effect::Charts(ChartsEffect::Rating { request, .. }) => Some(request),
            _ => None,
        })
        .expect("a computation starts");
    rig.app(ChartsMessage::RatingComputed {
        request,
        result: Box::new(Err(ChartsFailure::FilterSize)),
    });
    assert!(rig.shell.app.rating_view().issue.is_some());
    drop(rig.sim(300.0, 200.0));
}

#[test]
fn the_graphs_tell_a_failed_computation_on_both_tabs() {
    let _render = render_lock();
    for tab in [StatsTab::Relative, StatsTab::Absolute] {
        let mut rig = Rig::new();
        rig.open_games(&GAMES);
        rig.toggle(PaneKind::FilterStats);
        rig.app(ChartsMessage::RefreshStats);
        let effects = update(&mut rig.shell.app, ChartsMessage::SelectTab(tab).into());
        if let Some(request) = effects.into_iter().find_map(|e| match e {
            Effect::Charts(ChartsEffect::Graph { request, .. }) => Some(request),
            _ => None,
        }) {
            rig.app(ChartsMessage::GraphComputed {
                request,
                result: Box::new(Err(ChartsFailure::Internal)),
            });
            assert!(
                rig.sim(1300.0, 900.0)
                    .find(rig.tr("charts-notice-graph-failed").as_str())
                    .is_ok(),
                "{tab:?}"
            );
        }
        // Switching tabs many times in a row leaves a consistent panel.
        for _ in 0..20 {
            rig.app(ChartsMessage::SelectTab(StatsTab::Absolute));
            rig.app(ChartsMessage::SelectTab(StatsTab::Relative));
        }
        assert_eq!(rig.shell.app.stats_view().tab, StatsTab::Relative);
        drop(rig.sim(1300.0, 900.0));
    }
}

#[test]
fn the_tracker_says_computing_and_stop_sends_stop_to_the_application() {
    let _render = render_lock();
    let mut rig = Rig::new();
    rig.open_games(&GAMES);
    rig.toggle(PaneKind::PieceTracker);
    assert!(rig.shell.app.piece_tracker().is_running());
    let mut sim = rig.sim(1400.0, 900.0);
    sim.click(rig.tr("ptracker-stop").as_str()).unwrap();
    let sent: Vec<Message> = sim.into_messages().collect();
    assert!(
        sent.iter().any(|m| matches!(
            m,
            Message::App(AppMessage::PieceTracker(PieceTrackerMessage::Stop))
        )),
        "{sent:?}"
    );
    rig.ui(Message::App(AppMessage::PieceTracker(
        PieceTrackerMessage::Stop,
    )));
    // The count ends when its answer arrives; the panel stays drawable.
    drop(rig.sim(300.0, 200.0));
}

fn press_release(button: iced::mouse::Button) -> [iced::Event; 2] {
    [
        iced::Event::Mouse(iced::mouse::Event::ButtonPressed(button)),
        iced::Event::Mouse(iced::mouse::Event::ButtonReleased(button)),
    ]
}

#[test]
fn a_double_click_on_a_piece_selects_it_and_asks_for_a_refresh() {
    let _render = render_lock();
    let mut rig = Rig::new();
    rig.open_games(&GAMES);
    rig.toggle(PaneKind::PieceTracker);
    rig.app(PieceTrackerMessage::Refresh);
    let mut sim = rig.sim(1400.0, 900.0);
    let id = iced::widget::Id::from(format!("tracker-piece-{}", Square::G1));
    let target = sim.find(id).unwrap();
    sim.point_at(target.visible_bounds().unwrap().center());
    let mut events = Vec::new();
    events.extend(press_release(iced::mouse::Button::Left));
    events.extend(press_release(iced::mouse::Button::Left));
    drop(sim.simulate(events));
    let sent: Vec<PieceTrackerMessage> = sim
        .into_messages()
        .filter_map(|m| match m {
            Message::App(AppMessage::PieceTracker(t)) => Some(t),
            _ => None,
        })
        .collect();
    let selects = sent
        .iter()
        .filter(|m| matches!(m, PieceTrackerMessage::SelectPiece { sister: false, .. }))
        .count();
    assert!(selects >= 1, "{sent:?}");
    assert!(
        sent.iter()
            .any(|m| matches!(m, PieceTrackerMessage::Refresh)),
        "the double click refreshes: {sent:?}"
    );
}

#[test]
fn a_middle_click_and_a_click_outside_the_pieces_send_nothing_to_the_tracker() {
    let _render = render_lock();
    let mut rig = Rig::new();
    rig.open_games(&GAMES);
    rig.toggle(PaneKind::PieceTracker);
    rig.app(PieceTrackerMessage::Refresh);
    let mut sim = rig.sim(1400.0, 900.0);
    let id = iced::widget::Id::from(format!("tracker-piece-{}", Square::G1));
    let target = sim.find(id).unwrap();
    sim.point_at(target.visible_bounds().unwrap().center());
    drop(sim.simulate(press_release(iced::mouse::Button::Middle)));
    let selects = sim
        .into_messages()
        .filter(|m| {
            matches!(
                m,
                Message::App(AppMessage::PieceTracker(
                    PieceTrackerMessage::SelectPiece { .. }
                ))
            )
        })
        .count();
    assert_eq!(selects, 0);
}

#[test]
fn moving_over_the_heat_map_reports_valid_squares_and_leaving_clears_them() {
    let _render = render_lock();
    let mut rig = Rig::new();
    rig.open_games(&GAMES);
    rig.toggle(PaneKind::PieceTracker);
    rig.app(PieceTrackerMessage::Refresh);
    let mut sim = rig.sim(1400.0, 900.0);
    for y in (0..36u16).map(|n| 10 + n * 25) {
        for x in (0..56u16).map(|n| 10 + n * 25) {
            let position = iced::Point::new(f32::from(x), f32::from(y));
            sim.point_at(position);
            drop(
                sim.simulate([iced::Event::Mouse(iced::mouse::Event::CursorMoved {
                    position,
                })]),
            );
        }
    }
    let hovers: Vec<Option<Square>> = sim
        .into_messages()
        .filter_map(|m| match m {
            Message::Panels(DataPanelsMessage::TrackerHover(s)) => Some(s),
            _ => None,
        })
        .collect();
    let squares: std::collections::BTreeSet<usize> =
        hovers.iter().flatten().map(|s| s.to_usize()).collect();
    assert!(!squares.is_empty(), "the heat map answers the pointer");
    assert!(squares.iter().all(|s| *s < 64));
    assert!(hovers.contains(&None), "leaving the map clears the hover");
}

#[test]
fn the_panels_follow_the_language_while_they_stay_open() {
    let _render = render_lock();
    let mut rig = Rig::new();
    let slot = rig.open_games(&GAMES);
    rig.load(slot, 1);
    for kind in [
        PaneKind::Crosstable,
        PaneKind::FilterStats,
        PaneKind::RatingGraph,
        PaneKind::PieceTracker,
    ] {
        rig.toggle(kind);
    }
    rig.app(CrosstableMessage::Refresh);
    rig.app(ChartsMessage::RefreshStats);
    rig.app(PieceTrackerMessage::Refresh);
    assert!(rig.sim(1500.0, 900.0).find("Player").is_ok());
    rig.shell.app.prefs.general.language = mychess_app::LanguagePref::PtBr;
    rig.ui(Message::Ui(UiMessage::FlipBoard));
    assert_eq!(rig.shell.app.crosstable_view().labels.player, "Jogador");
    let mut sim = rig.sim(1500.0, 900.0);
    assert!(sim.find("Jogador").is_ok(), "the header follows");
    assert!(sim.find("Player").is_err());
    drop(sim);
    for theme in themes() {
        assert!(rig.sim(640.0, 400.0).snapshot(&theme).is_ok());
    }
    rig.shell.app.prefs.general.language = mychess_app::LanguagePref::EnUs;
    rig.ui(Message::Ui(UiMessage::FlipBoard));
    assert!(rig.sim(1500.0, 900.0).find("Player").is_ok());
}

#[test]
fn every_key_of_every_notice_of_the_four_panels_has_text_in_both_languages() {
    let _render = render_lock();
    let mut keys: Vec<&'static str> = Vec::new();
    for notice in [
        ChartsNotice::StatsFailed,
        ChartsNotice::GraphFailed,
        ChartsNotice::RatingFailed,
        ChartsNotice::Cancelled,
        ChartsNotice::TooManyPlayers,
        ChartsNotice::NoPlayers,
    ] {
        keys.push(notice.message_key());
    }
    for notice in [PieceTrackerNotice::Failed, PieceTrackerNotice::NotAPiece] {
        keys.push(notice.message_key());
    }
    for notice in [
        CrosstableNotice::NoGame,
        CrosstableNotice::NoTable,
        CrosstableNotice::InvalidGame,
        CrosstableNotice::Failed,
        CrosstableNotice::Busy,
        CrosstableNotice::Stale,
        CrosstableNotice::FilterSet,
        CrosstableNotice::FilterAdded,
        CrosstableNotice::Exported,
        CrosstableNotice::ExportFailed,
        CrosstableNotice::Copied,
        CrosstableNotice::Cancelled,
        CrosstableNotice::ExportStale,
    ] {
        keys.push(notice.message_key());
    }
    for warning in [
        CrossWarning::PlayerLimit,
        CrossWarning::TooManyPlayers,
        CrossWarning::GameLimit,
        CrossWarning::Unreadable(1),
        CrossWarning::LargeOutput,
    ] {
        keys.push(warning.message_key());
    }
    for language in Language::ALL {
        let i18n = I18n::new(language);
        for key in &keys {
            assert!(i18n.has(key), "{key} missing in {language:?}");
            let text = i18n.tr(key);
            assert!(!text.is_empty() && text != *key, "{key}: {text}");
        }
    }
}
