//! Contract tests of the permanent sort of the index (`scidt -S` / `-R`;
//! `docs/specs/m3-5-mychess-cli.md` rules 38-41, decision D623). The orders
//! come from `IndexEntry::Compare` (reference `src/index.cpp:679-820`), the
//! expected values are written here by hand or by an independent oracle.
#![allow(
    clippy::unwrap_used,
    clippy::too_many_lines,
    clippy::cast_possible_truncation,
    clippy::struct_excessive_bools,
    clippy::many_single_char_names,
    clippy::cast_possible_wrap,
    clippy::manual_midpoint,
    clippy::assigning_clones,
    clippy::chunks_exact_to_as_chunks
)]

use std::fs;
use std::path::{Path, PathBuf};

use mychess_core::{Eco, Elo, Game, GameResult, PartialDate, Rating, RatingType};
use mychess_db::si4::journal::FailPoint;
use mychess_db::si4::sort::sort_games_with_hook;
use mychess_db::si4::{
    ENTRY_LEN, GameFlags, HEADER_LEN, MAX_SORT_KEYS, Si4Database, Si4Writer, SortKey, SortKeyError,
    sort_games,
};
use pretty_assertions::assert_eq;
use proptest::prelude::*;

type Date = (Option<u16>, Option<u8>, Option<u8>);

#[derive(Debug, Clone)]
struct Spec {
    label: String,
    white: String,
    black: String,
    event: String,
    site: String,
    round: String,
    date: Date,
    event_date: Date,
    eco: Option<String>,
    result: GameResult,
    plies: usize,
    welo: u16,
    belo: u16,
    vars: u8,
    comments: u8,
    deleted: bool,
}

impl Spec {
    /// Variations need a main move to be alternatives to; comments need nodes.
    fn eff_plies(&self) -> usize {
        self.plies
            .max(usize::from(self.comments))
            .max(usize::from(self.vars > 0))
    }

    fn new(label: &str) -> Spec {
        Spec {
            label: label.into(),
            white: "W".into(),
            black: "B".into(),
            event: "E".into(),
            site: "S".into(),
            round: "1".into(),
            date: (Some(2000), Some(1), Some(1)),
            event_date: (None, None, None),
            eco: None,
            result: GameResult::Draw,
            plies: 4,
            welo: 0,
            belo: 0,
            vars: 0,
            comments: 0,
            deleted: false,
        }
    }
}

fn pdate(d: Date) -> PartialDate {
    PartialDate::new(d.0, d.1, d.2).unwrap()
}

fn build(spec: &Spec) -> Game {
    const CYCLE: [&str; 4] = ["g1f3", "g8f6", "f3g1", "f6g8"];
    let plies = spec.eff_plies();
    let mut game = Game::new();
    let mut at = game.root();
    let mut nodes = Vec::new();
    for i in 0..plies {
        let mv = game
            .position_at(at)
            .unwrap()
            .parse_uci(CYCLE[i % 4])
            .unwrap();
        at = game.add_move(at, mv).unwrap();
        nodes.push(at);
    }
    for (i, uci) in ["d2d4", "c2c4", "b1c3", "e2e4"]
        .iter()
        .take(usize::from(spec.vars))
        .enumerate()
    {
        let _ = i;
        let root = game.root();
        let mv = game.position_at(root).unwrap().parse_uci(uci).unwrap();
        game.add_move(root, mv).unwrap();
    }
    for node in nodes.iter().take(usize::from(spec.comments)) {
        game.set_comment(*node, "note").unwrap();
    }
    let t = game.tags_mut();
    t.white = spec.white.clone();
    t.black = spec.black.clone();
    t.event = spec.event.clone();
    t.site = spec.site.clone();
    t.round = spec.round.clone();
    t.date = pdate(spec.date);
    t.event_date = pdate(spec.event_date);
    t.result = spec.result;
    t.eco = spec.eco.as_deref().map(|e| e.parse::<Eco>().unwrap());
    if spec.welo > 0 {
        t.white_rating = Some(Rating::new(Elo::new(spec.welo).unwrap(), RatingType::Elo));
    }
    if spec.belo > 0 {
        t.black_rating = Some(Rating::new(Elo::new(spec.belo).unwrap(), RatingType::Elo));
    }
    t.set_extra("Label", &spec.label).unwrap();
    game
}

fn make(dir: &Path, specs: &[Spec]) -> PathBuf {
    let base = dir.join("db");
    let mut w = Si4Writer::create(&base, 0).unwrap();
    for s in specs {
        let flags = if s.deleted {
            GameFlags::DELETE
        } else {
            GameFlags::empty()
        };
        w.add_game(&build(s), flags).unwrap();
    }
    base
}

