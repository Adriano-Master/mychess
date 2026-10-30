//! Contract tests of the name checker, the application of name corrections
//! and "Add Elo" (M6-3a, criteria 23 to 32).
#![allow(clippy::unwrap_used, clippy::too_many_lines)]

#[path = "common/m6_3a_support.rs"]
mod support;

use std::path::Path;

use mychess_core::{GameResult, RatingType};
use mychess_db::si4::{FailPoint, NameChange, NameKind, Si4Database, Si4Error, Si4Writer};
use mychess_db::spelling::apply::{changes_from_proposals, proposals_to_text};
use mychess_db::spelling::check::{Proposal, ScanOptions, scan_names};
use mychess_db::spelling::elo::{AddEloError, EloOptions, apply_add_elo, plan_add_elo};
use mychess_db::{CancelToken, Filter, NoProgress};
use pretty_assertions::assert_eq;
use support::{G, build, date, example, g, games_bytes, index_bytes, names_bytes, open, read};

const W: GameResult = GameResult::White;

/// The database of criterion 23.
fn spelling_games() -> Vec<G> {
    let mut games = Vec::new();
    let mut add = |name: &str, times: usize| {
        for _ in 0..times {
            games.push(
                g(name, "Quux, Q", W)
                    .on("Brasileiro Ch", "Sao Paulo BRA", "????.??.??")
                    .round("1"),
            );
        }
    };
    add("Almeida, B", 3);
    add("Almeida, Bea", 1);
    add("GM Braga, Carlos", 2);
    add("Braga, Carlos", 1);
    add("Almeida", 1);
    add("Almeida, Bruno (BRA)", 1);
    games[0] = games[0]
        .clone()
        .on("Brasil-ch 2020", "Sao Paulo (Brazil)", "????.??.??")
        .round("(01)");
    games
}

fn proposals_of(base: &Path, opts: ScanOptions) -> Vec<Proposal> {
    let db = open(base);
    let set = example(2020);
    scan_names(
        &db,
        set.checker(opts.kind),
        opts,
        &CancelToken::new(),
        &mut NoProgress,
    )
    .proposals
}

fn pairs(list: &[Proposal]) -> Vec<(&str, &str, u32, bool)> {
    list.iter()
        .map(|p| (p.old.as_str(), p.new.as_str(), p.games, p.ambiguous))
        .collect()
}

#[test]
fn scan_of_players() {
    let dir = tempfile::tempdir().unwrap();
    let mut games = spelling_games();
    // A name with frequency 0: written, then replaced by another game.
    games.push(g("Ghost, G", "Quux, Q", W));
    let base = build(dir.path(), &games);
    let last = u32::try_from(games.len()).unwrap();
    let mut w = Si4Writer::open(&base).unwrap();
    w.replace_game(last, &mychess_core::Game::new()).unwrap();
    drop(w);

    let found = proposals_of(&base, ScanOptions::new(NameKind::Player));
    assert_eq!(
        pairs(&found),
        [
            ("Almeida, B", "Almeida, Bruno", 3, false),
            ("Almeida, Bea", "Almeida, Beatriz Costa", 1, false),
            ("Almeida, Bruno (BRA)", "Almeida, Bruno", 1, false),
            ("Braga, Carlos", "Braga, Carlos Eduardo", 1, false),
            ("GM Braga, Carlos", "Braga, Carlos", 2, false),
        ]
    );
    let bruno = &found[0];
    assert_eq!(bruno.born.to_string(), "1900.??.??");
    assert_eq!(bruno.died.to_string(), "1970.04.??");
    assert!(
        !found.iter().any(|p| p.old == "Ghost, G"),
        "unused names are skipped"
    );

    let mut with = ScanOptions::new(NameKind::Player);
    with.surnames = true;
    with.ambiguous = true;
    let found = proposals_of(&base, with);
    let almeida: Vec<_> = found.iter().filter(|p| p.old == "Almeida").collect();
    assert_eq!(almeida.len(), 2);
    assert!(almeida.iter().all(|p| p.ambiguous));
    with.ambiguous = false;
    let found = proposals_of(&base, with);
    assert!(!found.iter().any(|p| p.old == "Almeida"));
}

