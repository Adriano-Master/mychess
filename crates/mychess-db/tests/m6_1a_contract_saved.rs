//! Contract tests of the saved advanced searches and of the material `.sso`
//! import, written from the spec `docs/specs/m6-1-buscas-avancadas.md` (D3235,
//! D3250, D3251, "Arquivo `.sso` do Scid para a busca de material" and
//! `.claude/rules/multiplataforma.md` for file names).
#![allow(
    clippy::assert_is_empty,
    clippy::trivially_copy_pass_by_ref,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    unused_qualifications,
    clippy::field_reassign_with_default,
    clippy::unwrap_used,
    clippy::too_many_lines,
    clippy::cast_possible_truncation,
    clippy::similar_names,
    dead_code
)]

use std::fmt::Write as _;

use mychess_core::{Color, File, Piece, Position, Rank, Role};
use mychess_db::saved_search::{self, SsoWarningKind};
use mychess_db::{
    AdvancedQuery, AnySsoReport, BishopColors, BoardMatch, BoardSearch, CheckTest, FilterOp,
    HeaderSearch, MAX_FILE_BYTES, MaterialSearch, MovesSearch, Pattern, PieceRange, SavedAdvanced,
    SavedSearch, SavedSearchError, SideFilter, delete_advanced, file_stem_for, import_any_sso,
    import_any_sso_bytes, import_material_sso, import_material_sso_bytes, list_advanced,
    load_advanced, material_presets, save_advanced,
};
use pretty_assertions::assert_eq;
use proptest::prelude::*;

fn r(min: u8, max: u8) -> PieceRange {
    PieceRange::new(min, max)
}

fn saved(name: &str, query: AdvancedQuery) -> SavedAdvanced {
    SavedAdvanced::new(name, query).unwrap()
}

fn round_trip(query: AdvancedQuery) -> SavedAdvanced {
    let original = SavedAdvanced {
        created: Some("2026-10-27".into()),
        ..saved("round trip", query)
    };
    let text = original.to_toml().unwrap();
    let back = SavedAdvanced::from_toml(&text).unwrap();
    assert_eq!(back, original, "\n{text}");
    back
}

// ------------------------------------------------------------ TOML

#[test]
fn every_option_of_the_position_search_survives_toml() {
    let fens = [
        "rnbqkbnr/pppp1ppp/8/4p3/4P3/8/PPPP1PPP/RNBQKBNR w KQkq e6 0 2",
        "r3k2r/8/8/8/8/8/8/R3K2R b Kq - 5 40",
        "4k3/8/8/8/8/8/8/4K3 w - - 0 1",
    ];
    for fen in fens {
        for kind in [
            BoardMatch::Exact,
            BoardMatch::Pawns,
            BoardMatch::Files,
            BoardMatch::Material,
        ] {
            for (vars, flip) in [(false, false), (true, false), (false, true), (true, true)] {
                for op in [FilterOp::And, FilterOp::Or, FilterOp::Reset] {
                    let q = BoardSearch {
                        position: Position::from_fen(fen).unwrap(),
                        kind,
                        in_variations: vars,
                        ignore_colors: flip,
                        op,
                        shortcuts: true,
                    };
                    round_trip(AdvancedQuery::Position(q));
                }
            }
        }
    }
}

#[test]
fn the_material_search_with_ten_patterns_survives_toml() {
    let mut q = MaterialSearch::default();
    q.white.queen = r(1, 1);
    q.white.minor = r(2, 3);
    q.black.rook = r(0, 2);
    q.black.pawn = r(4, 8);
    q.diff = Some((-3, 7));
    q.bishops = BishopColors::Same;
    q.moves = (0, 999);
    q.min_plies = 99;
    q.end_only = true;
    q.ignore_colors = true;
    q.op = FilterOp::And;
    for (i, role) in [
        Role::Pawn,
        Role::Knight,
        Role::Bishop,
        Role::Rook,
        Role::Queen,
        Role::King,
    ]
    .into_iter()
    .cycle()
    .take(10)
    .enumerate()
    {
        let piece = Piece {
            color: if i % 2 == 0 {
                Color::White
            } else {
                Color::Black
            },
            role,
        };
        let file = File::new(u32::try_from(i).unwrap() % 8);
        let rank = Rank::new(u32::try_from(i).unwrap() % 8);
        q.patterns.push(match i % 3 {
            0 => Pattern::on_square(piece, file, rank, i % 2 == 0),
            1 => Pattern::on_file(piece, file, i % 2 == 1),
            _ => Pattern {
                piece: Some(piece),
                file: None,
                rank: Some(rank),
                present: true,
            },
        });
    }
    let back = round_trip(AdvancedQuery::Material(q.clone()));
    assert_eq!(back.query, AdvancedQuery::Material(q));
    // No restriction on the difference is not the same as -40..40.
    let none = MaterialSearch::default();
    assert_eq!(none.diff, None);
    round_trip(AdvancedQuery::Material(none));
}