fn labels(base: &Path) -> Vec<String> {
    let mut db = Si4Database::open(base).unwrap();
    (1..=db.len())
        .map(|n| {
            db.game(n)
                .unwrap()
                .tags()
                .get_extra("Label")
                .unwrap()
                .to_owned()
        })
        .collect()
}

fn sort_db(base: &Path, keys: &[SortKey], reverse: bool) -> Vec<String> {
    let w = Si4Writer::open(base).unwrap();
    let (w, _) = sort_games(w, keys, reverse).unwrap();
    drop(w);
    labels(base)
}

/// Creates the database from `specs` (labels in the order given), sorts it
/// and returns the labels in the new order.
fn order_of(specs: &[Spec], keys: &[SortKey], reverse: bool) -> Vec<String> {
    let dir = tempfile::tempdir().unwrap();
    let base = make(dir.path(), specs);
    sort_db(&base, keys, reverse)
}

fn s(label: &str, f: impl FnOnce(&mut Spec)) -> Spec {
    let mut spec = Spec::new(label);
    f(&mut spec);
    spec
}

fn strs(v: &[&str]) -> Vec<String> {
    v.iter().map(|x| (*x).to_owned()).collect()
}

// ------------------------------------------------------------------ parsing

#[test]
fn every_key_name_parses_to_itself_and_all_are_19() {
    assert_eq!(SortKey::ALL.len(), 19);
    for key in SortKey::ALL {
        assert_eq!(SortKey::parse_list(key.name()).unwrap(), vec![key]);
        assert_eq!(
            SortKey::parse_list(&key.name().to_uppercase()).unwrap(),
            vec![key],
            "case-insensitive"
        );
    }
    // The 19 names of `sortCriteriaNames` (Scid `index.cpp`).
    let names: Vec<&str> = SortKey::ALL.iter().map(|k| k.name()).collect();
    assert_eq!(
        names,
        [
            "date",
            "year",
            "event",
            "site",
            "round",
            "white",
            "black",
            "eco",
            "result",
            "length",
            "rating",
            "welo",
            "belo",
            "country",
            "month",
            "deleted",
            "eventdate",
            "variations",
            "comments"
        ]
    );
}

#[test]
fn unique_prefixes_resolve_and_ambiguous_ones_are_rejected() {
    use SortKey::*;
    for (text, key) in [
        ("da", Date),
        ("y", Year),
        ("s", Site),
        ("ro", Round),
        ("wh", White),
        ("wel", WhiteElo),
        ("bl", Black),
        ("be", BlackElo),
        ("ec", Eco),
        ("res", Result),
        ("l", Length),
        ("ra", Rating),
        ("cou", Country),
        ("com", Comments),
        ("m", Month),
        ("va", Variations),
    ] {
        assert_eq!(SortKey::parse_list(text).unwrap(), vec![key], "{text}");
    }
    for ambiguous in ["w", "e", "ev", "eve", "c", "co", "b", "r", "d"] {
        assert!(SortKey::parse_list(ambiguous).is_err(), "{ambiguous}");
    }
    // An exact name wins over being the prefix of another one.
    assert_eq!(
        SortKey::parse_list("event").unwrap(),
        vec![Event],
        "event is also a prefix of eventdate"
    );
    assert_eq!(
        SortKey::parse_list("date").unwrap(),
        vec![Date],
        "date is a prefix of nothing but must stay exact"
    );
}

#[test]
fn separators_are_comma_dot_and_space_and_the_limit_is_15() {
    assert_eq!(
        SortKey::parse_list("date, year.month  eco").unwrap(),
        vec![SortKey::Date, SortKey::Year, SortKey::Month, SortKey::Eco]
    );
    // INDEX_MaxSortCriteria is 16 but one slot is the sentinel (D632).
    assert_eq!(MAX_SORT_KEYS, 15);
    assert_eq!(SortKey::parse_list(&"date,".repeat(15)).unwrap().len(), 15);
    assert_eq!(
        SortKey::parse_list(&"date,".repeat(16)).unwrap_err(),
        SortKeyError::TooMany
    );
    for bad in ["", " ", ",,,", "date;year", "d4te", "dat\u{e9}", "-date"] {
        assert!(SortKey::parse_list(bad).is_err(), "{bad:?}");
    }
}

// ------------------------------------------------------------ one key a time

#[test]
fn date_orders_partial_dates_with_unknown_parts_first() {
    let d = |y, m, dd| (y, m, dd);
    let specs = [
        s("full", |x| x.date = d(Some(2020), Some(5), Some(10))),
        s("mon", |x| x.date = d(Some(2020), Some(5), None)),
        s("yr", |x| x.date = d(Some(2020), None, None)),
        s("none", |x| x.date = d(None, None, None)),
        s("old", |x| x.date = d(Some(2019), Some(12), Some(31))),
        s("jun", |x| x.date = d(Some(2020), Some(6), None)),
    ];
    assert_eq!(
        order_of(&specs, &[SortKey::Date], false),
        strs(&["none", "old", "yr", "mon", "full", "jun"])
    );
}

