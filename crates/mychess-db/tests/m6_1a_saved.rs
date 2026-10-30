//! Contract tests of the saved advanced searches and of the reading of the
//! material `.sso` files (spec `docs/specs/m6-1-buscas-avancadas.md`, D3235
//! and "Arquivo `.sso` do Scid para a busca de material").
#![allow(
    clippy::assert_is_empty,
    clippy::trivially_copy_pass_by_ref,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    unused_qualifications,
    clippy::field_reassign_with_default,
    clippy::unwrap_used,
    clippy::too_many_lines
)]

use std::fmt::Write as _;

use mychess_core::{Color, File, Piece, Position, Rank, Role};
use mychess_db::saved_search::{self, SsoWarningKind};
use mychess_db::{
    AdvancedQuery, AnySsoReport, BishopColors, BoardMatch, BoardSearch, CheckTest, FilterOp,
    HeaderSearch, MaterialSearch, MoveToken, MovesSearch, Pattern, PieceRange, SavedAdvanced,
    SavedSearch, SavedSearchError, SideFilter, SideRanges, import_any_sso_bytes,
    import_material_sso_bytes, import_sso_bytes,
};
use pretty_assertions::assert_eq;
use proptest::prelude::*;

fn range(min: u8, max: u8) -> PieceRange {
    PieceRange::new(min, max)
}

fn rich_material() -> MaterialSearch {
    let mut q = MaterialSearch::default();
    q.white.queen = range(1, 1);
    q.black.pawn = range(2, 7);
    q.black.minor = range(1, 3);
    q.diff = Some((-5, 12));
    q.bishops = BishopColors::Opposite;
    q.moves = (3, 40);
    q.min_plies = 5;
    q.end_only = true;
    q.ignore_colors = true;
    q.op = FilterOp::Or;
    q.patterns = vec![
        Pattern::on_square(
            Piece {
                color: Color::White,
                role: Role::King,
            },
            File::G,
            Rank::First,
            true,
        ),
        Pattern::on_file(
            Piece {
                color: Color::Black,
                role: Role::Pawn,
            },
            File::C,
            false,
        ),
        Pattern {
            piece: Some(Piece {
                color: Color::White,
                role: Role::Rook,
            }),
            file: None,
            rank: Some(Rank::Seventh),
            present: true,
        },
    ];
    q
}

fn rich_moves() -> MovesSearch {
    let mut q = MovesSearch::parse(
        "e4 ? Nf3 Qh7+",
        SideFilter::White,
        CheckTest::No,
        FilterOp::And,
    )
    .unwrap();
    q.in_variations = true;
    q.start_plies = (2, 100);
    q
}

fn rich_position() -> BoardSearch {
    BoardSearch {
        kind: BoardMatch::Files,
        in_variations: true,
        ignore_colors: true,
        op: FilterOp::Or,
        ..BoardSearch::new(
            Position::from_fen("rnbqkbnr/pppp1ppp/8/4p3/4P3/8/PPPP1PPP/RNBQKBNR w KQkq e6 0 2")
                .unwrap(),
        )
    }
}

#[test]
fn every_kind_round_trips_through_toml() {
    for query in [
        AdvancedQuery::Position(rich_position()),
        AdvancedQuery::Material(rich_material()),
        AdvancedQuery::Moves(rich_moves()),
        AdvancedQuery::Material(MaterialSearch::default()),
        AdvancedQuery::Position(BoardSearch::new(Position::new())),
    ] {
        let mut saved = SavedAdvanced::new("A search", query).unwrap();
        saved.created = Some("2026-10-27".to_owned());
        let text = saved.to_toml().unwrap();
        assert!(text.contains("format = \"mychess-saved-search\""), "{text}");
        assert!(text.contains("version = 1"));
        assert_eq!(SavedAdvanced::from_toml(&text).unwrap(), saved, "{text}");
    }
}

#[test]
fn the_kind_and_its_table_are_written() {
    let text = SavedAdvanced::new("m", AdvancedQuery::Material(rich_material()))
        .unwrap()
        .to_toml()
        .unwrap();
    assert!(text.contains("kind = \"material\""), "{text}");
    assert!(text.contains("[material"), "{text}");
    assert!(text.contains("[[material.patterns]]"), "{text}");
    let text = SavedAdvanced::new("p", AdvancedQuery::Moves(rich_moves()))
        .unwrap()
        .to_toml()
        .unwrap();
    assert!(
        text.contains("kind = \"moves\"") && text.contains("[moves]"),
        "{text}"
    );
}

