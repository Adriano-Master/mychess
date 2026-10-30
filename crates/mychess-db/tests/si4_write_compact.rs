//! Contract tests of the compaction of games and names and of the failure
//! hooks of the writer (spec M3-2b: rules 42-43; decisions D311-D316, D319).
#![allow(
    clippy::unwrap_used,
    clippy::too_many_lines,
    clippy::cast_possible_truncation,
    clippy::assertions_on_result_states,
    clippy::assert_is_empty,
    clippy::single_element_loop
)]

#[path = "common/si4_write_support.rs"]
mod support;

use std::path::Path;

use mychess_core::Game;
use mychess_db::si4::{
    FailPoint, FlagOp, GameFlags, HEADER_LEN, NameKind, Si4Database, Si4Error, Si4Writer,
    compact_games, compact_names,
};
use pretty_assertions::assert_eq;
use support::{
    BLOCK, ent_off, entry_bytes, ext, game_of_record_len, game_ucis, named, parse_sn4, read, tmp,
};

fn db_games(base: &Path) -> Vec<Game> {
    let mut db = Si4Database::open(base).unwrap();
    (1..=db.len()).map(|n| db.game(n).unwrap()).collect()
}

fn five() -> Vec<Game> {
    (0..5)
        .map(|i| {
            named(
                &format!("W{i}"),
                &format!("B{i}"),
                "Ev",
                "Si",
                &i.to_string(),
            )
        })
        .collect()
}

fn db_with(base: &Path, games: &[Game]) -> Si4Writer {
    let mut w = Si4Writer::create(base, 1).unwrap();
    for g in games {
        w.add_game(g, GameFlags::empty()).unwrap();
    }
    w
}

// ---------------------------------------------------------------- compact games

#[test]
fn compact_games_drops_deleted_games_and_keeps_order_flags_and_header() {
    let (_dir, base) = tmp();
    let games = five();
    let mut w = db_with(&base, &games);
    w.set_flag(2, GameFlags::DELETE, FlagOp::Set).unwrap();
    w.set_flag(4, GameFlags::DELETE, FlagOp::Set).unwrap();
    w.set_flag(3, GameFlags::CUSTOM1 | GameFlags::TACTICS, FlagOp::Set)
        .unwrap();
    w.set_description("my description").unwrap();
    w.set_base_type(9).unwrap();
    w.set_auto_load(3).unwrap();
    w.set_custom_flag_description(2, "mine").unwrap();
    let sn4_before = read(&base, "sn4");

    let (w, report) = compact_games(w).unwrap();

    assert_eq!((report.kept, report.deleted, report.corrupt), (3, 2, 0));
    assert_eq!(w.len(), 3);
    drop(w);
    let mut db = Si4Database::open(&base).unwrap();
    assert_eq!(db.len(), 3);
    for (n, original) in [(1, 0), (2, 2), (3, 4)] {
        assert!(
            games[original].same_content(&db.game(n).unwrap()),
            "game {n}"
        );
    }
    assert_eq!(
        db.entry(2).unwrap().flags,
        GameFlags::CUSTOM1 | GameFlags::TACTICS,
        "flags of kept games are kept"
    );
    let h = db.header();
    assert_eq!(h.base_type(), 9);
    assert_eq!(h.description(), b"my description");
    assert_eq!(h.auto_load_raw(), 4);
    assert_eq!(h.custom_flag_descriptions()[1], b"mine");
    assert_eq!(read(&base, "sn4"), sn4_before, ".sn4 is not touched");
    assert!(support::leftovers(base.parent().unwrap()).is_empty());
}

#[test]
fn compact_games_removes_dead_space_of_replaced_games() {
    let (_dir, base) = tmp();
    let mut w = db_with(&base, &five());
    let replacement = game_ucis(&["d2d4", "d7d5"]);
    w.replace_game(1, &replacement).unwrap();
    let before = read(&base, "sg4").len();
    let (w, report) = compact_games(w).unwrap();
    drop(w);

    let after = read(&base, "sg4").len();
    assert!(after < before);
    assert_eq!(report.games_bytes_before, before as u64);
    assert_eq!(report.games_bytes_after, after as u64);
    let db = Si4Database::open(&base).unwrap();
    // packed: offsets are contiguous from 0 and the file ends at the last record
    let mut at = 0u32;
    for n in 1..=db.len() {
        let e = db.entry(n).unwrap();
        assert_eq!(e.offset, at, "game {n}");
        at += e.length;
    }
    assert_eq!(after, at as usize);
    assert!(replacement.same_content(&db_games(&base)[0]));
}