#[test]
fn year_and_month_ignore_the_other_parts_and_are_stable() {
    let d = |y, m, dd| (y, m, dd);
    let specs = [
        s("a", |x| x.date = d(Some(2021), Some(3), Some(1))),
        s("b", |x| x.date = d(Some(2020), Some(9), Some(9))),
        s("c", |x| x.date = d(Some(2021), Some(1), Some(1))),
        s("d", |x| x.date = d(None, Some(2), None)),
        s("e", |x| x.date = d(Some(2020), None, None)),
    ];
    // Year: unknown(0) < 2020 < 2021; ties keep the original order (b, e) (a, c).
    assert_eq!(
        order_of(&specs, &[SortKey::Year], false),
        strs(&["d", "b", "e", "a", "c"])
    );
    // Month: unknown(0) first (e), then 1 (c), 2 (d), 3 (a), 9 (b).
    assert_eq!(
        order_of(&specs, &[SortKey::Month], false),
        strs(&["e", "c", "d", "a", "b"])
    );
}

#[test]
fn event_date_falls_back_to_the_game_date_when_unknown() {
    let d = |y, m, dd| (y, m, dd);
    let specs = [
        s("late-event", |x| {
            x.date = d(Some(2000), Some(1), Some(1));
            x.event_date = d(Some(2002), Some(1), Some(1));
        }),
        s("no-event-date", |x| {
            x.date = d(Some(2001), Some(1), Some(1));
        }),
        s("early-event", |x| {
            x.date = d(Some(2003), Some(1), Some(1));
            x.event_date = d(Some(2000), Some(1), Some(1));
        }),
    ];
    assert_eq!(
        order_of(&specs, &[SortKey::EventDate], false),
        strs(&["early-event", "no-event-date", "late-event"])
    );
}

#[test]
fn name_keys_sort_by_bytes_case_sensitively() {
    let mk = |f: fn(&mut Spec, &str)| {
        ["bravo", "Alpha", "alpha", "Zulu", "", "Charlie"]
            .iter()
            .enumerate()
            .map(|(i, n)| s(&format!("g{i}"), |x| f(x, n)))
            .collect::<Vec<_>>()
    };
    // Empty names are stored as "?" by the tag defaults of the writer? Use the
    // raw order of the others and only check relative order of the rest.
    let expect = |specs: &[Spec], key| {
        let got = order_of(specs, &[key], false);
        let pos = |l: &str| got.iter().position(|x| x == l).unwrap();
        // Alpha < Charlie < Zulu < alpha < bravo (ASCII)
        assert!(pos("g1") < pos("g5"), "{got:?}");
        assert!(pos("g5") < pos("g3"), "{got:?}");
        assert!(pos("g3") < pos("g2"), "{got:?}");
        assert!(pos("g2") < pos("g0"), "{got:?}");
    };
    expect(&mk(|x, n| x.white = n.into()), SortKey::White);
    expect(&mk(|x, n| x.black = n.into()), SortKey::Black);
    expect(&mk(|x, n| x.event = n.into()), SortKey::Event);
    expect(&mk(|x, n| x.site = n.into()), SortKey::Site);
}

#[test]
fn round_compares_integers_then_the_part_after_the_dot() {
    let rounds = ["10", "2", "3.10", "3.2", "?", "1", "2.1", "abc"];
    let specs: Vec<Spec> = rounds
        .iter()
        .map(|r| s(r, |x| x.round = (*r).into()))
        .collect();
    let got = order_of(&specs, &[SortKey::Round], false);
    // "?" and "abc" read as 0 (strtol); original order between them: "?" then "abc",
    // then byte order breaks the tie in Scid only through strCompare? No:
    // strCompareRound returns 0 for equal ints and equal non-dot prefixes
    // only after the byte comparison, so "?" < "abc".
    assert_eq!(
        got,
        strs(&["?", "abc", "1", "2", "2.1", "3.2", "3.10", "10"])
    );
}

#[test]
fn eco_orders_codes_with_unclassified_first() {
    let specs = [
        s("e", |x| x.eco = Some("E60".into())),
        s("none", |x| x.eco = None),
        s("a", |x| x.eco = Some("A00".into())),
        s("c", |x| x.eco = Some("C20".into())),
        s("a2", |x| x.eco = Some("A45".into())),
    ];
    assert_eq!(
        order_of(&specs, &[SortKey::Eco], false),
        strs(&["none", "a", "a2", "c", "e"])
    );
}

