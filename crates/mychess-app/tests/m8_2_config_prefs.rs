//! M8-2a contract tests of the configuration (spec D1263, D1266, D1268,
//! D1289, D1293, D1309, D1310, D1315) and of the `[tournament]` preferences
//! (D1299, D1310).
#![allow(
    clippy::assert_is_empty,
    clippy::cast_possible_truncation,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::too_many_lines
)]

mod m8_2_common;

use std::path::PathBuf;
use std::time::Duration;

use m8_2_common::*;
use mychess_app::tournament::{
    Adjudication, BookConfig, BookPick, ConfigError, DrawRule, OpeningSource, Participant,
    ResignRule, STATE_BYTES_PER_GAME, Scheduling, SuiteConfig, SuiteOrder, TimeControl,
    TournamentConfig,
};
use mychess_app::{
    AppState, Effect, Message, PrefChange, Preferences, PrefsIssue, PrefsMessage, load_preferences,
    perform, render_preferences, update,
};
use proptest::prelude::*;
use rstest::rstest;

fn ok(c: &TournamentConfig) {
    assert_eq!(c.validate(), Ok(()));
}

fn err(c: &TournamentConfig) -> ConfigError {
    c.validate().expect_err("should be refused")
}

#[test]
fn a_default_configuration_with_two_participants_is_valid() {
    ok(&cfg(2));
    let mut c = cfg(2);
    c.time = TimeControl::default();
    ok(&c);
}

#[test]
fn two_to_thirty_two_participants() {
    assert_eq!(err(&cfg(0)), ConfigError::TooFewParticipants);
    assert_eq!(err(&cfg(1)), ConfigError::TooFewParticipants);
    ok(&cfg(2));
    let mut c = cfg(32);
    c.cycles = 1;
    ok(&c);
    assert_eq!(err(&cfg(33)), ConfigError::TooManyParticipants);
}

#[test]
fn names_must_be_present_and_unique_regardless_of_case_and_padding() {
    let mut c = cfg(3);
    c.participants[1].name = "   ".into();
    assert_eq!(err(&c), ConfigError::EmptyName(1));
    let mut c = cfg(3);
    c.participants[1].name = String::new();
    assert_eq!(err(&c), ConfigError::EmptyName(1));
    let mut c = cfg(3);
    c.participants[2].name = "p0".into();
    assert!(matches!(err(&c), ConfigError::DuplicateName(_)));
    let mut c = cfg(3);
    c.participants[2].name = " P0 ".into();
    assert!(matches!(err(&c), ConfigError::DuplicateName(_)));
}

#[test]
fn the_same_engine_may_enter_twice_with_different_names_and_options() {
    let mut c = cfg(2);
    c.participants = vec![
        Participant {
            options: vec![("Hash".into(), "16".into())],
            ..Participant::new(7, "Engine 16MB")
        },
        Participant {
            options: vec![("Hash".into(), "256".into())],
            ..Participant::new(7, "Engine 256MB")
        },
    ];
    ok(&c);
}

#[rstest]
#[case(0, true)]
#[case(1, false)]
#[case(100, false)]
#[case(101, true)]
fn cycles_between_one_and_one_hundred(#[case] cycles: u32, #[case] bad: bool) {
    let mut c = cfg(2);
    c.cycles = cycles;
    assert_eq!(c.validate().is_err(), bad);
    if bad {
        assert_eq!(err(&c), ConfigError::BadCycles);
    }
}

#[test]
fn the_tested_participant_must_exist_only_in_a_gauntlet() {
    let mut c = cfg(3);
    c.tested = 3;
    ok(&c);
    c.scheduling = Scheduling::Gauntlet;
    assert_eq!(err(&c), ConfigError::BadTested);
    c.tested = 2;
    ok(&c);
}

#[rstest]
#[case(TimeControl::Clock { base_ms: 0, increment_ms: 0 })]
#[case(TimeControl::Clock { base_ms: 7_200_001, increment_ms: 0 })]
#[case(TimeControl::Clock { base_ms: 1000, increment_ms: 60_001 })]
#[case(TimeControl::MoveTime { ms: 0 })]
#[case(TimeControl::MoveTime { ms: 3_600_001 })]
#[case(TimeControl::Depth(0))]
#[case(TimeControl::Depth(101))]
#[case(TimeControl::Nodes(0))]
#[case(TimeControl::Nodes(u64::MAX))]
fn incoherent_time_controls_are_refused(#[case] time: TimeControl) {
    let mut c = cfg(2);
    c.time = time;
    assert!(matches!(err(&c), ConfigError::BadTime(_)), "{time:?}");
}

