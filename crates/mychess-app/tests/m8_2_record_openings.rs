//! M8-2a contract tests of the PGN of a tournament game (spec D1280,
//! D1281, D1302) and of the openings (D1272, D1273, D1293, D1316).
#![allow(
    clippy::assert_is_empty,
    clippy::cast_possible_truncation,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::too_many_lines
)]

mod m8_2_common;

use std::io::BufReader;
use std::time::{Duration, Instant};

use m8_2_common::*;
use mychess_app::tournament::{
    Adjudication, BookConfig, BookPick, CommentOptions, GameEnd, GameSetup, LogEvent, Opening,
    OpeningPlan, OpeningSource, OpeningSuite, Participant, PlayerInfo, RecordInput, Reply,
    Scheduling, ScoreComment, SearchOutcome, Step, SuiteConfig, SuiteOrder, Termination,
    TimeControl, TournamentConfig, TournamentGame, build_game, build_schedule, clock_text,
    load_suite, time_control_tag,
};
use mychess_app::{BookSelection, PlayBook};
use mychess_book::Book;
use mychess_core::{Game, GameResult, Position};
use mychess_engine::Score;
use mychess_pgn::read::{PgnReader, ReadOptions};
use mychess_pgn::write::{WriteOptions, game_to_string};
use proptest::prelude::*;
use rstest::rstest;

fn to_pgn(game: &Game) -> String {
    game_to_string(game, &WriteOptions::default()).expect("pgn")
}

fn parse_all(text: &str) -> Vec<Game> {
    PgnReader::new(BufReader::new(text.as_bytes()), ReadOptions::default())
        .filter_map(Result::ok)
        .map(|p| p.game)
        .collect()
}

fn game_setup(time: TimeControl, opening: Opening) -> GameSetup {
    GameSetup {
        opening,
        opening_is_book: false,
        time,
        leeway_percent: 175,
        margin: Duration::from_secs(2),
        depth_limit: Duration::from_secs(600),
        claim_draws: true,
        adjudication: Adjudication::default(),
        book: None,
        book_ply_limit: None,
        ponder: [false, false],
        seed: 1,
        number: 1,
        tablebase: None,
    }
}

/// Plays `(uci, white_pov_score)` pairs, 1.5 s each, and records the game.
fn recorded(
    config: &TournamentConfig,
    opening: &Opening,
    moves: &[(&str, Option<Score>)],
    end_override: Option<GameEnd>,
    white: &PlayerInfo,
    black: &PlayerInfo,
) -> Game {
    let now = Instant::now();
    let mut game = TournamentGame::new(game_setup(config.time, opening.clone()), now);
    for (uci, score) in moves {
        assert!(matches!(game.advance(now), Step::Think(_)));
        let m = game.position().parse_uci(uci).expect("uci");
        game.reply(
            &Reply {
                mv: Some(m),
                spent: Duration::from_millis(1_500),
                score: *score,
                depth: Some(12),
                ponder: None,
            },
            now,
        );
    }
    let end = end_override.unwrap_or(GameEnd {
        result: GameResult::Unknown,
        termination: Termination::Unterminated,
        comment: None,
    });
    let pairing = build_schedule(2, Scheduling::RoundRobin, 1, 0).pairings[0];
    build_game(&RecordInput {
        config,
        pairing: &pairing,
        white,
        black,
        opening,
        game: &game,
        end: &end,
        date: today(),
        redo: 0,
    })
    .expect("game")
}

fn player(name: &str) -> PlayerInfo {
    PlayerInfo {
        name: name.to_owned(),
        elo: None,
        engine_name: None,
    }
}

fn comments_of(game: &Game) -> Vec<Option<String>> {
    game.mainline()
        .filter(|id| *id != game.root())
        .map(|id| game.comment(id).unwrap().map(str::to_owned))
        .collect()
}

fn config_with(score: ScoreComment, clock: bool, move_time: bool) -> TournamentConfig {
    let mut c = cfg(2);
    c.time = TimeControl::Clock {
        base_ms: 60_000,
        increment_ms: 1_000,
    };
    c.comments = CommentOptions {
        score,
        clock,
        move_time,
    };
    c
}