#[test]
fn compact_games_repacks_blocks_with_the_block_rule() {
    let (_dir, base) = tmp();
    let mut w = Si4Writer::create(&base, 1).unwrap();
    for _ in 0..3 {
        w.add_game(&game_of_record_len(60_000), GameFlags::empty())
            .unwrap();
    }
    // A@0, B@60000, C does not fit (120000 + 60000 > 131072): C@131072
    drop(w);
    assert_eq!(
        Si4Database::open(&base).unwrap().entry(3).unwrap().offset as usize,
        BLOCK
    );

    let mut w = Si4Writer::open(&base).unwrap();
    w.set_flag(2, GameFlags::DELETE, FlagOp::Set).unwrap();
    let (w, _) = compact_games(w).unwrap();
    drop(w);

    let db = Si4Database::open(&base).unwrap();
    assert_eq!(db.entry(1).unwrap().offset, 0);
    assert_eq!(
        db.entry(2).unwrap().offset,
        60_000,
        "now fits in the first block"
    );
    assert_eq!(read(&base, "sg4").len(), 120_000);
}

#[test]
fn compact_games_keeps_a_hole_when_the_next_record_does_not_fit() {
    let (_dir, base) = tmp();
    let mut w = Si4Writer::create(&base, 1).unwrap();
    w.add_game(&game_of_record_len(100_000), GameFlags::empty())
        .unwrap();
    w.add_game(&game_of_record_len(10_000), GameFlags::empty())
        .unwrap();
    w.add_game(&game_of_record_len(40_000), GameFlags::empty())
        .unwrap();
    w.set_flag(2, GameFlags::DELETE, FlagOp::Set).unwrap();
    let (w, _) = compact_games(w).unwrap();
    drop(w);
    let sg4 = read(&base, "sg4");
    assert_eq!(sg4.len(), BLOCK + 40_000);
    assert!(sg4[100_000..BLOCK].iter().all(|&b| b == 0));
}

#[test]
fn compact_games_drops_corrupt_records_silently_but_reports_them() {
    let (_dir, base) = tmp();
    let games = five();
    let w = db_with(&base, &games);
    drop(w);
    let db = Si4Database::open(&base).unwrap();
    let e = db.entry(2).unwrap();
    drop(db);
    let mut sg4 = read(&base, "sg4");
    for b in &mut sg4[e.offset as usize..(e.offset + e.length) as usize] {
        *b = 0xFF;
    }
    std::fs::write(ext(&base, "sg4"), sg4).unwrap();

    let w = Si4Writer::open(&base).unwrap();
    let (w, report) = compact_games(w).unwrap();
    drop(w);

    assert_eq!((report.kept, report.corrupt, report.deleted), (4, 1, 0));
    let back = db_games(&base);
    assert_eq!(back.len(), 4);
    for (got, want) in back.iter().zip([0, 2, 3, 4]) {
        assert!(games[want].same_content(got));
    }
}

#[test]
fn compact_games_corrects_num_half_moves_and_copies_the_rest_raw() {
    let (_dir, base) = tmp();
    let games = [game_ucis(&["e2e4", "e7e5", "g1f3"]), game_ucis(&["d2d4"])];
    drop(db_with(&base, &games));
    // Break the stored value of game 1 (byte 37) and keep the pawn-count bits.
    let mut si4 = read(&base, "si4");
    let at = HEADER_LEN + ent_off(1);
    let good = si4[at + 37];
    si4[at + 37] = 99;
    std::fs::write(ext(&base, "si4"), &si4).unwrap();
    let old_entry = entry_bytes(&base, 1);

    let (w, report) = compact_games(Si4Writer::open(&base).unwrap()).unwrap();
    drop(w);

    assert_eq!(report.half_moves_fixed, 1);
    let new_entry = entry_bytes(&base, 1);
    assert_eq!(new_entry[37], good, "corrected to the real value (3 plies)");
    assert_eq!(good, 3);
    // everything else is copied raw (offset stays 0 here)
    let mut expected = old_entry;
    expected[37] = good;
    assert_eq!(new_entry, expected);
    assert_eq!(report.kept, 2);
}

