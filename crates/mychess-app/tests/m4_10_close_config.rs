//! M4-10 close-out contract tests, part 1: the "Configure report" dialog and
//! its preferences (spec `m4-10-relatorio-da-partida.md`, D1422, D1425,
//! D2100-D2105). A synthetic UCI engine answers the searches.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::assert_is_empty,
    clippy::too_many_lines
)]

mod m4_6_common;

use m4_6_common::{HANGING_QUEEN, Rig, game_of, last_search, material_engine};
use mychess_app::online::{MarkerInfo, OnlineNotice};
use mychess_app::report::{
    LIMIT_MAX_TENTHS, MoveClass, ReportConfig, ReportMessage, ReportPreset, ReportThresholds,
};
use mychess_app::{
    AppNotice, GameMessage, Message, PrefChange, PrefsMessage, load_preferences, render_preferences,
};
use mychess_engine::Score;

fn rig_with(plies: usize) -> Rig {
    Rig::new(game_of(&HANGING_QUEEN[..plies]))
}

// ---- the pure configuration ----

#[test]
fn the_default_configuration_is_the_standard_preset_and_the_spec_limits() {
    let config = ReportConfig::default();
    assert_eq!(config.depth, ReportPreset::Standard.depth());
    assert!(!config.by_time);
    assert!(config.board_marks, "marks are on by default");
    assert_eq!(config.thresholds(), ReportThresholds::default());
}

#[test]
fn every_configuration_is_sanitized_into_ordered_limits_in_range() {
    // A deterministic sweep of awkward values: the order and the range hold.
    let values = [0_u16, 1, 7, 50, 999, 1000, 1001, u16::MAX];
    for a in values {
        for b in values {
            for c in values {
                let config = ReportConfig {
                    best_tenths: a,
                    excellent_tenths: b,
                    good_tenths: c,
                    inaccuracy_tenths: a,
                    mistake_tenths: b,
                    depth: u32::from(a),
                    movetime_ms: u32::from(c).saturating_mul(1_000_000),
                    ..ReportConfig::default()
                }
                .sanitized();
                let limits = [
                    config.best_tenths,
                    config.excellent_tenths,
                    config.good_tenths,
                    config.inaccuracy_tenths,
                    config.mistake_tenths,
                ];
                assert!(limits.windows(2).all(|w| w[0] <= w[1]), "{limits:?}");
                assert!(limits.iter().all(|l| *l <= LIMIT_MAX_TENTHS));
                assert_eq!(config.sanitized(), config, "sanitizing is idempotent");
            }
        }
    }
}

// ---- the preferences ----

#[test]
fn the_configuration_round_trips_through_the_toml_file() {
    let mut prefs = load_preferences("").prefs;
    assert_eq!(*prefs.report_config, ReportConfig::default());
    *prefs.report_config = ReportConfig {
        by_time: true,
        depth: 30,
        movetime_ms: 3500,
        best_tenths: 3,
        excellent_tenths: 25,
        good_tenths: 60,
        inaccuracy_tenths: 120,
        mistake_tenths: 250,
        board_marks: false,
    };
    let text = render_preferences(&prefs);
    assert!(text.contains("[report_config]"), "{text}");
    let loaded = load_preferences(&text);
    assert!(loaded.issues.is_empty(), "{:?}", loaded.issues);
    assert_eq!(loaded.prefs.report_config, prefs.report_config);
}

#[test]
fn a_bad_value_in_the_file_falls_back_without_losing_the_rest() {
    let loaded = load_preferences(
        "[report_config]\nby_time = \"yes\"\ndepth = 99999\nbest_tenths = 7\nmistake_tenths = 3\n",
    );
    let config = *loaded.prefs.report_config;
    assert!(!config.by_time, "a wrong type keeps the default");
    assert_eq!(config.depth, 100, "an excess is limited to the range");
    assert_eq!(config.best_tenths, 7);
    assert!(
        config.mistake_tenths >= config.best_tenths,
        "the order is restored: {config:?}"
    );
    assert!(!loaded.issues.is_empty(), "the problems are reported");
}