#[test]
fn header_and_advanced_files_share_a_directory_without_mixing() {
    let dir = tempfile::tempdir().unwrap();
    saved_search::save(
        dir.path(),
        &SavedSearch::new("Header one", HeaderSearch::default()).unwrap(),
    )
    .unwrap();
    for (name, query) in [
        ("Material one", AdvancedQuery::Material(rich_material())),
        ("Moves one", AdvancedQuery::Moves(rich_moves())),
        ("Position one", AdvancedQuery::Position(rich_position())),
    ] {
        saved_search::save_advanced(dir.path(), &SavedAdvanced::new(name, query).unwrap()).unwrap();
    }
    let headers = saved_search::list(dir.path()).unwrap();
    assert_eq!(
        headers
            .entries
            .iter()
            .map(|e| e.name.as_str())
            .collect::<Vec<_>>(),
        vec!["Header one"]
    );
    assert!(headers.skipped.is_empty(), "{:?}", headers.skipped);
    let advanced = saved_search::list_advanced(dir.path()).unwrap();
    assert_eq!(
        advanced
            .entries
            .iter()
            .map(|e| e.name.as_str())
            .collect::<Vec<_>>(),
        vec!["Material one", "Moves one", "Position one"]
    );
    assert!(advanced.skipped.is_empty(), "{:?}", advanced.skipped);
    let loaded = saved_search::load_advanced(dir.path(), "Material one").unwrap();
    assert_eq!(loaded.query, AdvancedQuery::Material(rich_material()));
    assert!(matches!(
        saved_search::load(dir.path(), "Material one"),
        Err(SavedSearchError::NotFound { .. })
    ));
}

#[test]
fn saving_again_replaces_and_delete_removes() {
    let dir = tempfile::tempdir().unwrap();
    let mut saved = SavedAdvanced::new("Same name", AdvancedQuery::Moves(rich_moves())).unwrap();
    let first = saved_search::save_advanced(dir.path(), &saved).unwrap();
    saved.query = AdvancedQuery::Material(rich_material());
    let second = saved_search::save_advanced(dir.path(), &saved).unwrap();
    assert_eq!(first, second);
    assert_eq!(
        saved_search::load_advanced(dir.path(), "Same name")
            .unwrap()
            .query,
        AdvancedQuery::Material(rich_material())
    );
    assert!(saved_search::delete_advanced(dir.path(), "Same name").unwrap());
    assert!(!saved_search::delete_advanced(dir.path(), "Same name").unwrap());
    assert!(
        saved_search::list_advanced(dir.path())
            .unwrap()
            .entries
            .is_empty()
    );
}

#[test]
fn a_header_text_is_refused_as_advanced_and_the_other_way_round() {
    let header = SavedSearch::new("h", HeaderSearch::default())
        .unwrap()
        .to_toml()
        .unwrap();
    assert!(matches!(
        SavedAdvanced::from_toml(&header),
        Err(SavedSearchError::WrongKind { .. })
    ));
    let advanced = SavedAdvanced::new("a", AdvancedQuery::Moves(rich_moves()))
        .unwrap()
        .to_toml()
        .unwrap();
    assert!(matches!(
        SavedSearch::from_toml(&advanced),
        Err(SavedSearchError::WrongKind { found }) if found == "moves"
    ));
}