#[test]
fn scan_of_events_sites_and_rounds() {
    let dir = tempfile::tempdir().unwrap();
    let base = build(dir.path(), &spelling_games());
    let one = |kind| pairs_owned(&proposals_of(&base, ScanOptions::new(kind)));
    assert_eq!(
        one(NameKind::Site),
        [("Sao Paulo (Brazil)".to_owned(), "Sao Paulo BRA".to_owned())]
    );
    assert_eq!(
        one(NameKind::Event),
        [("Brasil-ch 2020".to_owned(), "Brasil Ch 2020".to_owned())]
    );
    assert_eq!(one(NameKind::Round), [("(01)".to_owned(), "1".to_owned())]);
}

fn pairs_owned(list: &[Proposal]) -> Vec<(String, String)> {
    list.iter()
        .map(|p| (p.old.clone(), p.new.clone()))
        .collect()
}

#[test]
fn scan_progress_is_monotonic_and_cancel_returns_the_partial() {
    let dir = tempfile::tempdir().unwrap();
    let base = build(dir.path(), &spelling_games());
    let db = open(&base);
    let set = example(2020);
    let checker = set.checker(NameKind::Player);
    let mut seen: Vec<(u64, u64)> = Vec::new();
    let result = scan_names(
        &db,
        checker,
        ScanOptions::new(NameKind::Player),
        &CancelToken::new(),
        &mut |d, t| seen.push((d, t)),
    );
    assert!(!result.interrupted);
    assert!(seen.windows(2).all(|w| w[0].0 <= w[1].0), "{seen:?}");
    let (done, total) = *seen.last().unwrap();
    assert_eq!(done, total);
    let token = CancelToken::new();
    token.cancel();
    let cancelled = scan_names(
        &db,
        checker,
        ScanOptions::new(NameKind::Player),
        &token,
        &mut NoProgress,
    );
    assert!(cancelled.interrupted);
    assert!(cancelled.proposals.len() <= result.proposals.len());
    // A checker of another kind finds nothing.
    let wrong = scan_names(
        &db,
        set.checker(NameKind::Site),
        ScanOptions::new(NameKind::Player),
        &CancelToken::new(),
        &mut NoProgress,
    );
    assert_eq!(wrong.proposals.len(), 0);
}

fn four_changes(base: &Path) -> Vec<NameChange> {
    let all = proposals_of(base, ScanOptions::new(NameKind::Player));
    let four: Vec<Proposal> = all
        .into_iter()
        .filter(|p| p.old != "Almeida, Bruno (BRA)")
        .collect();
    assert_eq!(four.len(), 4);
    changes_from_proposals(&four)
}

fn count_ids(db: &Si4Database<std::fs::File>, kind: NameKind) -> Vec<u32> {
    let mut counts = vec![0; usize::try_from(db.names().num_names(kind)).unwrap()];
    for n in 1..=db.len() {
        let e = db.entry(n).unwrap();
        let ids: &[u32] = match kind {
            NameKind::Player => &[e.white_id, e.black_id],
            NameKind::Event => &[e.event_id],
            NameKind::Site => &[e.site_id],
            NameKind::Round => &[e.round_id],
        };
        for &id in ids {
            counts[usize::try_from(id).unwrap()] += 1;
        }
    }
    counts
}

fn freq(db: &Si4Database<std::fs::File>, kind: NameKind, name: &str) -> Option<u32> {
    (0..db.names().num_names(kind))
        .find(|&id| db.names().get(kind, id) == Some(name.as_bytes()))
        .and_then(|id| db.names().frequency(kind, id))
}