// ---- tags ----

#[test]
fn a_tournament_game_has_every_tag_of_the_spec() {
    let mut config = cfg(2);
    config.name = "Spring Cup".into();
    config.site = "The Lab".into();
    config.time = TimeControl::Clock {
        base_ms: 60_000,
        increment_ms: 1_000,
    };
    config.participants[0].elo = Some(3100);
    let white = PlayerInfo {
        name: "P0".into(),
        elo: Some(3100),
        engine_name: Some("Stockfish 99".into()),
    };
    let black = PlayerInfo {
        name: "P1".into(),
        elo: None,
        engine_name: Some("Other 1".into()),
    };
    let g = recorded(
        &config,
        &Opening::initial(),
        &[("e2e4", Some(Score::Cp(10))), ("e7e5", Some(Score::Cp(10)))],
        Some(GameEnd {
            result: GameResult::Draw,
            termination: Termination::Normal,
            comment: Some("3 fold repetition".into()),
        }),
        &white,
        &black,
    );
    let t = g.tags();
    assert_eq!(
        (t.event.as_str(), t.site.as_str()),
        ("Spring Cup", "The Lab")
    );
    assert_eq!(t.date.to_string(), "2026.10.24");
    assert_eq!(t.round, "1.1");
    assert_eq!((t.white.as_str(), t.black.as_str()), ("P0", "P1"));
    assert_eq!(t.white_rating.map(|r| r.value.get()), Some(3100));
    assert!(t.black_rating.is_none());
    assert_eq!(t.result, GameResult::Draw);
    for (k, v) in [
        ("WhiteType", "program"),
        ("BlackType", "program"),
        ("Termination", "normal"),
        ("TimeControl", "60+1"),
        ("WhiteEngine", "Stockfish 99"),
        ("BlackEngine", "Other 1"),
        ("PlyCount", "2"),
    ] {
        assert_eq!(t.get_extra(k), Some(v), "tag {k}");
    }
    let pgn = to_pgn(&g);
    assert!(pgn.contains("[Result \"1/2-1/2\"]"));
    assert!(!pgn.contains("[FEN"), "standard start has no FEN: {pgn}");
}

#[rstest]
#[case(TimeControl::Clock { base_ms: 60_000, increment_ms: 1_000 }, "60+1")]
#[case(TimeControl::Clock { base_ms: 300_000, increment_ms: 0 }, "300")]
#[case(TimeControl::Clock { base_ms: 1_500, increment_ms: 500 }, "1.5+0.5")]
#[case(TimeControl::Clock { base_ms: 0, increment_ms: 2_000 }, "0+2")]
#[case(TimeControl::MoveTime { ms: 3_000 }, "1/3")]
#[case(TimeControl::MoveTime { ms: 250 }, "1/0.25")]
#[case(TimeControl::Depth(12), "-")]
#[case(TimeControl::Nodes(1_000_000), "-")]
fn the_pgn_time_control_tag(#[case] time: TimeControl, #[case] expected: &str) {
    assert_eq!(time_control_tag(time), expected);
}

#[test]
fn the_scid_extra_tags_depth_nodes_and_movetime_are_written() {
    let mk = |time| {
        let mut c = cfg(2);
        c.time = time;
        to_pgn(&recorded(
            &c,
            &Opening::initial(),
            &[],
            None,
            &player("A"),
            &player("B"),
        ))
    };
    assert!(mk(TimeControl::Depth(9)).contains("[Depth \"9\"]"));
    assert!(mk(TimeControl::Nodes(5000)).contains("[Nodes \"5000\"]"));
    assert!(mk(TimeControl::MoveTime { ms: 1500 }).contains("[Movetime \"1.5\"]"));
}

