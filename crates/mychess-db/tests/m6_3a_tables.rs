//! Contract tests of the player table, the tournament finder and the player
//! information (M6-3a, criteria 15 to 22).
#![allow(clippy::unwrap_used, clippy::too_many_lines)]

#[path = "common/m6_3a_support.rs"]
mod support;

use mychess_core::GameResult;
use mychess_db::finder::{
    CountryFilter, PlayerQuery, PlayerSort, PlayerTable, TournamentQuery, TournamentSort,
    find_tournaments,
};
use mychess_db::player_info::{
    GameSelection, PlayerFilterMode, player_games_filter, player_info, spelling_header,
};
use mychess_db::si4::NameKind;
use mychess_db::{CancelToken, Filter, NoProgress, PlayerStats};
use pretty_assertions::assert_eq;
use support::{build, date, g, open, read, six_games};

const SPELLING: &str = "@PLAYER\n\
Alfa, A #GM BRA [2600] 1980.01.01\n\
Beta, B #IM BRA [2500] 1980.01.01\n\
Gama, G #- ARG [2050] 1980.01.01\n\
Delta, D #- ARG [2100] 1980.01.01\n\
Zeta #- BRA [2700]\n";

fn six() -> (
    tempfile::TempDir,
    mychess_db::si4::Si4Database<std::fs::File>,
) {
    let dir = tempfile::tempdir().unwrap();
    let base = build(dir.path(), &six_games());
    let db = open(&base);
    (dir, db)
}

fn names(rows: &[mychess_db::finder::PlayerFound]) -> Vec<&str> {
    rows.iter().map(|r| &*r.name).collect()
}

fn table(db: &mychess_db::si4::Si4Database<std::fs::File>) -> PlayerTable {
    PlayerTable::build(db, None, &CancelToken::new(), &mut NoProgress).unwrap()
}

#[test]
fn player_table_statistics() {
    let (_dir, db) = six();
    let t = table(&db);
    assert_eq!(t.len(), 4);
    let row = |name: &str| t.rows().iter().find(|r| &*r.name == name).unwrap().clone();
    let alfa = row("Alfa, A");
    assert_eq!(alfa.games, 5);
    assert_eq!(alfa.first.to_string(), "2020.01.10");
    assert_eq!(alfa.last.to_string(), "2020.05.20");
    assert_eq!(alfa.max_elo, 2520);
    assert_eq!((row("Beta, B").games, row("Beta, B").max_elo), (3, 2410));
    assert_eq!((row("Gama, G").games, row("Gama, G").max_elo), (2, 0));
    let delta = row("Delta, D");
    assert_eq!((delta.games, delta.max_elo), (2, 0));
    assert!(delta.first.is_unknown() && delta.last.is_unknown());
    assert!(!t.has_estimates());
}

#[test]
fn spelling_estimates_the_missing_elo() {
    let dir = tempfile::tempdir().unwrap();
    let mut games = six_games();
    games.push(g("Zeta", "Eta, E", GameResult::Draw));
    let base = build(dir.path(), &games);
    let db = open(&base);
    let set = read(SPELLING);
    let checker = set.checker(NameKind::Player);
    let t = PlayerTable::build(&db, Some(checker), &CancelToken::new(), &mut NoProgress).unwrap();
    let elo = |name: &str| t.rows().iter().find(|r| &*r.name == name).unwrap().max_elo;
    assert_eq!(elo("Gama, G"), 2050);
    assert_eq!(elo("Delta, D"), 2100);
    assert_eq!(elo("Alfa, A"), 2520, "the games win over the file");
    assert_eq!(elo("Zeta"), 0, "a surname alone is not estimated");
    assert!(t.has_estimates());
    // The same through PlayerStats.
    let mut stats = PlayerStats::from_database(&db);
    stats.apply_spelling_estimates(&db, checker);
    let id = |name: &str| t.rows().iter().find(|r| &*r.name == name).unwrap().id;
    assert_eq!(stats.max_elo(id("Gama, G")), 2050);
    assert_eq!(stats.max_elo(id("Zeta")), 0);
    assert_eq!(stats.max_elo(id("Alfa, A")), 2520);
}

