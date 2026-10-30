//! M7-3a: the player report engine (acceptance criteria 2 to 7 and 9 of
//! `docs/specs/m7-3-relatorio-de-jogador.md`). The numbers are worked out by
//! hand from the fixture of the spec.
#![allow(clippy::unwrap_used, clippy::too_many_lines)]

#[path = "common/m7_3a_support.rs"]
mod support;

use mychess_core::Color::{Black, White};
use mychess_db::report::{NameMatch, PlayerReportOptions, ReportError};
use mychess_db::tree::performance;
use mychess_db::{CancelToken, NoProgress};
use support::{BadEntry, E4_E5_NF3, Flaky, ask, fixture, game, line_games, open, run, run_with};

#[test]
fn alfa_with_white_has_three_games_all_won() {
    let r = run(&mut fixture(), &ask("Alfa, A", White));
    assert_eq!(line_games(&r), [1, 4, 6]);
    assert_eq!(r.game_count(), 3);
    assert!(!r.core.interrupted);
    assert_eq!(r.core.skipped, 0);
    let s = r.player_score;
    assert_eq!((s.wins, s.draws, s.losses, s.unknown), (3, 0, 0, 0));
    assert_eq!(s.score_percent(), 100);
    assert_eq!(r.name, "Alfa, A");
    assert_eq!(r.color, White);
}

#[test]
fn average_ratings_and_performance_are_the_ones_of_the_player() {
    let r = run(&mut fixture(), &ask("Alfa, A", White));
    let mine = r.core.elo_report(White);
    let theirs = r.core.elo_report(Black);
    assert_eq!((mine.average, mine.games), (2500, 3));
    // Delta has no rating: (2400 + 2300 + 1) / 2 = 2350 over two games.
    assert_eq!((theirs.average, theirs.games), (2350, 2));
    // The player scored 100% against the rated opponents.
    assert_eq!(theirs.opponent_percent, 100);
    assert_eq!(theirs.opponent_performance, performance(2350, 100));
    // The opponents scored nothing against the rated games of the player.
    assert_eq!(mine.opponent_percent, 0);
    assert_eq!(mine.opponent_performance, performance(2500, 0));
}

#[test]
fn opponents_are_ranked_by_games_then_by_highest_rating() {
    let r = run(&mut fixture(), &ask("Alfa, A", White));
    let names: Vec<&str> = r.opponents.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["Beta, B", "Gama, G", "Delta, D"]);
    assert!(r.opponents.iter().all(|p| p.games == 1));
    assert!(r.opponents.iter().all(|p| p.score_percent == 100));
    let elos: Vec<u16> = r.opponents.iter().map(|p| p.max_elo).collect();
    assert_eq!(elos, [2400, 2300, 0]);
    let years: Vec<(u16, u16)> = r
        .opponents
        .iter()
        .map(|p| (p.first_year, p.last_year))
        .collect();
    assert_eq!(years, [(2020, 2020), (2019, 2019), (0, 0)]);
}

#[test]
fn eco_groups_are_tens_ordered_by_games_then_by_index() {
    let r = run(&mut fixture(), &ask("Alfa, A", White));
    let groups: Vec<(String, String, u32, u32)> = r
        .eco_groups
        .iter()
        .map(|g| (g.first.clone(), g.last.clone(), g.games, g.score_percent))
        .collect();
    let want = |a: &str, b: &str| (a.to_owned(), b.to_owned(), 1, 100);
    assert_eq!(
        groups,
        [want("C40", "C49"), want("C60", "C69"), want("C70", "C79")]
    );
}

#[test]
fn a_game_without_eco_does_not_enter_the_groups() {
    let pgn = [
        game(
            "P, P",
            "Q, Q",
            "1-0",
            "2000.01.01",
            "x",
            (0, 0),
            None,
            "1. e4 e5",
        ),
        game(
            "P, P",
            "Q, Q",
            "1-0",
            "2000.01.01",
            "x",
            (0, 0),
            Some("B90"),
            "1. e4 c5",
        ),
    ]
    .concat();
    let r = run(&mut open(&pgn), &ask("P, P", White));
    assert_eq!(r.game_count(), 2);
    assert_eq!(r.eco_groups.len(), 1);
    assert_eq!(r.eco_groups[0].first, "B90");
    assert_eq!(r.eco_groups[0].last, "B99");
    assert_eq!(r.eco_groups[0].games, 1);
}