#[test]
fn invalid_files_are_errors_not_panics() {
    let base = "format = \"mychess-saved-search\"\nversion = 1\nname = \"x\"\n";
    let bad = [
        // Unknown kind, missing table, wrong table.
        format!("{base}kind = \"cql\"\n"),
        format!("{base}kind = \"material\"\n"),
        format!("{base}kind = \"moves\"\n[position]\nfen = \"x\"\n"),
        // Bad values.
        format!("{base}kind = \"position\"\n[position]\nfen = \"nonsense\"\n"),
        format!("{base}kind = \"position\"\n[position]\nmatch_kind = \"loose\"\n"),
        format!(
            "{base}kind = \"moves\"\n[moves]\nmoves = \"? e5\"\nside = \"both\"\ncheck_test = \"no\"\nin_variations = false\nstart_min = 0\nstart_max = 254\nop = \"reset\"\n"
        ),
        format!(
            "{base}kind = \"moves\"\n[moves]\nmoves = \"Ng1-f3\"\nside = \"both\"\ncheck_test = \"no\"\nin_variations = false\nstart_min = 0\nstart_max = 254\nop = \"reset\"\n"
        ),
        format!(
            "{base}kind = \"moves\"\n[moves]\nmoves = \"e4\"\nside = \"nobody\"\ncheck_test = \"no\"\nin_variations = false\nstart_min = 0\nstart_max = 254\nop = \"reset\"\n"
        ),
        // Not TOML, newer version, unknown field.
        "kind = [".to_owned(),
        format!("{base}kind = \"position\"\nsurprise = 1\n"),
    ];
    for text in &bad {
        assert!(SavedAdvanced::from_toml(text).is_err(), "{text}");
    }
    let future =
        "format = \"mychess-saved-search\"\nversion = 2\nname = \"x\"\nkind = \"position\"\n";
    assert!(matches!(
        SavedAdvanced::from_toml(future),
        Err(SavedSearchError::UnsupportedVersion { found: 2 })
    ));
}

#[test]
fn a_material_file_with_an_invalid_pattern_is_refused() {
    let mut saved = SavedAdvanced::new("m", AdvancedQuery::Material(rich_material())).unwrap();
    let text = saved.to_toml().unwrap();
    let broken = text
        .replace("file = \"g\"", "file = \"?\"")
        .replace("rank = \"1\"", "rank = \"?\"");
    assert_ne!(text, broken);
    // A piece without a file and a rank cannot be searched (D3218).
    assert!(SavedAdvanced::from_toml(&broken).is_err());
    saved.name = "ok".into();
    assert!(SavedAdvanced::from_toml(&saved.to_toml().unwrap()).is_ok());
}

// --------------------------------------------------------------- .sso

/// What the window of Scid writes for the default search.
fn sso_text(extra: &str) -> String {
    let mut out = String::from("# Scid vs PC search options file\nset searchType Material\n");
    for kind in [
        "wq", "bq", "wr", "br", "wb", "bb", "wn", "bn", "wm", "bm", "wp", "bp",
    ] {
        let max = match &kind[1..] {
            "m" => 4,
            "p" => 8,
            _ => 2,
        };
        writeln!(out, "set pMin({kind}) 0\nset pMax({kind}) {max}").unwrap();
    }
    out.push_str(
        "set ignoreColors 0\nset matchEnd 0\nset minMoveNum 1\nset maxMoveNum 999\n\
         set minHalfMoves 1\nset oppBishops Either\nset ::search::filter::operation 2\n",
    );
    for i in 1..=10 {
        writeln!(
            out,
            "set pattPiece({i}) ?\nset pattFyle({i}) ?\nset pattRank({i}) ?\nset pattBool({i}) Yes"
        )
        .unwrap();
    }
    out.push_str(extra);
    out
}

#[test]
fn the_default_window_file_reads_as_the_default_search() {
    let report = import_material_sso_bytes(sso_text("").as_bytes()).unwrap();
    assert_eq!(report.search, MaterialSearch::default());
    assert!(report.warnings.is_empty());
}

#[test]
fn a_hand_written_file_gives_the_search_of_the_window() {
    let text = sso_text(concat!(
        "set pMin(wq) 1\nset pMax(wq) 1\nset pMin(bp) 2\nset pMax(bp) 6\n",
        "set pMin(wm) 2\nset pMax(wm) 3\n",
        "set ignoreColors 1\nset matchEnd 1\nset minMoveNum 3\nset maxMoveNum 40\n",
        "set minHalfMoves 5\nset oppBishops Opposite\nset ::search::filter::operation 1\n",
        "set pattPiece(1) wk\nset pattFyle(1) g\nset pattRank(1) 1\nset pattBool(1) Yes\n",
        "set pattPiece(2) bp\nset pattFyle(2) c\nset pattRank(2) ?\nset pattBool(2) No\n",
        "set pattPiece(3) wr\nset pattFyle(3) ?\nset pattRank(3) 7\nset pattBool(3) Yes\n",
    ));
    let search = import_material_sso_bytes(text.as_bytes()).unwrap().search;
    let mut expected = MaterialSearch::default();
    expected.white.queen = range(1, 1);
    expected.black.pawn = range(2, 6);
    expected.white.minor = range(2, 3);
    expected.ignore_colors = true;
    expected.end_only = true;
    expected.moves = (3, 40);
    expected.min_plies = 5;
    expected.bishops = BishopColors::Opposite;
    expected.op = FilterOp::Or;
    expected.patterns = rich_material().patterns;
    assert_eq!(search, expected);
    // The window does not save the difference: it stays unrestricted.
    assert_eq!(search.diff, None);
}