#[test]
fn player_queries() {
    let (_dir, db) = six();
    let t = table(&db);
    let q = |f: &dyn Fn(&mut PlayerQuery)| {
        let mut q = PlayerQuery::default();
        f(&mut q);
        t.query(&q, None)
    };
    let all = ["Alfa, A", "Beta, B", "Delta, D", "Gama, G"];
    assert_eq!(names(&q(&|q| q.sort = PlayerSort::Name)), all);
    assert_eq!(names(&q(&|q| q.sort = PlayerSort::Games)), all);
    assert_eq!(names(&q(&|q| q.sort = PlayerSort::Elo)), all);
    assert_eq!(
        names(&q(&|q| q.sort = PlayerSort::Oldest)),
        ["Alfa, A", "Beta, B", "Gama, G", "Delta, D"]
    );
    assert_eq!(
        names(&q(&|q| q.sort = PlayerSort::Newest)),
        ["Alfa, A", "Beta, B", "Gama, G", "Delta, D"]
    );
    assert_eq!(names(&q(&|q| q.prefix = "be".into())), ["Beta, B"]);
    assert_eq!(names(&q(&|q| q.prefix = "BE".into())), ["Beta, B"]);
    assert_eq!(
        names(&q(&|q| q.prefix = "Beta, B ".into())).len(),
        0,
        "spaces count"
    );
    assert_eq!(names(&q(&|q| q.limit = 2)), ["Alfa, A", "Beta, B"]);
    assert_eq!(q(&|q| q.limit = 0).len(), 4);
    assert_eq!(names(&q(&|q| q.games = 3..=100)), ["Alfa, A", "Beta, B"]);
    assert_eq!(names(&q(&|q| q.elo = 2450..=4000)), ["Alfa, A"]);
    // Direction: the second click reverses the column.
    assert_eq!(
        names(&q(&|q| {
            q.sort = PlayerSort::Games;
            q.descending = Some(false);
        })),
        ["Delta, D", "Gama, G", "Beta, B", "Alfa, A"]
    );
    assert_eq!(
        names(&q(&|q| {
            q.sort = PlayerSort::Name;
            q.descending = Some(true);
        })),
        ["Gama, G", "Delta, D", "Beta, B", "Alfa, A"]
    );
    // No country without the column; "~~~" without a spelling file.
    assert!(q(&|_| {}).iter().all(|r| r.country.is_none()));
    let rows = q(&|q| q.country = CountryFilter::All);
    assert!(rows.iter().all(|r| r.country.as_deref() == Some("~~~")));
    assert_eq!(
        q(&|q| q.country = CountryFilter::Prefix("BRA".into())).len(),
        0
    );
    assert_eq!(
        q(&|q| q.country = CountryFilter::Prefix(String::new())).len(),
        4
    );
}

#[test]
fn player_queries_by_country_with_spelling() {
    let (_dir, db) = six();
    let set = read(SPELLING);
    let checker = set.checker(NameKind::Player);
    let t = PlayerTable::build(&db, Some(checker), &CancelToken::new(), &mut NoProgress).unwrap();
    let q = PlayerQuery {
        country: CountryFilter::Prefix("bra".into()),
        ..PlayerQuery::default()
    };
    let rows = t.query(&q, Some(checker));
    assert_eq!(names(&rows), ["Alfa, A", "Beta, B"]);
    assert_eq!(rows[0].country.as_deref(), Some("BRA"));
    let q = PlayerQuery {
        country: CountryFilter::All,
        sort: PlayerSort::Country,
        ..PlayerQuery::default()
    };
    let rows = t.query(&q, Some(checker));
    let countries: Vec<_> = rows.iter().map(|r| r.country.clone().unwrap()).collect();
    assert_eq!(countries, ["ARG", "ARG", "BRA", "BRA"]);
    // Ties fall to the name.
    assert_eq!(names(&rows), ["Delta, D", "Gama, G", "Alfa, A", "Beta, B"]);
}