#[test]
fn applying_corrections_recounts_and_keeps_everything_else() {
    let dir = tempfile::tempdir().unwrap();
    let base = build(dir.path(), &spelling_games());
    let changes = four_changes(&base);
    let before_games = games_bytes(&base);
    let before_index = index_bytes(&base);
    let before = open(&base);
    let n = before.len();
    drop(before);

    let mut w = Si4Writer::open(&base).unwrap();
    let report = w
        .apply_name_changes(
            NameKind::Player,
            &changes,
            &CancelToken::new(),
            &mut NoProgress,
        )
        .unwrap();
    drop(w);
    assert_eq!(report.mapped, 4);
    assert_eq!(report.games_changed, 7);
    assert_eq!(
        (
            report.names_not_found,
            report.skipped_full,
            report.skipped_by_date
        ),
        (0, 0, 0)
    );
    assert!(!report.interrupted);

    let db = open(&base);
    assert_eq!(db.len(), n);
    assert_eq!(freq(&db, NameKind::Player, "Almeida, B"), Some(0));
    assert_eq!(freq(&db, NameKind::Player, "Almeida, Bruno"), Some(3));
    assert_eq!(
        freq(&db, NameKind::Player, "Braga, Carlos"),
        Some(2),
        "reused id: 2 from GM, none left of its own"
    );
    assert_eq!(
        freq(&db, NameKind::Player, "Braga, Carlos Eduardo"),
        Some(1)
    );
    // The stored frequencies are the recounted ones.
    let counts = count_ids(&db, NameKind::Player);
    for id in 0..db.names().num_names(NameKind::Player) {
        assert_eq!(
            db.names().frequency(NameKind::Player, id),
            Some(counts[usize::try_from(id).unwrap()]),
            "id {id}"
        );
    }
    // The old names are still in the .sn4.
    assert!(db.names().get(NameKind::Player, 0).is_some());
    // Nothing but the player ids moved.
    assert_eq!(games_bytes(&base), before_games);
    let after_index = index_bytes(&base);
    assert_eq!(after_index.len(), before_index.len());
    let header = 182;
    for (i, (a, b)) in before_index.iter().zip(&after_index).enumerate() {
        if i < header {
            assert_eq!(a, b, "header byte {i}");
            continue;
        }
        let offset = (i - header) % 47;
        if !matches!(offset, 9..=13) {
            assert_eq!(a, b, "byte {i} (entry offset {offset})");
        }
    }
}

#[test]
fn applying_event_site_and_round_corrections() {
    let dir = tempfile::tempdir().unwrap();
    let base = build(dir.path(), &spelling_games());
    for (kind, old, new) in [
        (NameKind::Event, "Brasil-ch 2020", "Brasil Ch 2020"),
        (NameKind::Site, "Sao Paulo (Brazil)", "Sao Paulo BRA"),
        (NameKind::Round, "(01)", "1"),
    ] {
        let mut w = Si4Writer::open(&base).unwrap();
        let r = w
            .apply_name_changes(
                kind,
                &[NameChange::new(old, new)],
                &CancelToken::new(),
                &mut NoProgress,
            )
            .unwrap();
        drop(w);
        assert_eq!((r.mapped, r.games_changed), (1, 1), "{kind}");
        let db = open(&base);
        assert_eq!(freq(&db, kind, old), Some(0));
        assert!(freq(&db, kind, new).unwrap() >= 1);
        let counts = count_ids(&db, kind);
        for id in 0..db.names().num_names(kind) {
            assert_eq!(
                db.names().frequency(kind, id),
                Some(counts[usize::try_from(id).unwrap()])
            );
        }
    }
}