#[test]
fn result_follows_the_code_of_the_scid_white_black_draw_then_no_result() {
    // Spec rule 6.2 (D630): `1-0`, `0-1`, `1/2-1/2`, `*` (`common.h` and
    // `SORT_result`, not the comment of the Scid); the next key is only a
    // tie-break inside each result.
    let specs = [
        s("draw-z", |x| {
            x.result = GameResult::Draw;
            x.white = "Z".into();
        }),
        s("none-a", |x| {
            x.result = GameResult::Unknown;
            x.white = "A".into();
        }),
        s("black", |x| x.result = GameResult::Black),
        s("white", |x| x.result = GameResult::White),
        s("draw-m", |x| {
            x.result = GameResult::Draw;
            x.white = "M".into();
        }),
    ];
    assert_eq!(
        order_of(&specs, &[SortKey::Result, SortKey::White], false),
        strs(&["white", "black", "draw-m", "draw-z", "none-a"])
    );
}

#[test]
fn length_orders_by_half_moves_fewest_first() {
    let specs: Vec<Spec> = [6usize, 0, 2, 11]
        .iter()
        .map(|p| s(&format!("p{p}"), |x| x.plies = *p))
        .collect();
    assert_eq!(
        order_of(&specs, &[SortKey::Length], false),
        strs(&["p0", "p2", "p6", "p11"])
    );
}

#[test]
fn ratings_sort_highest_first_and_elo_zero_last() {
    let specs = [
        s("w1500", |x| {
            x.welo = 1500;
            x.belo = 1500;
        }),
        s("unrated", |_| {}),
        s("w2700", |x| {
            x.welo = 2700;
            x.belo = 0;
        }),
        s("b2600", |x| {
            x.welo = 0;
            x.belo = 2600;
        }),
        s("avg", |x| {
            x.welo = 2000;
            x.belo = 2001;
        }),
    ];
    // Average (integer division): w1500=1500, unrated=0, w2700=1350, b2600=1300, avg=2000.
    assert_eq!(
        order_of(&specs, &[SortKey::Rating], false),
        strs(&["avg", "w1500", "w2700", "b2600", "unrated"])
    );
    // White Elo: 2700, 2000, 1500, then 0 (b2600, unrated in original order).
    assert_eq!(
        order_of(&specs, &[SortKey::WhiteElo], false),
        strs(&["w2700", "avg", "w1500", "unrated", "b2600"])
    );
    // Black Elo: 2600, 2001, 1500, then 0 (unrated, w2700).
    assert_eq!(
        order_of(&specs, &[SortKey::BlackElo], false),
        strs(&["b2600", "avg", "w1500", "unrated", "w2700"])
    );
}

#[test]
fn country_uses_the_last_three_letters_and_puts_games_without_one_last() {
    let specs = [
        s("none1", |x| x.site = "Paris".into()),
        s("ita", |x| x.site = "Rome ITA".into()),
        s("fra", |x| x.site = "Paris FRA".into()),
        s("lower", |x| x.site = "Paris fra".into()),
        s("short", |x| x.site = "NOR".into()),
        s("none2", |x| x.site = "Oslo".into()),
        s("fra2", |x| x.site = "Lyon FRA".into()),
    ];
    // FRA (fra, fra2 in original order) < ITA < the rest in original order.
    assert_eq!(
        order_of(&specs, &[SortKey::Country], false),
        strs(&["fra", "fra2", "ita", "none1", "lower", "short", "none2"])
    );
}

#[test]
fn deleted_puts_unmarked_games_first() {
    let specs = [
        s("d1", |x| x.deleted = true),
        s("k1", |_| {}),
        s("d2", |x| x.deleted = true),
        s("k2", |_| {}),
    ];
    assert_eq!(
        order_of(&specs, &[SortKey::Deleted], false),
        strs(&["k1", "k2", "d1", "d2"])
    );
}

#[test]
fn variations_and_comments_sort_most_first() {
    let specs = [
        s("v0", |_| {}),
        s("v2", |x| x.vars = 2),
        s("v1", |x| x.vars = 1),
        s("v3", |x| x.vars = 3),
    ];
    assert_eq!(
        order_of(&specs, &[SortKey::Variations], false),
        strs(&["v3", "v2", "v1", "v0"])
    );
    let specs = [
        s("c0", |_| {}),
        s("c3", |x| x.comments = 3),
        s("c1", |x| x.comments = 1),
        s("c2", |x| x.comments = 2),
    ];
    assert_eq!(
        order_of(&specs, &[SortKey::Comments], false),
        strs(&["c3", "c2", "c1", "c0"])
    );
}