#[test]
fn a_non_initial_opening_writes_fen_setup_and_its_name() {
    let opening = Opening {
        name: Some("Rook ending".into()),
        start: Position::from_fen("4k3/8/8/8/8/8/8/R3K3 w - - 0 1").unwrap(),
        moves: Vec::new(),
    };
    let g = recorded(
        &cfg(2),
        &opening,
        &[("a1a2", None)],
        None,
        &player("A"),
        &player("B"),
    );
    let pgn = to_pgn(&g);
    assert!(
        pgn.contains("[FEN \"4k3/8/8/8/8/8/8/R3K3 w - - 0 1\"]"),
        "{pgn}"
    );
    assert!(pgn.contains("[SetUp \"1\"]"));
    assert!(pgn.contains("[Opening \"Rook ending\"]"));
    let back = parse_all(&pgn);
    assert_eq!(back.len(), 1);
    assert_eq!(
        back[0].start_position().fen(),
        "4k3/8/8/8/8/8/8/R3K3 w - - 0 1"
    );
}

#[test]
fn hostile_player_names_cannot_inject_tags_or_break_the_game_apart() {
    let evil = "A\"]\n[Result \"1-0\"]\n\n1. e4 {x} *\n[White \"Z";
    let white = player(evil);
    let black = player("B\\\"\\");
    let g = recorded(
        &cfg(2),
        &Opening::initial(),
        &[("e2e4", Some(Score::Cp(0)))],
        Some(GameEnd {
            result: GameResult::Black,
            termination: Termination::Adjudication,
            comment: Some("Manual adjudication".into()),
        }),
        &white,
        &black,
    );
    let games = parse_all(&to_pgn(&g));
    assert_eq!(games.len(), 1, "a name never starts another game");
    assert_eq!(
        games[0].tags().result,
        GameResult::Black,
        "the real result survives"
    );
    assert_eq!(
        games[0]
            .mainline()
            .filter(|i| *i != games[0].root())
            .count(),
        1
    );
}

// ---- comments ----

#[test]
fn default_comments_carry_eval_clock_and_move_time() {
    let c = config_with(ScoreComment::Eval, true, true);
    let g = recorded(
        &c,
        &Opening::initial(),
        &[("e2e4", Some(Score::Cp(35))), ("e7e5", Some(Score::Cp(35)))],
        None,
        &player("A"),
        &player("B"),
    );
    let cm = comments_of(&g);
    assert_eq!(
        cm[0].as_deref(),
        Some("[%eval 0.35,12] [%clk 0:00:59] [%emt 0:00:01]")
    );
    // Black's reply: +35 for White, 60 - 1.5 + 1 = 59.5 s on its clock.
    assert_eq!(
        cm[1].as_deref(),
        Some("[%eval 0.35,12] [%clk 0:00:59] [%emt 0:00:01]")
    );
}

#[rstest]
#[case(ScoreComment::Text, "+0.35")]
#[case(ScoreComment::Pct, "[% +0.35]")]
#[case(ScoreComment::Eval, "[%eval 0.35,12]")]
fn each_score_style_of_the_scid(#[case] style: ScoreComment, #[case] expected: &str) {
    let c = config_with(style, false, false);
    let g = recorded(
        &c,
        &Opening::initial(),
        &[("e2e4", Some(Score::Cp(35)))],
        None,
        &player("A"),
        &player("B"),
    );
    assert_eq!(comments_of(&g)[0].as_deref(), Some(expected));
}

#[test]
fn score_none_and_clock_off_and_emt_off_leave_no_comment() {
    let c = config_with(ScoreComment::None, false, false);
    let g = recorded(
        &c,
        &Opening::initial(),
        &[("e2e4", Some(Score::Cp(35)))],
        None,
        &player("A"),
        &player("B"),
    );
    assert_eq!(comments_of(&g), [None]);
    let c = config_with(ScoreComment::None, true, false);
    let g = recorded(
        &c,
        &Opening::initial(),
        &[("e2e4", None)],
        None,
        &player("A"),
        &player("B"),
    );
    assert_eq!(comments_of(&g)[0].as_deref(), Some("[%clk 0:00:59]"));
    let c = config_with(ScoreComment::None, false, true);
    let g = recorded(
        &c,
        &Opening::initial(),
        &[("e2e4", None)],
        None,
        &player("A"),
        &player("B"),
    );
    assert_eq!(comments_of(&g)[0].as_deref(), Some("[%emt 0:00:01]"));
}