#[test]
fn life_spans_gate_the_player_changes() {
    let dir = tempfile::tempdir().unwrap();
    let dates = [
        "1975.01.01",
        "1970.04.20",
        "1899.12.31",
        "????.??.??",
        "1900.01.01",
        "1970.04.31",
    ];
    let games: Vec<G> = dates
        .iter()
        .map(|d| g("Almeida, B", "Quux, Q", W).on("E", "S", d))
        .collect();
    let base = build(dir.path(), &games);
    let change = NameChange {
        old: "Almeida, B".into(),
        new: "Almeida, Bruno".into(),
        born: date("1900.??.??"),
        died: date("1970.04.??"),
    };
    let mut w = Si4Writer::open(&base).unwrap();
    let r = w
        .apply_name_changes(
            NameKind::Player,
            &[change],
            &CancelToken::new(),
            &mut NoProgress,
        )
        .unwrap();
    drop(w);
    assert_eq!((r.games_changed, r.skipped_by_date), (4, 2));
    let db = open(&base);
    let bruno = (0..db.names().num_names(NameKind::Player))
        .find(|&id| db.names().get(NameKind::Player, id) == Some(b"Almeida, Bruno"))
        .unwrap();
    let changed: Vec<u32> = (1..=db.len())
        .filter(|&n| db.entry(n).unwrap().white_id == bruno)
        .collect();
    assert_eq!(changed, [2, 4, 5, 6]);
}

#[test]
fn death_with_only_a_year_does_not_block_that_year() {
    let dir = tempfile::tempdir().unwrap();
    let games = vec![
        g("Old, O", "Quux, Q", W).on("E", "S", "1979.06.10"),
        g("Old, O", "Quux, Q", W).on("E", "S", "1980.01.01"),
    ];
    let base = build(dir.path(), &games);
    let change = NameChange {
        died: date("1979"),
        ..NameChange::new("Old, O", "Old, Oscar")
    };
    let mut w = Si4Writer::open(&base).unwrap();
    let r = w
        .apply_name_changes(
            NameKind::Player,
            &[change],
            &CancelToken::new(),
            &mut NoProgress,
        )
        .unwrap();
    assert_eq!((r.games_changed, r.skipped_by_date), (1, 1));
}

#[test]
fn nothing_valid_writes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let base = build(dir.path(), &spelling_games());
    let files = |b: &Path| (index_bytes(b), names_bytes(b), games_bytes(b));
    let before = files(&base);
    let mut w = Si4Writer::open(&base).unwrap();
    let r = w
        .apply_name_changes(
            NameKind::Player,
            &[
                NameChange::new("Nobody", "Somebody"),
                NameChange::new("Almeida", "Almeida"),
            ],
            &CancelToken::new(),
            &mut NoProgress,
        )
        .unwrap();
    drop(w);
    assert_eq!((r.mapped, r.names_not_found, r.games_changed), (0, 1, 0));
    assert_eq!(files(&base), before);
}

#[test]
fn an_existing_new_name_is_reused() {
    let dir = tempfile::tempdir().unwrap();
    let base = build(dir.path(), &spelling_games());
    let names_before = open(&base).names().num_names(NameKind::Player);
    let mut w = Si4Writer::open(&base).unwrap();
    w.apply_name_changes(
        NameKind::Player,
        &[NameChange::new("Almeida, B", "Almeida")],
        &CancelToken::new(),
        &mut NoProgress,
    )
    .unwrap();
    drop(w);
    let db = open(&base);
    assert_eq!(db.names().num_names(NameKind::Player), names_before);
    assert_eq!(freq(&db, NameKind::Player, "Almeida"), Some(4));
}

#[test]
fn failures_at_every_stage_leave_a_valid_database() {
    for label in ["names:superset", "names:entries", "names:final"] {
        let dir = tempfile::tempdir().unwrap();
        let base = build(dir.path(), &spelling_games());
        let changes = four_changes(&base);
        let games = open(&base).len();
        let mut w = Si4Writer::open(&base).unwrap();
        w.set_fail_point(FailPoint::at_label(label));
        let err = w
            .apply_name_changes(
                NameKind::Player,
                &changes,
                &CancelToken::new(),
                &mut NoProgress,
            )
            .unwrap_err();
        assert!(
            matches!(err, Si4Error::InjectedFailure { .. }),
            "{label}: {err}"
        );
        drop(w);
        // It opens (every id exists) and keeps its games.
        let db = open(&base);
        assert_eq!(db.len(), games, "{label}");
        let after = Si4Writer::open(&base).unwrap();
        assert_eq!(after.len(), games);
    }
}