#[test]
fn compact_games_removes_the_stc_cache_and_returns_a_working_writer() {
    let (_dir, base) = tmp();
    let mut w = db_with(&base, &five());
    w.set_flag(1, GameFlags::DELETE, FlagOp::Set).unwrap();
    std::fs::write(ext(&base, "stc"), b"cache").unwrap();
    let (mut w, _) = compact_games(w).unwrap();
    assert!(!ext(&base, "stc").exists());
    assert_eq!(w.add_game(&Game::new(), GameFlags::empty()).unwrap(), 5);
    assert!(
        matches!(Si4Writer::open(&base), Err(Si4Error::AlreadyOpen { .. })),
        "lock is held"
    );
}

#[test]
fn compact_games_with_nothing_to_drop_keeps_every_game_and_entry() {
    let (_dir, base) = tmp();
    let games = five();
    drop(db_with(&base, &games));
    let si4 = read(&base, "si4");
    let (w, report) = compact_games(Si4Writer::open(&base).unwrap()).unwrap();
    drop(w);
    assert_eq!((report.kept, report.deleted, report.corrupt), (5, 0, 0));
    assert_eq!(read(&base, "si4"), si4);
    for (g, back) in games.iter().zip(db_games(&base)) {
        assert!(g.same_content(&back));
    }
}

#[test]
fn compact_games_of_an_empty_database_and_of_all_deleted_games() {
    let (_dir, base) = tmp();
    let (w, r) = compact_games(Si4Writer::create(&base, 1).unwrap()).unwrap();
    assert_eq!((r.kept, r.deleted), (0, 0));
    drop(w);
    assert_eq!(read(&base, "sg4").len(), 0);

    let (_d2, base2) = tmp();
    let mut w = db_with(&base2, &five());
    for n in 1..=5 {
        w.set_flag(n, GameFlags::DELETE, FlagOp::Set).unwrap();
    }
    let (w, r) = compact_games(w).unwrap();
    assert_eq!(r.deleted, 5);
    drop(w);
    assert_eq!(Si4Database::open(&base2).unwrap().len(), 0);
    assert_eq!(read(&base2, "sg4").len(), 0);
    assert_eq!(read(&base2, "si4").len(), HEADER_LEN);
}

// ---------------------------------------------------------------- compact names

fn names_db(base: &Path) -> Si4Writer {
    let mut w = db_with(base, &five());
    // W1 and its game's round become unused.
    w.replace_game(2, &named("W9", "B1", "Ev", "Si", "1"))
        .unwrap();
    w
}

#[test]
fn compact_names_removes_unused_names_and_renumbers_preserving_order() {
    let (_dir, base) = tmp();
    let w = names_db(&base);
    let before = db_games(&base_of_open(w, &base));
    let (w, report) = compact_names(Si4Writer::open(&base).unwrap()).unwrap();

    assert!(report.changed);
    assert_eq!(report.removed, [1, 0, 0, 0], "only W1 lost its last game");
    drop(w);

    let sn4 = parse_sn4(&read(&base, "sn4"));
    for t in 0..4 {
        assert!(
            sn4.sections[t].iter().all(|r| r.freq > 0),
            "no unused name left"
        );
        let mut ids: Vec<u64> = sn4.sections[t].iter().map(|r| r.id).collect();
        ids.sort_unstable();
        assert!(
            ids.iter().enumerate().all(|(i, &id)| id == i as u64),
            "dense ids"
        );
    }
    // relative order of the old ids is kept: W0 < W2 < W3 < W4 < W9 (insertion order)
    let id = |n: &[u8]| sn4.by_name(0, n).unwrap().id;
    assert!(
        id(b"W0") < id(b"W2")
            && id(b"W2") < id(b"W3")
            && id(b"W3") < id(b"W4")
            && id(b"W4") < id(b"W9")
    );
    // games still read the same
    let after = db_games(&base);
    assert_eq!(before.len(), after.len());
    for (a, b) in before.iter().zip(&after) {
        assert!(a.same_content(b));
    }
}

/// Drops the writer and reads the games (the writer must not live on Windows).
fn base_of_open(w: Si4Writer, base: &Path) -> std::path::PathBuf {
    drop(w);
    base.to_path_buf()
}