#[test]
fn the_moves_search_survives_toml_with_every_option() {
    for (side, test, op, vars, plies) in [
        (
            SideFilter::White,
            CheckTest::No,
            FilterOp::Reset,
            false,
            (0, 254),
        ),
        (
            SideFilter::Black,
            CheckTest::Check,
            FilterOp::And,
            true,
            (3, 40),
        ),
        (
            SideFilter::Both,
            CheckTest::Mate,
            FilterOp::Or,
            true,
            (0, 0),
        ),
    ] {
        let mut q = MovesSearch::parse("e4 ? Nf3 exd5 O-O e8=Q", side, test, op).unwrap();
        q.in_variations = vars;
        q.start_plies = plies;
        let back = round_trip(AdvancedQuery::Moves(q.clone()));
        assert_eq!(back.query, AdvancedQuery::Moves(q));
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]

    #[test]
    fn random_moves_searches_round_trip(
        words in prop::collection::vec(prop::sample::select(vec![
            "e4", "Nf3", "exd5", "O-O", "O-O-O", "e8=Q", "Qh4+", "Rxe1#", "R1a3", "Nbd7", "--", "a8=N", "?",
        ]), 1..12),
        side in 0usize..3,
        test in 0usize..3,
        op in 0usize..3,
        vars in any::<bool>(),
        first in 0u16..300,
        span in 0u16..300,
    ) {
        let mut text = String::from("e4");
        for w in &words { text.push(' '); text.push_str(w); }
        let mut q = MovesSearch::parse(
            &text,
            [SideFilter::White, SideFilter::Black, SideFilter::Both][side],
            [CheckTest::No, CheckTest::Check, CheckTest::Mate][test],
            [FilterOp::And, FilterOp::Or, FilterOp::Reset][op],
        ).unwrap();
        q.in_variations = vars;
        q.start_plies = (first, first.saturating_add(span).min(254));
        let original = saved("p", AdvancedQuery::Moves(q));
        let text = original.to_toml().unwrap();
        prop_assert_eq!(SavedAdvanced::from_toml(&text).unwrap(), original);
    }

    #[test]
    fn random_position_searches_round_trip(
        picks in prop::collection::vec(any::<u8>(), 0..40),
        kind in 0usize..4,
        vars in any::<bool>(),
        flip in any::<bool>(),
    ) {
        let mut pos = Position::new();
        for c in picks {
            let moves = pos.legal_moves();
            if moves.is_empty() { break; }
            let mv = moves[usize::from(c) % moves.len()];
            pos.play(mv).unwrap();
        }
        let q = BoardSearch {
            kind: [BoardMatch::Exact, BoardMatch::Pawns, BoardMatch::Files, BoardMatch::Material][kind],
            in_variations: vars,
            ignore_colors: flip,
            ..BoardSearch::new(pos)
        };
        let original = saved("p", AdvancedQuery::Position(q));
        let text = original.to_toml().unwrap();
        let back = SavedAdvanced::from_toml(&text).unwrap();
        // Only the squares and the side to move are searched.
        let (AdvancedQuery::Position(a), AdvancedQuery::Position(b)) = (&original.query, &back.query) else { unreachable!() };
        let (fa, fb) = (a.position.fen(), b.position.fen());
        prop_assert_eq!(
            fa.split(' ').take(2).collect::<Vec<_>>(),
            fb.split(' ').take(2).collect::<Vec<_>>()
        );
        prop_assert_eq!((a.kind, a.in_variations, a.ignore_colors), (b.kind, b.in_variations, b.ignore_colors));
    }
}