#[test]
fn cancelling_in_the_middle_is_valid_and_reported() {
    let dir = tempfile::tempdir().unwrap();
    let games: Vec<G> = (0..9000)
        .map(|_| g("Almeida, B", "Quux, Q", W).on("E", "S", "????.??.??"))
        .collect();
    let base = build(dir.path(), &games);
    let token = CancelToken::new();
    let mut w = Si4Writer::open(&base).unwrap();
    let t2 = token.clone();
    let r = w
        .apply_name_changes(
            NameKind::Player,
            &[NameChange::new("Almeida, B", "Almeida, Bruno")],
            &token,
            &mut move |done, _| {
                if done >= 4096 {
                    t2.cancel();
                }
            },
        )
        .unwrap();
    drop(w);
    assert!(r.interrupted);
    assert_eq!(r.games_changed, 4096);
    let db = open(&base);
    assert_eq!(freq(&db, NameKind::Player, "Almeida, Bruno"), Some(4096));
    assert_eq!(freq(&db, NameKind::Player, "Almeida, B"), Some(9000 - 4096));
    // Running again finishes the work.
    let mut w = Si4Writer::open(&base).unwrap();
    let r = w
        .apply_name_changes(
            NameKind::Player,
            &[NameChange::new("Almeida, B", "Almeida, Bruno")],
            &CancelToken::new(),
            &mut NoProgress,
        )
        .unwrap();
    drop(w);
    assert_eq!((r.games_changed, r.interrupted), (9000 - 4096, false));
    assert_eq!(
        freq(&open(&base), NameKind::Player, "Almeida, Bruno"),
        Some(9000)
    );
}

#[test]
fn text_of_the_proposals_names_the_heading() {
    let dir = tempfile::tempdir().unwrap();
    let base = build(dir.path(), &spelling_games());
    let list = proposals_of(&base, ScanOptions::new(NameKind::Player));
    let text = proposals_to_text(NameKind::Player, &list);
    assert!(text.starts_with("Scid found 5 player name corrections.\n"));
    assert!(text.contains("\"Almeida, B\"\t>> \"Almeida, Bruno\" (3)  1900.??.??--1970.04.??"));
}

// ---- Add Elo ----

const ELO_SSP: &str = "@PLAYER\nAlmeida, Beatriz Costa #WGM BRA [2400]\n  %Elo 2013:2300,2301,2305,2310,2315,2320,2325,2330,2335,2340,2345,2350 2001:2200,2210,2220,2230\n  = Almeida, Beatriz\n";

fn elo_games() -> Vec<G> {
    let b = "Almeida, Beatriz Costa";
    vec![
        g(b, "Quux, Q", W).on("E", "S", "2013.03.01"),
        g("Quux, Q", b, W).on("E", "S", "2013.00.00"),
        g(b, "Quux, Q", W).on("E", "S", "2012.07.01"),
        g("Almeida, Beatriz", "Quux, Q", W).on("E", "S", "2013.03.01"),
        g(b, "Quux, Q", W)
            .on("E", "S", "2013.03.01")
            .elo(2400, 0)
            .kind(RatingType::Uscf),
    ]
}

fn elo_of(db: &Si4Database<std::fs::File>, n: u32) -> ((u16, u8), (u16, u8)) {
    let e = db.entry(n).unwrap();
    (
        (e.white_elo, e.white_rating_type),
        (e.black_elo, e.black_rating_type),
    )
}

