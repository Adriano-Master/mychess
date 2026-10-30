//! M8-1c (slice A) contract tests of `Store clock` (spec D1506, D1514): the
//! `[%emt h:mm:ss]` / `[%clk h:mm:ss]` comments of the moves that cost time,
//! the readers of the time graph (M4-7) and the `[play]` preference.
#![allow(
    clippy::assert_is_empty,
    clippy::manual_assert_eq,
    clippy::unwrap_used,
    clippy::expect_used
)]

mod m8_1c_common;

use std::time::Duration;

use m8_1c_common::*;
use mychess_app::engines::EngineId;
use mychess_app::graphs::time_series;
use mychess_app::{
    BookSelection, PlayConfig, PlayPrefs, Preferences, TimeMode, load_preferences,
    render_preferences,
};
use mychess_core::{Color, clock_of, emt_of};

const DEPTH: TimeMode = TimeMode::Depth(2);

fn stored(color: Color, mode: TimeMode) -> mychess_app::PlaySetup {
    let mut s = setup(color, mode);
    s.config.store_clock = true;
    s
}

fn comments(c: &mychess_app::PlayController) -> Vec<Option<String>> {
    let game = c.game();
    game.mainline()
        .filter(|id| *id != game.root())
        .map(|id| game.comment(id).unwrap().map(str::to_owned))
        .collect()
}

// ---- what is written ----

#[test]
fn a_timebonus_move_gets_emt_then_clk_after_the_increment() {
    let (mut c, _, t0) = begin(stored(Color::White, blitz()));
    c.player_move(uci(&c, "e2e4"), t0 + 7 * SEC);
    // 300 - 7 + 3 = 296 s = 0:04:56.
    assert_eq!(
        comment_at_tip(&c).as_deref(),
        Some("[%emt 0:00:07] [%clk 0:04:56]")
    );
    c.opponent_moved(Some(uci(&c, "e7e5")), Some(12 * SEC), t0 + 19 * SEC, None);
    // 300 - 12 + 3 = 291 s = 0:04:51.
    assert_eq!(
        comment_at_tip(&c).as_deref(),
        Some("[%emt 0:00:12] [%clk 0:04:51]")
    );
}

#[test]
fn the_clock_of_the_engine_measured_by_the_instants_is_written_too() {
    let (mut c, _, t0) = begin(stored(Color::White, blitz()));
    c.player_move(uci(&c, "e2e4"), t0 + 7 * SEC);
    c.opponent_moved(Some(uci(&c, "e7e5")), None, t0 + 17 * SEC, None);
    assert_eq!(
        comment_at_tip(&c).as_deref(),
        Some("[%emt 0:00:10] [%clk 0:04:53]")
    );
}

#[test]
fn modes_without_a_countdown_write_only_emt() {
    for mode in [
        TimeMode::Depth(2),
        TimeMode::Nodes(500),
        TimeMode::MoveTime(5),
    ] {
        let (mut c, _, t0) = begin(stored(Color::White, mode));
        c.player_move(uci(&c, "e2e4"), t0 + 7 * SEC);
        assert_eq!(
            comment_at_tip(&c).as_deref(),
            Some("[%emt 0:00:07]"),
            "{mode:?}"
        );
        c.opponent_moved(Some(uci(&c, "e7e5")), Some(3 * SEC), t0 + 10 * SEC, None);
        assert_eq!(
            comment_at_tip(&c).as_deref(),
            Some("[%emt 0:00:03]"),
            "{mode:?}"
        );
    }
}

#[test]
fn the_time_is_cut_to_the_second_and_a_zero_move_is_still_written() {
    let (mut c, _, t0) = begin(stored(Color::White, DEPTH));
    c.player_move(uci(&c, "e2e4"), t0 + Duration::from_millis(7900));
    // D1514 (truncated, like the tournaments); D1506 says "rounded".
    assert_eq!(comment_at_tip(&c).as_deref(), Some("[%emt 0:00:07]"));
    c.opponent_moved(
        Some(uci(&c, "e7e5")),
        Some(Duration::ZERO),
        t0 + 8 * SEC,
        None,
    );
    assert_eq!(comment_at_tip(&c).as_deref(), Some("[%emt 0:00:00]"));
}

