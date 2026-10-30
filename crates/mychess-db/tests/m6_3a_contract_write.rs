//! Contract tests of the name scan, `apply_name_changes`, `set_elo_batch` and
//! "Add Elo" (M6-3a): rules 37 to 59 and 65 to 74 of
//! `docs/specs/m6-3-jogadores-torneios-spelling.md`. Synthetic data only.
#![allow(clippy::unwrap_used, clippy::too_many_lines)]

#[path = "common/m6_3a_support.rs"]
mod support;

use std::path::Path;

use mychess_core::{GameResult, PartialDate, RatingType};
use mychess_db::si4::{
    FailPoint, FlagOp, GameFlags, NAME_MAX_ID, NameChange, NameKind, Si4Database, Si4Error,
    Si4Writer, life_span_allows,
};
use mychess_db::spelling::apply::{
    changes_from_proposals, parse_corrections_text, proposals_to_text,
};
use mychess_db::spelling::check::{Proposal, ScanOptions, ScanResult, is_surname_only, scan_names};
use mychess_db::spelling::elo::{AddEloError, EloOptions, apply_add_elo, plan_add_elo};
use mychess_db::{CancelToken, Filter, NoProgress};
use pretty_assertions::assert_eq;
use rstest::rstest;
use support::{G, build, date, g, games_bytes, index_bytes, names_bytes, open, read};

const W: GameResult = GameResult::White;
const HEADER: usize = 182;
const ENTRY: usize = 47;

fn freq(db: &Si4Database<std::fs::File>, kind: NameKind, name: &str) -> Option<u32> {
    (0..db.names().num_names(kind))
        .find(|&id| db.names().get(kind, id) == Some(name.as_bytes()))
        .and_then(|id| db.names().frequency(kind, id))
}