#[test]
fn a_change_of_preference_is_sanitized() {
    let mut rig = rig_with(4);
    rig.send(Message::Prefs(PrefsMessage::Set(PrefChange::ReportConfig(
        Box::new(ReportConfig {
            depth: 1,
            best_tenths: 900,
            excellent_tenths: 10,
            ..ReportConfig::default()
        }),
    ))));
    let config = *rig.state.prefs.report_config;
    assert_eq!(config.depth, 10);
    assert_eq!(config.excellent_tenths, 900, "carried along by the best");
}

// ---- the dialog ----

#[test]
fn opening_the_dialog_shows_the_saved_choices_and_closing_discards_the_draft() {
    let mut rig = rig_with(4);
    rig.state.prefs.report_config.depth = 20;
    assert!(rig.state.report.config_dialog.is_none());
    rig.send(ReportMessage::OpenConfig);
    let dialog = rig.state.report.config_dialog.expect("open");
    assert_eq!(dialog.config.depth, 20);
    assert_eq!(dialog.engine, None);

    rig.send(ReportMessage::ConfigSet(ReportConfig {
        depth: 25,
        ..dialog.config
    }));
    assert_eq!(rig.state.report.config_dialog.unwrap().config.depth, 25);
    rig.send(ReportMessage::CloseConfig);
    assert!(rig.state.report.config_dialog.is_none());
    assert_eq!(rig.state.prefs.report_config.depth, 20, "nothing saved");
}

#[test]
fn the_dialog_limits_what_it_is_given_and_restores_the_defaults() {
    let mut rig = rig_with(4);
    rig.send(ReportMessage::OpenConfig);
    rig.send(ReportMessage::ConfigSet(ReportConfig {
        best_tenths: 5000,
        depth: 0,
        ..ReportConfig::default()
    }));
    let config = rig.state.report.config_dialog.unwrap().config;
    assert_eq!(config.best_tenths, LIMIT_MAX_TENTHS);
    assert_eq!(config.mistake_tenths, LIMIT_MAX_TENTHS);
    assert_eq!(config.depth, 10);
    rig.send(ReportMessage::ConfigDefaults);
    assert_eq!(
        rig.state.report.config_dialog.unwrap().config,
        ReportConfig::default()
    );
}

#[test]
fn messages_of_the_dialog_without_the_dialog_do_nothing() {
    let mut rig = rig_with(4);
    for message in [
        ReportMessage::ConfigSet(ReportConfig::default()),
        ReportMessage::ConfigEngine(None),
        ReportMessage::ConfigDefaults,
        ReportMessage::ConfigStart,
        ReportMessage::CloseConfig,
    ] {
        rig.send(message);
        assert!(!rig.state.report_running(), "{message:?}");
    }
    assert!(rig.state.report.config_dialog.is_none());
}

#[test]
fn the_dialog_blocks_the_edits_of_the_game_while_it_is_open() {
    let mut rig = rig_with(4);
    rig.send(ReportMessage::OpenConfig);
    assert!(rig.state.modal_open());
    rig.state.report.config_dialog.take();
    rig.send(GameMessage::Navigate(mychess_app::Nav::Start));
    rig.send(ReportMessage::OpenConfig);
    let before = rig.game().clone();
    rig.send(GameMessage::TruncateEnd);
    let after = rig.game();
    assert!(after.same_content(&before), "the game was not edited");
}

#[test]
fn ok_saves_the_choices_and_starts_the_run_with_the_depth_chosen() {
    let mut rig = rig_with(6);
    rig.send(ReportMessage::OpenConfig);
    rig.send(ReportMessage::ConfigSet(ReportConfig {
        depth: 21,
        ..ReportConfig::default()
    }));
    let fx = rig.send(ReportMessage::ConfigStart);
    assert!(
        rig.state.report.config_dialog.is_none(),
        "the dialog closed"
    );
    assert_eq!(rig.state.prefs.report_config.depth, 21, "saved");
    assert!(rig.state.report_running());
    let request = last_search(&fx).expect("the first search");
    assert_eq!(request.limits.depth, Some(21));
    assert_eq!(rig.state.report.preset, ReportPreset::Deep, "nearest label");
}