#[test]
fn the_file_says_its_kind_and_the_version_stays_one() {
    for (query, kind) in [
        (
            AdvancedQuery::Position(BoardSearch::new(Position::new())),
            "position",
        ),
        (
            AdvancedQuery::Material(MaterialSearch::default()),
            "material",
        ),
        (
            AdvancedQuery::Moves(
                MovesSearch::parse("e4", SideFilter::Both, CheckTest::No, FilterOp::Reset).unwrap(),
            ),
            "moves",
        ),
    ] {
        let text = saved("k", query).to_toml().unwrap();
        let table: toml::Table = toml::from_str(&text).unwrap();
        assert_eq!(table["kind"].as_str(), Some(kind));
        assert_eq!(table["version"].as_integer(), Some(1));
    }
}

// ----------------------------------------------- directory and names

#[test]
fn header_and_advanced_searches_never_see_each_other() {
    let dir = tempfile::tempdir().unwrap();
    saved_search::save(
        dir.path(),
        &SavedSearch::new("Header one", HeaderSearch::default()).unwrap(),
    )
    .unwrap();
    let a = saved(
        "Advanced one",
        AdvancedQuery::Material(MaterialSearch::zero()),
    );
    save_advanced(dir.path(), &a).unwrap();
    std::fs::write(dir.path().join("notes.txt"), "not a search").unwrap();
    std::fs::write(dir.path().join("broken.toml"), "this is = = not toml").unwrap();
    let advanced = list_advanced(dir.path()).unwrap();
    assert_eq!(
        advanced
            .entries
            .iter()
            .map(|e| e.name.as_str())
            .collect::<Vec<_>>(),
        vec!["Advanced one"]
    );
    assert_eq!(
        advanced.skipped.len(),
        1,
        "only the broken toml is reported"
    );
    let headers = saved_search::list(dir.path()).unwrap();
    assert_eq!(
        headers
            .entries
            .iter()
            .map(|e| e.name.as_str())
            .collect::<Vec<_>>(),
        vec!["Header one"]
    );
    // Loading by the wrong function is a not-found, never the other kind.
    assert!(matches!(
        load_advanced(dir.path(), "Header one"),
        Err(SavedSearchError::NotFound { .. })
    ));
    assert!(saved_search::load(dir.path(), "Advanced one").is_err());
    // Deleting one kind leaves the other.
    assert!(!delete_advanced(dir.path(), "Header one").unwrap());
    assert!(delete_advanced(dir.path(), "Advanced one").unwrap());
    assert_eq!(saved_search::list(dir.path()).unwrap().entries.len(), 1);
    assert!(list_advanced(dir.path()).unwrap().entries.is_empty());
    // A header text parsed as advanced, and the opposite.
    let header_text = SavedSearch::new("h", HeaderSearch::default())
        .unwrap()
        .to_toml()
        .unwrap();
    assert!(matches!(
        SavedAdvanced::from_toml(&header_text),
        Err(SavedSearchError::WrongKind { .. })
    ));
    assert!(SavedSearch::from_toml(&a.to_toml().unwrap()).is_err());
}

#[test]
fn a_missing_directory_is_an_empty_list_and_the_extension_ignores_case() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("nothing");
    assert!(list_advanced(&missing).unwrap().entries.is_empty());
    let text = saved("Shouting", AdvancedQuery::Material(MaterialSearch::zero()))
        .to_toml()
        .unwrap();
    std::fs::write(dir.path().join("LOUD.TOML"), text).unwrap();
    let list = list_advanced(dir.path()).unwrap();
    assert_eq!(list.entries.len(), 1);
    assert_eq!(
        load_advanced(dir.path(), "Shouting").unwrap().name,
        "Shouting"
    );
}