fn recount(db: &Si4Database<std::fs::File>, kind: NameKind) -> Vec<u32> {
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

fn name_of(db: &Si4Database<std::fs::File>, kind: NameKind, id: u32) -> String {
    String::from_utf8(db.names().get(kind, id).unwrap().to_vec()).unwrap()
}

fn apply(base: &Path, kind: NameKind, changes: &[NameChange]) -> mychess_db::si4::NameChangeReport {
    let mut w = Si4Writer::open(base).unwrap();
    let report = w
        .apply_name_changes(kind, changes, &CancelToken::new(), &mut NoProgress)
        .unwrap();
    drop(w);
    report
}

// ---------------------------------------------------------------------------
// Rule 53: life spans (public predicate).
// ---------------------------------------------------------------------------

fn bits(text: &str) -> u32 {
    date(text).to_scid_u32()
}

#[rstest]
#[case::unknown_date_always_passes("0000.00.00", "1900.01.01", "1950.01.01", true)]
#[case::inside("1920.06.15", "1900.01.01", "1950.01.01", true)]
#[case::before_birth("1899.12.31", "1900.01.01", "1950.01.01", false)]
#[case::on_birth_day("1900.01.01", "1900.01.01", "1950.01.01", true)]
#[case::after_death("1950.01.02", "1900.01.01", "1950.01.01", false)]
#[case::on_death_day("1950.01.01", "1900.01.01", "1950.01.01", true)]
#[case::birth_year_only_accepts_the_whole_year("1904.00.00", "1904.??.??", "????.??.??", true)]
#[case::birth_year_only_accepts_dated_games("1904.01.01", "1904.??.??", "????.??.??", true)]
#[case::birth_year_only_rejects_the_year_before("1903.12.31", "1904", "????", false)]
#[case::death_year_only_keeps_the_whole_year("1979.12.31", "????", "1979", true)]
#[case::death_year_only_rejects_next_year("1980.01.01", "????", "1979", false)]
#[case::death_month_only_keeps_the_month("1979.08.31", "????", "1979.08.??", true)]
#[case::death_month_only_rejects_next_month("1979.09.01", "????", "1979.08.??", false)]
#[case::no_limits("2030.01.01", "????", "????", true)]
#[case::game_with_only_a_year_before_the_death_year("1979.00.00", "????", "1979.05.05", true)]
#[case::game_with_only_a_year_in_the_birth_year("1900.00.00", "1900.06.06", "????", false)]
fn life_span_rule(
    #[case] game: &str,
    #[case] born: &str,
    #[case] died: &str,
    #[case] allowed: bool,
) {
    assert_eq!(
        life_span_allows(bits(game), date(born), date(died)),
        allowed
    );
}

// ---------------------------------------------------------------------------
// Rules 37 to 47: the scan.
// ---------------------------------------------------------------------------

const SSP: &str = "@PLAYER \"., -_*\"
%Prefix \"Mr. \" \"\"
%Suffix \" Jr\" \"\"
Teles, Marta Luz #GM POR [2400] 1950.03.04--2010.05.06
  = Teles, M
  = Teles, Marta
Teles, Mario #- POR [2000] 1960
Ramos, Rita #- ESP [2100] 1970
  = Rita Ramos
Silva #- BRA [2000]
@SITE \"., -_()\"
%Suffix \"(Spain)\" \"ESP\"
Madrid ESP
  = Madryd ESP
Toledo ESP
@EVENT \",. -_\"
%Infix \"-ch \" \" Ch \"
Open Ch
@ROUND \"\"
1
  = (01)
";

fn scan_games() -> Vec<G> {
    let mut games = Vec::new();
    for (name, times) in [
        ("Teles, M", 2),
        ("Teles, Mar", 1),
        ("Mr. Ramos, Rita", 1),
        ("Ramos, Rita Jr", 1),
        ("Rita Ramos", 1),
        ("Marta Luz Teles", 1),
        ("Sil", 1),
        ("Ramos, Rita (ESP)", 1),
    ] {
        for _ in 0..times {
            games.push(g(name, "Quux, Q", W).on("E", "S", "????.??.??"));
        }
    }
    games[0] = games[0]
        .clone()
        .on("Open-ch 2020", "Madrid (Spain)", "????.??.??")
        .round("(01)");
    games[1] = games[1].clone().on("E", "Toledo", "????.??.??");
    games
}

fn scan(base: &Path, opts: ScanOptions) -> Vec<Proposal> {
    let db = open(base);
    let set = read(SSP);
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
        .map(|p| (&*p.old, &*p.new, p.games, p.ambiguous))
        .collect()
}

#[test]
fn scan_default_options_propose_only_unambiguous_non_surname_names() {
    let dir = tempfile::tempdir().unwrap();
    let base = build(dir.path(), &scan_games());
    let before = (index_bytes(&base), names_bytes(&base), games_bytes(&base));
    let found = scan(&base, ScanOptions::new(NameKind::Player));
    assert_eq!(
        pairs(&found),
        [
            ("Marta Luz Teles", "Teles, Marta Luz", 1, false),
            ("Mr. Ramos, Rita", "Ramos, Rita", 1, false),
            ("Ramos, Rita (ESP)", "Ramos, Rita", 1, false),
            ("Ramos, Rita Jr", "Ramos, Rita", 1, false),
            ("Rita Ramos", "Ramos, Rita", 1, false),
            ("Teles, M", "Teles, Marta Luz", 2, false),
        ],
        "promoted surname, prefix rule, country in parentheses, suffix rule, alias, exact alias"
    );
    // Rule 47: the scan never writes.
    assert_eq!(
        before,
        (index_bytes(&base), names_bytes(&base), games_bytes(&base))
    );
}

#[test]
fn scan_dates_come_from_the_exact_comment_of_the_new_name() {
    let dir = tempfile::tempdir().unwrap();
    let base = build(dir.path(), &scan_games());
    let found = scan(&base, ScanOptions::new(NameKind::Player));
    let teles = found.iter().find(|p| p.old == "Teles, M").unwrap();
    assert_eq!(teles.born.to_string(), "1950.03.04");
    assert_eq!(teles.died.to_string(), "2010.05.06");
    let ramos = found.iter().find(|p| p.old == "Rita Ramos").unwrap();
    assert_eq!(ramos.born.to_string(), "1970.??.??");
    assert_eq!(ramos.died.to_string(), "????.??.??");
}

#[test]
fn scan_ambiguous_candidates_are_proposed_only_when_asked_and_marked() {
    let dir = tempfile::tempdir().unwrap();
    let base = build(dir.path(), &scan_games());
    let mut opts = ScanOptions::new(NameKind::Player);
    opts.ambiguous = true;
    let found = scan(&base, opts);
    let ambiguous: Vec<_> = found.iter().filter(|p| p.ambiguous).collect();
    assert_eq!(
        pairs(&found)
            .iter()
            .filter(|p| p.3)
            .map(|p| (p.0, p.1))
            .collect::<Vec<_>>(),
        [
            ("Teles, Mar", "Teles, Mario"),
            ("Teles, Mar", "Teles, Marta Luz")
        ],
        "scan order of the Scid: the candidate of the highest file index first"
    );
    assert_eq!(ambiguous.len(), 2);
    assert!(
        found
            .iter()
            .filter(|p| !p.ambiguous)
            .all(|p| p.old != "Teles, Mar")
    );
    // Ambiguous proposals are never applied.
    assert!(
        changes_from_proposals(&found)
            .iter()
            .all(|c| c.old != "Teles, Mar")
    );
}

#[test]
fn scan_surnames_option_checks_names_that_are_only_a_surname() {
    let dir = tempfile::tempdir().unwrap();
    let base = build(dir.path(), &scan_games());
    assert!(
        scan(&base, ScanOptions::new(NameKind::Player))
            .iter()
            .all(|p| p.old != "Sil")
    );
    let mut opts = ScanOptions::new(NameKind::Player);
    opts.surnames = true;
    let found = scan(&base, opts);
    assert_eq!(
        pairs(&found).into_iter().find(|p| p.0 == "Sil"),
        Some(("Sil", "Silva", 1, false))
    );
}

#[test]
fn scan_never_proposes_the_name_it_already_has() {
    let dir = tempfile::tempdir().unwrap();
    let games = vec![
        g("Ramos, Rita", "Teles, Marta Luz", W),
        g("Teles, Mario", "Silva", W),
    ];
    let base = build(dir.path(), &games);
    let mut opts = ScanOptions::new(NameKind::Player);
    opts.surnames = true;
    opts.ambiguous = true;
    let found = scan(&base, opts);
    assert!(found.iter().all(|p| p.old != p.new), "{found:?}");
    assert!(
        found
            .iter()
            .all(|p| p.old != "Ramos, Rita" && p.old != "Teles, Marta Luz"),
        "an exact correct name is left alone"
    );
}

#[rstest]
#[case::space_and_three_capitals("Ramos, Rita (ESP)", true)]
#[case::six_characters_are_too_short(" (ESP)", false)]
#[case::lowercase_letters("Ramos, Rita (Esp)", false)]
#[case::no_space_before("Ramos, Rita(ESP)", false)]
#[case::four_letters("Ramos, Rita (ESPA)", false)]
#[case::only_two_letters("Ramos, Rita (ES)", false)]
#[case::digits("Ramos, Rita (E5P)", false)]
#[case::not_at_the_end("Ramos, Rita (ESP) Jr", false)]
fn scan_country_in_parentheses_is_removed_only_in_its_exact_form(
    #[case] name: &str,
    #[case] proposed: bool,
) {
    let dir = tempfile::tempdir().unwrap();
    let base = build(dir.path(), &[g(name, "Quux, Q", W)]);
    let mut opts = ScanOptions::new(NameKind::Player);
    opts.surnames = true;
    let found = scan(&base, opts);
    let hit = found.iter().find(|p| p.old == name);
    // "Ramos, Rita Jr" style rules aside, only the exact form maps to the correct name.
    if proposed {
        assert_eq!(hit.map(|p| &*p.new), Some("Ramos, Rita"), "{name}");
    } else {
        assert!(
            hit.is_none_or(|p| p.new != "Ramos, Rita") || name.ends_with(" Jr"),
            "{name}: {hit:?}"
        );
    }
}

#[test]
fn scan_events_sites_and_rounds_use_rules_aliases_and_prefix_search() {
    let dir = tempfile::tempdir().unwrap();
    let base = build(dir.path(), &scan_games());
    let site = scan(&base, ScanOptions::new(NameKind::Site));
    assert_eq!(
        pairs(&site),
        [
            ("Madrid (Spain)", "Madrid ESP", 1, false),
            ("Toledo", "Toledo ESP", 1, false),
        ]
    );
    let event = scan(&base, ScanOptions::new(NameKind::Event));
    assert_eq!(pairs(&event), [("Open-ch 2020", "Open Ch 2020", 1, false)]);
    let round = scan(&base, ScanOptions::new(NameKind::Round));
    assert_eq!(pairs(&round), [("(01)", "1", 1, false)]);
    // Dates only exist for players.
    assert!(
        site.iter()
            .all(|p| p.born.is_unknown() && p.died.is_unknown())
    );
}

#[test]
fn scan_with_a_checker_of_another_kind_finds_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let base = build(dir.path(), &scan_games());
    let db = open(&base);
    let set = read(SSP);
    let r: ScanResult = scan_names(
        &db,
        set.checker(NameKind::Site),
        ScanOptions::new(NameKind::Player),
        &CancelToken::new(),
        &mut NoProgress,
    );
    assert_eq!(r.proposals.len(), 0);
    assert!(!r.interrupted);
}

