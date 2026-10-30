//! PGN -> SI4 -> PGN round trips and `Game` -> record -> `Game` properties
//! (spec M3-2a: "Compat nos dois sentidos", round-trip de PGN).
#![allow(
    clippy::unwrap_used,
    clippy::too_many_lines,
    clippy::cast_possible_truncation
)]

#[path = "common/si4_write_support.rs"]
mod support;

use mychess_core::{Game, GameResult, Nag, NodeId, Position};
use mychess_db::si4::{GameFlags, PreparedGame, Si4Database, Si4Writer};
use mychess_pgn::read::{ReadOptions, parse_pgn_text};
use mychess_pgn::write::{WriteOptions, game_to_string};
use pretty_assertions::assert_eq;
use proptest::prelude::*;
use support::{read, tmp};

const PGN: &str = r#"[Event "Casual"]
[Site "Lisbon"]
[Date "2020.05.06"]
[EventDate "2020.05.01"]
[Round "3"]
[White "Smith, John"]
[Black "Doe, Jane"]
[Result "1-0"]
[WhiteElo "2650"]
[BlackElo "2410"]
[ECO "C84"]
[Annotator "tester"]

1. e4 e5 2. Nf3 {develop} Nc6 3. Bb5 a6 $1 (3... Nf6 4. O-O (4. d3) Nxe4)
4. Ba4 Nf6 5. O-O Be7 6. Re1 b5 7. Bb3 d6 8. c3 O-O 9. h3 Nb8 10. d4 Nbd7 1-0

[Event "Promotion"]
[Site "?"]
[Date "2021.??.??"]
[Round "?"]
[White "A"]
[Black "B"]
[Result "*"]
[SetUp "1"]
[FEN "8/P6k/8/8/8/8/8/K7 w - - 0 1"]

1. a8=N+ Kg6 2. Nb6 *

[Event "En passant"]
[Site "?"]
[Date "????.??.??"]
[Round "-"]
[White "Müller, Jörg"]
[Black "Åsa"]
[Result "1/2-1/2"]

1. e4 a6 2. e5 d5 3. exd6 {é ç ß} Nf6 4. O-O-O? 1/2-1/2

[Event "Empty"]
[Site "?"]
[Date "????.??.??"]
[Round "?"]
[White "?"]
[Black "?"]
[Result "*"]

*
"#;

fn pgn_of(game: &Game) -> String {
    game_to_string(game, &WriteOptions::default()).unwrap()
}

#[test]
fn pgn_to_database_to_pgn_is_stable() {
    let games: Vec<Game> = parse_pgn_text(PGN, &ReadOptions::default())
        .into_iter()
        .map(|p| p.game)
        .collect();
    assert!(games.len() >= 3, "sample parsed: {}", games.len());

    let (_dir, base) = tmp();
    let mut w = Si4Writer::create(&base, 3).unwrap();
    let prepared: Vec<_> = games
        .iter()
        .map(|g| PreparedGame::from_game(g, GameFlags::empty()).unwrap())
        .collect();
    w.add_prepared_batch(&prepared).unwrap();
    drop(w);

    let mut db = Si4Database::open(&base).unwrap();
    assert_eq!(db.len() as usize, games.len());
    for (i, g) in games.iter().enumerate() {
        let back = db.game(u32::try_from(i).unwrap() + 1).unwrap();
        assert_eq!(pgn_of(&back), pgn_of(g), "game {}", i + 1);
        assert!(g.same_content(&back), "game {} content", i + 1);
    }
}

#[test]
fn index_entry_matches_the_tags_of_the_pgn_game() {
    let games: Vec<Game> = parse_pgn_text(PGN, &ReadOptions::default())
        .into_iter()
        .map(|p| p.game)
        .collect();
    let (_dir, base) = tmp();
    let mut w = Si4Writer::create(&base, 1).unwrap();
    w.add_game(&games[0], GameFlags::empty()).unwrap();
    drop(w);
    let db = Si4Database::open(&base).unwrap();
    let e = db.entry(1).unwrap();
    assert_eq!(e.result, GameResult::White);
    assert_eq!(e.white_elo, 2650);
    assert_eq!(e.black_elo, 2410);
    assert_eq!(e.date, games[0].tags().date);
    assert_eq!(e.event_date, games[0].tags().event_date);
    assert_eq!(e.eco(), games[0].tags().eco);
    assert_eq!(e.num_half_moves, 20);
    assert_eq!(
        db.names()
            .get(mychess_db::si4::NameKind::Player, e.white_id)
            .unwrap(),
        b"Smith, John"
    );
    assert_eq!(
        db.names()
            .get(mychess_db::si4::NameKind::Site, e.site_id)
            .unwrap(),
        b"Lisbon"
    );
}