#[test]
fn later_keys_break_ties_and_reverse_inverts_the_whole_order() {
    let specs = [
        s("a", |x| {
            x.event = "X".into();
            x.white = "2".into();
        }),
        s("b", |x| {
            x.event = "X".into();
            x.white = "1".into();
        }),
        s("c", |x| {
            x.event = "A".into();
            x.white = "9".into();
        }),
    ];
    let keys = [SortKey::Event, SortKey::White];
    assert_eq!(order_of(&specs, &keys, false), strs(&["c", "b", "a"]));
    assert_eq!(order_of(&specs, &keys, true), strs(&["a", "b", "c"]));
}

#[test]
fn reverse_keeps_tied_games_in_their_original_order() {
    // Scid negates the comparison result, so a tie stays a tie.
    let specs = [
        s("t1", |_| {}),
        s("t2", |_| {}),
        s("hi", |x| x.date = (Some(2030), None, None)),
        s("t3", |_| {}),
    ];
    assert_eq!(
        order_of(&specs, &[SortKey::Year], true),
        strs(&["hi", "t1", "t2", "t3"])
    );
}

#[test]
fn sorting_with_every_single_key_keeps_the_same_games() {
    let specs: Vec<Spec> = (0..7)
        .map(|i| {
            s(&format!("g{i}"), |x| {
                x.plies = (i * 3) % 7;
                x.white = format!("W{}", (i * 5) % 7);
                x.round = format!("{}", (i * 2) % 5);
                x.welo = (i as u16 * 100) % 700;
                x.date = (Some(2000 + (i as u16 % 3)), None, None);
            })
        })
        .collect();
    for key in SortKey::ALL {
        for reverse in [false, true] {
            let mut got = order_of(&specs, &[key], reverse);
            got.sort();
            let mut want: Vec<String> = specs.iter().map(|x| x.label.clone()).collect();
            want.sort();
            assert_eq!(got, want, "{key} reverse={reverse}");
        }
    }
}

#[test]
fn non_ascii_names_follow_the_signed_char_compare_of_the_scid() {
    // `strCompare` (misc.cpp:289) subtracts `char` values; with the signed
    // `char` of the reference build the UTF-8 lead byte of "Éric" (0xC3) is
    // negative, so "Éric" < "Alan" < "Zed".
    let specs = [
        s("zed", |x| x.white = "Zed".into()),
        s("eric", |x| x.white = "\u{c9}ric".into()),
        s("alan", |x| x.white = "Alan".into()),
    ];
    assert_eq!(
        order_of(&specs, &[SortKey::White], false),
        strs(&["eric", "alan", "zed"])
    );
}

#[test]
fn result_orders_one_zero_then_half_then_star_whatever_the_file_order() {
    let specs = [
        s("star", |x| x.result = GameResult::Unknown),
        s("half", |x| x.result = GameResult::Draw),
        s("zero", |x| x.result = GameResult::Black),
        s("one", |x| x.result = GameResult::White),
    ];
    assert_eq!(
        order_of(&specs, &[SortKey::Result], false),
        strs(&["one", "zero", "half", "star"])
    );
    assert_eq!(
        order_of(&specs, &[SortKey::Result], true),
        strs(&["star", "half", "zero", "one"])
    );
}

#[test]
fn non_ascii_names_still_give_a_total_order_with_a_reverse() {
    let specs = [
        s("zed", |x| x.white = "Zed".into()),
        s("eric", |x| x.white = "\u{c9}ric".into()),
        s("alan", |x| x.white = "Alan".into()),
        s("eric2", |x| x.white = "\u{c9}ric".into()),
    ];
    assert_eq!(
        order_of(&specs, &[SortKey::White], true),
        strs(&["zed", "alan", "eric", "eric2"])
    );
}

// ------------------------------------------------------------ 0 and 1 games

#[test]
fn sorting_zero_and_one_game_is_a_no_op() {
    let dir = tempfile::tempdir().unwrap();
    let base = make(dir.path(), &[]);
    let before = fs::read(base.with_extension("si4")).unwrap();
    let w = Si4Writer::open(&base).unwrap();
    let (w, report) = sort_games(w, &SortKey::ALL, true).unwrap();
    assert_eq!((report.games, report.moved), (0, 0));
    drop(w);
    assert_eq!(fs::read(base.with_extension("si4")).unwrap(), before);

    let dir = tempfile::tempdir().unwrap();
    let base = make(dir.path(), &[Spec::new("only")]);
    let before = fs::read(base.with_extension("si4")).unwrap();
    let w = Si4Writer::open(&base).unwrap();
    let (w, report) = sort_games(w, &[SortKey::Date], true).unwrap();
    assert_eq!((report.games, report.moved), (1, 0));
    drop(w);
    assert_eq!(fs::read(base.with_extension("si4")).unwrap(), before);
    assert_eq!(labels(&base), strs(&["only"]));
}

// ------------------------------------------------------ what a sort touches