#[test]
fn a_pattern_without_a_square_is_ignored_with_a_warning() {
    let text = sso_text(
        "set pattPiece(4) wp\nset pattFyle(4) ?\nset pattRank(4) ?\nset pattBool(4) No\n\
         set pattPiece(5) zz\nset pattFyle(5) a\nset pattRank(5) 1\n",
    );
    let report = import_material_sso_bytes(text.as_bytes()).unwrap();
    assert!(report.search.patterns.is_empty());
    let kinds: Vec<_> = report.warnings.iter().map(|w| w.kind).collect();
    assert_eq!(kinds, vec![SsoWarningKind::InvalidPattern; 2]);
}

#[test]
fn the_dispatcher_tells_header_from_material_and_the_header_reader_still_refuses() {
    let material = sso_text("");
    assert!(matches!(
        import_any_sso_bytes(material.as_bytes()).unwrap(),
        AnySsoReport::Material(_)
    ));
    assert!(matches!(
        import_any_sso_bytes(b"set searchType Header\nset sWhite {Carlsen}\n").unwrap(),
        AnySsoReport::Header(r) if r.search.white == "Carlsen"
    ));
    assert!(matches!(
        import_any_sso_bytes(b"set sWhite {x}\n").unwrap(),
        AnySsoReport::Header(_)
    ));
    assert!(matches!(
        import_sso_bytes(material.as_bytes()),
        Err(SavedSearchError::UnsupportedSearchType { .. })
    ));
    assert!(matches!(
        import_material_sso_bytes(b"set searchType Header\n"),
        Err(SavedSearchError::UnsupportedSearchType { .. })
    ));
}