#[test]
fn file_names_are_safe_on_the_three_platforms() {
    let dir = tempfile::tempdir().unwrap();
    let names = [
        "a/b",
        "a\\b",
        "..",
        "../outside",
        "..\\outside",
        "con",
        "CON",
        "Nul",
        "aux.txt",
        "prn",
        "COM1",
        "lpt9",
        "com0",
        "a:b",
        "x<y>z",
        "q\"r",
        "p|q",
        "what?",
        "star*",
        "trailing.",
        "trailing ",
        "   ",
        ".hidden",
        "ünïcödé ♞",
        "日本語",
        "A",
        "a",
        &"x".repeat(100),
    ];
    let mut expected = Vec::new();
    for name in names {
        let Ok(s) = SavedAdvanced::new(name, AdvancedQuery::Material(MaterialSearch::zero()))
        else {
            // Names the model refuses (empty after trimming) are errors, never panics.
            continue;
        };
        let path = save_advanced(dir.path(), &s).unwrap();
        assert_eq!(
            path.parent().unwrap(),
            dir.path(),
            "{name:?} escaped the directory"
        );
        let file = path.file_name().unwrap().to_str().unwrap().to_owned();
        assert!(file.to_ascii_lowercase().ends_with(".toml"), "{file}");
        let stem = file.strip_suffix(".toml").unwrap();
        assert!(
            !stem.contains(['<', '>', ':', '"', '/', '\\', '|', '?', '*']),
            "{file}"
        );
        assert!(!stem.chars().any(char::is_control), "{file}");
        assert!(!stem.ends_with(['.', ' ']), "{file}");
        let upper = stem.split('.').next().unwrap().to_ascii_uppercase();
        let reserved = ["CON", "PRN", "AUX", "NUL"].contains(&upper.as_str())
            || (upper.len() == 4
                && (upper.starts_with("COM") || upper.starts_with("LPT"))
                && upper.ends_with(|c: char| ('1'..='9').contains(&c)));
        assert!(!reserved, "{file} is reserved on Windows");
        assert!(stem.chars().count() <= 80, "{file}");
        expected.push(s.name.clone());
        assert_eq!(load_advanced(dir.path(), &s.name).unwrap(), s, "{name:?}");
    }
    // No file escaped to the parent and no temporary file is left behind.
    let outside = dir.path().parent().unwrap().join("outside.toml");
    assert!(!outside.exists());
    for entry in std::fs::read_dir(dir.path()).unwrap() {
        let entry = entry.unwrap();
        assert!(
            entry.path().extension().is_some_and(|e| e == "toml"),
            "{:?}",
            entry.path()
        );
    }
    let listed: Vec<String> = list_advanced(dir.path())
        .unwrap()
        .entries
        .into_iter()
        .map(|e| e.name)
        .collect();
    for name in &expected {
        assert!(listed.contains(name), "{name:?} missing from the list");
    }
    // Names that differ only by case may share a stem; they never share a file.
    let paths: std::collections::HashSet<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_ascii_lowercase_lossy())
        .collect();
    assert_eq!(paths.len(), std::fs::read_dir(dir.path()).unwrap().count());
}

trait LowerLossy {
    fn to_ascii_lowercase_lossy(&self) -> String;
}
impl LowerLossy for std::ffi::OsString {
    fn to_ascii_lowercase_lossy(&self) -> String {
        self.to_string_lossy().to_lowercase()
    }
}

#[test]
fn stems_never_come_out_empty_reserved_or_with_separators() {
    for name in [
        "",
        ".",
        "..",
        "...",
        " ",
        "CON",
        "con.tar",
        "COM9",
        "COM10",
        "a/b/c",
        "a\\b",
        "\u{7}bell",
    ] {
        let stem = file_stem_for(name);
        assert!(!stem.is_empty(), "{name:?}");
        assert!(
            !stem.contains(['/', '\\', ':', '*', '?', '"', '<', '>', '|']),
            "{name:?} -> {stem:?}"
        );
        assert!(
            !stem.starts_with('.') && !stem.ends_with(['.', ' ']),
            "{name:?} -> {stem:?}"
        );
    }
    assert_ne!(file_stem_for("con"), "con");
    assert_ne!(file_stem_for("COM1"), "com1");
    assert_ne!(file_stem_for("com9"), "com9");
}