#[test]
fn only_the_index_file_changes_and_the_cache_is_removed() {
    let dir = tempfile::tempdir().unwrap();
    let specs: Vec<Spec> = (0..6)
        .map(|i| s(&format!("g{i}"), |x| x.white = format!("W{}", 5 - i)))
        .collect();
    let base = make(dir.path(), &specs);
    let sn4 = fs::read(base.with_extension("sn4")).unwrap();
    let sg4 = fs::read(base.with_extension("sg4")).unwrap();
    let si4 = fs::read(base.with_extension("si4")).unwrap();
    fs::write(base.with_extension("stc"), b"cache").unwrap();
    let got = sort_db(&base, &[SortKey::White], false);
    assert_eq!(got, strs(&["g5", "g4", "g3", "g2", "g1", "g0"]));
    assert_eq!(fs::read(base.with_extension("sn4")).unwrap(), sn4);
    assert_eq!(fs::read(base.with_extension("sg4")).unwrap(), sg4);
    assert!(
        !base.with_extension("stc").exists(),
        "cache must be removed"
    );
    let new = fs::read(base.with_extension("si4")).unwrap();
    assert_eq!(new.len(), si4.len());
    // The header (182 bytes, including the game count) is unchanged.
    assert_eq!(&new[..HEADER_LEN], &si4[..HEADER_LEN]);
    // The entries are a permutation of the old ones, byte for byte.
    let chunks = |v: &[u8]| {
        let mut c: Vec<Vec<u8>> = v[HEADER_LEN..]
            .chunks_exact(ENTRY_LEN)
            .map(<[u8]>::to_vec)
            .collect();
        c.sort();
        c
    };
    assert_eq!(chunks(&new), chunks(&si4));
    // No journal, no temporaries left behind.
    let mut names: Vec<String> = fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert_eq!(names, ["db.sg4", "db.si4", "db.sn4"]);
}

#[test]
fn sorting_twice_is_idempotent_and_flags_survive() {
    let dir = tempfile::tempdir().unwrap();
    let specs = [
        s("b", |x| {
            x.white = "B".into();
            x.deleted = true;
        }),
        s("a", |x| x.white = "A".into()),
        s("c", |x| x.white = "C".into()),
    ];
    let base = make(dir.path(), &specs);
    let first = sort_db(&base, &[SortKey::White], false);
    let bytes = fs::read(base.with_extension("si4")).unwrap();
    let w = Si4Writer::open(&base).unwrap();
    let (w, report) = sort_games(w, &[SortKey::White], false).unwrap();
    assert_eq!(report.moved, 0);
    drop(w);
    assert_eq!(fs::read(base.with_extension("si4")).unwrap(), bytes);
    assert_eq!(first, strs(&["a", "b", "c"]));
    let db = Si4Database::open(&base).unwrap();
    let deleted: Vec<bool> = (1..=3)
        .map(|n| db.entry(n).unwrap().flags.contains(GameFlags::DELETE))
        .collect();
    assert_eq!(deleted, [false, true, false]);
}

#[test]
fn a_failure_at_any_stage_leaves_old_or_new_never_a_mix() {
    let specs: Vec<Spec> = (0..5)
        .map(|i| s(&format!("g{i}"), |x| x.white = format!("W{}", 4 - i)))
        .collect();
    // Count the stages of a successful run.
    let stages = {
        let dir = tempfile::tempdir().unwrap();
        let base = make(dir.path(), &specs);
        let w = Si4Writer::open(&base).unwrap();
        let mut hook = FailPoint::never();
        sort_games_with_hook(w, &[SortKey::White], false, &mut hook).unwrap();
        hook.reached().len()
    };
    assert!(stages >= 3, "{stages}");
    let old = strs(&["g0", "g1", "g2", "g3", "g4"]);
    let new = strs(&["g4", "g3", "g2", "g1", "g0"]);
    for stage in 0..stages {
        let dir = tempfile::tempdir().unwrap();
        let base = make(dir.path(), &specs);
        let sn4 = fs::read(base.with_extension("sn4")).unwrap();
        let sg4 = fs::read(base.with_extension("sg4")).unwrap();
        let w = Si4Writer::open(&base).unwrap();
        let mut hook = FailPoint::at(stage);
        let result = sort_games_with_hook(w, &[SortKey::White], false, &mut hook);
        assert!(result.is_err(), "stage {stage} must fail");
        // The next writer recovers (D290), then the database is readable.
        drop(Si4Writer::open(&base).unwrap());
        let got = labels(&base);
        assert!(got == old || got == new, "stage {stage}: {got:?}");
        assert_eq!(fs::read(base.with_extension("sn4")).unwrap(), sn4);
        assert_eq!(fs::read(base.with_extension("sg4")).unwrap(), sg4);
        let left: Vec<String> = fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|n| !["db.sg4", "db.si4", "db.sn4"].contains(&n.as_str()))
            .collect();
        assert!(left.is_empty(), "stage {stage}: leftovers {left:?}");
    }
}