#[test]
fn scan_skips_names_no_game_uses_even_after_a_correction_left_them_behind() {
    let dir = tempfile::tempdir().unwrap();
    let base = build(dir.path(), &scan_games());
    let found = scan(&base, ScanOptions::new(NameKind::Player));
    let changes = changes_from_proposals(&found);
    apply(&base, NameKind::Player, &changes);
    // The old names stay in the .sn4 with frequency 0 (rule 55) and must not
    // be proposed again; the second scan has nothing left to propose.
    let again = scan(&base, ScanOptions::new(NameKind::Player));
    assert_eq!(pairs(&again).len(), 0, "{again:?}");
    let db = open(&base);
    assert_eq!(freq(&db, NameKind::Player, "Teles, M"), Some(0));
}

#[test]
fn scan_counts_games_from_the_index_not_from_the_stored_frequency() {
    // Rule 38. A failure after the index was rewritten leaves the old
    // frequencies in the .sn4: "Teles, M" still says 2 games but has none.
    let dir = tempfile::tempdir().unwrap();
    let base = build(dir.path(), &scan_games());
    let mut w = Si4Writer::open(&base).unwrap();
    w.set_fail_point(FailPoint::at_label("names:final"));
    assert!(
        w.apply_name_changes(
            NameKind::Player,
            &[NameChange::new("Teles, M", "Teles, Marta Luz")],
            &CancelToken::new(),
            &mut NoProgress,
        )
        .is_err()
    );
    drop(w);
    let found = scan(&base, ScanOptions::new(NameKind::Player));
    assert!(
        found.iter().all(|p| p.old != "Teles, M"),
        "a name no game uses is not proposed: {found:?}"
    );
    let marta = open(&base);
    assert_eq!(
        recount(&marta, NameKind::Player)
            .iter()
            .enumerate()
            .filter(
                |(i, _)| name_of(&marta, NameKind::Player, u32::try_from(*i).unwrap())
                    == "Teles, M"
            )
            .map(|(_, c)| *c)
            .sum::<u32>(),
        0
    );
}

#[test]
fn scan_progress_is_monotonic_and_ends_at_the_total() {
    let dir = tempfile::tempdir().unwrap();
    let games: Vec<G> = (0..20_000).map(|_| g("Teles, M", "Quux, Q", W)).collect();
    let base = build(dir.path(), &games);
    let db = open(&base);
    let set = read(SSP);
    let mut calls: Vec<(u64, u64)> = Vec::new();
    let r = scan_names(
        &db,
        set.checker(NameKind::Player),
        ScanOptions::new(NameKind::Player),
        &CancelToken::new(),
        &mut |d, t| calls.push((d, t)),
    );
    assert_eq!(r.proposals[0].games, 20_000);
    assert!(calls.windows(2).all(|w| w[0].0 <= w[1].0));
    assert!(calls.iter().all(|&(d, t)| d <= t));
    assert_eq!(calls.last().map(|c| c.0 == c.1), Some(true));
}

#[test]
fn scan_text_form_round_trips_through_the_reader_of_the_scid() {
    let dir = tempfile::tempdir().unwrap();
    let base = build(dir.path(), &scan_games());
    let mut opts = ScanOptions::new(NameKind::Player);
    opts.ambiguous = true;
    let found = scan(&base, opts);
    let text = proposals_to_text(NameKind::Player, &found);
    assert!(text.starts_with(&format!(
        "Scid found {} player name corrections.\n",
        found.len()
    )));
    assert!(text.contains("Ambiguous: \"Teles, Mar\"\t>> \"Teles, Mario\" (1)"));
    // Groups of different initial are separated by a blank line.
    assert!(text.contains("\n\n\"Ramos, Rita (ESP)\""), "{text}");
    let back = parse_corrections_text(&text);
    assert_eq!(
        back,
        changes_from_proposals(&found),
        "ambiguous lines are not read back"
    );
    // The user can remove a line or indent it to skip it.
    let edited = text.replace("\"Teles, M\"\t", " \"Teles, M\"\t");
    assert!(
        parse_corrections_text(&edited)
            .iter()
            .all(|c| c.old != "Teles, M")
    );
}

#[rstest]
#[case("Kasparov", true)]
#[case("kasparov", true)]
#[case("", true)]
#[case("KASPAROV", false)]
#[case("McDonald", false)]
#[case("Van der Wiel", false)]
#[case("Müller", false)]
#[case("O'Neil", false)]
#[case("Smith, J", false)]
#[case("Smith2", false)]
#[case("Ab", true)]
#[case("aB", true)]
#[case("AB", false)]
fn surname_only(#[case] name: &str, #[case] expected: bool) {
    assert_eq!(is_surname_only(name), expected, "{name:?}");
}

// ---------------------------------------------------------------------------
// Rules 48 to 59: apply_name_changes.
// ---------------------------------------------------------------------------

fn three_games() -> Vec<G> {
    vec![
        g("Old, A", "Other, O", W).on("E", "S", "1945.05.05"),
        g("Other, O", "Old, A", W).on("E", "S", "1955.05.05"),
        g("Old, A", "Old, A", W).on("E", "S", "????.??.??"),
    ]
}

#[test]
fn applying_changes_both_sides_of_one_game_and_counts_each_side() {
    let dir = tempfile::tempdir().unwrap();
    let base = build(dir.path(), &three_games());
    let r = apply(
        &base,
        NameKind::Player,
        &[NameChange::new("Old, A", "New, A")],
    );
    assert_eq!(
        (r.mapped, r.games_changed),
        (1, 4),
        "games 1, 2 and both sides of 3"
    );
    let db = open(&base);
    assert_eq!(freq(&db, NameKind::Player, "Old, A"), Some(0));
    assert_eq!(freq(&db, NameKind::Player, "New, A"), Some(4));
    assert_eq!(freq(&db, NameKind::Player, "Other, O"), Some(2));
}