#[test]
fn a_white_pov_negative_score_on_a_black_move_is_written_negative_for_white() {
    // D1302: the engine layer speaks White's point of view; Black playing
    // with Cp(-700) means Black is winning, and the comment keeps -7.00.
    let c = config_with(ScoreComment::Eval, false, false);
    let g = recorded(
        &c,
        &Opening::initial(),
        &[
            ("e2e4", Some(Score::Cp(20))),
            ("e7e5", Some(Score::Cp(-700))),
        ],
        None,
        &player("A"),
        &player("B"),
    );
    let cm = comments_of(&g);
    assert_eq!(cm[0].as_deref(), Some("[%eval 0.20,12]"));
    assert_eq!(cm[1].as_deref(), Some("[%eval -7.00,12]"));
    let c = config_with(ScoreComment::Text, false, false);
    let g = recorded(
        &c,
        &Opening::initial(),
        &[("e2e4", None), ("e7e5", Some(Score::Cp(-700)))],
        None,
        &player("A"),
        &player("B"),
    );
    assert_eq!(comments_of(&g)[1].as_deref(), Some("-7.00"));
}

#[test]
fn mate_scores_are_written_as_mate_in_n_for_white() {
    let c = config_with(ScoreComment::Eval, false, false);
    let g = recorded(
        &c,
        &Opening::initial(),
        &[
            ("e2e4", Some(Score::Mate(3))),
            ("e7e5", Some(Score::Mate(-3))),
        ],
        None,
        &player("A"),
        &player("B"),
    );
    let cm = comments_of(&g);
    assert_eq!(cm[0].as_deref(), Some("[%eval #3,12]"));
    assert_eq!(cm[1].as_deref(), Some("[%eval #-3,12]"));
}

#[test]
fn the_reason_of_the_result_goes_on_the_last_move() {
    let c = config_with(ScoreComment::None, false, false);
    let g = recorded(
        &c,
        &Opening::initial(),
        &[("e2e4", None), ("e7e5", None)],
        Some(GameEnd {
            result: GameResult::White,
            termination: Termination::TimeForfeit,
            comment: Some("Black loses on time".into()),
        }),
        &player("A"),
        &player("B"),
    );
    let cm = comments_of(&g);
    assert_eq!(cm[0], None);
    assert_eq!(cm[1].as_deref(), Some("Black loses on time"));
    assert!(to_pgn(&g).contains("[Termination \"time forfeit\"]"));
}

#[test]
fn a_game_without_moves_keeps_the_reason_in_the_pgn() {
    let g = recorded(
        &cfg(2),
        &Opening::initial(),
        &[],
        Some(GameEnd {
            result: GameResult::Black,
            termination: Termination::Abandoned,
            comment: Some("White engine failed".into()),
        }),
        &player("A"),
        &player("B"),
    );
    let pgn = to_pgn(&g);
    assert!(pgn.contains("White engine failed"), "{pgn}");
    assert_eq!(parse_all(&pgn).len(), 1);
}

#[rstest]
#[case(0, "0:00:00")]
#[case(59_999, "0:00:59")]
#[case(61_000, "0:01:01")]
#[case(3_661_000, "1:01:01")]
#[case(36_000_000, "10:00:00")]
fn clock_text_is_h_mm_ss(#[case] ms: u64, #[case] expected: &str) {
    assert_eq!(clock_text(Duration::from_millis(ms)), expected);
}