#[rstest]
#[case(TimeControl::Clock { base_ms: 0, increment_ms: 1 })]
#[case(TimeControl::Clock { base_ms: 7_200_000, increment_ms: 60_000 })]
#[case(TimeControl::MoveTime { ms: 1 })]
#[case(TimeControl::MoveTime { ms: 3_600_000 })]
#[case(TimeControl::Depth(1))]
#[case(TimeControl::Depth(100))]
#[case(TimeControl::Nodes(1))]
#[case(TimeControl::Nodes(i64::MAX as u64))]
fn time_controls_on_the_edge_are_accepted(#[case] time: TimeControl) {
    let mut c = cfg(2);
    c.time = time;
    ok(&c);
}

#[test]
fn watchdog_settings_have_ranges() {
    for (f, bad) in [
        (
            Box::new(|c: &mut TournamentConfig| c.leeway_percent = 99)
                as Box<dyn Fn(&mut TournamentConfig)>,
            true,
        ),
        (Box::new(|c| c.leeway_percent = 100), false),
        (Box::new(|c| c.leeway_percent = 300), false),
        (Box::new(|c| c.leeway_percent = 301), true),
        (Box::new(|c| c.margin_ms = 999), true),
        (Box::new(|c| c.margin_ms = 1_000), false),
        (Box::new(|c| c.margin_ms = 30_000), false),
        (Box::new(|c| c.margin_ms = 30_001), true),
        (Box::new(|c| c.depth_limit_secs = 9), true),
        (Box::new(|c| c.depth_limit_secs = 10), false),
        (Box::new(|c| c.depth_limit_secs = 3_600), false),
        (Box::new(|c| c.depth_limit_secs = 3_601), true),
    ] {
        let mut c = cfg(2);
        f(&mut c);
        assert_eq!(c.validate().is_err(), bad);
        if bad {
            assert!(matches!(err(&c), ConfigError::BadWatchdog(_)));
        }
    }
}

#[test]
fn zeroed_adjudication_fields_are_refused() {
    // D1309.
    let cases: Vec<Adjudication> = vec![
        Adjudication {
            max_plies: Some(0),
            ..Adjudication::default()
        },
        Adjudication {
            resign: Some(ResignRule {
                centipawns: 0,
                moves: 4,
                require_both: true,
            }),
            ..Adjudication::default()
        },
        Adjudication {
            resign: Some(ResignRule {
                centipawns: 600,
                moves: 0,
                require_both: true,
            }),
            ..Adjudication::default()
        },
        Adjudication {
            draw: Some(DrawRule {
                after_move: 40,
                centipawns: 10,
                moves: 0,
            }),
            ..Adjudication::default()
        },
        Adjudication {
            tb_pieces: 0,
            ..Adjudication::default()
        },
        Adjudication {
            tb_pieces: 8,
            ..Adjudication::default()
        },
        Adjudication {
            max_plies: Some(100_001),
            ..Adjudication::default()
        },
    ];
    for adj in cases {
        let mut c = cfg(2);
        c.adjudication = adj;
        assert!(
            matches!(err(&c), ConfigError::BadAdjudication(_)),
            "{adj:?}"
        );
    }
    let mut c = cfg(2);
    c.adjudication = Adjudication {
        resign: Some(ResignRule::default()),
        draw: Some(DrawRule::default()),
        max_plies: Some(400),
        tablebase: true,
        tb_pieces: 7,
    };
    ok(&c);
}

#[test]
fn failure_limits() {
    let mut c = cfg(2);
    c.failures.max_failures = 0;
    assert_eq!(err(&c), ConfigError::BadFailureLimit);
    c.failures.max_failures = 101;
    assert_eq!(err(&c), ConfigError::BadFailureLimit);
    c.failures.max_failures = 100;
    ok(&c);
}