#[test]
fn alfa_with_black_scores_from_the_black_side() {
    let r = run(&mut fixture(), &ask("Alfa, A", Black));
    assert_eq!(line_games(&r), [2, 7]);
    let s = r.player_score;
    // Game 2 is a draw and game 7 a win of Black: +1 =1 -0, (1 + 2) * 50 / 2.
    assert_eq!((s.wins, s.draws, s.losses), (1, 1, 0));
    assert_eq!(s.score_tenths(), 750);
    assert_eq!(s.score_percent(), 75);
    // Opponents are White's players: Gama twice, scoring 75% for Alfa.
    assert_eq!(r.opponents.len(), 1);
    assert_eq!(r.opponents[0].name, "Gama, G");
    assert_eq!(r.opponents[0].games, 2);
    assert_eq!(r.opponents[0].score_percent, 75);
    // ECO groups: C50 and D06, 1 game each; the draw scores 50, the win 100.
    let groups: Vec<(&str, u32)> = r
        .eco_groups
        .iter()
        .map(|g| (g.first.as_str(), g.score_percent))
        .collect();
    assert_eq!(groups, [("C50", 50), ("D00", 100)]);
}

#[test]
fn the_current_position_keeps_the_games_that_reach_it_and_folds_the_transposition_into_one_row() {
    let mut a = ask("Alfa, A", White);
    a.ucis = Some(E4_E5_NF3);
    let r = run(&mut fixture(), &a);
    // Game 6 (1.Nf3 e5 2.e4) reaches the same position by transposition.
    assert_eq!(line_games(&r), [1, 4, 6]);
    assert!(r.position.is_some());
    assert_eq!(r.stem, ["e4", "e5", "Nf3"]);
    assert_eq!(r.first_move_number(), 2);
    // The theory table has two rows, not three: the continuation of game 6
    // is a prefix of the one of game 1 or 4, and the table builder shared
    // with the opening report (M7-2) folds it into that row, which then
    // counts 2 (accepted deviation from the Scid, spec criterion 6).
    assert_eq!(r.core.rows.len(), 2);
    assert_eq!(r.core.rows[0].count, 2);
    assert_eq!(r.core.rows.iter().map(|row| row.count).sum::<u32>(), 3);
}

#[test]
fn a_game_that_ends_at_the_position_is_not_in_the_report() {
    // Game 5 (Delta, White) ends at 1.e4 e5 2.Nf3.
    let mut a = ask("Delta, D", White);
    a.ucis = Some(E4_E5_NF3);
    let r = run(&mut fixture(), &a);
    assert_eq!(r.game_count(), 0);
    assert_eq!(line_games(&r), Vec::<u32>::new());
}

#[test]
fn prefix_matching_includes_the_second_spelling_and_keeps_the_typed_name() {
    let mut a = ask("Alfa", White);
    a.matching = NameMatch::Prefix;
    let r = run(&mut fixture(), &a);
    assert_eq!(line_games(&r), [1, 4, 6, 8]);
    assert_eq!(r.name, "Alfa");
    assert_eq!(r.matching, NameMatch::Prefix);
    let whites: std::collections::BTreeSet<&str> =
        r.core.lines.iter().map(|l| l.white.as_str()).collect();
    assert_eq!(
        whites.into_iter().collect::<Vec<_>>(),
        ["Alfa, A", "Alfa, Z"]
    );
}

#[test]
fn unknown_player_gives_an_empty_report_and_no_error() {
    let r = run(&mut fixture(), &ask("Zeta, Z", White));
    assert_eq!(r.game_count(), 0);
    assert!(r.core.lines.is_empty() && r.opponents.is_empty() && r.eco_groups.is_empty());
    assert!(!r.core.interrupted);
    assert_eq!(r.core.database.games, 8);
}

#[test]
fn a_player_without_games_of_that_colour_gives_an_empty_report() {
    let r = run(&mut fixture(), &ask("Alfa, Z", Black));
    assert_eq!(r.game_count(), 0);
    assert_eq!(r.player_score.finished(), 0);
    assert_eq!(r.player_score.score_percent(), 0);
}

#[test]
fn an_empty_database_gives_an_empty_report() {
    let r = run(&mut open(""), &ask("Alfa, A", White));
    assert_eq!(r.game_count(), 0);
    assert_eq!(r.core.database.games, 0);
}