#[test]
fn applying_changes_deleted_games_and_keeps_every_flag() {
    let dir = tempfile::tempdir().unwrap();
    let base = build(dir.path(), &three_games());
    let mut w = Si4Writer::open(&base).unwrap();
    w.set_flag(2, GameFlags::DELETE, FlagOp::Set).unwrap();
    w.set_flag(1, GameFlags::WHITE_OPENING, FlagOp::Set)
        .unwrap();
    drop(w);
    let before = index_bytes(&base);
    let r = apply(
        &base,
        NameKind::Player,
        &[NameChange::new("Old, A", "New, A")],
    );
    assert_eq!(r.games_changed, 4);
    let db = open(&base);
    assert!(db.entry(2).unwrap().flags.contains(GameFlags::DELETE));
    assert!(
        db.entry(1)
            .unwrap()
            .flags
            .contains(GameFlags::WHITE_OPENING)
    );
    assert_eq!(
        name_of(&db, NameKind::Player, db.entry(2).unwrap().black_id),
        "New, A"
    );
    // Only bytes 9..=20 of the entries (the name IDs) differ.
    let after = index_bytes(&base);
    for (i, (a, b)) in before.iter().zip(&after).enumerate() {
        if i >= HEADER && matches!((i - HEADER) % ENTRY, 9..=20) {
            continue;
        }
        assert_eq!(a, b, "byte {i}");
    }
}

#[test]
fn white_and_black_are_tested_separately_against_the_life_span() {
    let dir = tempfile::tempdir().unwrap();
    let games = vec![
        g("Old1", "Old2", W).on("E", "S", "1945.05.05"),
        g("Old1", "Old2", W).on("E", "S", "????.??.??"),
    ];
    let base = build(dir.path(), &games);
    let mut one = NameChange::new("Old1", "New1");
    one.born = date("1900.??.??");
    one.died = date("1950.??.??");
    let mut two = NameChange::new("Old2", "New2");
    two.born = date("1960.??.??");
    let r = apply(&base, NameKind::Player, &[one, two]);
    assert_eq!(r.mapped, 2);
    assert_eq!(r.skipped_by_date, 1, "Old2 was born after the 1945 game");
    assert_eq!(r.games_changed, 3, "game 1 White; game 2 both sides");
    let db = open(&base);
    let e1 = db.entry(1).unwrap();
    assert_eq!(name_of(&db, NameKind::Player, e1.white_id), "New1");
    assert_eq!(name_of(&db, NameKind::Player, e1.black_id), "Old2");
    let e2 = db.entry(2).unwrap();
    assert_eq!(
        name_of(&db, NameKind::Player, e2.black_id),
        "New2",
        "unknown date passes"
    );
}

#[test]
fn the_life_span_never_limits_events_sites_or_rounds() {
    let dir = tempfile::tempdir().unwrap();
    let base = build(dir.path(), &[g("A", "B", W).on("Old", "S", "2020.01.01")]);
    let mut change = NameChange::new("Old", "New");
    change.born = date("2100");
    let r = apply(&base, NameKind::Event, &[change]);
    assert_eq!((r.games_changed, r.skipped_by_date), (1, 0));
}

#[test]
fn the_last_change_for_an_old_name_wins() {
    let dir = tempfile::tempdir().unwrap();
    let base = build(dir.path(), &three_games());
    let r = apply(
        &base,
        NameKind::Player,
        &[
            NameChange::new("Old, A", "First, F"),
            NameChange::new("Old, A", "Second, S"),
        ],
    );
    assert_eq!(r.mapped, 1);
    let db = open(&base);
    assert_eq!(freq(&db, NameKind::Player, "Second, S"), Some(4));
    assert_eq!(freq(&db, NameKind::Player, "First, F").unwrap_or(0), 0);
}

#[test]
fn changes_do_not_chain_within_one_application() {
    let dir = tempfile::tempdir().unwrap();
    let games = vec![g("A", "Z", W), g("B", "Z", W)];
    let base = build(dir.path(), &games);
    apply(
        &base,
        NameKind::Player,
        &[NameChange::new("A", "B"), NameChange::new("B", "C")],
    );
    let db = open(&base);
    let first = db.entry(1).unwrap();
    let second = db.entry(2).unwrap();
    assert_eq!(
        name_of(&db, NameKind::Player, first.white_id),
        "B",
        "A -> B only"
    );
    assert_eq!(name_of(&db, NameKind::Player, second.white_id), "C");
}

#[test]
fn a_change_to_the_same_name_or_to_an_empty_one_is_no_change_at_all() {
    let dir = tempfile::tempdir().unwrap();
    let base = build(dir.path(), &three_games());
    let before = (index_bytes(&base), names_bytes(&base));
    let r = apply(
        &base,
        NameKind::Player,
        &[
            NameChange::new("Old, A", "Old, A"),
            NameChange::new("Other, O", ""),
        ],
    );
    assert_eq!((r.mapped, r.games_changed, r.names_not_found), (0, 0, 0));
    assert_eq!(before, (index_bytes(&base), names_bytes(&base)));
}

#[test]
fn old_names_that_do_not_exist_are_counted_and_the_rest_still_applies() {
    let dir = tempfile::tempdir().unwrap();
    let base = build(dir.path(), &three_games());
    let r = apply(
        &base,
        NameKind::Player,
        &[
            NameChange::new("Nobody", "Somebody"),
            NameChange::new("old, a", "Case"),
            NameChange::new("Other, O", "Other, Oscar"),
        ],
    );
    assert_eq!((r.names_not_found, r.mapped, r.games_changed), (2, 1, 2));
    let db = open(&base);
    assert_eq!(
        freq(&db, NameKind::Player, "Somebody"),
        None,
        "no name is invented"
    );
    assert_eq!(freq(&db, NameKind::Player, "Case"), None);
}

#[test]
fn nothing_valid_leaves_every_file_and_the_cache_alone() {
    let dir = tempfile::tempdir().unwrap();
    let base = build(dir.path(), &three_games());
    let stc = base.with_extension("stc");
    std::fs::write(&stc, b"cache").unwrap();
    let before = (index_bytes(&base), names_bytes(&base), games_bytes(&base));
    let r = apply(
        &base,
        NameKind::Player,
        &[NameChange::new("Nobody", "Somebody")],
    );
    assert_eq!(r.names_not_found, 1);
    assert_eq!(
        before,
        (index_bytes(&base), names_bytes(&base), games_bytes(&base))
    );
    assert!(
        stc.exists(),
        "nothing changed, the tree cache is still valid"
    );
}