#[test]
fn book_moves_have_no_score_clock_or_time_comment_only_the_last_book_note() {
    let now = Instant::now();
    let pos = Position::new();
    let e4 = pos.parse_uci("e2e4").unwrap();
    let e5 = pos.after(e4).unwrap().parse_uci("e7e5").unwrap();
    let mut b = Book::new();
    b.set_position_moves(&pos, &[(e4, 10)]).unwrap();
    b.set_position_moves(&pos.after(e4).unwrap(), &[(e5, 10)])
        .unwrap();
    let mut s = game_setup(
        TimeControl::Clock {
            base_ms: 60_000,
            increment_ms: 1_000,
        },
        Opening::initial(),
    );
    s.book = Some(PlayBook::new(b, BookSelection::Weighted));
    let mut game = TournamentGame::new(s, now);
    let Step::Think(_) = game.advance(now) else {
        panic!("think")
    };
    let config = config_with(ScoreComment::Eval, true, true);
    let pairing = build_schedule(2, Scheduling::RoundRobin, 1, 0).pairings[0];
    let built = build_game(&RecordInput {
        config: &config,
        pairing: &pairing,
        white: &player("A"),
        black: &player("B"),
        opening: &Opening::initial(),
        game: &game,
        end: &GameEnd {
            result: GameResult::Unknown,
            termination: Termination::Unterminated,
            comment: None,
        },
        date: today(),
        redo: 0,
    })
    .unwrap();
    let cm = comments_of(&built);
    assert_eq!(cm[0], None);
    assert!(cm[1].as_deref().unwrap().starts_with("Last book move"));
    assert!(!cm[1].as_deref().unwrap().contains("[%"));
}

// ---- openings: suites through whole tournaments ----

fn write_epd(dir: &std::path::Path, lines: &[&str]) -> std::path::PathBuf {
    let path = dir.join("suite.epd");
    std::fs::write(&path, lines.join("\n")).unwrap();
    path
}

fn first_legal_policy() -> Policy {
    Box::new(|job, _| {
        let position = position_of(job);
        SearchOutcome::Moved(Reply {
            mv: first_legal(&position),
            spent: Duration::from_millis(1),
            score: Some(Score::Cp(0)),
            depth: Some(1),
            ponder: None,
        })
    })
}

fn suite_config(
    path: &std::path::Path,
    n: usize,
    cycles: u32,
    order: SuiteOrder,
) -> TournamentConfig {
    let mut c = cfg(n);
    c.cycles = cycles;
    c.opening = OpeningSource::Suite;
    c.suite = Some(SuiteConfig {
        path: path.to_path_buf(),
        order,
        plies: 4,
    });
    c.adjudication = Adjudication {
        max_plies: Some(3),
        ..Adjudication::default()
    };
    c.seed = 99;
    c
}

const ROOKS: [&str; 3] = [
    "4k3/8/8/8/8/8/8/R3K3 w - - 0 1 id \"Rook a1\";",
    "4k3/8/8/8/8/8/8/1R2K3 w - - 0 1 id \"Rook b1\";",
    "4k3/8/8/8/8/8/8/2R1K3 w - - 0 1 id \"Rook c1\";",
];

fn fen_of(game: &Game) -> String {
    game.start_position().fen()
}

#[test]
fn the_two_games_of_a_pair_share_the_suite_opening_with_the_colours_swapped() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_epd(dir.path(), &ROOKS);
    let config = suite_config(&path, 3, 2, SuiteOrder::Sequential);
    let suite = load_suite(&path, 4).unwrap();
    assert_eq!(suite.len(), 3);
    let mut sim = Sim::with(
        mychess_app::tournament::Tournament::new(config, "t", today(), None, Some(suite)).unwrap(),
        first_legal_policy(),
    );
    sim.start();
    assert_eq!(sim.games.len(), 6);
    let g = |i: usize| &sim.games[i].1;
    for pair in 0..3 {
        let (a, b) = (g(pair * 2), g(pair * 2 + 1));
        assert_eq!(fen_of(a), fen_of(b), "pair {pair} shares the opening");
        assert_eq!(a.tags().white, b.tags().black);
        assert_eq!(a.tags().black, b.tags().white);
        assert_eq!(a.tags().get_extra("Opening"), b.tags().get_extra("Opening"));
        assert!(to_pgn(a).contains("[SetUp \"1\"]"));
    }
    let names: Vec<_> = (0..3)
        .map(|p| g(p * 2).tags().get_extra("Opening").unwrap().to_owned())
        .collect();
    assert_eq!(names, ["Rook a1", "Rook b1", "Rook c1"], "sequential order");
}