#[test]
fn sort_of_a_truncated_index_fails_and_leaves_the_files_alone() {
    let dir = tempfile::tempdir().unwrap();
    let specs: Vec<Spec> = (0..4).map(|i| Spec::new(&format!("g{i}"))).collect();
    let base = make(dir.path(), &specs);
    let si4 = base.with_extension("si4");
    let bytes = fs::read(&si4).unwrap();
    fs::write(&si4, &bytes[..bytes.len() - 20]).unwrap();
    // Either the writer refuses to open or the sort fails: never a panic,
    // never a rewrite of the broken file.
    if let Ok(w) = Si4Writer::open(&base) {
        assert!(sort_games(w, &[SortKey::Date], false).is_err());
    }
    assert_eq!(fs::read(&si4).unwrap(), &bytes[..bytes.len() - 20]);
}

#[test]
fn a_second_writer_blocks_the_sort_and_the_database_stays_intact() {
    let dir = tempfile::tempdir().unwrap();
    let specs = [
        s("b", |x| x.white = "B".into()),
        s("a", |x| x.white = "A".into()),
    ];
    let base = make(dir.path(), &specs);
    let holder = Si4Writer::open(&base).unwrap();
    assert!(Si4Writer::open(&base).is_err(), "AlreadyOpen expected");
    drop(holder);
    assert_eq!(sort_db(&base, &[SortKey::White], false), strs(&["a", "b"]));
}

// -------------------------------------------------------------- properties

fn arb_date() -> impl Strategy<Value = Date> {
    (
        prop::option::of(1990u16..2030),
        prop::option::of(1u8..=12),
        prop::option::of(1u8..=28),
    )
        .prop_map(|(y, m, d)| {
            // Scid dates keep a month only with a year, and a day only with a month? No:
            // each part is independent in the format; keep them independent.
            (y, m, d)
        })
}

fn arb_spec() -> impl Strategy<Value = Spec> {
    let names = prop::sample::select(vec!["alpha", "Bravo", "charlie", "Delta", "echo", "a", "Z"]);
    let sites = prop::sample::select(vec![
        "Paris FRA",
        "Rome ITA",
        "Oslo",
        "Lima PER",
        "NOR",
        "x ABC",
        "Berlin GER",
        "q xyz",
    ]);
    let ecos = prop::sample::select(vec!["", "A00", "B12", "C20", "C21", "D35", "E97"]);
    let results = prop::sample::select(vec![
        GameResult::White,
        GameResult::Black,
        GameResult::Draw,
        GameResult::Unknown,
    ]);
    (
        (names.clone(), names.clone(), names, sites),
        (
            0u8..12,
            arb_date(),
            // The event date is stored as an offset of -3..=3 years from the
            // game year (docs/formatos/si4.md): it exists only with a game year.
            prop::option::of((
                -3i16..=3,
                prop::option::of(1u8..=12),
                prop::option::of(1u8..=28),
            )),
            ecos,
        ),
        (results, 0usize..9, 0u16..4, 0u16..4),
        (0u8..3, 0u8..3, any::<bool>()),
    )
        .prop_map(
            |(
                (white, black, event, site),
                (round, date, event_offset, eco),
                (result, plies, welo, belo),
                (vars, comments, deleted),
            )| Spec {
                label: String::new(),
                white: white.into(),
                black: black.into(),
                event: event.into(),
                site: site.into(),
                round: round.to_string(),
                date,
                event_date: match (date.0, event_offset) {
                    (Some(y), Some((dy, m, d))) => (y.checked_add_signed(dy), m, d),
                    _ => (None, None, None),
                },
                eco: (!eco.is_empty()).then(|| eco.to_owned()),
                result,
                plies,
                welo: welo * 700,
                belo: belo * 650,
                vars,
                comments,
                deleted,
            },
        )
}