#[test]
fn compact_names_sets_max_frequency_to_the_real_maximum_and_time_stamp_to_zero() {
    let (_dir, base) = tmp();
    let mut w = Si4Writer::create(&base, 1).unwrap();
    for i in 0..3 {
        w.add_game(
            &named("X", &format!("B{i}"), "E", "S", &i.to_string()),
            GameFlags::empty(),
        )
        .unwrap();
    }
    // raises the marker to 4 transiently; leaves "Y" with frequency 1 and "B1" unused
    w.replace_game(1, &named("X", "B0", "E", "S", "0")).unwrap();
    w.replace_game(2, &named("Y", "B0", "E", "S", "1")).unwrap();
    drop(w);
    let mut sn4 = read(&base, "sn4");
    sn4[8..12].copy_from_slice(&0xABCD_EF01u32.to_be_bytes());
    std::fs::write(ext(&base, "sn4"), &sn4).unwrap();
    let marker = parse_sn4(&read(&base, "sn4")).max_freq[0];
    assert!(marker >= 4, "historic marker {marker}");

    let (w, report) = compact_names(Si4Writer::open(&base).unwrap()).unwrap();
    assert!(report.changed);
    drop(w);
    let after = parse_sn4(&read(&base, "sn4"));
    let real_max = after
        .sections
        .iter()
        .flatten()
        .map(|r| r.freq)
        .max()
        .unwrap();
    assert_eq!(after.max_freq.iter().copied().max().unwrap(), real_max);
    assert_eq!(after.max_freq[0], 2, "X and B0 are used by two games");
    assert_eq!(after.time_stamp, 0);
}

#[test]
fn compact_names_does_nothing_when_every_name_is_used() {
    let (_dir, base) = tmp();
    drop(db_with(&base, &five()));
    let files = (read(&base, "si4"), read(&base, "sn4"), read(&base, "sg4"));
    let (w, report) = compact_names(Si4Writer::open(&base).unwrap()).unwrap();
    assert!(!report.changed);
    assert_eq!(report.removed, [0; 4]);
    drop(w);
    assert_eq!(
        files,
        (read(&base, "si4"), read(&base, "sn4"), read(&base, "sg4"))
    );
}

#[test]
fn compact_names_leaves_games_header_and_record_file_untouched() {
    let (_dir, base) = tmp();
    let mut w = names_db(&base);
    w.set_description("keep me").unwrap();
    w.set_flag(1, GameFlags::USER, FlagOp::Set).unwrap();
    drop(w);
    let sg4 = read(&base, "sg4");
    let si4 = read(&base, "si4");
    let (w, _) = compact_names(Si4Writer::open(&base).unwrap()).unwrap();
    drop(w);
    assert_eq!(read(&base, "sg4"), sg4);
    let after = read(&base, "si4");
    assert_eq!(&after[..HEADER_LEN], &si4[..HEADER_LEN], "header untouched");
    // only name ids may differ in the entries
    for n in 1..=5u32 {
        let a = &si4[HEADER_LEN + ent_off(n)..][..47];
        let b = &after[HEADER_LEN + ent_off(n)..][..47];
        assert_eq!(&a[..9], &b[..9], "offset/length/flags of game {n}");
        assert_eq!(&a[21..], &b[21..], "everything after the ids of game {n}");
    }
}

#[test]
fn name_frequencies_after_compact_names_match_the_entries() {
    let (_dir, base) = tmp();
    let w = names_db(&base);
    let (w, _) = compact_names(w).unwrap();
    for kind in NameKind::ALL {
        for id in 0..w.names().num_names(kind) {
            assert!(w.names().frequency(kind, id).unwrap() > 0);
        }
    }
}

// ---------------------------------------------------------------- tolerance