#[test]
fn a_name_outside_the_directory_cannot_be_loaded_by_path() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("searches");
    std::fs::create_dir(&dir).unwrap();
    let evil = saved("../evil", AdvancedQuery::Material(MaterialSearch::zero()));
    std::fs::write(root.path().join("evil.toml"), evil.to_toml().unwrap()).unwrap();
    // The search named "../evil" is outside; loading from `dir` finds nothing.
    assert!(matches!(
        load_advanced(&dir, "../evil"),
        Err(SavedSearchError::NotFound { .. })
    ));
    assert!(list_advanced(&dir).unwrap().entries.is_empty());
    assert!(!delete_advanced(&dir, "../evil").unwrap());
    assert!(root.path().join("evil.toml").exists());
}

#[test]
fn saving_the_same_name_replaces_and_leaves_one_file() {
    let dir = tempfile::tempdir().unwrap();
    let first = saved("same", AdvancedQuery::Material(MaterialSearch::zero()));
    let p1 = save_advanced(dir.path(), &first).unwrap();
    let second = saved("same", AdvancedQuery::Material(MaterialSearch::default()));
    let p2 = save_advanced(dir.path(), &second).unwrap();
    assert_eq!(p1, p2);
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    assert_eq!(load_advanced(dir.path(), "same").unwrap(), second);
}

#[test]
fn invalid_names_dates_and_files_are_errors() {
    for name in ["", "   ", "tab\there", "line\nbreak", &"n".repeat(1000)] {
        assert!(
            SavedAdvanced::new(name, AdvancedQuery::Material(MaterialSearch::zero())).is_err(),
            "{name:?}"
        );
    }
    let mut s = saved("d", AdvancedQuery::Material(MaterialSearch::zero()));
    for bad in ["2026-13-45x", "27/10/2026", "", "2026-1-1"] {
        s.created = Some(bad.into());
        assert!(s.to_toml().is_err(), "{bad:?}");
    }
    let dir = tempfile::tempdir().unwrap();
    s.created = None;
    s.name = "x".into();
    let big = "#".repeat(usize::try_from(MAX_FILE_BYTES).unwrap() + 10);
    std::fs::write(dir.path().join("big.toml"), big).unwrap();
    let list = list_advanced(dir.path()).unwrap();
    assert_eq!(list.skipped.len(), 1);
}

// ---------------------------------------------------------------- .sso

fn window_sso() -> String {
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
    out
}

fn sso_with(extra: &str) -> Vec<u8> {
    format!("{}{extra}", window_sso()).into_bytes()
}

#[test]
fn a_file_for_the_rooks_ending_reads_as_the_preset() {
    let text = sso_with(concat!(
        "set pMin(wr) 1\nset pMax(wr) 1\nset pMin(br) 1\nset pMax(br) 1\n",
        "set pMin(wq) 0\nset pMax(wq) 0\nset pMin(bq) 0\nset pMax(bq) 0\n",
        "set pMin(wb) 0\nset pMax(wb) 0\nset pMin(bb) 0\nset pMax(bb) 0\n",
        "set pMin(wn) 0\nset pMax(wn) 0\nset pMin(bn) 0\nset pMax(bn) 0\n",
        "set pMin(wm) 0\nset pMax(wm) 0\nset pMin(bm) 0\nset pMax(bm) 0\n",
        "set pMax(wp) 8\nset pMax(bp) 8\n",
    ));
    let report = import_material_sso_bytes(&text).unwrap();
    let preset = material_presets()
        .into_iter()
        .find(|p| p.key == "ending-rooks")
        .unwrap();
    assert_eq!(report.search.white, preset.white);
    assert_eq!(report.search.black, preset.black);
    assert_eq!(
        report.search.diff, None,
        "the file never carries the difference"
    );
    assert!(report.warnings.is_empty());
}