#[test]
fn invalid_names_are_refused() {
    for name in ["", "  ", "?"] {
        let r = run_with(
            &mut fixture(),
            &ask(name, White),
            &mut NoProgress,
            &CancelToken::new(),
        );
        assert!(matches!(r, Err(ReportError::InvalidName)), "{name:?}");
    }
}

#[test]
fn a_line_that_does_not_lead_to_the_position_is_refused() {
    use mychess_db::report::{PlayerReportRequest, ReportStart, player_report};
    let mut db = fixture();
    let stats = mychess_db::PlayerStats::from_database(&db);
    let (_, moves) = support::line("e2e4 e7e5");
    let (other, _) = support::line("d2d4");
    let opts = PlayerReportOptions::default();
    let r = player_report(
        &mut db,
        &PlayerReportRequest {
            name: "Alfa, A",
            color: White,
            matching: NameMatch::Exact,
            start: ReportStart::Position(&other, &moves),
            options: &opts,
            eco_book: None,
            player_stats: &stats,
            stats_config: None,
            today: support::today(),
        },
        &mut NoProgress,
        &CancelToken::new(),
    );
    assert!(matches!(r, Err(ReportError::StemMismatch)));
}

#[test]
fn the_games_of_the_report_are_the_highest_rated_up_to_the_table_limit() {
    let mut a = ask("Alfa, A", White);
    a.options.max_table_games = 2;
    let r = run(&mut fixture(), &a);
    // The statistics still use the three games; the table takes two.
    assert_eq!(r.game_count(), 3);
    assert_eq!(r.core.table_order.len(), 2);
}

#[test]
fn a_cancelled_run_before_the_start_is_an_interrupted_empty_report() {
    let cancel = CancelToken::new();
    cancel.cancel();
    let r = run_with(
        &mut fixture(),
        &ask("Alfa, A", White),
        &mut NoProgress,
        &cancel,
    )
    .unwrap();
    assert!(r.core.interrupted);
    assert_eq!(r.game_count(), 0);
    assert_eq!(line_games(&r), Vec::<u32>::new());
}

fn many_games() -> String {
    let mut pgn = String::new();
    for i in 0..6000 {
        let moves = if i % 2 == 0 {
            "1. e4 e5 2. Nf3"
        } else {
            "1. e4 e5 2. Nf3 Nc6"
        };
        pgn.push_str(&game(
            "A, A",
            "B, B",
            "1-0",
            "2000.01.01",
            "x",
            (0, 0),
            Some("C44"),
            moves,
        ));
    }
    pgn
}

#[test]
fn cancelled_in_the_middle_gives_a_coherent_partial_report_and_monotonic_progress() {
    let mut db = open(&many_games());
    let cancel = CancelToken::new();
    let token = cancel.clone();
    let mut seen: Vec<(u64, u64)> = Vec::new();
    let mut calls = 0u32;
    let mut sink = |done: u64, total: u64| {
        seen.push((done, total));
        calls += 1;
        // Let the selection finish, then stop in the pass over the games.
        if done > 6000 {
            token.cancel();
        }
    };
    let r = run_with(&mut db, &ask("A, A", White), &mut sink, &cancel).unwrap();
    assert!(r.core.interrupted);
    assert!(
        seen.windows(2).all(|w| w[0].0 <= w[1].0),
        "monotonic: {seen:?}"
    );
    assert!(seen.iter().all(|(d, t)| d <= t));
    // Every figure agrees with the lines that were read.
    let read = r.game_count();
    assert!(read < 3000, "a partial report: {read}");
    assert_eq!(u32::try_from(r.core.lines.len()).unwrap(), read);
    assert_eq!(r.player_score.wins, read);
}

#[test]
fn progress_is_monotonic_and_ends_at_the_total() {
    let mut seen: Vec<(u64, u64)> = Vec::new();
    let mut sink = |done: u64, total: u64| seen.push((done, total));
    let mut a = ask("Alfa, A", White);
    a.ucis = Some(E4_E5_NF3);
    run_with(&mut fixture(), &a, &mut sink, &CancelToken::new()).unwrap();
    assert!(seen.windows(2).all(|w| w[0].0 <= w[1].0), "{seen:?}");
    assert!(seen.windows(2).all(|w| w[0].1 == w[1].1), "fixed total");
    let (done, total) = *seen.last().unwrap();
    assert_eq!(done, total);
}

