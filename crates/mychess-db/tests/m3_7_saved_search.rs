//! Saved searches (D635 to D648): round-trip, `.sso` files and storage,
//! through the public API only.

use std::fs;

use mychess_core::{Eco, PartialDate};
use mychess_db::saved_search::{self, SavedSearch, SavedSearchError, SsoWarningKind};
use mychess_db::search::{EcoRange, GameEndKind, HeaderSearch, MinMax, ResultSet, TitleSet, Tri};
use mychess_db::si4::GameFlags;
use mychess_db::{FilterOp, import_sso, import_sso_bytes};
use proptest::prelude::*;

const ALL_FLAGS: [GameFlags; 22] = [
    GameFlags::START,
    GameFlags::PROMOTION,
    GameFlags::UNDER_PROMOTION,
    GameFlags::DELETE,
    GameFlags::WHITE_OPENING,
    GameFlags::BLACK_OPENING,
    GameFlags::MIDDLEGAME,
    GameFlags::ENDGAME,
    GameFlags::NOVELTY,
    GameFlags::PAWN_STRUCTURE,
    GameFlags::TACTICS,
    GameFlags::KINGSIDE,
    GameFlags::QUEENSIDE,
    GameFlags::BRILLIANCY,
    GameFlags::BLUNDER,
    GameFlags::USER,
    GameFlags::CUSTOM1,
    GameFlags::CUSTOM2,
    GameFlags::CUSTOM3,
    GameFlags::CUSTOM4,
    GameFlags::CUSTOM5,
    GameFlags::CUSTOM6,
];

fn date() -> impl Strategy<Value = PartialDate> {
    "([0-9]{4}|\\?\\?\\?\\?)\\.([0-9]{2}|\\?\\?)\\.([0-9]{2}|\\?\\?)"
        .prop_map(|t| PartialDate::parse_lenient(&t))
}

fn text() -> impl Strategy<Value = String> {
    prop_oneof![
        Just(String::new()),
        "\\PC{0,20}",
        Just("\"Kas*\"".to_owned()),
        Just("multi\nline # = \" \\ ' [x]".to_owned()),
    ]
}

fn range<T: Strategy + 'static>(t: T) -> impl Strategy<Value = MinMax<T::Value>>
where
    T::Value: Clone + std::fmt::Debug,
{
    let t = std::sync::Arc::new(t);
    (t.clone(), t).prop_map(|(a, b)| MinMax::new(a, b))
}