#[test]
fn every_field_of_the_window_file_is_read() {
    let text = sso_with(concat!(
        "set ignoreColors 1\nset matchEnd 1\nset minMoveNum 0\nset maxMoveNum 55\nset minHalfMoves 7\n",
        "set oppBishops Same\nset ::search::filter::operation 0\n",
        "set pattPiece(10) {bn}\nset pattFyle(10) {h}\nset pattRank(10) {8}\nset pattBool(10) {No}\n",
    ));
    let s = import_material_sso_bytes(&text).unwrap().search;
    assert!(s.ignore_colors && s.end_only);
    assert_eq!(s.moves, (0, 55));
    assert_eq!(s.min_plies, 7);
    assert_eq!(s.bishops, BishopColors::Same);
    assert_eq!(s.op, FilterOp::And);
    assert_eq!(
        s.patterns,
        vec![Pattern::on_square(
            Piece {
                color: Color::Black,
                role: Role::Knight
            },
            File::H,
            Rank::Eighth,
            false
        )]
    );
    for (word, expected) in [
        ("Opposite", BishopColors::Opposite),
        ("Either", BishopColors::Either),
        ("Same", BishopColors::Same),
    ] {
        let t = sso_with(&format!("set oppBishops {word}\n"));
        assert_eq!(
            import_material_sso_bytes(&t).unwrap().search.bishops,
            expected
        );
    }
    for (value, op) in [
        ("0", FilterOp::And),
        ("1", FilterOp::Or),
        ("2", FilterOp::Reset),
    ] {
        let t = sso_with(&format!("set ::search::filter::operation {value}\n"));
        assert_eq!(import_material_sso_bytes(&t).unwrap().search.op, op);
    }
}

#[test]
fn invalid_patterns_are_warnings_not_failures() {
    let text = sso_with(concat!(
        "set pattPiece(1) wk\nset pattFyle(1) ?\nset pattRank(1) ?\nset pattBool(1) Yes\n",
        "set pattPiece(2) xx\nset pattFyle(2) a\nset pattRank(2) 1\nset pattBool(2) Yes\n",
        "set pattPiece(3) wp\nset pattFyle(3) z\nset pattRank(3) 1\nset pattBool(3) Yes\n",
        "set pattPiece(4) wp\nset pattFyle(4) a\nset pattRank(4) 9\nset pattBool(4) Yes\n",
        "set pattPiece(5) wp\nset pattFyle(5) a\nset pattRank(5) 2\nset pattBool(5) Maybe\n",
        "set pattPiece(6) bp\nset pattFyle(6) h\nset pattRank(6) 7\nset pattBool(6) Yes\n",
    ));
    let report = import_material_sso_bytes(&text).unwrap();
    assert_eq!(report.search.patterns.len(), 1, "only pattern 6 is valid");
    let bad = report
        .warnings
        .iter()
        .filter(|w| w.kind == SsoWarningKind::InvalidPattern)
        .count();
    assert_eq!(bad, 5);
    assert_eq!(report.search.validate().ok(), Some(()));
}

#[test]
fn unknown_variables_warn_and_the_hostile_text_is_refused_without_running_anything() {
    let dir = tempfile::tempdir().unwrap();
    let marker = dir.path().join("pwned");
    let unknown =
        import_material_sso_bytes(&sso_with("set somethingElse 1\nset pMin(zz) 1\n")).unwrap();
    assert_eq!(unknown.warnings.len(), 2);
    let hostile = [
        format!("exec touch {}\n", marker.display()),
        format!("set pMin(wq) [exec touch {}]\n", marker.display()),
        format!("set pMin(wq) $(touch {})\n", marker.display()),
        "source /etc/passwd\n".to_owned(),
        "proc x {} { }\n".to_owned(),
        "set pMin(wq) 1; exec ls\n".to_owned(),
        "set pMin(wq) {1\n".to_owned(),
        "set pMin(wq) \"1\n".to_owned(),
        "set pMin(wq 1\n".to_owned(),
        "set pMin(wq) 1 2 3\n".to_owned(),
        "set\n".to_owned(),
        "set pMin(wq) -1\n".to_owned(),
        "set pMin(wq) 100\n".to_owned(),
        "set pMin(wq) 99999999999999999999\n".to_owned(),
        "set pMin(wq) abc\n".to_owned(),
        "set minMoveNum 1000\n".to_owned(),
        "set minHalfMoves 100\n".to_owned(),
        "set ignoreColors maybe\n".to_owned(),
        "set ::search::filter::operation 7\n".to_owned(),
        "set searchType Header\n".to_owned(),
        "}\n".to_owned(),
    ];
    for extra in &hostile {
        let result = import_material_sso_bytes(&sso_with(extra));
        // Either an error or a warning, never a silent success of a command.
        if let Ok(report) = result {
            assert!(
                !report.warnings.is_empty() || extra.starts_with("set pMin(wq) 1 2 3"),
                "accepted silently: {extra:?}"
            );
        }
    }
    assert!(!marker.exists(), "a command of the file was executed");
}