#[test]
fn add_elo_defaults() {
    let dir = tempfile::tempdir().unwrap();
    let base = build(dir.path(), &elo_games());
    let set = read(ELO_SSP);
    let checker = set.checker(NameKind::Player);
    let db = open(&base);
    let plan = plan_add_elo(
        &db,
        checker,
        None,
        EloOptions::default(),
        &CancelToken::new(),
        &mut NoProgress,
    )
    .unwrap();
    assert_eq!(plan.report.ratings, 2);
    assert_eq!(plan.report.games, 2);
    let games: Vec<u32> = plan.changes.iter().map(|c| c.game).collect();
    assert_eq!(games, [1, 2]);
    assert_eq!(plan.changes[0].white, Some((2305, 0)));
    assert_eq!(plan.changes[1].black, Some((2300, 0)));
    drop(db);
    let before_games = games_bytes(&base);
    let mut w = Si4Writer::open(&base).unwrap();
    let (report, result) = apply_add_elo(&mut w, &plan, &CancelToken::new(), &mut NoProgress);
    result.unwrap();
    drop(w);
    assert_eq!((report.ratings, report.games), (2, 2));
    assert_eq!(games_bytes(&base), before_games);
    let db = open(&base);
    assert_eq!(elo_of(&db, 1), ((2305, 0), (0, 0)));
    assert_eq!(elo_of(&db, 2), ((0, 0), (2300, 0)));
    assert_eq!(elo_of(&db, 3).0.0, 0, "a year without a list");
    assert_eq!(elo_of(&db, 4).0.0, 0, "alias");
    assert_eq!(elo_of(&db, 5), ((2400, 4), (0, 0)), "existing rating kept");
    // A second run changes nothing.
    let again = plan_add_elo(
        &db,
        checker,
        None,
        EloOptions::default(),
        &CancelToken::new(),
        &mut NoProgress,
    )
    .unwrap();
    assert_eq!(
        (
            again.report.ratings,
            again.report.games,
            again.changes.len()
        ),
        (0, 0, 0)
    );
}

#[test]
fn add_elo_options() {
    let dir = tempfile::tempdir().unwrap();
    let base = build(dir.path(), &elo_games());
    let set = read(ELO_SSP);
    let checker = set.checker(NameKind::Player);
    let db = open(&base);
    let plan = |opts: EloOptions, filter: Option<&Filter>| {
        plan_add_elo(
            &db,
            checker,
            filter,
            opts,
            &CancelToken::new(),
            &mut NoProgress,
        )
        .unwrap()
    };
    // Games without a month are skipped when asked.
    let p = plan(
        EloOptions {
            no_month: false,
            ..EloOptions::default()
        },
        None,
    );
    assert_eq!(p.changes.iter().map(|c| c.game).collect::<Vec<_>>(), [1]);
    // Overwrite replaces the 2400 (USCF) with the table value and the Elo kind.
    let p = plan(
        EloOptions {
            overwrite: true,
            ..EloOptions::default()
        },
        None,
    );
    let five = p.changes.iter().find(|c| c.game == 5).unwrap();
    assert_eq!(five.white, Some((2305, 0)));
    // The filter limits the games.
    let mut f = Filter::new(db.len());
    f.fill(0);
    f.set(2, 1);
    let p = plan(EloOptions::default(), Some(&f));
    assert_eq!(p.changes.iter().map(|c| c.game).collect::<Vec<_>>(), [2]);
    let p = plan(
        EloOptions {
            only_filter: false,
            ..EloOptions::default()
        },
        Some(&f),
    );
    assert_eq!(p.changes.len(), 2);
    // A wrong filter size is refused.
    let small = Filter::new(1);
    assert!(matches!(
        plan_add_elo(
            &db,
            checker,
            Some(&small),
            EloOptions::default(),
            &CancelToken::new(),
            &mut NoProgress
        ),
        Err(AddEloError::FilterSize { .. })
    ));
    // update = false only counts: nothing is written.
    let counting = plan(
        EloOptions {
            update: false,
            ..EloOptions::default()
        },
        None,
    );
    assert_eq!(counting.report.games, 2);
    drop(db);
    let before = index_bytes(&base);
    let mut w = Si4Writer::open(&base).unwrap();
    let (report, result) = apply_add_elo(&mut w, &counting, &CancelToken::new(), &mut NoProgress);
    result.unwrap();
    drop(w);
    assert_eq!(report.games, 2);
    assert_eq!(index_bytes(&base), before);
}