fn search() -> impl Strategy<Value = HeaderSearch> {
    let names = (text(), text(), text(), text(), text());
    let ranges = (
        (date(), date()),
        range(any::<u16>()),
        range(any::<u16>()),
        range(any::<i32>()),
        range(any::<i64>()),
        proptest::option::of(range(any::<u16>())),
    );
    let sets = (
        any::<[bool; 4]>(),
        any::<u8>(),
        any::<u8>(),
        any::<[bool; 2]>(),
        proptest::option::of((1u16..=65500, 1u16..=65500, any::<bool>())),
    );
    let rest = (
        any::<[bool; 4]>(),
        0u8..3,
        0u8..3,
        proptest::collection::vec(0u8..3, 22),
        text(),
        text(),
        proptest::collection::vec(text(), 0..4),
    );
    (names, ranges, sets, rest).prop_map(|(n, r, s, t)| {
        let mut h = HeaderSearch {
            white: n.0,
            black: n.1,
            event: n.2,
            site: n.3,
            round: n.4,
            date: MinMax::new((r.0).0, (r.0).1),
            white_elo: r.1,
            black_elo: r.2,
            elo_diff: r.3,
            game_number: r.4,
            length: r.5,
            results: ResultSet {
                white: s.0[0],
                draw: s.0[1],
                black: s.0[2],
                none: s.0[3],
            },
            white_titles: TitleSet::from_bits_retain(s.1),
            black_titles: TitleSet::from_bits_retain(s.2),
            end_white_to_move: s.3[0],
            end_black_to_move: s.3[1],
            eco: s.4.and_then(|(a, b, none)| {
                Some(EcoRange {
                    min: Eco::from_scid_code(a)?,
                    max: Eco::from_scid_code(b)?,
                    include_none: none,
                })
            }),
            ignore_colors: t.0[0],
            ignore_case: t.0[1],
            pre_comment: t.0[2],
            post_comment: t.0[3],
            op: [FilterOp::And, FilterOp::Or, FilterOp::Reset][usize::from(t.1)],
            game_end: [
                GameEndKind::Any,
                GameEndKind::Checkmate,
                GameEndKind::Stalemate,
            ][usize::from(t.2)],
            extra_tag: t.4,
            extra_tag_value: t.5,
            pgn_text: t.6,
            ..HeaderSearch::default()
        };
        for (flag, c) in ALL_FLAGS.iter().zip(&t.3) {
            h.flags
                .set(*flag, [Tri::Yes, Tri::No, Tri::Both][usize::from(*c)]);
        }
        h
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn search_to_toml_to_search_is_identity(s in search()) {
        let saved = SavedSearch::new("round trip", s).unwrap();
        let text = saved.to_toml().unwrap();
        prop_assert_eq!(SavedSearch::from_toml(&text).unwrap(), saved);
    }

    #[test]
    fn saved_to_disk_and_back_is_identity(s in search()) {
        let dir = tempfile::tempdir().unwrap();
        let saved = SavedSearch::new("disk", s).unwrap();
        saved_search::save(dir.path(), &saved).unwrap();
        prop_assert_eq!(saved_search::load(dir.path(), "disk").unwrap(), saved);
    }

    #[test]
    fn arbitrary_bytes_never_panic(bytes in proptest::collection::vec(any::<u8>(), 0..400)) {
        drop(import_sso_bytes(&bytes));
        drop(SavedSearch::from_toml(&String::from_utf8_lossy(&bytes)));
    }

    #[test]
    fn arbitrary_names_stay_in_the_directory(name in "\\PC{1,40}") {
        let stem = saved_search::file_stem_for(&name);
        prop_assert!(!stem.is_empty());
        prop_assert!(!stem.contains(['/', '\\', ':', '*', '?', '"', '<', '>', '|']));
        prop_assert!(!stem.starts_with('.') && !stem.ends_with('.') && !stem.ends_with(' '));
    }
}

#[test]
fn sso_written_like_scid_saves_is_read() {
    // The shape of `::search::header::save` (header.tcl:613-653).
    let text = "\
# Scid vs PC search options file
set searchType Header
set sWhite {Fischer, Robert J}
set sBlack {}
set sEvent {}
set sSite {}
set sRound {}
set sDateMin 1970.01.01
set sDateMax 1972
set sIgnoreCol No
set sWhiteEloMin 2600
set sWhiteEloMax 4000
set sBlackEloMin 0
set sBlackEloMax 4000
set sEloDiffMin -4000
set sEloDiffMax +4000
set sGlMin 0
set sGlMax 999
set sGlMin 0
set sGlMax 999
set sGnumMin 1
set sGnumMax -1
set sEco Yes
set sEcoMin B00
set sEcoMax B99
set sSideToMove wb
set preComment 0
set postComment 0
set sTag {}
set sTagValue {}
set sResWin 1
set sResLoss 0
set sResDraw 1
set sResOther 1
set sPgncase 0
set sGameEnd Any
set ::search::filter::operation 1
set sHeaderFlags(Tactics) both
set sHeaderFlags(TacticsFlag) yes
set sHeaderFlags(Comments) both
set sPgntext(1) {king's gambit}
set sPgntext(2) {}
set sPgntext(3) {}
set sTitles(w:gm) 1
set sTitles(b:gm) 1
set sTitles(w:none) 0
";
    let r = import_sso_bytes(text.as_bytes()).unwrap();
    let s = &r.search;
    assert_eq!(s.white, "Fischer, Robert J");
    assert_eq!(s.date.min.to_string(), "1970.??.??");
    assert_eq!(s.date.max.to_string(), "1972.12.31");
    assert_eq!(s.white_elo, MinMax::new(2600, 4000));
    assert_eq!(s.eco.map(|e| e.min.to_string()), Some("B00".to_owned()));
    assert!(!s.results.black);
    assert_eq!(s.op, FilterOp::Or);
    assert_eq!(s.flags.get(GameFlags::TACTICS), Tri::Yes);
    assert_eq!(s.pgn_text, vec!["king's gambit"]);
    assert!(!s.white_titles.contains(TitleSet::NONE));
    assert!(s.black_titles.contains(TitleSet::NONE));
    // `sHeaderFlags(Tactics)` is not a name Scid writes.
    assert_eq!(r.warnings.len(), 1);
    assert_eq!(r.warnings[0].kind, SsoWarningKind::Unknown);
    assert_eq!(r.warnings[0].line, 38);
}

#[test]
fn sso_with_material_variables_warns_and_keeps_the_header_part() {
    let text = "set searchType Header\nset sWhite {A}\nset pMin(wq) 1\nset pMax(wq) 2\n\
                set matchEnd 0\nset pattPiece(1) K\n";
    let r = import_sso_bytes(text.as_bytes()).unwrap();
    assert_eq!(r.search.white, "A");
    assert_eq!(r.warnings.len(), 4);
    assert!(
        r.warnings
            .iter()
            .all(|w| w.kind == SsoWarningKind::Material)
    );
}

#[test]
fn material_sso_is_refused() {
    let text = "# Scid search options file\nset searchType Material\nset pMin(wq) 0\n";
    assert!(matches!(
        import_sso_bytes(text.as_bytes()),
        Err(SavedSearchError::UnsupportedSearchType { .. })
    ));
}

#[test]
fn sso_file_import_and_limits() {
    let dir = tempfile::tempdir().unwrap();
    let ok = dir.path().join("a.sso");
    fs::write(&ok, "set sWhite {A}\n").unwrap();
    assert_eq!(import_sso(&ok).unwrap().search.white, "A");
    let big = dir.path().join("big.sso");
    fs::write(&big, vec![b' '; 2 << 20]).unwrap();
    assert!(matches!(
        import_sso(&big),
        Err(SavedSearchError::TooLarge { .. })
    ));
    let bin = dir.path().join("bin.sso");
    fs::write(&bin, [0xFFu8, 0, 0xFE, 1]).unwrap();
    assert!(matches!(import_sso(&bin), Err(SavedSearchError::NotText)));
    assert!(matches!(
        import_sso(&dir.path().join("none.sso")),
        Err(SavedSearchError::Io { .. })
    ));
}

#[test]
fn sso_truncated_at_every_byte_never_panics() {
    let text = b"# x\nset searchType Header\nset sWhite {A b}\nset sBlack \"c\\\"d\"\nset sHeaderFlags(TacticsFlag) yes\nset sPgntext(1) e\\ f\n";
    for n in 0..=text.len() {
        drop(import_sso_bytes(&text[..n]));
    }
}

#[test]
fn collection_workflow() {
    let dir = tempfile::tempdir().unwrap();
    let store = dir.path().join("cfg").join("searches");
    for n in ["B", "a", "C"] {
        let mut s = SavedSearch::new(n, HeaderSearch::default()).unwrap();
        s.created = Some("2026-10-23".to_owned());
        saved_search::save(&store, &s).unwrap();
    }
    let names: Vec<_> = saved_search::list(&store)
        .unwrap()
        .entries
        .into_iter()
        .map(|e| e.name)
        .collect();
    assert_eq!(names, ["a", "B", "C"]);
    assert!(saved_search::delete(&store, "B").unwrap());
    assert_eq!(saved_search::list(&store).unwrap().entries.len(), 2);
}

#[test]
fn loading_a_corrupt_file_is_an_error_not_a_panic() {
    let dir = tempfile::tempdir().unwrap();
    let good = SavedSearch::new("g", HeaderSearch::default()).unwrap();
    let path = saved_search::save(dir.path(), &good).unwrap();
    let text = fs::read(&path).unwrap();
    for n in 0..text.len() {
        fs::write(&path, &text[..n]).unwrap();
        drop(saved_search::load_file(&path));
    }
}