#[test]
fn country_cache_follows_the_checker_that_is_asked_for() {
    // Regression (D3360): the first query used to fix the countries.
    let (_dir, db) = six();
    let other = "@PLAYER\nAlfa, A #GM PER [2600] 1980.01.01\n";
    let set = read(SPELLING);
    let set2 = read(other);
    let (c1, c2) = (
        set.checker(NameKind::Player),
        set2.checker(NameKind::Player),
    );
    assert_ne!(c1.fingerprint(), c2.fingerprint());
    assert_eq!(c1.fingerprint(), c1.clone().fingerprint());
    let t = table(&db);
    let q = PlayerQuery {
        country: CountryFilter::All,
        ..PlayerQuery::default()
    };
    let country = |rows: &[mychess_db::finder::PlayerFound], name: &str| {
        rows.iter()
            .find(|r| &*r.name == name)
            .and_then(|r| r.country.clone())
    };
    let none = t.query(&q, None);
    assert!(none.iter().all(|r| r.country.as_deref() == Some("~~~")));
    let first = t.query(&q, Some(c1));
    assert_eq!(country(&first, "Alfa, A").as_deref(), Some("BRA"));
    let second = t.query(&q, Some(c2));
    assert_eq!(country(&second, "Alfa, A").as_deref(), Some("PER"));
    assert_eq!(country(&second, "Beta, B").as_deref(), Some("~~~"));
    // Back to the first checker, and to none.
    assert_eq!(t.query(&q, Some(c1)), first);
    assert_eq!(t.query(&q, None), none);
}

#[test]
fn player_table_can_be_cancelled() {
    let (_dir, db) = six();
    let token = CancelToken::new();
    token.cancel();
    let mut ticks = 0;
    let result = PlayerTable::build(&db, None, &token, &mut |_, _| ticks += 1);
    // With six games the ticker may not look at the token; both ways are valid,
    // but a cancelled build never returns a half table.
    if let Ok(t) = result {
        assert_eq!(t.len(), 4);
    }
}

fn tq() -> TournamentQuery {
    TournamentQuery {
        start: date("1960.??.??"),
        end: date("2026.12.31"),
        games: 1..=9999,
        players: 1..=999,
        ..TournamentQuery::default()
    }
}

fn find(
    db: &mychess_db::si4::Si4Database<std::fs::File>,
    q: &TournamentQuery,
) -> Vec<mychess_db::finder::TournamentRow> {
    find_tournaments(db, q, &CancelToken::new(), &mut NoProgress).unwrap()
}

#[test]
fn tournaments_are_grouped_by_window() {
    let (_dir, db) = six();
    let rows = find(&db, &tq());
    assert_eq!(rows.len(), 2, "the undated Solo games are out");
    let a = rows.iter().find(|r| r.games == 3).unwrap();
    assert_eq!(
        (a.players, a.mean_elo, a.first_game, a.date.to_string()),
        (3, 2453, 1, "2020.01.10".to_owned())
    );
    assert_eq!(&*a.game_numbers, [1, 2, 3]);
    assert_eq!((&*a.event, &*a.site), ("Aberto", "Rio BRA"));
    let b = rows.iter().find(|r| r.games == 1).unwrap();
    assert_eq!(
        (b.players, b.mean_elo, b.date.to_string()),
        (2, 2465, "2020.05.20".to_owned())
    );
    // Without a start date the two undated games are two tournaments of one game.
    let mut open_q = tq();
    open_q.start = mychess_core::PartialDate::UNKNOWN;
    assert_eq!(find(&db, &open_q).len(), 4);
}

#[test]
fn tournament_winner_and_runner_up() {
    let (_dir, db) = six();
    let rows = find(&db, &tq());
    let a = rows.iter().find(|r| r.games == 3).unwrap();
    let w = a.winner.as_ref().unwrap();
    assert_eq!(
        (&*w.name, w.elo, w.points2, w.points_text()),
        ("Alfa, A", 2510, 4, "2.0".to_owned())
    );
    let r = a.runner_up.as_ref().unwrap();
    assert_eq!(
        (&*r.name, r.elo, r.points_text()),
        ("Beta, B", 2400, "0.5".to_owned())
    );
    let b = rows.iter().find(|r| r.games == 1).unwrap();
    assert_eq!(
        (
            b.winner.as_ref().unwrap().elo,
            b.runner_up.as_ref().unwrap().elo
        ),
        (2520, 2410)
    );
}