#[test]
fn a_successful_application_removes_the_tree_cache() {
    let dir = tempfile::tempdir().unwrap();
    let base = build(dir.path(), &three_games());
    let stc = base.with_extension("stc");
    std::fs::write(&stc, b"cache").unwrap();
    apply(
        &base,
        NameKind::Player,
        &[NameChange::new("Old, A", "New, A")],
    );
    assert!(!stc.exists());
}

#[test]
fn non_ascii_and_long_names_are_stored_and_found() {
    let dir = tempfile::tempdir().unwrap();
    let base = build(
        dir.path(),
        &[g("José, J", "Quux, Q", W), g("Müller, M", "Quux, Q", W)],
    );
    apply(
        &base,
        NameKind::Player,
        &[
            NameChange::new("José, J", "Jose, J"),
            NameChange::new("Müller, M", "Ä".repeat(400)),
        ],
    );
    let db = open(&base);
    assert_eq!(freq(&db, NameKind::Player, "Jose, J"), Some(1));
    let e = db.entry(2).unwrap();
    let stored = name_of(&db, NameKind::Player, e.white_id);
    assert!(
        stored.len() <= mychess_db::si4::MAX_NAME_LEN,
        "{}",
        stored.len()
    );
    assert!(stored.starts_with('Ä'));
}

#[test]
fn a_full_kind_skips_new_names_but_keeps_the_ones_already_added() {
    let dir = tempfile::tempdir().unwrap();
    let base = build(
        dir.path(),
        &[g("A", "B", W).round("r0"), g("A", "B", W).round("r1")],
    );
    let limit = NAME_MAX_ID[3];
    let changes: Vec<NameChange> = (0..limit + 50)
        .map(|i| NameChange::new("r0", format!("n{i}")))
        .collect();
    let r = apply(&base, NameKind::Round, &changes);
    assert!(r.skipped_full >= 50, "{r:?}");
    assert_eq!((r.mapped, r.games_changed), (1, 1));
    let db = open(&base);
    assert_eq!(db.names().num_names(NameKind::Round), limit);
    assert_eq!(freq(&db, NameKind::Round, "r0"), Some(0));
    // The database still reads and every id is valid.
    for n in 1..=db.len() {
        assert!(db.entry(n).unwrap().round_id < limit);
    }
}