#[test]
fn a_time_limit_searches_by_time_and_not_by_depth() {
    let mut rig = rig_with(4);
    rig.send(ReportMessage::OpenConfig);
    rig.send(ReportMessage::ConfigSet(ReportConfig {
        by_time: true,
        movetime_ms: 1500,
        ..ReportConfig::default()
    }));
    let fx = rig.send(ReportMessage::ConfigStart);
    let request = last_search(&fx).expect("the first search");
    assert_eq!(request.limits.depth, None);
    assert_eq!(
        request.limits.movetime,
        Some(std::time::Duration::from_millis(1500))
    );
}

#[test]
fn the_preset_buttons_set_the_saved_depth() {
    let mut rig = rig_with(4);
    for preset in ReportPreset::ALL {
        rig.send(ReportMessage::SetPreset(preset));
        assert_eq!(rig.state.prefs.report_config.depth, preset.depth());
        assert!(!rig.state.prefs.report_config.by_time);
    }
    // A preset after a time limit goes back to a depth.
    rig.state.prefs.report_config.by_time = true;
    rig.send(ReportMessage::SetPreset(ReportPreset::Fast));
    assert!(!rig.state.prefs.report_config.by_time);
}

#[test]
fn the_limits_of_the_dialog_decide_the_classes_of_the_report() {
    // The engine of material: a hanging queen is a blunder by any scale.
    let mut strict = rig_with(8);
    strict.send(ReportMessage::OpenConfig);
    strict.send(ReportMessage::ConfigSet(ReportConfig {
        // Everything above one point is a blunder.
        best_tenths: 1,
        excellent_tenths: 2,
        good_tenths: 5,
        inaccuracy_tenths: 8,
        mistake_tenths: 10,
        ..ReportConfig::default()
    }));
    let fx = strict.send(ReportMessage::ConfigStart);
    strict.drive(fx, 5000, &mut material_engine);
    let strict_data = strict.state.report_data().expect("report");
    assert_eq!(
        strict_data.thresholds.mistake, 1.0,
        "the report remembers the limits it used"
    );

    let mut lax = rig_with(8);
    let fx = lax.send(ReportMessage::Generate);
    lax.drive(fx, 5000, &mut material_engine);
    let lax_data = lax.state.report_data().expect("report");
    let blunders = |d: &mychess_app::report::ReportData| d.report.white.count(MoveClass::Blunder);
    assert!(blunders(strict_data) >= blunders(lax_data));
    let worst = |d: &mychess_app::report::ReportData| {
        d.report
            .moves
            .iter()
            .map(|m| m.loss)
            .fold(0.0_f32, f32::max)
    };
    assert!(
        (worst(strict_data) - worst(lax_data)).abs() < 1e-3,
        "the losses do not depend on the limits"
    );
    let _ = Score::Cp(0);
}

#[test]
fn the_dialog_is_closed_during_an_online_game() {
    let mut rig = rig_with(4);
    rig.state.online.marker = Some(MarkerInfo {
        game_id: "abcd1234".to_owned(),
        date: "2026-10-24".to_owned(),
    });
    assert!(rig.state.assist_locked());
    let fx = rig.send(ReportMessage::OpenConfig);
    assert!(fx.is_empty());
    assert!(rig.state.report.config_dialog.is_none());
    assert_eq!(
        rig.state.notice,
        Some(AppNotice::Online(OnlineNotice::AssistanceDisabled))
    );
}

#[test]
fn the_dialog_does_not_open_while_a_run_is_in_progress() {
    let mut rig = rig_with(6);
    rig.send(ReportMessage::Generate);
    assert!(rig.state.report_running());
    rig.send(ReportMessage::OpenConfig);
    assert!(rig.state.report.config_dialog.is_none());
}

#[test]
fn the_engine_chosen_in_the_dialog_is_used_when_it_runs() {
    let mut rig = rig_with(4);
    rig.send(ReportMessage::OpenConfig);
    rig.send(ReportMessage::ConfigEngine(Some(rig.run)));
    assert_eq!(
        rig.state.report.config_dialog.unwrap().engine,
        Some(rig.run)
    );
    rig.send(ReportMessage::ConfigStart);
    assert_eq!(rig.state.report.engine, Some(rig.run));
    assert!(rig.state.report_running());
    // The next dialog starts from the engine of the last one.
    rig.send(ReportMessage::Cancel);
    rig.send(ReportMessage::OpenConfig);
    assert_eq!(
        rig.state.report.config_dialog.unwrap().engine,
        Some(rig.run)
    );
}