#[test]
fn tournament_orders_and_filters() {
    let (_dir, db) = six();
    let order = |sort: TournamentSort| -> Vec<u32> {
        let mut q = tq();
        q.sort = sort;
        find(&db, &q).iter().map(|r| r.games).collect()
    };
    assert_eq!(order(TournamentSort::Date), [1, 3]);
    assert_eq!(order(TournamentSort::Games), [3, 1]);
    assert_eq!(order(TournamentSort::Players), [3, 1]);
    assert_eq!(order(TournamentSort::Elo), [1, 3]);
    assert_eq!(
        order(TournamentSort::Event),
        [3, 1],
        "ties by the first game"
    );
    assert_eq!(order(TournamentSort::Site), [3, 1]);
    assert_eq!(order(TournamentSort::Winner), [3, 1]);
    let with = |f: &dyn Fn(&mut TournamentQuery)| {
        let mut q = tq();
        f(&mut q);
        find(&db, &q).iter().map(|r| r.games).collect::<Vec<_>>()
    };
    assert_eq!(with(&|q| q.player = "gam".into()), [3]);
    assert_eq!(with(&|q| q.site = "rio".into()), [1, 3]);
    assert_eq!(with(&|q| q.event = "ABER".into()), [1, 3]);
    assert_eq!(with(&|q| q.country = Some(*b"BRA")), [1, 3]);
    assert_eq!(with(&|q| q.country = Some(*b"PER")).len(), 0);
    assert_eq!(with(&|q| q.limit = 1), [1]);
    assert_eq!(with(&|q| q.players = 3..=999), [3]);
    assert_eq!(with(&|q| q.mean_elo = 2460..=4000), [1]);
    assert_eq!(with(&|q| q.limit = 0).len(), 2);
    // Only the Peruvian site without a start date.
    let mut q = tq();
    q.start = mychess_core::PartialDate::UNKNOWN;
    q.country = Some(*b"PER");
    assert_eq!(find(&db, &q).len(), 2);
}

#[test]
fn tournaments_can_be_cancelled() {
    let (_dir, db) = six();
    let token = CancelToken::new();
    token.cancel();
    let result = find_tournaments(&db, &tq(), &token, &mut NoProgress);
    if let Ok(rows) = result {
        assert_eq!(rows.len(), 2);
    }
}

fn filter_of(db: &mychess_db::si4::Si4Database<std::fs::File>, games: &[u32]) -> Filter {
    let mut f = Filter::new(mychess_db::GameSource::len(db));
    f.fill(0);
    for &n in games {
        f.set(n, 1);
    }
    f
}

#[test]
fn player_information_for_alfa() {
    let dir = tempfile::tempdir().unwrap();
    let mut games = six_games();
    games[0] = games[0].clone().eco("B90");
    games[3] = games[3].clone().eco("B97");
    let base = build(dir.path(), &games);
    let db = open(&base);
    let filter = filter_of(&db, &[1, 2, 3, 4]);
    let info = player_info(
        &db,
        "Alfa, A",
        Some(&filter),
        Some("Beta, B"),
        &CancelToken::new(),
        &mut NoProgress,
    )
    .unwrap();
    assert!(info.found);
    let all = info.all;
    assert_eq!(
        (
            all.total.wins,
            all.total.draws,
            all.total.losses,
            all.total.unfinished
        ),
        (3, 1, 0, 1)
    );
    assert_eq!(all.total.points2(), 8);
    assert_eq!(all.total.percent_hundredths(), 8000);
    assert_eq!(all.total.points_text(), "4.0");
    assert_eq!(
        (all.white.games(), all.white.wins, all.white.draws),
        (3, 2, 1)
    );
    assert_eq!(
        (all.black.games(), all.black.wins, all.black.unfinished),
        (2, 1, 1)
    );
    let f = info.filter.unwrap();
    assert_eq!((f.total.wins, f.total.games()), (3, 3));
    assert_eq!(f.total.percent_hundredths(), 10_000);
    assert_eq!(f.total.points_text(), "3.0");
    assert!(!info.filter_is_everything);
    let (opponent, versus) = info.versus.unwrap();
    assert_eq!(opponent, "Beta, B");
    assert_eq!(
        (
            versus.total.games(),
            versus.white.wins,
            versus.total.percent_hundredths()
        ),
        (2, 2, 10_000)
    );
    assert_eq!(info.first.to_string(), "2020.01.10");
    assert_eq!(info.last.to_string(), "2020.05.20");
    assert_eq!(info.openings[1].len(), 0);
    assert_eq!(info.openings[0].len(), 1);
    let share = info.openings[0][0];
    assert_eq!(
        (share.label(), share.games, share.score_percent),
        ("B9".to_owned(), 2, 100)
    );
    assert_eq!(info.ratings, [(2020, 1, 2510), (2020, 5, 2520)]);
}