#[test]
fn overwrite_with_a_known_month_uses_exact_lookup() {
    let dir = tempfile::tempdir().unwrap();
    let b = "Almeida, Beatriz Costa";
    let games = vec![g(b, "Quux, Q", W).on("E", "S", "2013.00.00").elo(2400, 0)];
    let base = build(dir.path(), &games);
    let set = read("@PLAYER\nAlmeida, Beatriz Costa\n%Elo 2013:?,2301,2305\n");
    let db = open(&base);
    let opts = EloOptions {
        overwrite: true,
        ..EloOptions::default()
    };
    let p = plan_add_elo(
        &db,
        set.checker(NameKind::Player),
        None,
        opts,
        &CancelToken::new(),
        &mut NoProgress,
    )
    .unwrap();
    assert_eq!(p.changes.len(), 0, "exact: the sweep of the year is off");
    let p = plan_add_elo(
        &db,
        set.checker(NameKind::Player),
        None,
        EloOptions {
            overwrite: false,
            ..opts
        },
        &CancelToken::new(),
        &mut NoProgress,
    )
    .unwrap();
    assert_eq!(p.changes.len(), 0, "rating already there");
}

#[test]
fn add_elo_requires_elo_data() {
    let dir = tempfile::tempdir().unwrap();
    let base = build(dir.path(), &elo_games());
    let db = open(&base);
    let set = read("@PLAYER\nAlmeida, Beatriz Costa\n@SITE\nX\n");
    let err = plan_add_elo(
        &db,
        set.checker(NameKind::Player),
        None,
        EloOptions::default(),
        &CancelToken::new(),
        &mut NoProgress,
    )
    .unwrap_err();
    assert_eq!(err, AddEloError::NoEloData);
    let err = plan_add_elo(
        &db,
        set.checker(NameKind::Site),
        None,
        EloOptions::default(),
        &CancelToken::new(),
        &mut NoProgress,
    )
    .unwrap_err();
    assert_eq!(err, AddEloError::NotPlayers);
}

#[test]
fn set_elo_batch_clamps_and_touches_four_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let base = build(dir.path(), &elo_games());
    let before = index_bytes(&base);
    let mut w = Si4Writer::open(&base).unwrap();
    let (changed, result) = w.set_elo_batch(&[
        (1, Some((5000, 2)), None),
        (2, None, Some((2100, 0))),
        (3, None, None),
    ]);
    result.unwrap();
    assert_eq!(changed, 2);
    // The same values again change nothing.
    let (again, result) = w.set_elo_batch(&[(1, Some((4000, 2)), None)]);
    result.unwrap();
    assert_eq!(again, 0);
    assert!(w.set_elo_batch(&[(99, Some((1, 0)), None)]).1.is_err());
    drop(w);
    let after = index_bytes(&base);
    for (i, (a, b)) in before.iter().zip(&after).enumerate() {
        if i < 182 {
            assert_eq!(a, b);
            continue;
        }
        let offset = (i - 182) % 47;
        let entry = (i - 182) / 47 + 1;
        let allowed = (entry == 1 && (29..=30).contains(&offset))
            || (entry == 2 && (31..=32).contains(&offset));
        if !allowed {
            assert_eq!(a, b, "entry {entry} offset {offset}");
        }
    }
    let db = open(&base);
    assert_eq!(elo_of(&db, 1).0, (4000, 2));
    assert_eq!(elo_of(&db, 2).1, (2100, 0));
}