#[test]
fn hours_use_the_h_mm_ss_format() {
    let (mut c, _, t0) = begin(stored(Color::White, timebonus(120, 0, 120, 0)));
    c.player_move(uci(&c, "e2e4"), t0 + 3725 * SEC);
    // 7200 - 3725 = 3475 s = 0:57:55.
    assert_eq!(
        comment_at_tip(&c).as_deref(),
        Some("[%emt 1:02:05] [%clk 0:57:55]")
    );
}

#[test]
fn it_is_off_by_default() {
    assert!(!PlayConfig::new(EngineId(1)).store_clock);
    let (mut c, _, t0) = begin(setup(Color::White, blitz()));
    c.player_move(uci(&c, "e2e4"), t0 + 5 * SEC);
    c.opponent_moved(Some(uci(&c, "e7e5")), Some(2 * SEC), t0 + 7 * SEC, None);
    assert_eq!(comments(&c), [None, None]);
}

// ---- free moves, existing comments ----

#[test]
fn free_moves_of_the_book_get_no_time_comment_but_keep_their_own() {
    // Book 1.e4 e5 2.Nf3 with the engine as White: e4 and Nf3 are free.
    let mut s = stored(Color::Black, blitz());
    s.book = Some(mini_book(BookSelection::Best));
    let (mut c, _, t0) = begin(s);
    assert_eq!(comment_at_tip(&c), None, "e4 is free: no time comment");
    c.player_move(uci(&c, "e7e5"), t0 + 4 * SEC);
    assert_eq!(mainline_ucis(c.game()), ["e2e4", "e7e5", "g1f3"]);
    let all = comments(&c);
    assert_eq!(all[0], None);
    // 300 - 4 + 3 = 299 s.
    assert_eq!(all[1].as_deref(), Some("[%emt 0:00:04] [%clk 0:04:59]"));
    assert_eq!(all[2].as_deref(), Some("Last book move (book)"));
}

#[test]
fn a_move_with_time_and_a_comment_keeps_the_text_and_adds_the_codes() {
    // Book 1.e4 e5 2.Nf3, the player is White: Nf3 is the last book move of
    // the player and gets the comment of the book plus the time.
    let mut s = stored(Color::White, blitz());
    s.book = Some(mini_book(BookSelection::Best));
    let (mut c, _, t0) = begin(s);
    c.player_move(uci(&c, "e2e4"), t0 + SEC);
    assert_eq!(mainline_ucis(c.game()), ["e2e4", "e7e5"]);
    c.player_move(uci(&c, "g1f3"), t0 + 5 * SEC);
    let text = comment_at_tip(&c).expect("comment");
    assert!(text.starts_with("Last book move (book)"), "{text}");
    assert!(
        text.contains("[%emt 0:00:00]") || text.contains("[%emt 0:00:0"),
        "{text}"
    );
    assert!(text.contains("[%clk 0:"), "{text}");
    let emt_at = text.find("[%emt").unwrap();
    let clk_at = text.find("[%clk").unwrap();
    assert!(emt_at < clk_at, "emt first (D1514): {text}");
    assert_eq!(emt_of(&text).map(|s| s <= 4), Some(true));
}

#[test]
fn free_moves_of_an_opening_line_get_no_time_comment() {
    let mut s = stored(Color::White, blitz());
    s.config.start = mychess_app::StartFrom::Opening(
        mychess_app::OpeningLine::parse("Sicilian: e4 c5 Nf3").unwrap(),
    );
    let (mut c, _, t0) = begin(s);
    c.player_move(uci(&c, "e2e4"), t0 + 2 * SEC);
    let all = comments(&c);
    assert!(
        all[0]
            .as_deref()
            .is_some_and(|t| t.starts_with("[%emt 0:00:02]"))
    );
    assert_eq!(all[1], None, "c5 is free");
}

// ---- the readers of the time graph (M4-7) ----