#[test]
fn name_and_site_have_at_most_sixty_four_characters_not_bytes() {
    let mut c = cfg(2);
    c.name = "é".repeat(64);
    c.site = "é".repeat(64);
    ok(&c);
    c.name = "x".repeat(65);
    assert_eq!(err(&c), ConfigError::TitleTooLong);
    c.name = "x".into();
    c.site = "é".repeat(65);
    assert_eq!(err(&c), ConfigError::TitleTooLong);
}

#[test]
fn long_participant_names_are_accepted_without_breaking_the_schedule() {
    let mut c = cfg(2);
    c.participants[0].name = "N".repeat(5_000);
    ok(&c);
}

#[test]
fn seed_and_engine_identity_must_fit_a_signed_toml_integer() {
    let mut c = cfg(2);
    c.seed = i64::MAX as u64;
    ok(&c);
    c.seed = i64::MAX as u64 + 1;
    assert_eq!(err(&c), ConfigError::BadSeed);
    let mut c = cfg(2);
    c.participants[0].engine = u64::MAX;
    assert!(c.validate().is_err());
}

#[test]
fn opening_sources_need_their_data() {
    let mut c = cfg(2);
    c.opening = OpeningSource::CurrentPosition;
    assert!(matches!(err(&c), ConfigError::MissingOpeningData(_)));
    c.start_fen = Some("not a fen".into());
    assert_eq!(err(&c), ConfigError::BadStartFen);
    c.start_fen = Some("4k3/8/8/8/8/8/8/R3K3 w - - 0 1".into());
    ok(&c);
    c.opening = OpeningSource::CurrentPositionFirstGame;
    ok(&c);
    c.opening = OpeningSource::Book;
    assert!(matches!(err(&c), ConfigError::MissingOpeningData(_)));
    c.book = Some(BookConfig {
        path: PathBuf::from("b.bin"),
        pick: BookPick::Best,
    });
    ok(&c);
    c.opening = OpeningSource::Suite;
    assert!(matches!(err(&c), ConfigError::MissingOpeningData(_)));
    c.suite = Some(SuiteConfig {
        path: PathBuf::from("s.epd"),
        order: SuiteOrder::Random,
        plies: 8,
    });
    ok(&c);
}

#[test]
fn book_ply_limit_is_one_to_forty() {
    let mut c = cfg(2);
    for (limit, bad) in [(0, true), (1, false), (40, false), (41, true)] {
        c.book_ply_limit = Some(limit);
        assert_eq!(c.validate().is_err(), bad, "limit {limit}");
    }
    c.book_ply_limit = None;
    ok(&c);
}

#[test]
fn check_files_looks_at_the_book_and_the_suite_only_when_used() {
    let dir = tempfile::tempdir().unwrap();
    let mut c = cfg(2);
    c.book = Some(BookConfig {
        path: dir.path().join("missing.bin"),
        pick: BookPick::Best,
    });
    c.suite = Some(SuiteConfig {
        path: dir.path().join("missing.epd"),
        order: SuiteOrder::Sequential,
        plies: 4,
    });
    assert_eq!(c.check_files(), Ok(()), "unused files are not looked at");
    c.opening = OpeningSource::Book;
    assert!(matches!(c.check_files(), Err(ConfigError::BookNotFound(_))));
    c.opening = OpeningSource::Suite;
    assert!(matches!(
        c.check_files(),
        Err(ConfigError::SuiteUnreadable(_))
    ));
    // A directory is not a book.
    c.opening = OpeningSource::Book;
    c.book = Some(BookConfig {
        path: dir.path().to_path_buf(),
        pick: BookPick::Best,
    });
    assert!(matches!(c.check_files(), Err(ConfigError::BookNotFound(_))));
    let suite = dir.path().join("s.epd");
    std::fs::write(&suite, "x").unwrap();
    c.opening = OpeningSource::Suite;
    c.suite = Some(SuiteConfig {
        path: suite,
        order: SuiteOrder::Sequential,
        plies: 4,
    });
    assert_eq!(c.check_files(), Ok(()));
}

#[test]
fn a_schedule_whose_state_cannot_be_saved_is_refused() {
    // D1315: 32 participants, 100 cycles = 49 600 games = ~19 MB of state.
    let mut c = cfg(32);
    c.cycles = 100;
    assert_eq!(err(&c), ConfigError::TooManyGames);
    c.cycles = 80;
    ok(&c);
    assert!(c.state_bytes() <= 16 * 1024 * 1024);
    assert_eq!(
        c.state_bytes(),
        c.game_count() as u64 * STATE_BYTES_PER_GAME
    );
}