#[test]
fn invalid_material_values_and_tcl_are_errors() {
    for text in [
        "set pMin(wq) many\n",
        "set pMin(wq) -1\n",
        "set pMax(wp) 100\n",
        "set minMoveNum 1000\n",
        "set minHalfMoves 100\n",
        "set ignoreColors maybe\n",
        "set ::search::filter::operation 7\n",
        "puts hello\n",
        "set pMin(wq) [exec ls]\n",
        "set x {unterminated\n",
    ] {
        assert!(
            import_material_sso_bytes(text.as_bytes()).is_err(),
            "{text}"
        );
    }
    // Unknown variables only warn.
    let r = import_material_sso_bytes(b"set sWhite {x}\nset pMin(zz) 1\n").unwrap();
    assert_eq!(r.warnings.len(), 2);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// Whatever the bytes, reading a `.sso` file or a TOML never panics.
    #[test]
    fn arbitrary_bytes_never_panic(bytes in prop::collection::vec(any::<u8>(), 0..300)) {
        drop(import_material_sso_bytes(&bytes));
        drop(import_any_sso_bytes(&bytes));
        drop(SavedAdvanced::from_toml(&String::from_utf8_lossy(&bytes)));
    }

    /// Saving a material search and reading it back gives the same search.
    #[test]
    fn material_searches_round_trip(
        ranges in prop::collection::vec((0u8..10, 0u8..10), 12),
        diff in prop::option::of((-99i16..100, -99i16..100)),
        moves in (0u16..1000, 0u16..1000).prop_map(|(a, b)| (a.min(b), a.max(b))),
        plies in 0u16..100,
        flags in prop::collection::vec(any::<bool>(), 3),
        bishops in 0usize..3,
        patterns in prop::collection::vec((0usize..12, 0usize..9, 0usize..9, any::<bool>()), 0..10),
    ) {
        let mut q = MaterialSearch::default();
        let side = |r: &[(u8, u8)]| SideRanges {
            queen: range(r[0].0, r[0].1),
            rook: range(r[1].0, r[1].1),
            bishop: range(r[2].0, r[2].1),
            knight: range(r[3].0, r[3].1),
            minor: range(r[4].0, r[4].1),
            pawn: range(r[5].0, r[5].1),
        };
        q.white = side(&ranges[..6]);
        q.black = side(&ranges[6..]);
        q.diff = diff;
        q.moves = moves;
        q.min_plies = plies;
        q.end_only = flags[0];
        q.ignore_colors = flags[1];
        q.op = if flags[2] { FilterOp::And } else { FilterOp::Or };
        q.bishops = [BishopColors::Either, BishopColors::Same, BishopColors::Opposite][bishops];
        for (p, f, r, present) in patterns {
            let color = if p % 2 == 0 { Color::White } else { Color::Black };
            let role = [Role::Pawn, Role::Knight, Role::Bishop, Role::Rook, Role::Queen, Role::King][p / 2];
            let file = (f < 8).then(|| File::new(u32::try_from(f).unwrap()));
            let mut rank = (r < 8).then(|| Rank::new(u32::try_from(r).unwrap()));
            if file.is_none() && rank.is_none() {
                rank = Some(Rank::First);
            }
            q.patterns.push(Pattern { piece: Some(Piece { color, role }), file, rank, present });
        }
        let saved = SavedAdvanced::new("round trip", AdvancedQuery::Material(q)).unwrap();
        let back = SavedAdvanced::from_toml(&saved.to_toml().unwrap()).unwrap();
        prop_assert_eq!(back, saved);
    }

    /// The same for the list of moves.
    #[test]
    fn moves_searches_round_trip(
        picks in prop::collection::vec(0usize..8, 1..6),
        side in 0usize..3,
        test in 0usize..3,
        vars in any::<bool>(),
        start in (0u16..300, 0u16..300),
    ) {
        const WORDS: [&str; 8] = ["e4", "?", "Nf3", "exd5", "O-O", "e8=Q", "R1a3", "Qh7"];
        let mut words: Vec<&str> = picks.iter().map(|&i| WORDS[i]).collect();
        if words[0] == "?" { words[0] = "e4"; }
        let side = [SideFilter::Both, SideFilter::White, SideFilter::Black][side];
        let test = [CheckTest::No, CheckTest::Check, CheckTest::Mate][test];
        let mut q = MovesSearch::parse(&words.join(" "), side, test, FilterOp::Or).unwrap();
        q.in_variations = vars;
        q.start_plies = start;
        prop_assert!(matches!(q.tokens[0], MoveToken::San(_)));
        let saved = SavedAdvanced::new("moves", AdvancedQuery::Moves(q)).unwrap();
        let back = SavedAdvanced::from_toml(&saved.to_toml().unwrap()).unwrap();
        prop_assert_eq!(back, saved);
    }
}

#[test]
fn a_kind_that_is_not_text_is_listed_as_skipped() {
    let dir = tempfile::tempdir().unwrap();
    saved_search::save_advanced(
        dir.path(),
        &SavedAdvanced::new("Good", AdvancedQuery::Moves(rich_moves())).unwrap(),
    )
    .unwrap();
    let text = "format = \"mychess-saved-search\"\nversion = 1\nname = \"Bad\"\nkind = 7\n";
    std::fs::write(dir.path().join("bad.toml"), text).unwrap();
    assert!(matches!(
        SavedAdvanced::from_toml(text),
        Err(SavedSearchError::InvalidValue { .. })
    ));
    let list = saved_search::list_advanced(dir.path()).unwrap();
    assert_eq!(list.entries.len(), 1);
    assert_eq!(list.skipped.len(), 1, "{:?}", list.skipped);
    assert!(matches!(
        list.skipped[0].error,
        SavedSearchError::InvalidValue { .. }
    ));
}

#[test]
fn a_material_move_range_out_of_bounds_is_refused_on_load() {
    for (min, max) in [(5, 3), (1, 1000)] {
        // What a hand-edited file would hold.
        let saved =
            SavedAdvanced::new("m", AdvancedQuery::Material(MaterialSearch::default())).unwrap();
        let text = saved
            .to_toml()
            .unwrap()
            .replace("move_min = 1", &format!("move_min = {min}"))
            .replace("move_max = 999", &format!("move_max = {max}"));
        assert!(
            matches!(
                SavedAdvanced::from_toml(&text),
                Err(SavedSearchError::InvalidValue { .. })
            ),
            "{min}..{max}"
        );
    }
}