#[test]
fn an_unreadable_game_is_skipped_and_counted() {
    let mut db = Flaky {
        inner: fixture(),
        broken: 4,
    };
    let r = run_with(
        &mut db,
        &ask("Alfa, A", White),
        &mut NoProgress,
        &CancelToken::new(),
    )
    .unwrap();
    assert_eq!(r.core.skipped, 1);
    assert_eq!(line_games(&r), [1, 6]);
    assert_eq!(r.game_count(), 2);
}

#[test]
fn the_result_is_the_same_with_one_thread_and_many() {
    let one = rayon::ThreadPoolBuilder::new()
        .num_threads(1)
        .build()
        .unwrap();
    let many = rayon::ThreadPoolBuilder::new()
        .num_threads(3)
        .build()
        .unwrap();
    let a = one.install(|| run(&mut open(&support::fixture_pgn()), &ask("Alfa, A", White)));
    let b = many.install(|| run(&mut open(&support::fixture_pgn()), &ask("Alfa, A", White)));
    assert_eq!(a.core, b.core);
    assert_eq!(a.opponents, b.opponents);
    assert_eq!(a.eco_groups, b.eco_groups);
    assert_eq!(a.player_score, b.player_score);
}

#[test]
fn the_selection_does_not_depend_on_the_options_shown() {
    let mut off = ask("Alfa, A", White);
    off.options = PlayerReportOptions {
        stats: false,
        oldest: 0,
        newest: 0,
        opponents: 0,
        results: false,
        avg_perf: false,
        high_rating: 0,
        eco_codes: 0,
        themes: false,
        endgames: false,
        max_table_games: 0,
        extra_moves: 0,
    };
    let a = run(&mut fixture(), &ask("Alfa, A", White));
    let b = run(&mut fixture(), &off);
    assert_eq!(a.core.total, b.core.total);
    assert_eq!(a.player_score, b.player_score);
    assert!(b.core.rows.is_empty(), "no theory table when it is off");
}

#[test]
fn an_unreadable_game_in_the_search_by_position_is_counted() {
    // Game 4 is Alfa's and passes through 1.e4 e5 2.Nf3: the search by
    // position cannot read it, so it leaves the report and must be counted.
    let mut db = Flaky {
        inner: fixture(),
        broken: 4,
    };
    let mut a = ask("Alfa, A", White);
    a.ucis = Some(E4_E5_NF3);
    let r = run_with(&mut db, &a, &mut NoProgress, &CancelToken::new()).unwrap();
    assert_eq!(r.core.skipped, 1);
    assert_eq!(line_games(&r), [1, 6]);
}

#[test]
fn accents_are_folded_by_the_prefix_and_contains_modes() {
    let pgn = game(
        "Álvaro, X",
        "Q, Q",
        "1-0",
        "2000.01.01",
        "x",
        (0, 0),
        None,
        "1. e4 e5",
    );
    let mut a = ask("álvaro", White);
    a.matching = NameMatch::Prefix;
    let r = run(&mut open(&pgn), &a);
    assert_eq!(r.game_count(), 1);
    a.matching = NameMatch::Contains;
    a.name = "ÁLVARO";
    assert_eq!(run(&mut open(&pgn), &a).game_count(), 1);
    // The exact mode keeps the case, accents included.
    a.matching = NameMatch::Exact;
    a.name = "álvaro, X";
    assert_eq!(run(&mut open(&pgn), &a).game_count(), 0);
}

#[test]
fn a_name_without_a_letter_or_a_digit_is_refused() {
    for bad in ["", "?", ",", " , ", "-"] {
        let r = run_with(
            &mut fixture(),
            &ask(bad, White),
            &mut NoProgress,
            &CancelToken::new(),
        );
        assert!(matches!(r, Err(ReportError::InvalidName)), "{bad:?}");
    }
}

#[test]
fn a_trailing_space_does_not_keep_longer_names_out_of_the_prefix_mode() {
    // `trim` is applied (documented): "Alfa " selects what "Alfa" selects.
    let mut a = ask("Alfa ", White);
    a.matching = NameMatch::Prefix;
    assert_eq!(line_games(&run(&mut fixture(), &a)), [1, 4, 6, 8]);
}

#[test]
fn an_unreadable_index_entry_during_the_selection_is_counted() {
    let mut db = BadEntry {
        inner: fixture(),
        broken: 3,
    };
    let r = run(&mut db, &ask("Alfa, A", White));
    assert_eq!(r.core.skipped, 1);
}