#[test]
fn truncated_and_corrupted_files_never_panic() {
    let valid = window_sso().into_bytes();
    for cut in 0..=valid.len() {
        if let Ok(report) = import_material_sso_bytes(&valid[..cut]) {
            assert!(report.search.white.queen.max <= 99);
            assert!(report.search.patterns.len() <= 32);
        }
    }
    for at in (0..valid.len()).step_by(3) {
        for byte in [0u8, 0xFF, b'{', b'"', b'[', b'\\', b'\r', b'$', 0xE9] {
            let mut broken = valid.clone();
            broken[at] = byte;
            drop(import_material_sso_bytes(&broken));
            drop(import_any_sso_bytes(&broken));
        }
    }
    // Pathological shapes.
    let shapes: Vec<Vec<u8>> = vec![
        vec![b'{'; 100_000],
        vec![b'"'; 100_001],
        "set a ".repeat(50_000).into_bytes(),
        "\n".repeat(200_000).into_bytes(),
        format!("set pMin(wq) {}\n", "9".repeat(100_000)).into_bytes(),
        format!("set pattPiece({}) wp\n", "1".repeat(1000)).into_bytes(),
        vec![0xFF; 5000],
        Vec::new(),
        b"\xEF\xBB\xBF".to_vec(),
    ];
    for shape in shapes {
        drop(import_material_sso_bytes(&shape));
        drop(import_any_sso_bytes(&shape));
    }
}

#[test]
fn nul_bytes_and_huge_files_are_refused_and_latin1_is_read() {
    assert!(matches!(
        import_material_sso_bytes(b"set searchType Material\0\n"),
        Err(SavedSearchError::NotText)
    ));
    let big = vec![b'#'; usize::try_from(MAX_FILE_BYTES).unwrap() + 1];
    assert!(matches!(
        import_material_sso_bytes(&big),
        Err(SavedSearchError::TooLarge { .. })
    ));
    let mut latin1 = b"# Pesquisa de posi\xE7\xE3o\n".to_vec();
    latin1.extend_from_slice(&sso_with(""));
    assert!(import_material_sso_bytes(&latin1).is_ok());
    let mut bom = b"\xEF\xBB\xBF".to_vec();
    bom.extend_from_slice(&sso_with(""));
    assert!(import_material_sso_bytes(&bom).is_ok());
    // CRLF and CR line ends.
    let crlf = String::from_utf8(sso_with("set pMax(wq) 1\n"))
        .unwrap()
        .replace('\n', "\r\n");
    assert_eq!(
        import_material_sso_bytes(crlf.as_bytes())
            .unwrap()
            .search
            .white
            .queen
            .max,
        1
    );
}

#[test]
fn the_dispatcher_and_the_readers_by_path() {
    let dir = tempfile::tempdir().unwrap();
    let material = dir.path().join("m.sso");
    std::fs::write(&material, sso_with("")).unwrap();
    assert!(matches!(
        import_any_sso(&material).unwrap(),
        AnySsoReport::Material(_)
    ));
    assert_eq!(
        import_material_sso(&material).unwrap().search,
        MaterialSearch::default()
    );
    let header = dir.path().join("h.sso");
    std::fs::write(&header, "set searchType Header\nset sWhite Kasparov\n").unwrap();
    assert!(matches!(
        import_any_sso(&header).unwrap(),
        AnySsoReport::Header(_)
    ));
    assert!(import_material_sso(&header).is_err());
    assert!(
        saved_search::import_sso(&material).is_err(),
        "the header reader still refuses material"
    );
    assert!(import_material_sso(&dir.path().join("missing.sso")).is_err());
}