#[test]
fn player_information_edge_cases() {
    let (_dir, db) = six();
    let missing = player_info(
        &db,
        "Nobody",
        None,
        None,
        &CancelToken::new(),
        &mut NoProgress,
    )
    .unwrap();
    assert!(!missing.found);
    assert_eq!(missing.all.total.games(), 0);
    let delta = player_info(
        &db,
        "Delta, D",
        None,
        Some("Nobody"),
        &CancelToken::new(),
        &mut NoProgress,
    )
    .unwrap();
    assert!(delta.found && delta.versus.is_none() && delta.filter.is_none());
    assert!(delta.first.is_unknown());
    assert_eq!(delta.ratings.len(), 0);
    let all = filter_of(&db, &[1, 2, 3, 4, 5, 6]);
    let alfa = player_info(
        &db,
        "Alfa, A",
        Some(&all),
        None,
        &CancelToken::new(),
        &mut NoProgress,
    )
    .unwrap();
    assert!(alfa.filter_is_everything);
}

#[test]
fn filter_links() {
    let (_dir, db) = six();
    let run = |letters: &str,
               mode: PlayerFilterMode,
               start: &[u32],
               opponent: Option<&str>|
     -> Vec<u32> {
        let mut f = filter_of(&db, start);
        let selection = GameSelection::from_letters(letters).unwrap();
        let n = player_games_filter(
            &db,
            "Alfa, A",
            selection,
            mode,
            opponent,
            &mut f,
            &CancelToken::new(),
            &mut NoProgress,
        )
        .unwrap();
        let games: Vec<u32> = f.iter_games().collect();
        assert_eq!(u32::try_from(games.len()).unwrap(), n);
        games
    };
    let all = [1, 2, 3, 4, 5, 6];
    assert_eq!(
        run("aA", PlayerFilterMode::Set, &all, None),
        [1, 3, 4, 5, 6]
    );
    assert_eq!(run("wW", PlayerFilterMode::Set, &all, None), [1, 3, 4]);
    assert_eq!(run("w", PlayerFilterMode::Set, &all, None), [1, 4]);
    assert_eq!(run("W", PlayerFilterMode::Set, &all, None), [3]);
    assert_eq!(run("d", PlayerFilterMode::Set, &all, None), [5]);
    assert_eq!(run("A", PlayerFilterMode::Set, &all, None), [3, 6]);
    assert_eq!(
        run("", PlayerFilterMode::Refine, &[1, 2, 3, 4, 5], None),
        [1, 3, 4, 5]
    );
    assert_eq!(
        run("aA", PlayerFilterMode::Opponent, &all, Some("Beta, B")),
        [1, 4]
    );
    assert_eq!(
        run("aA", PlayerFilterMode::Opponent, &all, Some("Nobody")).len(),
        0
    );
    assert!(GameSelection::from_letters("z").is_none());
}

#[test]
fn spelling_header_from_the_example() {
    let set = support::example(2020);
    let p = set.checker(NameKind::Player);
    let h = spelling_header(p, "Braga, Carlos Eduardo").unwrap();
    assert_eq!(h.title_names, ["International Master"]);
    assert_eq!(h.country_names, ["Portugal", "Brazil"]);
    assert_eq!(h.peak, 2450);
    assert_eq!(h.born, "1975.01.02");
    assert!(h.exact);
    let h = spelling_header(p, "Almeida, Beatriz Costa").unwrap();
    assert_eq!(h.fide_id.as_deref(), Some("9000001"));
    assert_eq!(h.bio.len(), 2);
    let loose = spelling_header(p, "Braga, Carlos").unwrap();
    assert!(
        loose.exact,
        "an alias key equal to the query is an exact match"
    );
    let approximate = spelling_header(p, "Almeida, Bea").unwrap();
    assert!(!approximate.exact);
    assert!(spelling_header(p, "Nobody").is_none());
}