#[test]
fn the_readers_of_the_core_and_the_time_graph_read_what_was_written() {
    let (mut c, _, t0) = begin(stored(Color::White, blitz()));
    let times = [(7u32, 12u32), (3, 30), (65, 1)];
    let moves = [("e2e4", "e7e5"), ("g1f3", "b8c6"), ("f1b5", "a7a6")];
    let mut now = t0;
    for ((mine, theirs), (a, b)) in moves.iter().zip(times) {
        let (a, b) = (SEC * a, SEC * b);
        now += a;
        c.player_move(uci(&c, mine), now);
        now += b;
        c.opponent_moved(Some(uci(&c, theirs)), Some(b), now, None);
    }
    let all = comments(&c);
    let emts: Vec<_> = all.iter().map(|t| emt_of(t.as_deref().unwrap())).collect();
    assert_eq!(
        emts,
        [Some(7), Some(12), Some(3), Some(30), Some(65), Some(1)]
    );
    let clks: Vec<_> = all
        .iter()
        .map(|t| clock_of(t.as_deref().unwrap()))
        .collect();
    assert_eq!(
        clks,
        [
            Some(296),
            Some(291),
            Some(296 - 3 + 3),
            Some(291 - 30 + 3),
            Some(296 - 65 + 3),
            Some(264 - 1 + 3),
        ]
    );
    let series = time_series(c.game());
    let seconds: Vec<u32> = series.per_move.iter().map(|p| p.seconds).collect();
    assert_eq!(seconds, [7, 12, 3, 30, 65, 1]);
    assert_eq!(series.white_total.last().map(|p| p.1), Some(75.0));
    assert_eq!(series.black_total.last().map(|p| p.1), Some(43.0));
}

#[test]
fn a_game_without_the_option_has_an_empty_time_graph() {
    let (mut c, _, t0) = begin(setup(Color::White, blitz()));
    c.player_move(uci(&c, "e2e4"), t0 + SEC);
    c.opponent_moved(Some(uci(&c, "e7e5")), Some(SEC), t0 + 2 * SEC, None);
    assert!(time_series(c.game()).per_move.is_empty());
}

// ---- take back and the end ----

#[test]
fn the_last_move_that_ends_the_game_still_gets_its_time() {
    let (mut c, _, t0) = begin(stored(Color::White, blitz()));
    play_ucis(&mut c, &["f2f3", "e7e5", "g2g4"], t0);
    c.opponent_moved(Some(uci(&c, "d8h4")), Some(2 * SEC), t0 + 2 * SEC, None);
    let text = comment_at_tip(&c).expect("comment");
    assert!(text.contains("[%emt 0:00:02]"), "{text}");
}

// ---- the preference ----

#[test]
fn the_preference_defaults_to_off_and_goes_to_the_config() {
    assert!(!PlayPrefs::default().store_clock);
    assert!(!PlayPrefs::default().to_config(EngineId(1)).store_clock);
    let on = PlayPrefs {
        store_clock: true,
        ..PlayPrefs::default()
    };
    assert!(on.to_config(EngineId(1)).store_clock);
}

#[test]
fn the_preference_is_persisted_in_the_play_section() {
    let prefs = Preferences {
        play: Box::new(PlayPrefs {
            store_clock: true,
            ..PlayPrefs::default()
        }),
        ..Preferences::default()
    };
    let text = render_preferences(&prefs);
    let play = text.split("[play]").nth(1).expect("[play]");
    assert!(play.contains("store_clock = true"), "{text}");
    let loaded = load_preferences(&text);
    assert!(loaded.prefs.play.store_clock);
    // And the default is not turned on by a file that says nothing.
    assert!(
        !load_preferences("[play]\nmovetime = 5\n")
            .prefs
            .play
            .store_clock
    );
    assert!(!load_preferences("").prefs.play.store_clock);
}

#[test]
fn a_malformed_value_resets_only_that_field() {
    let loaded = load_preferences("[play]\nstore_clock = \"yes\"\nmovetime = 9\n");
    assert!(!loaded.prefs.play.store_clock);
    assert_eq!(loaded.prefs.play.movetime, 9);
    assert!(!loaded.issues.is_empty());
}