#[test]
fn every_error_has_a_translation_key() {
    let mut keys = std::collections::HashSet::new();
    for e in [
        ConfigError::TooFewParticipants,
        ConfigError::TooManyParticipants,
        ConfigError::EmptyName(0),
        ConfigError::DuplicateName("x".into()),
        ConfigError::BadCycles,
        ConfigError::BadTested,
        ConfigError::BadTime("x"),
        ConfigError::BadWatchdog("x"),
        ConfigError::MissingOpeningData("x"),
        ConfigError::BadStartFen,
        ConfigError::BadBookLimit,
        ConfigError::BadSeed,
        ConfigError::TitleTooLong,
        ConfigError::BadAdjudication("x"),
        ConfigError::BadFailureLimit,
        ConfigError::TooManyGames,
        ConfigError::BookNotFound(PathBuf::new()),
        ConfigError::SuiteUnreadable(PathBuf::new()),
    ] {
        let key = e.message_key();
        assert!(key.starts_with("tournament-error-"), "{key}");
        assert!(keys.insert(key), "{key} is shared");
    }
}

// ---- estimate ----

#[test]
fn game_counts_follow_the_formulas_of_the_spec() {
    let mut c = cfg(5);
    c.cycles = 3;
    assert_eq!(c.game_count(), 10 * 3);
    c.scheduling = Scheduling::Carousel;
    assert_eq!(c.game_count(), 30);
    c.scheduling = Scheduling::Gauntlet;
    assert_eq!(c.game_count(), 4 * 3);
    // The schedule agrees with the count.
    for s in [
        Scheduling::RoundRobin,
        Scheduling::Carousel,
        Scheduling::Gauntlet,
    ] {
        c.scheduling = s;
        assert_eq!(
            mychess_app::tournament::build_schedule(5, s, 3, 0)
                .pairings
                .len(),
            c.game_count()
        );
    }
}

#[test]
fn the_estimate_uses_two_times_base_plus_sixty_increments_per_game() {
    let mut c = cfg(4);
    c.cycles = 2;
    c.time = TimeControl::Clock {
        base_ms: 60_000,
        increment_ms: 1_000,
    };
    let e = c.estimate();
    assert_eq!(e.games, 12);
    assert_eq!(e.max_duration, Duration::from_mins(48));
    assert!(!e.warn && !e.large_state);
    c.time = TimeControl::MoveTime { ms: 3_000 };
    assert_eq!(c.estimate().max_duration, Duration::from_mins(72));
    c.time = TimeControl::Depth(10);
    assert_eq!(c.estimate().max_duration, Duration::ZERO);
}

#[test]
fn the_estimate_warns_above_1000_games_or_24_hours_and_not_at_the_limit() {
    let mut c = cfg(2);
    c.time = TimeControl::Depth(1);
    c.cycles = 1000;
    assert!(!c.estimate().warn, "1000 games exactly");
    c.cycles = 1001;
    assert!(c.estimate().warn);
    // 24 h exactly: one game with 2 * 43 200 s.
    let mut c = cfg(2);
    c.cycles = 1;
    c.time = TimeControl::Clock {
        base_ms: 43_200_000,
        increment_ms: 0,
    };
    assert_eq!(c.estimate().max_duration, Duration::from_hours(24));
    assert!(!c.estimate().warn);
    c.time = TimeControl::Clock {
        base_ms: 43_201_000,
        increment_ms: 0,
    };
    assert!(c.estimate().warn);
}

#[test]
fn the_estimate_flags_a_large_state_above_ten_thousand_games() {
    let mut c = cfg(2);
    c.cycles = 10_000;
    assert!(!c.estimate().large_state);
    c.cycles = 10_001;
    assert!(c.estimate().large_state);
    assert_eq!(c.estimate().state_bytes, 10_001 * STATE_BYTES_PER_GAME);
}

#[test]
fn the_estimate_never_overflows_on_absurd_input() {
    let mut c = cfg(32);
    c.cycles = u32::MAX;
    c.time = TimeControl::Clock {
        base_ms: u64::MAX,
        increment_ms: u64::MAX,
    };
    let e = c.estimate();
    assert!(e.warn && e.large_state);
    let _ = c.state_bytes();
}

// ---- clamped ----