/// The oracle: a comparison key per sort field, written from `IndexEntry::Compare`.
fn oracle_key(spec: &Spec, key: SortKey) -> Vec<i64> {
    let d = |d: Date| {
        i64::from(d.0.unwrap_or(0)) << 9
            | i64::from(d.1.unwrap_or(0)) << 5
            | i64::from(d.2.unwrap_or(0))
    };
    let bytes = |t: &str| t.bytes().map(i64::from).collect::<Vec<_>>();
    let plies = spec.eff_plies() as i64;
    match key {
        SortKey::Date => vec![d(spec.date)],
        SortKey::Year => vec![i64::from(spec.date.0.unwrap_or(0))],
        SortKey::Month => vec![i64::from(spec.date.1.unwrap_or(0))],
        SortKey::EventDate => {
            let e = d(spec.event_date);
            vec![if e == 0 { d(spec.date) } else { e }]
        }
        SortKey::Event => bytes(&spec.event),
        SortKey::Site => bytes(&spec.site),
        SortKey::White => bytes(&spec.white),
        SortKey::Black => bytes(&spec.black),
        SortKey::Round => vec![spec.round.parse::<i64>().unwrap()],
        SortKey::Eco => vec![spec.eco.as_ref().map_or(0, |e| {
            let b = e.as_bytes();
            i64::from(b[0] - b'A' + 1) * 100 + i64::from((b[1] - b'0') * 10 + (b[2] - b'0'))
        })],
        SortKey::Result => vec![match spec.result {
            GameResult::White => 1,
            GameResult::Black => 2,
            GameResult::Draw => 3,
            _ => 4,
        }],
        SortKey::Length => vec![plies],
        SortKey::Rating => vec![-((i64::from(spec.welo) + i64::from(spec.belo)) / 2)],
        SortKey::WhiteElo => vec![-i64::from(spec.welo)],
        SortKey::BlackElo => vec![-i64::from(spec.belo)],
        SortKey::Country => {
            let site = spec.site.as_bytes();
            let n = site.len();
            let has = n >= 4 && site[n - 4] == b' ' && site[n - 3].is_ascii_uppercase();
            if has {
                let mut v = vec![0];
                v.extend(bytes(&spec.site[n - 3..]));
                v
            } else {
                vec![1]
            }
        }
        SortKey::Deleted => vec![i64::from(spec.deleted)],
        SortKey::Variations => vec![-i64::from(spec.vars)],
        SortKey::Comments => vec![-i64::from(spec.comments)],
        _ => unreachable!("a new sort key needs an oracle"),
    }
}

/// Byte order that pads with a terminator smaller than any letter, as `strCompare`.
fn cmp_vec(a: &[i64], b: &[i64]) -> std::cmp::Ordering {
    let mut i = 0;
    loop {
        let x = a.get(i).copied().unwrap_or(-1);
        let y = b.get(i).copied().unwrap_or(-1);
        if x != y {
            return x.cmp(&y);
        }
        if i >= a.len() && i >= b.len() {
            return std::cmp::Ordering::Equal;
        }
        i += 1;
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 48, .. ProptestConfig::default() })]

    #[test]
    fn sort_is_a_stable_permutation_that_matches_the_oracle(
        mut specs in prop::collection::vec(arb_spec(), 0..14),
        key_idx in prop::collection::vec(0usize..19, 1..6),
        reverse in any::<bool>(),
    ) {
        for (i, spec) in specs.iter_mut().enumerate() {
            spec.label = format!("g{i:02}");
        }
        let keys: Vec<SortKey> = key_idx.iter().map(|&i| SortKey::ALL[i]).collect();
        let got = order_of(&specs, &keys, reverse);

        // Permutation.
        let mut sorted_got = got.clone();
        sorted_got.sort();
        let mut want_labels: Vec<String> = specs.iter().map(|x| x.label.clone()).collect();
        want_labels.sort();
        prop_assert_eq!(&sorted_got, &want_labels);

        // Stable sort by the oracle comparator (negated, not reversed, for -r).
        let mut expected: Vec<&Spec> = specs.iter().collect();
        expected.sort_by(|a, b| {
            let mut o = std::cmp::Ordering::Equal;
            for k in &keys {
                o = cmp_vec(&oracle_key(a, *k), &oracle_key(b, *k));
                if o != std::cmp::Ordering::Equal {
                    break;
                }
            }
            if reverse { o.reverse() } else { o }
        });
        let expected: Vec<String> = expected.iter().map(|x| x.label.clone()).collect();
        prop_assert_eq!(got, expected, "keys {:?} reverse {}", keys, reverse);
    }
}

#[test]
fn country_with_many_games_without_country_never_panics() {
    // The Scid comparison answers "greater" for two games without a country,
    // which is not a total order and makes `sort_by` unsafe to call with it.
    let specs: Vec<Spec> = (0..200)
        .map(|i| {
            s(&format!("g{i:03}"), |x| {
                x.site = if i % 3 == 0 {
                    format!("Town{i} FRA")
                } else if i % 3 == 1 {
                    format!("Town{i}")
                } else {
                    String::new()
                }
            })
        })
        .collect();
    let got = order_of(&specs, &[SortKey::Country], false);
    assert_eq!(got.len(), 200);
    // All with a country first, in code order then original order.
    assert!(
        got[..67]
            .iter()
            .all(|l| l[1..].parse::<u32>().unwrap() % 3 == 0)
    );
}