#[test]
fn the_suite_wraps_with_one_log_entry_per_game_even_when_the_game_is_retried() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_epd(dir.path(), &ROOKS[..1]);
    let mut config = suite_config(&path, 2, 4, SuiteOrder::Sequential);
    config.failures.max_retries = 1;
    let suite = load_suite(&path, 4).unwrap();
    let died = std::rc::Rc::new(std::cell::Cell::new(false));
    let d = died.clone();
    let mut inner = first_legal_policy();
    let mut sim = Sim::with(
        mychess_app::tournament::Tournament::new(config, "t", today(), None, Some(suite)).unwrap(),
        Box::new(move |job, ctx| {
            if ctx.number == 3 && !d.get() {
                d.set(true);
                return dead();
            }
            inner(job, ctx)
        }),
    );
    sim.start();
    let wraps: Vec<u32> = sim
        .t
        .log()
        .iter()
        .filter_map(|e| match e {
            LogEvent::SuiteWrapped { number } => Some(*number),
            _ => None,
        })
        .collect();
    // One entry per game number (D1316): game 4 shares the group of game 3.
    assert_eq!(wraps, [3, 4], "not repeated for the retry of game 3");
    // Redoing game 3 does not repeat it either.
    let actions = sim.t.redo(3);
    sim.run(actions);
    let actions = sim.t.resume_play(sim.now);
    sim.run(actions);
    let again = sim
        .t
        .log()
        .iter()
        .filter(|e| matches!(e, LogEvent::SuiteWrapped { .. }))
        .count();
    assert_eq!(again, 2, "a redo does not report the wrap again");
}

fn suite_of(n: usize) -> OpeningSuite {
    OpeningSuite::new(
        (0..n)
            .map(|i| Opening {
                name: Some(format!("op{i}")),
                start: Position::from_fen(&format!("8/8/4k3/8/8/3K4/8/8 w - - 0 {}", i + 1))
                    .unwrap(),
                moves: Vec::new(),
            })
            .collect(),
    )
}

fn suite_names(seed: u64, len: usize) -> Vec<String> {
    let mut c = cfg(2);
    c.opening = OpeningSource::Suite;
    c.suite = Some(SuiteConfig {
        path: "x.epd".into(),
        order: SuiteOrder::Random,
        plies: 4,
    });
    c.seed = seed;
    let plan = OpeningPlan::new(&c, Some(suite_of(len)));
    let pairing = build_schedule(2, Scheduling::RoundRobin, 2, 0).pairings[0];
    (0..len as u32)
        .map(|group| {
            let mut p = pairing;
            p.group = group;
            plan.opening_for(&p).opening.name.unwrap()
        })
        .collect()
}

proptest! {
    #[test]
    fn a_random_suite_is_reproducible_and_uses_every_opening_once_per_round(
        seed in 1u64..=i64::MAX as u64, len in 1usize..40
    ) {
        let a = suite_names(seed, len);
        prop_assert_eq!(&a, &suite_names(seed, len));
        let mut sorted = a.clone();
        sorted.sort();
        sorted.dedup();
        prop_assert_eq!(sorted.len(), len);
    }
}

#[test]
fn another_seed_gives_another_order() {
    assert_ne!(suite_names(1, 30), suite_names(2, 30));
}