fn arb_time() -> impl Strategy<Value = TimeControl> {
    prop_oneof![
        (1u64..u64::MAX, 0u64..u64::MAX).prop_map(|(b, i)| TimeControl::Clock {
            base_ms: b,
            increment_ms: i
        }),
        any::<u64>().prop_map(|ms| TimeControl::MoveTime { ms }),
        any::<u32>().prop_map(TimeControl::Depth),
        any::<u64>().prop_map(TimeControl::Nodes),
    ]
}

proptest! {
    #[test]
    fn a_clamped_configuration_always_validates_and_clamping_is_idempotent(
        cycles in any::<u32>(), leeway in any::<u32>(), margin in any::<u32>(),
        ceiling in any::<u32>(), seed in any::<u64>(), retries in any::<u8>(),
        failures in any::<u32>(), book_limit in proptest::option::of(any::<u32>()),
        time in arb_time(), rc in any::<u32>(), rm in any::<u32>(), dm in any::<u32>(),
        mp in proptest::option::of(any::<u32>()), tb in any::<u32>(),
        engine in any::<u64>(), name in ".{0,200}", site in ".{0,200}",
    ) {
        let mut c = cfg(3);
        c.cycles = cycles; c.leeway_percent = leeway; c.margin_ms = margin;
        c.depth_limit_secs = ceiling; c.seed = seed;
        c.failures.max_retries = retries; c.failures.max_failures = failures;
        c.book_ply_limit = book_limit; c.time = time;
        c.adjudication = Adjudication {
            resign: Some(ResignRule { centipawns: rc, moves: rm, require_both: true }),
            draw: Some(DrawRule { after_move: dm, centipawns: rc, moves: rm }),
            max_plies: mp, tablebase: true, tb_pieces: tb,
        };
        c.participants[0].engine = engine;
        c.name = name; c.site = site;
        let fixed = c.clone().clamped();
        prop_assert_eq!(fixed.validate(), Ok(()), "{:?}", fixed);
        prop_assert!(fixed.failures.max_retries <= 3);
        prop_assert_eq!(fixed.clone().clamped(), fixed);
    }
}

#[test]
fn clamping_leaves_a_valid_configuration_untouched() {
    let mut c = cfg(3);
    c.cycles = 7;
    c.seed = 12345;
    c.name = "Cup".into();
    assert_eq!(c.clone().clamped(), c);
}

#[test]
fn max_retries_above_three_is_clamped_to_three() {
    // Spec D1269: 0 to 3.
    let mut c = cfg(2);
    c.failures.max_retries = 200;
    assert_eq!(c.clamped().failures.max_retries, 3);
}

#[test]
fn validate_refuses_a_retry_count_outside_zero_to_three() {
    // Spec D1269 says 0 to 3; `validate` does not look at it (D1309 does not
    // list it). A user-edited preferences file is clamped, but a state
    // file or `Start` message with 200 retries passes validation.
    let mut c = cfg(2);
    c.failures.max_retries = 200;
    assert!(
        c.validate().is_err(),
        "max_retries 200 accepted by validate"
    );
}

// ---- preferences [tournament] ----

fn tournament_of(text: &str) -> TournamentConfig {
    *load_preferences(text).prefs.tournament
}

#[test]
fn missing_table_gives_the_default_configuration() {
    let loaded = load_preferences("");
    assert_eq!(*loaded.prefs.tournament, TournamentConfig::default());
    assert!(loaded.issues.is_empty());
}

#[test]
fn numbers_out_of_range_are_clamped_not_refused() {
    let t = tournament_of(
        "[tournament]\ncycles = 9999\nleeway_percent = 5\nmargin_ms = 99999999\ndepth_limit_secs = 0\n",
    );
    assert_eq!(t.cycles, 100);
    assert_eq!(t.leeway_percent, 100);
    assert_eq!(t.margin_ms, 30_000);
    assert_eq!(t.depth_limit_secs, 10);
}