fn list_dir(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[test]
fn failure_before_anything_is_written_leaves_the_files_identical() {
    let dir = tempfile::tempdir().unwrap();
    let base = build(dir.path(), &three_games());
    let before = (index_bytes(&base), names_bytes(&base), games_bytes(&base));
    let listing = list_dir(dir.path());
    let mut w = Si4Writer::open(&base).unwrap();
    w.set_fail_point(FailPoint::at_label("names:superset"));
    let err = w
        .apply_name_changes(
            NameKind::Player,
            &[NameChange::new("Old, A", "New, A")],
            &CancelToken::new(),
            &mut NoProgress,
        )
        .unwrap_err();
    assert!(matches!(err, Si4Error::InjectedFailure { .. }), "{err}");
    drop(w);
    assert_eq!(
        before,
        (index_bytes(&base), names_bytes(&base), games_bytes(&base))
    );
    assert_eq!(
        listing,
        list_dir(dir.path()),
        "no temporary file is left behind"
    );
    // The writer of the next attempt works.
    let r = apply(
        &base,
        NameKind::Player,
        &[NameChange::new("Old, A", "New, A")],
    );
    assert_eq!(r.games_changed, 4);
}

#[test]
fn failure_after_the_new_names_leaves_a_superset_and_the_old_index() {
    let dir = tempfile::tempdir().unwrap();
    let base = build(dir.path(), &three_games());
    let before_index = index_bytes(&base);
    let old_db = open(&base);
    let old_count = old_db.names().num_names(NameKind::Player);
    let old_names: Vec<String> = (0..old_count)
        .map(|i| name_of(&old_db, NameKind::Player, i))
        .collect();
    drop(old_db);
    let listing = list_dir(dir.path());
    let mut w = Si4Writer::open(&base).unwrap();
    w.set_fail_point(FailPoint::at_label("names:entries"));
    assert!(
        w.apply_name_changes(
            NameKind::Player,
            &[NameChange::new("Old, A", "New, A")],
            &CancelToken::new(),
            &mut NoProgress,
        )
        .is_err()
    );
    drop(w);
    assert_eq!(
        index_bytes(&base),
        before_index,
        "the index was not touched"
    );
    assert_eq!(listing, list_dir(dir.path()));
    let db = open(&base);
    assert_eq!(
        db.names().num_names(NameKind::Player),
        old_count + 1,
        "New, A is already there"
    );
    for (i, name) in old_names.iter().enumerate() {
        assert_eq!(
            &name_of(&db, NameKind::Player, u32::try_from(i).unwrap()),
            name,
            "ids are stable"
        );
    }
    // The stored frequencies still describe the index.
    assert_eq!(
        db.names().frequency(NameKind::Player, 0),
        Some(recount(&db, NameKind::Player)[0])
    );
}

#[test]
fn failure_after_the_index_leaves_the_new_index_and_a_readable_names_file() {
    let dir_ok = tempfile::tempdir().unwrap();
    let dir_bad = tempfile::tempdir().unwrap();
    let base_ok = build(dir_ok.path(), &three_games());
    let base_bad = build(dir_bad.path(), &three_games());
    apply(
        &base_ok,
        NameKind::Player,
        &[NameChange::new("Old, A", "New, A")],
    );
    let mut w = Si4Writer::open(&base_bad).unwrap();
    w.set_fail_point(FailPoint::at_label("names:final"));
    assert!(
        w.apply_name_changes(
            NameKind::Player,
            &[NameChange::new("Old, A", "New, A")],
            &CancelToken::new(),
            &mut NoProgress,
        )
        .is_err()
    );
    drop(w);
    let (good, bad) = (index_bytes(&base_ok), index_bytes(&base_bad));
    assert_eq!(
        good[HEADER..],
        bad[HEADER..],
        "every entry was already rewritten"
    );
    let db = open(&base_bad);
    let names = db.names().num_names(NameKind::Player);
    for n in 1..=db.len() {
        let e = db.entry(n).unwrap();
        assert!(e.white_id < names && e.black_id < names);
    }
    assert_eq!(
        list_dir(dir_bad.path()),
        list_dir(dir_ok.path()),
        "no stray temporary file"
    );
}

#[test]
fn cancelling_before_the_index_changes_nothing_but_the_superset() {
    let dir = tempfile::tempdir().unwrap();
    let base = build(dir.path(), &three_games());
    let before_index = index_bytes(&base);
    let token = CancelToken::new();
    token.cancel();
    let mut w = Si4Writer::open(&base).unwrap();
    let r = w
        .apply_name_changes(
            NameKind::Player,
            &[NameChange::new("Old, A", "New, A")],
            &token,
            &mut NoProgress,
        )
        .unwrap();
    drop(w);
    assert!(r.interrupted);
    assert_eq!(r.games_changed, 0);
    assert_eq!(index_bytes(&base), before_index);
    let db = open(&base);
    assert_eq!(freq(&db, NameKind::Player, "Old, A"), Some(4));
}

#[test]
fn stored_frequencies_equal_an_independent_recount_after_every_kind_of_change() {
    let dir = tempfile::tempdir().unwrap();
    let mut games = scan_games();
    games.push(
        g("Ramos, Rita", "Teles, Mario", W)
            .on("Open Ch", "Madrid ESP", "????.??.??")
            .round("1"),
    );
    let base = build(dir.path(), &games);
    for kind in NameKind::ALL {
        let found = scan(&base, ScanOptions::new(kind));
        let changes = changes_from_proposals(&found);
        if !changes.is_empty() {
            apply(&base, kind, &changes);
        }
    }
    let db = open(&base);
    for kind in NameKind::ALL {
        let counts = recount(&db, kind);
        for (id, want) in counts.iter().enumerate() {
            assert_eq!(
                db.names().frequency(kind, u32::try_from(id).unwrap()),
                Some(*want),
                "{kind:?} id {id} ({})",
                name_of(&db, kind, u32::try_from(id).unwrap())
            );
        }
    }
}

#[test]
fn a_second_application_of_the_same_changes_has_nothing_left_to_do() {
    let dir = tempfile::tempdir().unwrap();
    let base = build(dir.path(), &three_games());
    let change = [NameChange::new("Old, A", "New, A")];
    assert_eq!(apply(&base, NameKind::Player, &change).games_changed, 4);
    let again = apply(&base, NameKind::Player, &change);
    assert_eq!(again.games_changed, 0, "no game uses the old name any more");
    assert_eq!(
        again.mapped, 1,
        "the old name still exists in the name base"
    );
}

// ---------------------------------------------------------------------------
// set_elo_batch (rule 68, 32).
// ---------------------------------------------------------------------------

fn elo_db(dir: &Path) -> std::path::PathBuf {
    build(
        dir,
        &[
            g("A", "B", W).elo(2500, 2400),
            g("A", "B", W).elo(2501, 0).kind(RatingType::Uscf),
            g("A", "B", W),
        ],
    )
}

fn ratings(db: &Si4Database<std::fs::File>, n: u32) -> ((u16, u8), (u16, u8)) {
    let e = db.entry(n).unwrap();
    (
        (e.white_elo, e.white_rating_type),
        (e.black_elo, e.black_rating_type),
    )
}

#[test]
fn set_elo_batch_counts_only_entries_that_change_and_leaves_none_sides_alone() {
    let dir = tempfile::tempdir().unwrap();
    let base = elo_db(dir.path());
    let mut w = Si4Writer::open(&base).unwrap();
    let (n, r) = w.set_elo_batch(&[
        (1, Some((2500, 0)), Some((2400, 0))),
        (2, None, Some((2000, 0))),
        (3, Some((1800, 3)), None),
    ]);
    r.unwrap();
    assert_eq!(n, 2, "game 1 already had these values");
    let (n, r) = w.set_elo_batch(&[
        (2, None, Some((2000, 0))),
        (3, Some((1800, 3)), None),
        (1, None, None),
    ]);
    r.unwrap();
    assert_eq!(n, 0, "repeating a batch changes nothing");
    assert_eq!(w.set_elo_batch(&[]).0, 0);
    drop(w);
    let db = open(&base);
    assert_eq!(ratings(&db, 1), ((2500, 0), (2400, 0)));
    assert_eq!(
        ratings(&db, 2),
        ((2501, 4), (2000, 0)),
        "White untouched, type kept"
    );
    assert_eq!(ratings(&db, 3), ((1800, 3), (0, 0)));
}

#[test]
fn set_elo_batch_limits_to_4000_and_can_clear_a_rating() {
    let dir = tempfile::tempdir().unwrap();
    let base = elo_db(dir.path());
    let mut w = Si4Writer::open(&base).unwrap();
    let (n, r) = w.set_elo_batch(&[
        (1, Some((9999, 0)), Some((4095, 2))),
        (2, Some((0, 0)), None),
    ]);
    r.unwrap();
    assert_eq!(n, 2);
    drop(w);
    let db = open(&base);
    assert_eq!(ratings(&db, 1), ((4000, 0), (4000, 2)));
    assert_eq!(ratings(&db, 2).0, (0, 0));
}

#[test]
fn set_elo_batch_with_a_bad_game_number_fails_without_panic_and_keeps_earlier_writes() {
    let dir = tempfile::tempdir().unwrap();
    let base = elo_db(dir.path());
    for (i, bad) in [0u32, 4, u32::MAX].into_iter().enumerate() {
        let value = 1111 + u16::try_from(i).unwrap();
        let mut w = Si4Writer::open(&base).unwrap();
        let (n, r) = w.set_elo_batch(&[(3, Some((value, 0)), None), (bad, Some((2222, 0)), None)]);
        assert!(r.is_err(), "game {bad}");
        assert_eq!(n, 1);
        drop(w);
        let db = open(&base);
        assert_eq!(ratings(&db, 3).0, (value, 0));
    }
}

// ---------------------------------------------------------------------------
// Rules 65 to 74: Add Elo.
// ---------------------------------------------------------------------------

const ELO_SSP: &str = "@PLAYER
Pina, Paula #- BRA [2200]
%Elo 2013:2300,2301,2302,2303,2304,2305,2306,2307,2308,2309,2310,2311 2012:2250,2251,2252,2253,2254,2255,2256,2257,2258 2005:9999,3000,2500,2400 2010:2100
  = Pina, P
Rocha, Rui #- BRA [2000]
";

const PINA: &str = "Pina, Paula";

fn elo_games() -> Vec<G> {
    let q = "Quux, Q";
    vec![
        g(PINA, q, W).on("E", "S", "2013.02.10"),         // 1: 2301
        g(q, PINA, W).on("E", "S", "2013.11.30"),         // 2: black 2310
        g(PINA, q, W).on("E", "S", "2005.01.01"),         // 3: 9999 -> 4000
        g(PINA, q, W).on("E", "S", "2010.09.09"),         // 4: fallback 2100
        g(PINA, q, W).on("E", "S", "2012.12.12"),         // 5: 2258
        g("Pina, P", q, W).on("E", "S", "2013.02.10"),    // 6: alias
        g("Rocha, Rui", q, W).on("E", "S", "2013.02.10"), // 7: no %Elo
        g(PINA, q, W).on("E", "S", "2013.00.00"),         // 8: no month -> 2300
        g(PINA, q, W).on("E", "S", "????.??.??"),         // 9: no date
        g(PINA, q, W)
            .on("E", "S", "2013.02.10")
            .elo(2000, 0)
            .kind(RatingType::Uscf), // 10
        g(PINA, q, W)
            .on("E", "S", "2013.02.10")
            .elo(2301, 0)
            .kind(RatingType::Uscf), // 11
        g(PINA, q, W).on("E", "S", "2013.02.10").elo(2301, 0), // 12
        g(PINA, PINA, W).on("E", "S", "2013.02.10"),      // 13: both sides
        g(PINA, q, W).on("E", "S", "2014.02.10"),         // 14: year without list
    ]
}

fn plan(
    base: &Path,
    filter: Option<&Filter>,
    opts: EloOptions,
) -> Result<mychess_db::spelling::elo::EloPlan, AddEloError> {
    let db = open(base);
    let set = read(ELO_SSP);
    plan_add_elo(
        &db,
        set.checker(NameKind::Player),
        filter,
        opts,
        &CancelToken::new(),
        &mut NoProgress,
    )
}

fn defaults_all() -> EloOptions {
    EloOptions {
        only_filter: false,
        ..EloOptions::default()
    }
}

#[test]
fn add_elo_defaults_are_the_documented_ones() {
    let d = EloOptions::default();
    assert_eq!(
        (d.overwrite, d.only_filter, d.no_month, d.update),
        (false, true, true, true)
    );
}

#[test]
fn add_elo_plan_follows_the_period_tables_and_skips_what_it_must() {
    let dir = tempfile::tempdir().unwrap();
    let base = build(dir.path(), &elo_games());
    let plan = plan(&base, None, EloOptions::default()).unwrap();
    let got: Vec<(
        u32,
        mychess_db::si4::SideRating,
        mychess_db::si4::SideRating,
    )> = plan
        .changes
        .iter()
        .map(|c| (c.game, c.white, c.black))
        .collect();
    assert_eq!(
        got,
        [
            (1, Some((2301, 0)), None),
            (2, None, Some((2310, 0))),
            (3, Some((4000, 0)), None),
            (4, Some((2100, 0)), None),
            (5, Some((2258, 0)), None),
            (8, Some((2300, 0)), None),
            (13, Some((2301, 0)), Some((2301, 0))),
        ]
    );
    assert_eq!((plan.report.ratings, plan.report.games), (8, 7));
    assert!(!plan.report.interrupted);
    assert!(plan.update);
}

#[test]
fn add_elo_without_month_support_skips_games_whose_month_is_unknown() {
    let dir = tempfile::tempdir().unwrap();
    let base = build(dir.path(), &elo_games());
    let opts = EloOptions {
        no_month: false,
        ..EloOptions::default()
    };
    let plan = plan(&base, None, opts).unwrap();
    assert!(plan.changes.iter().all(|c| c.game != 8));
    assert_eq!(plan.report.games, 6);
}

#[test]
fn add_elo_overwrite_replaces_values_and_types_and_counts_only_real_changes() {
    let dir = tempfile::tempdir().unwrap();
    let base = build(dir.path(), &elo_games());
    let opts = EloOptions {
        overwrite: true,
        ..EloOptions::default()
    };
    let plan = plan(&base, None, opts).unwrap();
    let by_game = |n: u32| plan.changes.iter().find(|c| c.game == n);
    assert_eq!(
        by_game(10).unwrap().white,
        Some((2301, 0)),
        "value and type replaced"
    );
    assert_eq!(
        by_game(11).unwrap().white,
        Some((2301, 0)),
        "same value, type Uscf -> Elo counts"
    );
    assert!(
        by_game(12).is_none(),
        "already 2301 with type Elo: nothing to write"
    );
}

#[test]
fn add_elo_overwrite_uses_exact_lookup_for_unknown_months() {
    // Rule 67: with overwrite on an existing rating, the sweep of the year
    // for an unknown month is off.
    let dir = tempfile::tempdir().unwrap();
    let ssp = "@PLAYER\nPina, Paula\n%Elo 2013:?,?,2302\n";
    let games = vec![
        g(PINA, "Quux, Q", W).on("E", "S", "2013.00.00"),
        g(PINA, "Quux, Q", W)
            .on("E", "S", "2013.00.00")
            .elo(2000, 0),
    ];
    let base = build(dir.path(), &games);
    let db = open(&base);
    let set = read(ssp);
    let opts = EloOptions {
        overwrite: true,
        only_filter: false,
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
    let games: Vec<u32> = p.changes.iter().map(|c| c.game).collect();
    assert_eq!(
        games,
        [1],
        "game 1 has no rating: sweeps to 2302; game 2 has one: exact lookup finds none"
    );
}

#[test]
fn add_elo_applies_and_a_second_run_reports_zero() {
    let dir = tempfile::tempdir().unwrap();
    let base = build(dir.path(), &elo_games());
    let before_games = games_bytes(&base);
    let p = plan(&base, None, defaults_all()).unwrap();
    let mut w = Si4Writer::open(&base).unwrap();
    let (report, result) = apply_add_elo(&mut w, &p, &CancelToken::new(), &mut NoProgress);
    result.unwrap();
    drop(w);
    assert_eq!(
        (report.ratings, report.games, report.interrupted),
        (8, 7, false)
    );
    let db = open(&base);
    assert_eq!(ratings(&db, 1).0, (2301, 0));
    assert_eq!(ratings(&db, 3).0, (4000, 0), "9999 is limited to 4000");
    assert_eq!(ratings(&db, 13), ((2301, 0), (2301, 0)));
    assert_eq!(ratings(&db, 6).0, (0, 0), "an alias never gets a rating");
    assert_eq!(ratings(&db, 7).0, (0, 0));
    assert_eq!(
        ratings(&db, 10).0,
        (2000, 4),
        "existing rating kept without overwrite"
    );
    assert_eq!(games_bytes(&base), before_games);
    drop(db);
    let again = plan(&base, None, defaults_all()).unwrap();
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
fn add_elo_update_off_counts_without_writing() {
    let dir = tempfile::tempdir().unwrap();
    let base = build(dir.path(), &elo_games());
    let before = index_bytes(&base);
    let opts = EloOptions {
        update: false,
        only_filter: false,
        ..EloOptions::default()
    };
    let p = plan(&base, None, opts).unwrap();
    assert!(!p.update);
    assert_eq!((p.report.ratings, p.report.games), (8, 7));
    let mut w = Si4Writer::open(&base).unwrap();
    let (report, result) = apply_add_elo(&mut w, &p, &CancelToken::new(), &mut NoProgress);
    result.unwrap();
    drop(w);
    assert_eq!((report.ratings, report.games), (8, 7));
    assert_eq!(index_bytes(&base), before, "nothing was written");
}

#[test]
fn add_elo_only_filter_respects_the_filter_and_checks_its_size() {
    let dir = tempfile::tempdir().unwrap();
    let base = build(dir.path(), &elo_games());
    let db = open(&base);
    let mut filter = Filter::new(db.len());
    filter.fill(0);
    filter.set(1, 1);
    filter.set(5, 1);
    filter.set(7, 1);
    drop(db);
    let p = plan(&base, Some(&filter), EloOptions::default()).unwrap();
    let games: Vec<u32> = p.changes.iter().map(|c| c.game).collect();
    assert_eq!(games, [1, 5]);
    // Not limited to the filter: the filter is ignored, even a wrong-sized one.
    let all = plan(&base, Some(&filter), defaults_all()).unwrap();
    assert_eq!(all.changes.len(), 7);
    let wrong = Filter::new(3);
    assert!(matches!(
        plan(&base, Some(&wrong), EloOptions::default()),
        Err(AddEloError::FilterSize {
            filter: 3,
            games: 14
        })
    ));
    assert_eq!(
        plan(&base, Some(&wrong), defaults_all())
            .unwrap()
            .changes
            .len(),
        7
    );
}

#[test]
fn add_elo_needs_player_elo_data() {
    let dir = tempfile::tempdir().unwrap();
    let base = build(dir.path(), &elo_games());
    let db = open(&base);
    let no_elo = read("@PLAYER\nPina, Paula\n@SITE\nS\n%Elo 2013:2300\n");
    let run = |kind| {
        plan_add_elo(
            &db,
            no_elo.checker(kind),
            None,
            defaults_all(),
            &CancelToken::new(),
            &mut NoProgress,
        )
    };
    // The %Elo after the site belongs to the last player too (D3350).
    assert!(run(NameKind::Player).is_ok());
    assert!(matches!(run(NameKind::Site), Err(AddEloError::NotPlayers)));
    let plain = read("@PLAYER\nPina, Paula\n");
    let err = plan_add_elo(
        &db,
        plain.checker(NameKind::Player),
        None,
        defaults_all(),
        &CancelToken::new(),
        &mut NoProgress,
    );
    assert!(matches!(err, Err(AddEloError::NoEloData)));
    assert_ne!(AddEloError::NoEloData.to_string(), "");
}

#[test]
fn add_elo_cancelled_while_writing_keeps_valid_entries_and_can_be_resumed() {
    let dir = tempfile::tempdir().unwrap();
    let games: Vec<G> = (0..3000)
        .map(|_| g(PINA, "Quux, Q", W).on("E", "S", "2013.02.10"))
        .collect();
    let base = build(dir.path(), &games);
    let p = plan(&base, None, defaults_all()).unwrap();
    assert_eq!(p.report.games, 3000);
    let token = CancelToken::new();
    let t2 = token.clone();
    let mut w = Si4Writer::open(&base).unwrap();
    let (report, result) = apply_add_elo(&mut w, &p, &token, &mut move |done, _| {
        if done >= 1000 {
            t2.cancel();
        }
    });
    result.unwrap();
    drop(w);
    assert!(report.interrupted);
    assert!(report.games >= 1000 && report.games < 3000, "{report:?}");
    let db = open(&base);
    let written = (1..=3000)
        .filter(|&n| ratings(&db, n).0 == (2301, 0))
        .count();
    assert_eq!(written, usize::try_from(report.games).unwrap());
    drop(db);
    // Running it again finishes the rest and reports only the remainder.
    let rest = plan(&base, None, defaults_all()).unwrap();
    assert_eq!(rest.report.games, 3000 - report.games);
}

#[test]
fn add_elo_plan_cancelled_returns_a_marked_partial_plan() {
    let dir = tempfile::tempdir().unwrap();
    let games: Vec<G> = (0..5000)
        .map(|_| g(PINA, "Quux, Q", W).on("E", "S", "2013.02.10"))
        .collect();
    let base = build(dir.path(), &games);
    let db = open(&base);
    let set = read(ELO_SSP);
    let token = CancelToken::new();
    let t2 = token.clone();
    let p = plan_add_elo(
        &db,
        set.checker(NameKind::Player),
        None,
        defaults_all(),
        &token,
        &mut move |done, _| {
            if done >= 1000 {
                t2.cancel();
            }
        },
    )
    .unwrap();
    assert!(p.report.interrupted);
    assert!(p.report.games < 5000);
    assert_eq!(usize::try_from(p.report.games).unwrap(), p.changes.len());
}

#[test]
fn add_elo_only_touches_rating_bytes_of_the_index() {
    let dir = tempfile::tempdir().unwrap();
    let base = build(dir.path(), &elo_games());
    let before = index_bytes(&base);
    let p = plan(&base, None, defaults_all()).unwrap();
    let mut w = Si4Writer::open(&base).unwrap();
    let (_, r) = apply_add_elo(&mut w, &p, &CancelToken::new(), &mut NoProgress);
    r.unwrap();
    drop(w);
    let after = index_bytes(&base);
    assert_ne!(before, after);
    for (i, (a, b)) in before.iter().zip(&after).enumerate() {
        if i >= HEADER && matches!((i - HEADER) % ENTRY, 29..=32) {
            continue;
        }
        assert_eq!(
            a,
            b,
            "byte {i} (entry offset {})",
            (i.saturating_sub(HEADER)) % ENTRY
        );
    }
}

#[test]
fn partial_dates_used_by_the_tests_are_what_they_claim() {
    // Guards the fixtures: "2013.00.00" has a year and no month.
    let d: PartialDate = date("2013.00.00");
    assert_eq!((d.year(), d.month()), (Some(2013), None));
}