#[test]
fn book_pairs_repeat_the_book_line_of_the_first_game() {
    let mut b = Book::new();
    let pos = Position::new();
    let firsts: Vec<_> = ["e2e4", "d2d4", "c2c4", "g1f3"]
        .iter()
        .map(|u| (pos.parse_uci(u).unwrap(), 10u16))
        .collect();
    b.set_position_moves(&pos, &firsts).unwrap();
    for (m, _) in &firsts {
        let after = pos.after(*m).unwrap();
        let replies: Vec<_> = after
            .legal_moves()
            .into_iter()
            .take(5)
            .map(|r| (r, 10u16))
            .collect();
        b.set_position_moves(&after, &replies).unwrap();
    }
    let play_book = PlayBook::new(b, BookSelection::Weighted);
    let mut config = cfg(3);
    config.cycles = 2;
    config.opening = OpeningSource::Book;
    config.book = Some(BookConfig {
        path: "book.bin".into(),
        pick: BookPick::Weighted,
    });
    config.seed = 31;
    config.adjudication = Adjudication {
        max_plies: Some(6),
        ..Adjudication::default()
    };
    let t = mychess_app::tournament::Tournament::new(config, "t", today(), Some(play_book), None)
        .unwrap();
    let mut sim = Sim::with(t, first_legal_policy());
    sim.start();
    assert_eq!(sim.games.len(), 6);
    let line = |i: usize| -> Vec<String> {
        let game = &sim.games[i].1;
        let mut pos = game.start_position().clone();
        game.mainline()
            .filter(|id| *id != game.root())
            .take(2)
            .map(|id| {
                let m = game.mv(id).unwrap();
                let u = pos.uci(m);
                pos.play(m).unwrap();
                u
            })
            .collect()
    };
    for pair in 0..3 {
        assert_eq!(line(pair * 2), line(pair * 2 + 1), "pair {pair}");
    }
    let text = to_pgn(&sim.games[0].1);
    assert!(text.contains("Last book move"), "{text}");
}

#[test]
fn current_position_first_game_applies_only_to_game_one() {
    let mut config = cfg(2);
    config.cycles = 2;
    config.opening = OpeningSource::CurrentPositionFirstGame;
    config.start_fen = Some("4k3/8/8/8/8/8/8/R3K3 w - - 0 1".into());
    config.adjudication = Adjudication {
        max_plies: Some(3),
        ..Adjudication::default()
    };
    let mut sim = Sim::new(config, first_legal_policy());
    sim.start();
    assert!(to_pgn(&sim.games[0].1).contains("[FEN "));
    assert!(!to_pgn(&sim.games[1].1).contains("[FEN "));
}

// ---- suite files ----

#[test]
fn suite_files_that_are_missing_empty_or_garbage_are_errors_not_panics() {
    let dir = tempfile::tempdir().unwrap();
    assert!(load_suite(&dir.path().join("nope.epd"), 4).is_err());
    let garbage = dir.path().join("garbage.epd");
    std::fs::write(&garbage, [0xff_u8, 0xfe, 0x00, 0x80, b'\n', 0x00, 0x01]).unwrap();
    assert!(load_suite(&garbage, 4).is_err());
    let text = dir.path().join("text.pgn");
    std::fs::write(&text, "this is not a pgn\n").unwrap();
    assert!(load_suite(&text, 4).is_err());
    let empty = dir.path().join("empty.epd");
    std::fs::write(&empty, "").unwrap();
    assert!(load_suite(&empty, 4).is_err());
}

#[test]
fn a_pgn_suite_takes_plies_from_the_main_line_and_keeps_a_fen_start() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("s.pgn");
    std::fs::write(
        &path,
        "[Event \"a\"]\n[SetUp \"1\"]\n[FEN \"4k3/8/8/8/8/8/8/R3K3 w - - 0 1\"]\n\n1. Ra2 (1. Rb1) Ke7 2. Ra3 *\n\n[Event \"b\"]\n\n1. e4 e5 2. Nf3 *\n",
    )
    .unwrap();
    let suite = load_suite(&path, 2).unwrap();
    assert_eq!(suite.len(), 2);
    let first = suite.get(0).unwrap();
    assert_eq!(first.moves.len(), 2);
    assert_eq!(first.start.fen(), "4k3/8/8/8/8/8/8/R3K3 w - - 0 1");
    assert_eq!(suite.get(1).unwrap().moves.len(), 2, "plies caps the line");
    let zero = load_suite(&path, 0).unwrap();
    assert!(zero.get(0).unwrap().moves.is_empty());
}

#[test]
fn an_epd_suite_with_a_bad_line_does_not_panic() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_epd(dir.path(), &[ROOKS[0], "not a position at all;", ROOKS[1]]);
    // Either the good lines load or the file is refused: never a panic.
    let _result = load_suite(&path, 4);
}

#[test]
fn participants_construct_with_the_engine_identity_and_no_options() {
    let p = Participant::new(5, "X");
    assert_eq!((p.engine, p.name.as_str(), p.options.len()), (5, "X", 0));
}