#[test]
fn a_table_that_does_not_fit_becomes_the_default_with_one_remark_and_the_rest_survives() {
    for bad in [
        "[board]\ntheme = \"wood\"\n\n[tournament]\nscheduling = \"Banana\"\n",
        "[board]\ntheme = \"wood\"\n\n[tournament]\ncycles = \"many\"\n",
        "[board]\ntheme = \"wood\"\n\n[tournament]\ncycles = -4\n",
        "[board]\ntheme = \"wood\"\n\n[tournament]\nparticipants = 3\n",
    ] {
        let loaded = load_preferences(bad);
        assert_eq!(
            *loaded.prefs.tournament,
            TournamentConfig::default(),
            "{bad}"
        );
        assert!(
            loaded
                .issues
                .iter()
                .any(|i| matches!(i, PrefsIssue::InvalidValue { key } if key == "tournament")),
            "{bad}: {:?}",
            loaded.issues
        );
        assert_eq!(loaded.prefs.board.theme, "wood", "{bad}");
    }
}

#[test]
fn a_tournament_key_that_is_not_a_table_is_tolerated() {
    let loaded = load_preferences("tournament = 5\n");
    assert_eq!(*loaded.prefs.tournament, TournamentConfig::default());
    assert!(!loaded.issues.is_empty());
}

#[test]
fn the_full_configuration_round_trips_through_the_preferences_text() {
    let mut c = cfg(3);
    c.name = "Spring Cup".into();
    c.scheduling = Scheduling::Gauntlet;
    c.tested = 1;
    c.cycles = 4;
    c.time = TimeControl::MoveTime { ms: 2_500 };
    c.opening = OpeningSource::Suite;
    c.suite = Some(SuiteConfig {
        path: PathBuf::from("suites").join("o.epd"),
        order: SuiteOrder::Random,
        plies: 6,
    });
    c.seed = 424_242;
    c.ponder = true;
    c.hash_mb = Some(128);
    c.threads = Some(2);
    c.adjudication.resign = Some(ResignRule::default());
    c.failures.max_retries = 2;
    c.participants[1].options = vec![("Skill Level".into(), "5".into())];
    c.participants[1].elo = Some(2400);
    let prefs = Preferences {
        tournament: Box::new(c.clone()),
        ..Preferences::default()
    };
    let loaded = load_preferences(&render_preferences(&prefs));
    assert_eq!(*loaded.prefs.tournament, c);
    assert!(loaded.issues.is_empty(), "{:?}", loaded.issues);
}

#[tokio::test]
async fn a_wild_configuration_never_empties_the_preferences_file() {
    // D1310: u64::MAX in several fields.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("preferences.toml");
    let mut state = AppState::with_preferences_path(Some(path.clone()));
    let mut wild = cfg(2);
    wild.time = TimeControl::Nodes(u64::MAX);
    wild.seed = u64::MAX;
    wild.participants[0].engine = u64::MAX;
    wild.adjudication.max_plies = Some(u32::MAX);
    let effects = update(
        &mut state,
        Message::Prefs(PrefsMessage::Set(PrefChange::Tournament(Box::new(wild)))),
    );
    let mut saved = false;
    for effect in effects {
        if matches!(effect, Effect::SavePreferences { .. }) {
            let message = perform(effect).await;
            assert_eq!(message, Message::PreferencesSaved(Ok(())));
            saved = true;
        }
    }
    assert!(
        saved,
        "a change of the tournament preferences is saved at once"
    );
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(!text.trim().is_empty());
    let loaded = load_preferences(&text);
    assert!(loaded.issues.is_empty(), "{:?}", loaded.issues);
    assert_eq!(
        loaded.prefs.tournament.time,
        TimeControl::Nodes(i64::MAX as u64)
    );
    assert!(loaded.prefs.tournament.seed <= u64::MAX >> 1);
}

#[tokio::test]
async fn saving_preferences_that_cannot_be_rendered_leaves_the_old_file_alone() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("preferences.toml");
    let good = Preferences::default();
    let m = perform(Effect::SavePreferences {
        path: path.clone(),
        prefs: Box::new(good),
    })
    .await;
    assert_eq!(m, Message::PreferencesSaved(Ok(())));
    let before = std::fs::read(&path).unwrap();
    // The field is public: a caller may skip `clamped`.
    let mut bad = Preferences::default();
    bad.tournament.time = TimeControl::Nodes(u64::MAX);
    let m = perform(Effect::SavePreferences {
        path: path.clone(),
        prefs: Box::new(bad),
    })
    .await;
    assert!(matches!(m, Message::PreferencesSaved(Err(_))), "{m:?}");
    assert_eq!(std::fs::read(&path).unwrap(), before);
}