#[test]
fn trailing_partial_or_full_entry_after_the_counted_games_is_tolerated() {
    for extra in [23usize, 47] {
        let (_dir, base) = tmp();
        drop(db_with(&base, &five()[..2]));
        let mut si4 = read(&base, "si4");
        let tail = si4[si4.len() - 47..].to_vec();
        si4.extend_from_slice(&tail[..extra]);
        std::fs::write(ext(&base, "si4"), &si4).unwrap();

        assert_eq!(Si4Database::open(&base).unwrap().len(), 2, "extra {extra}");
        let mut w = Si4Writer::open(&base).unwrap();
        assert_eq!(w.len(), 2);
        let g = game_ucis(&["c2c4"]);
        assert_eq!(w.add_game(&g, GameFlags::empty()).unwrap(), 3);
        drop(w);
        assert_eq!(read(&base, "si4").len(), HEADER_LEN + 3 * 47);
        assert!(g.same_content(&db_games(&base)[2]));
    }
}

// ---------------------------------------------------------------- writer fail points

fn fail_cases() -> [&'static str; 2] {
    ["write:entries", "write:header"]
}

#[test]
fn failed_add_leaves_the_old_state_on_disk_and_in_the_session() {
    for label in fail_cases() {
        let (_dir, base) = tmp();
        let games = five();
        let mut w = db_with(&base, &games[..2]);
        let (sn4, sg4_len) = (read(&base, "sn4"), read(&base, "sg4").len());
        let header = read(&base, "si4")[..HEADER_LEN].to_vec();

        w.set_fail_point(FailPoint::at_label(label));
        let err = w
            .add_game(
                &named("Ghost", "Ghost2", "GhostEv", "S", "7"),
                GameFlags::empty(),
            )
            .unwrap_err();
        assert!(
            matches!(err, Si4Error::InjectedFailure { .. }),
            "{label}: {err:?}"
        );

        // session: consistent
        assert_eq!(w.len(), 2, "{label}");
        assert_eq!(w.names().find(NameKind::Player, b"Ghost"), None, "{label}");
        assert_eq!(w.names().num_names(NameKind::Event), 1, "{label}");
        // disk: the header still counts 2 games and the names are the old ones
        assert_eq!(read(&base, "si4")[..HEADER_LEN], header[..], "{label}");
        assert_eq!(read(&base, "sn4"), sn4, "{label}: .sn4 restored");
        assert!(read(&base, "sg4").len() >= sg4_len);

        // the same session keeps working and the ghost never shows up
        w.set_fail_point(FailPoint::never());
        let retry = game_ucis(&["c2c4"]);
        assert_eq!(
            w.add_game(&retry, GameFlags::empty()).unwrap(),
            3,
            "{label}"
        );
        drop(w);
        let back = db_games(&base);
        assert_eq!(back.len(), 3);
        assert!(back[2].same_content(&retry));
        let names = parse_sn4(&read(&base, "sn4"));
        assert!(names.by_name(0, b"Ghost").is_none(), "{label}");
        assert!(names.by_name(1, b"GhostEv").is_none(), "{label}");
    }
}

#[test]
fn failed_replace_keeps_the_old_game_and_the_old_frequencies() {
    // replace has no header stage: only "write:entries" exists
    for label in ["write:entries"] {
        let (_dir, base) = tmp();
        let games = five();
        let mut w = db_with(&base, &games[..3]);
        let sn4 = read(&base, "sn4");
        let freq = w.names().frequency(NameKind::Player, 0);

        w.set_fail_point(FailPoint::at_label(label));
        let err = w
            .replace_game(1, &named("Ghost", "Ghost2", "E", "S", "9"))
            .unwrap_err();
        assert!(
            matches!(err, Si4Error::InjectedFailure { .. }),
            "{label}: {err:?}"
        );

        assert_eq!(w.len(), 3);
        assert_eq!(w.names().frequency(NameKind::Player, 0), freq, "{label}");
        assert_eq!(read(&base, "sn4"), sn4, "{label}");
        drop(w);
        let back = db_games(&base);
        // "write:entries" fails before the entry write: the old game stays.
        assert!(games[0].same_content(&back[0]), "{label}");
        assert!(games[1].same_content(&back[1]));
    }
}

#[test]
fn fail_point_records_the_stages_it_saw() {
    let (_dir, base) = tmp();
    let mut w = Si4Writer::create(&base, 1).unwrap();
    w.set_fail_point(FailPoint::never());
    w.add_game(&Game::new(), GameFlags::empty()).unwrap();
    let seen: Vec<&str> = w
        .fail_point()
        .unwrap()
        .reached()
        .iter()
        .map(String::as_str)
        .collect();
    assert_eq!(seen, ["write:entries", "write:header"]);
}