#[test]
fn writing_the_same_games_twice_gives_identical_files() {
    let games: Vec<Game> = parse_pgn_text(PGN, &ReadOptions::default())
        .into_iter()
        .map(|p| p.game)
        .collect();
    let make = || {
        let (dir, base) = tmp();
        let mut w = Si4Writer::create(&base, 1).unwrap();
        for g in &games {
            w.add_game(g, GameFlags::empty()).unwrap();
        }
        drop(w);
        (dir, base)
    };
    let (_d1, a) = make();
    let (_d2, b) = make();
    for e in ["si4", "sn4", "sg4"] {
        assert_eq!(read(&a, e), read(&b, e), ".{e} is deterministic");
    }
}

#[test]
fn rewriting_what_was_read_reproduces_the_record_bytes() {
    // read (our reader) -> Game -> write (our writer) -> same record bytes
    let games: Vec<Game> = parse_pgn_text(PGN, &ReadOptions::default())
        .into_iter()
        .map(|p| p.game)
        .collect();
    let (_dir, base) = tmp();
    let mut w = Si4Writer::create(&base, 1).unwrap();
    for g in &games {
        w.add_game(g, GameFlags::empty()).unwrap();
    }
    drop(w);
    let mut db = Si4Database::open(&base).unwrap();
    for n in 1..=db.len() {
        let original = db.record_bytes(n).unwrap().to_vec();
        let game = db.game(n).unwrap();
        let again = PreparedGame::from_game(&game, GameFlags::empty()).unwrap();
        assert_eq!(again.record(), original.as_slice(), "game {n}");
    }
}

// ---------------------------------------------------------------- proptest

type Op = (u8, u8, u8, u8);

fn build(ops: &[Op], fen: bool, tag_seed: u8) -> Game {
    let mut game = if fen {
        Game::from_position(
            Position::from_fen("r3k2r/pppq1ppp/2n5/8/8/2N5/PPPQ1PPP/R3K2R w KQkq - 0 1").unwrap(),
        )
    } else {
        Game::new()
    };
    let mut nodes: Vec<NodeId> = vec![game.root()];
    for &(p, m, kind, val) in ops {
        let parent = nodes[usize::from(p) % nodes.len()];
        let legal = game.position_at(parent).unwrap().legal_moves();
        if legal.is_empty() {
            continue;
        }
        let mv = legal[usize::from(m) % legal.len()];
        let id = game.add_move(parent, mv).unwrap();
        if !nodes.contains(&id) {
            nodes.push(id);
        }
        if kind & 1 == 1 {
            game.set_comment(id, &format!("note {val} \u{e9}")).unwrap();
        }
        if kind & 2 == 2
            && game.nags(id).unwrap().len() < 5
            && let Ok(nag) = Nag::new(u16::from(val % 139) + 1)
        {
            game.add_nag(id, nag).unwrap();
        }
    }
    let names = ["Anand, V", "Carlsen", "Åsa", "", "Zed", "?"];
    let t = game.tags_mut();
    t.white = names[usize::from(tag_seed) % names.len()].into();
    t.black = names[usize::from(tag_seed / 7) % names.len()].into();
    t.event = format!("Ev{}", tag_seed % 5);
    t.result = [
        GameResult::Unknown,
        GameResult::White,
        GameResult::Black,
        GameResult::Draw,
    ][usize::from(tag_seed) % 4];
    if tag_seed.is_multiple_of(3) {
        game.set_initial_comment("opening remark");
    }
    game
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 48, ..ProptestConfig::default() })]

    #[test]
    fn game_to_record_to_game_is_the_identity(
        ops in prop::collection::vec(any::<Op>(), 0..40),
        fen in any::<bool>(),
        tag_seed in any::<u8>(),
    ) {
        let game = build(&ops, fen, tag_seed);
        let (_dir, base) = tmp();
        let mut w = Si4Writer::create(&base, 1).unwrap();
        let n = w.add_game(&game, GameFlags::empty()).unwrap();
        drop(w);
        let mut db = Si4Database::open(&base).unwrap();
        let back = db.game(n).unwrap();
        prop_assert!(game.same_content(&back), "{}", pgn_of(&game));
        // and the record is canonical: encoding the decoded game gives the same bytes
        let first = PreparedGame::from_game(&game, GameFlags::empty()).unwrap();
        let second = PreparedGame::from_game(&back, GameFlags::empty()).unwrap();
        prop_assert_eq!(first.record(), second.record());
        prop_assert_eq!(first.entry(), second.entry());
    }

    #[test]
    fn replace_then_read_returns_the_new_game(
        a in prop::collection::vec(any::<Op>(), 0..20),
        b in prop::collection::vec(any::<Op>(), 0..20),
        sa in any::<u8>(),
        sb in any::<u8>(),
    ) {
        let (g1, g2) = (build(&a, false, sa), build(&b, false, sb));
        let (_dir, base) = tmp();
        let mut w = Si4Writer::create(&base, 1).unwrap();
        w.add_game(&g1, GameFlags::empty()).unwrap();
        w.add_game(&g1, GameFlags::empty()).unwrap();
        w.replace_game(1, &g2).unwrap();
        drop(w);
        let mut db = Si4Database::open(&base).unwrap();
        prop_assert!(g2.same_content(&db.game(1).unwrap()));
        prop_assert!(g1.same_content(&db.game(2).unwrap()));
    }
}
