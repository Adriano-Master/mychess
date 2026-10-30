//! M6-2a contract tests for the pattern model, its validation and the TOML
//! file (`docs/specs/m6-2-taticas-e-padroes.md`: "Modelo do padrão",
//! "Validação", "Dados e formatos", rules 1, 21, 23 and 25). The TOML texts
//! below are copied from the spec, not from the implementation fixtures.

#![allow(clippy::too_many_lines)]

use mychess_db::pattern::*;
use std::fmt::Write as _;

use pretty_assertions::assert_eq;
use proptest::prelude::*;
use rstest::rstest;

const SPEC_MATE_IN_2: &str = r#"format = "mychess-pattern"
version = 1
name = "Mate em 2 (brancas)"
description = "A posição em que as brancas têm mate forçado em 2 lances (o mais curto)."

[scope]
min_move = 0
max_move = 999
hold = 1
end_only = false
ignore_colors = false

[root]
kind = "mate"
by = "white"
moves_min = 2
moves_max = 2
mode = "available"
forcing_only = false
max_nodes = 20000
"#;

const SPEC_FORK: &str = r#"format = "mychess-pattern"
version = 1
name = "Garfo de cavalo (brancas)"
description = "Um cavalo branco ataca duas peças que valem a pena (ou o rei), sem poder ser capturado com ganho."

[scope]
min_move = 1
max_move = 999
hold = 1
end_only = false
ignore_colors = false

[root]
kind = "relation"
relation = "forks"
when = "actor_just_moved"
new_this_move = true
min_targets = 2

[root.actor]
side = "white"
roles = ["knight"]
region = "anywhere"

[root.target]
roles = []
region = "anywhere"
"#;

const SPEC_PIN: &str = r#"format = "mychess-pattern"
version = 1
name = "Cravada absoluta (brancas)"
description = "Uma torre, bispo ou dama branca crava uma peça preta contra o rei preto."

[scope]
min_move = 1
max_move = 999
hold = 1
end_only = false
ignore_colors = false

[root]
kind = "relation"
relation = "pins"
when = "any"
new_this_move = false
min_targets = 2

[root.actor]
side = "white"
roles = ["bishop", "rook", "queen"]
region = "anywhere"

[root.target]
roles = ["knight", "bishop", "rook", "queen", "pawn"]
region = "anywhere"

[root.behind]
roles = ["king"]
region = "anywhere"
"#;

const SPEC_QUEEN_SACRIFICE: &str = r#"format = "mychess-pattern"
version = 1
name = "Sacrifício de dama com mate (brancas)"
description = "As brancas deixam a dama ser capturada (perda líquida de pelo menos 5) e dão mate em até 3 lances."

[scope]
min_move = 1
max_move = 999
hold = 1
end_only = false
ignore_colors = false

[root]
kind = "all"

[[root.items]]
kind = "result"
results = ["white_wins"]

[[root.items]]
kind = "sacrifice"
by = "white"
roles = ["queen"]
min_net = 5
outcome = "mate"
within = 3
"#;

const SPEC_CUSTOM: &str = r#"format = "mychess-pattern"
version = 1
name = "Custom"

[scope]
min_move = 1
max_move = 999
hold = 1
end_only = false
ignore_colors = false

[root]
kind = "all"

[[root.items]]
kind = "feature"
feature = "exposed_king"
side = "white"
region = "anywhere"
min = 1

[[root.items]]
kind = "any"

[[root.items.items]]
kind = "piece"
side = "black"
roles = ["queen"]
region = { named = "black_half" }
min = 0
max = 0

[[root.items.items]]
kind = "status"
status = "check"

[[root.items]]
kind = "not"

[root.items.item]
kind = "piece"
side = "white"
roles = ["queen"]
region = "anywhere"
min = 1
"#;

fn load(text: &str) -> Pattern {
    PatternFile::from_toml_str(text)
        .unwrap_or_else(|e| panic!("load: {e}\n{text}"))
        .pattern
}

fn err(text: &str) -> PatternError {
    PatternFile::from_toml_str(text).expect_err("must be refused")
}

fn sq(s: &str) -> Sq {
    Sq(s.parse().expect("square"))
}

fn leaf() -> Condition {
    Condition::SideToMove { side: Side::White }
}

fn pattern(root: Condition) -> Pattern {
    Pattern {
        name: "t".into(),
        description: String::new(),
        scope: Scope::default(),
        root,
    }
}

fn all_of(n: usize) -> Condition {
    Condition::All {
        items: vec![leaf(); n],
    }
}

const SPEC_FILES: [(&str, &str); 5] = [
    ("mate", SPEC_MATE_IN_2),
    ("fork", SPEC_FORK),
    ("pin", SPEC_PIN),
    ("sacrifice", SPEC_QUEEN_SACRIFICE),
    ("custom", SPEC_CUSTOM),
];

// ---------------------------------------------------------- round trips

#[test]
fn spec_presets_are_read_and_written_back_equal() {
    for (name, text) in SPEC_FILES {
        let first = PatternFile::from_toml_str(text).unwrap_or_else(|e| panic!("{name}: {e}"));
        let written = first.to_toml().expect("write");
        let second = PatternFile::from_toml_str(&written).expect("reread");
        assert_eq!(second, first, "{name}: value changed by a round trip");
        // Writing is a fixed point: the text does not drift.
        assert_eq!(second.to_toml().expect("write"), written, "{name}");
    }
}

#[test]
fn spec_presets_are_written_with_every_documented_key() {
    // The spec says the examples "are also the saved format". The writer may
    // add defaulted keys (`gain = "any"`) and order the keys of a table its
    // own way, but never drops a documented key and keeps the scalars of a
    // table before its sub-tables.
    for (name, text) in &SPEC_FILES[..4] {
        let written = PatternFile::from_toml_str(text)
            .expect("load")
            .to_toml()
            .expect("write");
        let keys = |t: &str| -> Vec<String> {
            let mut table = String::new();
            let mut out = Vec::new();
            for l in t.lines().filter(|l| !l.trim().is_empty()) {
                if l.starts_with('[') {
                    table = l.to_owned();
                } else {
                    out.push(format!(
                        "{table} {}",
                        l.split(" = ").next().unwrap_or("").trim()
                    ));
                }
            }
            out.sort();
            out
        };
        let (want, got) = (keys(text), keys(&written));
        for k in &want {
            assert!(got.contains(k), "{name}: key {k:?} lost\n{written}");
        }
        let tables: Vec<&str> = written.lines().filter(|l| l.starts_with('[')).collect();
        let spec_tables: Vec<&str> = text.lines().filter(|l| l.starts_with('[')).collect();
        assert_eq!(tables, spec_tables, "{name}: table order");
    }
}

#[test]
fn spec_examples_have_the_documented_trees() {
    let mate = load(SPEC_MATE_IN_2);
    assert_eq!(mate.scope.min_move, 0);
    assert_eq!(
        mate.root,
        Condition::Mate(MateCond {
            by: Who::White,
            moves_min: 2,
            moves_max: 2,
            mode: MateMode::Available,
            forcing_only: false,
            max_nodes: 20_000,
        })
    );
    let pin = load(SPEC_PIN);
    let Condition::Relation(r) = &pin.root else {
        panic!("relation");
    };
    assert_eq!(r.kind, RelKind::Pins);
    assert_eq!(r.when, When::Any);
    assert!(!r.new_this_move);
    assert_eq!(
        r.actor.roles,
        vec![RoleName::Bishop, RoleName::Rook, RoleName::Queen]
    );
    assert_eq!(
        r.behind.as_ref().map(|b| b.roles.clone()),
        Some(vec![RoleName::King])
    );
    let sac = load(SPEC_QUEEN_SACRIFICE);
    let Condition::All { items } = &sac.root else {
        panic!("all");
    };
    assert_eq!(items.len(), 2);
    assert_eq!(
        items[0],
        Condition::Result {
            results: vec![ResultKind::WhiteWins]
        }
    );
    assert_eq!(
        items[1],
        Condition::Sacrifice(SacrificeCond {
            by: Who::White,
            roles: vec![RoleName::Queen],
            min_net: 5,
            outcome: Outcome::Mate,
            within: 3,
        })
    );
    let custom = load(SPEC_CUSTOM);
    let Condition::All { items } = &custom.root else {
        panic!("all");
    };
    assert_eq!(items.len(), 3);
    assert!(matches!(items[1], Condition::Any { .. }));
    let Condition::Not { item } = &items[2] else {
        panic!("not");
    };
    assert!(matches!(**item, Condition::Piece(_)));
    let Condition::Any { items: inner } = &items[1] else {
        panic!("any");
    };
    assert_eq!(
        inner[0],
        Condition::Piece(PieceCond {
            side: Who::Black,
            roles: vec![RoleName::Queen],
            region: Region::Named(NamedRegion::BlackHalf),
            min: 0,
            max: Some(0),
        })
    );
}

#[test]
fn defaults_apply_when_optional_keys_are_absent() {
    let text = "format = \"mychess-pattern\"\nversion = 1\nname = \"x\"\n[root]\nkind = \"side_to_move\"\nside = \"white\"\n";
    let p = load(text);
    assert_eq!(p.scope, Scope::default());
    assert_eq!(p.scope.min_move, 1);
    assert_eq!(p.scope.max_move, 999);
    assert_eq!(p.scope.hold, 1);
    assert!(!p.scope.end_only && !p.scope.ignore_colors);
    assert_eq!(p.description, "");
    // A piece condition defaults to at least one, anywhere, any role.
    let piece = "format = \"mychess-pattern\"\nversion = 1\nname = \"x\"\n[root]\nkind = \"piece\"\nside = \"any\"\n";
    let Condition::Piece(c) = load(piece).root else {
        panic!("piece");
    };
    assert_eq!((c.min, c.max), (1, None));
    assert_eq!(c.roles, Vec::new());
    assert_eq!(c.region, Region::Anywhere);
}

#[test]
fn every_region_shape_survives_the_file() {
    let regions = vec![
        Region::Anywhere,
        Region::Squares(vec![sq("e4"), sq("d4"), sq("e4")]),
        Region::File('d'),
        Region::Rank(7),
        Region::OwnRank(2),
        Region::Rect {
            from: sq("f6"),
            to: sq("c3"),
        },
        Region::Named(NamedRegion::LongDiagonals),
    ];
    for r in regions {
        let p = pattern(Condition::Piece(PieceCond {
            side: Who::Opponent,
            roles: vec![RoleName::Knight, RoleName::King],
            region: r.clone(),
            min: 2,
            max: Some(3),
        }));
        let file = PatternFile::new(p);
        let text = file.to_toml().expect("write");
        assert_eq!(
            PatternFile::from_toml_str(&text).expect("read"),
            file,
            "{r:?}\n{text}"
        );
    }
}

// ------------------------------------------------------------ the header

#[rstest]
#[case::missing_format("version = 1\nname = \"x\"\n", "")]
#[case::other_format("format = \"cql\"\nversion = 1\nname = \"x\"\n", "cql")]
#[case::case_matters(
    "format = \"Mychess-Pattern\"\nversion = 1\nname = \"x\"\n",
    "Mychess-Pattern"
)]
fn wrong_format_is_reported_before_anything_else(#[case] text: &str, #[case] found: &str) {
    // Garbage after the header must not change the verdict.
    let full = format!("{text}[root]\nkind = \"nonsense\"\nunknown = 3\n");
    for t in [text.to_owned(), full] {
        match err(&t) {
            PatternError::WrongFormat { found: f } => assert_eq!(f, found, "{t}"),
            other => panic!("{other:?} for {t}"),
        }
    }
}

#[rstest]
#[case::future(2, 2)]
#[case::far_future(1000, 1000)]
#[case::zero(0, 0)]
#[case::negative(-1, -1)]
fn other_versions_are_unsupported_and_never_read_partially(#[case] v: i64, #[case] found: i64) {
    let text = format!(
        "format = \"mychess-pattern\"\nversion = {v}\nname = \"x\"\nbrand_new = true\n[root]\nkind = \"future_kind\"\n"
    );
    match err(&text) {
        PatternError::UnsupportedVersion { found: f } => assert_eq!(f, found),
        other => panic!("{other:?}"),
    }
}

#[test]
fn missing_version_is_unsupported() {
    let text = "format = \"mychess-pattern\"\nname = \"x\"\n[root]\nkind = \"side_to_move\"\nside = \"white\"\n";
    assert!(matches!(err(text), PatternError::UnsupportedVersion { .. }));
}

#[test]
fn a_version_that_is_not_a_number_is_a_typed_error_not_a_panic() {
    let text = "format = \"mychess-pattern\"\nversion = \"1\"\nname = \"x\"\n";
    assert!(matches!(
        err(text),
        PatternError::Toml(_) | PatternError::UnsupportedVersion { .. }
    ));
}

// ------------------------------------------------- deny_unknown_fields

#[rstest]
#[case::top_level(
    "name = \"Mate em 2 (brancas)\"",
    "name = \"Mate em 2 (brancas)\"\ntypo = 1"
)]
#[case::scope("hold = 1", "hold = 1\nholds = 2")]
#[case::root_leaf("max_nodes = 20000", "max_nodes = 20000\nmax_node = 1")]
#[case::misspelled_key("moves_min = 2", "moves_mim = 2\nmoves_min = 2")]
fn unknown_fields_are_errors_at_every_level(#[case] from: &str, #[case] to: &str) {
    let text = SPEC_MATE_IN_2.replacen(from, to, 1);
    assert_ne!(text, SPEC_MATE_IN_2);
    assert!(matches!(err(&text), PatternError::Toml(_)), "{text}");
}

#[rstest]
#[case::actor(
    "side = \"white\"\nroles = [\"knight\"]",
    "side = \"white\"\nroles = [\"knight\"]\ncolour = 1"
)]
#[case::target(
    "[root.target]\nroles = []",
    "[root.target]\nlabel = \"x\"\nroles = []"
)]
#[case::region_table(
    "region = \"anywhere\"\n\n[root.target]",
    "region = { squares = [\"e4\"], extra = 1 }\n\n[root.target]"
)]
fn unknown_fields_in_nested_tables_are_errors(#[case] from: &str, #[case] to: &str) {
    let text = SPEC_FORK.replacen(from, to, 1);
    assert_ne!(text, SPEC_FORK);
    assert!(matches!(err(&text), PatternError::Toml(_)), "{text}");
}

#[test]
fn unknown_kind_variant_or_role_is_an_error() {
    for (from, to) in [
        ("kind = \"mate\"", "kind = \"cql\""),
        ("by = \"white\"", "by = \"nobody\""),
        ("mode = \"available\"", "mode = \"sometimes\""),
    ] {
        let text = SPEC_MATE_IN_2.replacen(from, to, 1);
        assert!(matches!(err(&text), PatternError::Toml(_)), "{to}");
    }
    let text = SPEC_FORK.replacen("\"knight\"", "\"elephant\"", 1);
    assert!(matches!(err(&text), PatternError::Toml(_)));
}

// --------------------------------------------------------------- limits

fn nested_nots(count: usize) -> Condition {
    let mut c = leaf();
    for _ in 1..count {
        c = Condition::Not { item: Box::new(c) };
    }
    c
}

#[rstest]
#[case::depth_1(1, true)]
#[case::depth_6(6, true)]
#[case::depth_7(7, false)]
#[case::depth_20(20, false)]
fn depth_limit_is_six_counting_the_leaf(#[case] depth: usize, #[case] ok: bool) {
    let p = pattern(nested_nots(depth));
    match p.validate() {
        Ok(()) => assert!(ok, "depth {depth} must be refused"),
        Err(PatternError::TooDeep { max }) => {
            assert!(!ok, "depth {depth} must be accepted");
            assert_eq!(max, 6);
        }
        Err(other) => panic!("{other:?}"),
    }
}

#[test]
fn the_depth_limit_also_holds_through_the_file() {
    let mut text = String::from("format = \"mychess-pattern\"\nversion = 1\nname = \"x\"\n");
    // Seven levels of `not` around a leaf.
    for depth in 0..7 {
        write!(text, "[root{}]\nkind = \"not\"\n", ".item".repeat(depth)).expect("string write");
    }
    write!(
        text,
        "[root{}]\nkind = \"side_to_move\"\nside = \"white\"\n",
        ".item".repeat(7)
    )
    .expect("string write");
    assert!(matches!(err(&text), PatternError::TooDeep { .. }), "{text}");
}

#[test]
fn node_limit_is_48_inclusive() {
    // 1 + 25 + 22 = 48 nodes.
    let ok = Condition::All {
        items: vec![all_of(24), all_of(21)],
    };
    assert_eq!(pattern(ok.clone()).node_count(), 48);
    assert!(pattern(ok).validate().is_ok());
    // One more leaf: 49.
    let over = Condition::All {
        items: vec![all_of(24), all_of(22)],
    };
    assert_eq!(pattern(over.clone()).node_count(), 49);
    match pattern(over).validate() {
        Err(PatternError::TooManyNodes { max }) => assert_eq!(max, 48),
        other => panic!("{other:?}"),
    }
}

#[test]
fn group_size_limit_is_24() {
    assert!(pattern(all_of(24)).validate().is_ok());
    assert!(matches!(
        pattern(all_of(25)).validate(),
        Err(PatternError::InvalidValue { .. })
    ));
    let any = Condition::Any {
        items: vec![leaf(); 25],
    };
    assert!(matches!(
        pattern(any).validate(),
        Err(PatternError::InvalidValue { .. })
    ));
}

#[test]
fn empty_groups_carry_the_path_of_the_group() {
    for (cond, path) in [
        (Condition::All { items: vec![] }, "root.items"),
        (
            Condition::Not {
                item: Box::new(Condition::Any { items: vec![] }),
            },
            "root.item.items",
        ),
    ] {
        match pattern(cond).validate() {
            Err(PatternError::EmptyGroup { path: p }) => assert_eq!(p, path),
            other => panic!("{other:?}"),
        }
    }
    let text = "format = \"mychess-pattern\"\nversion = 1\nname = \"x\"\n[root]\nkind = \"all\"\nitems = []\n";
    assert!(matches!(err(text), PatternError::EmptyGroup { .. }));
}

fn padded_to(total: usize) -> Vec<u8> {
    let base = SPEC_MATE_IN_2.to_owned();
    assert!(base.len() < total);
    // A comment line keeps the document valid and the size exact.
    let pad = total - base.len();
    let mut text = base;
    text.push('#');
    text.push_str(&" ".repeat(pad - 1));
    let bytes = text.into_bytes();
    assert_eq!(bytes.len(), total);
    bytes
}

#[test]
fn file_size_limit_is_256_kib_inclusive() {
    assert_eq!(MAX_PATTERN_FILE_BYTES, 256 * 1024);
    let limit = 256 * 1024;
    assert!(PatternFile::from_bytes(&padded_to(limit)).is_ok());
    match PatternFile::from_bytes(&padded_to(limit + 1)) {
        Err(PatternError::TooLarge { size, max }) => {
            assert_eq!(size, u64::try_from(limit + 1).expect("fits"));
            assert_eq!(max, 256 * 1024);
        }
        other => panic!("{other:?}"),
    }
    // The size is checked before the content: binary junk of that size is
    // "too large", not "not text".
    let junk = vec![0u8; limit + 1];
    assert!(matches!(
        PatternFile::from_bytes(&junk),
        Err(PatternError::TooLarge { .. })
    ));
    // `from_toml_str` has the same limit.
    let text = String::from_utf8(padded_to(limit + 1)).expect("ascii");
    assert!(matches!(
        PatternFile::from_toml_str(&text),
        Err(PatternError::TooLarge { .. })
    ));
}

#[test]
fn nul_makes_the_file_not_text_even_when_small() {
    let mut bytes = SPEC_MATE_IN_2.as_bytes().to_vec();
    bytes.insert(10, 0);
    assert!(matches!(
        PatternFile::from_bytes(&bytes),
        Err(PatternError::NotText)
    ));
}

#[test]
fn encodings_bom_utf8_and_windows_1252() {
    let mut bom = b"\xEF\xBB\xBF".to_vec();
    bom.extend_from_slice(SPEC_MATE_IN_2.as_bytes());
    let from_bom = PatternFile::from_bytes(&bom).expect("bom");
    assert_eq!(
        from_bom,
        PatternFile::from_toml_str(SPEC_MATE_IN_2).expect("utf8")
    );
    // Windows-1252: the accented letter is one byte, which is not UTF-8.
    let text = SPEC_MATE_IN_2.replace("Mate em 2", "Mate em \u{e9}");
    let (encoded, _, bad) = encoding_rs::WINDOWS_1252.encode(&text);
    assert!(!bad);
    let file = PatternFile::from_bytes(&encoded).expect("cp1252");
    assert_eq!(file.pattern.name, "Mate em \u{e9} (brancas)");
    assert!(file.pattern.description.contains("posição"));
}

#[test]
fn hostile_documents_are_typed_errors_and_never_panic() {
    let header = "format = \"mychess-pattern\"\nversion = 1\nname = \"x\"\n";
    let mut docs: Vec<String> = vec![
        String::new(),
        "$".into(),
        "[".into(),
        "]".into(),
        "[[".into(),
        "= = =".into(),
        "\"unterminated".into(),
        "'''".into(),
        "\u{feff}".into(),
        format!("{header}[root"),
        format!("{header}[root]\nkind = \"all\"\nitems = [[[[[[[[[[1]]]]]]]]]]\n"),
        format!("{header}[root]\nkind = \"not\"\nitem = {{}}\n"),
        format!("{header}name = \"dup\"\n"),
        format!("x = {}1{}", "[".repeat(100_000), "]".repeat(100_000)),
        format!("x = {}1{}", "{ a = ".repeat(20_000), " }".repeat(20_000)),
        format!("[a{}]\n", ".b".repeat(30_000)),
        format!(
            "{header}[root]\nkind = \"all\"\n{}",
            "[[root.items]]\nkind = \"not\"\n".repeat(9_000)
        ),
        format!("name = \"{}\"\n", "é".repeat(100_000)),
        format!("{header}description = \"{}\"\n", "x".repeat(200_000)),
        format!(
            "{header}[root]\nkind = \"piece\"\nside = \"white\"\nroles = [{}]\n",
            "\"king\",".repeat(30_000)
        ),
        format!(
            "{header}[root]\nkind = \"piece\"\nside = \"white\"\nregion = {{ squares = [{}] }}\n",
            "\"e4\",".repeat(30_000)
        ),
        format!("{header}[root]\nkind = \"piece\"\nside = \"white\"\nmin = 99999999999999999999\n"),
        format!("{header}[root]\nkind = \"piece\"\nside = \"white\"\nmin = -1\n"),
        format!("{header}[root]\nkind = \"piece\"\nside = \"white\"\nregion = {{ file = \"\" }}\n"),
        format!(
            "{header}[root]\nkind = \"piece\"\nside = \"white\"\nregion = {{ file = \"\u{1f600}\" }}\n"
        ),
        "name = \"a\\u0000b\"\n".to_owned(),
        "name = \"\\uD800\"\n".to_owned(),
    ];
    for n in [1usize, 2, 3, 8, 40, 41, 100] {
        let mut s = header.to_owned();
        for d in 0..n {
            write!(s, "[root{}]\nkind = \"not\"\n", ".item".repeat(d)).expect("string write");
        }
        docs.push(s);
    }
    for text in &docs {
        let a = PatternFile::from_toml_str(text);
        let b = PatternFile::from_bytes(text.as_bytes());
        // Both entry points agree on success / failure (up to the size and
        // NUL rules that only the byte entry point has).
        assert_eq!(
            a.is_ok(),
            b.is_ok() || text.contains('\0'),
            "{}",
            &text[..text.len().min(80)]
        );
        if let Ok(file) = a {
            file.pattern
                .validate()
                .expect("anything accepted validates");
        }
    }
}

#[test]
fn deeply_nested_tables_in_the_file_are_refused_as_too_deep() {
    // 41+ levels of tables: refused before they reach serde (D3299 item 8).
    let mut s = String::from("format = \"mychess-pattern\"\nversion = 1\nname = \"x\"\n");
    write!(s, "[extra{}]\nv = 1\n", ".a".repeat(60)).expect("string write");
    let e = err(&s);
    assert!(matches!(e, PatternError::TooDeep { .. }), "{e:?}");
}

#[test]
fn truncated_valid_files_never_panic_and_never_load_partially() {
    for (name, text) in SPEC_FILES {
        let bytes = text.as_bytes();
        for cut in 0..bytes.len() {
            let r = PatternFile::from_bytes(&bytes[..cut]);
            if let Ok(file) = r {
                // A truncation that still parses must be a valid pattern.
                file.pattern
                    .validate()
                    .unwrap_or_else(|e| panic!("{name}@{cut}: {e}"));
            }
        }
    }
}

// ------------------------------------------------------------ validation

fn invalid_path(cond: Condition) -> String {
    match pattern(cond).validate() {
        Err(PatternError::InvalidValue { path, .. }) => path,
        other => panic!("expected InvalidValue, got {other:?}"),
    }
}

fn piece_with(region: Region, min: u8, max: Option<u8>) -> Condition {
    Condition::Piece(PieceCond {
        side: Who::White,
        roles: vec![],
        region,
        min,
        max,
    })
}

#[rstest]
#[case::file_i(Region::File('i'))]
#[case::file_upper(Region::File('A'))]
#[case::file_digit(Region::File('1'))]
#[case::rank_0(Region::Rank(0))]
#[case::rank_9(Region::Rank(9))]
#[case::own_rank_0(Region::OwnRank(0))]
#[case::own_rank_9(Region::OwnRank(9))]
#[case::no_squares(Region::Squares(vec![]))]
fn bad_regions_are_invalid_values(#[case] region: Region) {
    assert_eq!(invalid_path(piece_with(region, 1, None)), "root.region");
}

#[test]
fn region_limits_are_inclusive() {
    for ok in [
        Region::File('a'),
        Region::File('h'),
        Region::Rank(1),
        Region::Rank(8),
        Region::OwnRank(1),
        Region::OwnRank(8),
        Region::Squares(vec![sq("a1")]),
        Region::Rect {
            from: sq("h8"),
            to: sq("a1"),
        },
    ] {
        assert!(
            pattern(piece_with(ok.clone(), 1, None)).validate().is_ok(),
            "{ok:?}"
        );
    }
    let sixty_four: Vec<Sq> = (0..64u32)
        .map(|i| Sq(mychess_core::Square::new(i)))
        .collect();
    assert!(
        pattern(piece_with(Region::Squares(sixty_four.clone()), 1, None))
            .validate()
            .is_ok()
    );
    // Repeated squares are accepted as a set (rule 25) as long as the list
    // is within 64.
    assert!(
        pattern(piece_with(Region::Squares(vec![sq("e4"); 10]), 1, None))
            .validate()
            .is_ok()
    );
    let mut sixty_five = sixty_four;
    sixty_five.push(sq("a1"));
    assert_eq!(
        invalid_path(piece_with(Region::Squares(sixty_five), 1, None)),
        "root.region"
    );
}

#[test]
fn min_greater_than_max_is_refused_everywhere() {
    // PieceCond
    assert!(
        pattern(piece_with(Region::Anywhere, 3, Some(2)))
            .validate()
            .is_err()
    );
    assert!(
        pattern(piece_with(Region::Anywhere, 2, Some(2)))
            .validate()
            .is_ok()
    );
    assert!(
        pattern(piece_with(Region::Anywhere, 0, Some(0)))
            .validate()
            .is_ok()
    );
    // Counts
    let counts = |min, max| Counts {
        queens: Some(MinMax { min, max }),
        ..Counts::default()
    };
    let material = |white: Counts, diff: Option<MinMax<i32>>| {
        Condition::Material(MaterialCond {
            white,
            black: Counts::default(),
            diff,
            bishops: BishopColors::Either,
        })
    };
    assert!(pattern(material(counts(2, 1), None)).validate().is_err());
    assert!(pattern(material(counts(1, 1), None)).validate().is_ok());
    // diff
    assert!(
        pattern(material(
            Counts::default(),
            Some(MinMax { min: 3, max: -3 })
        ))
        .validate()
        .is_err()
    );
    assert!(
        pattern(material(
            Counts::default(),
            Some(MinMax { min: -3, max: 3 })
        ))
        .validate()
        .is_ok()
    );
    // every Counts field is checked, for both sides
    for field in 0..6 {
        let mut c = Counts::default();
        let bad = Some(MinMax { min: 5u8, max: 1u8 });
        match field {
            0 => c.queens = bad,
            1 => c.rooks = bad,
            2 => c.bishops = bad,
            3 => c.knights = bad,
            4 => c.minors = bad,
            _ => c.pawns = bad,
        }
        assert!(
            pattern(material(c.clone(), None)).validate().is_err(),
            "field {field}"
        );
        let black = Condition::Material(MaterialCond {
            white: Counts::default(),
            black: c,
            diff: None,
            bishops: BishopColors::Same,
        });
        assert!(pattern(black).validate().is_err(), "black field {field}");
    }
}

#[test]
fn scope_rules() {
    let with = |f: &dyn Fn(&mut Scope)| {
        let mut p = pattern(leaf());
        f(&mut p.scope);
        p.validate()
    };
    assert!(with(&|s| s.hold = 1).is_ok());
    assert!(with(&|s| s.hold = 40).is_ok());
    assert!(with(&|s| s.hold = 0).is_err());
    assert!(with(&|s| s.hold = 41).is_err());
    assert!(
        with(&|s| {
            s.min_move = 5;
            s.max_move = 5;
        })
        .is_ok()
    );
    assert!(
        with(&|s| {
            s.min_move = 6;
            s.max_move = 5;
        })
        .is_err()
    );
    assert!(
        with(&|s| {
            s.min_move = 0;
            s.max_move = 0;
        })
        .is_ok()
    );
    // end_only does not make the other fields free: they must still be sane.
    assert!(
        with(&|s| {
            s.end_only = true;
            s.hold = 0;
        })
        .is_err()
    );
}

#[test]
fn name_and_description_limits() {
    let named = |name: &str, desc: &str| {
        let mut p = pattern(leaf());
        p.name = name.into();
        p.description = desc.into();
        p.validate()
    };
    assert!(named("x", "").is_ok());
    assert!(named("", "").is_err());
    assert!(named(&"n".repeat(80), "").is_ok());
    assert!(named(&"n".repeat(81), "").is_err());
    // Limits count characters, not bytes.
    assert!(named(&"é".repeat(80), "").is_ok());
    assert!(named(&"é".repeat(81), "").is_err());
    assert!(named("a\u{0}b", "").is_err());
    assert!(named("a\nb", "").is_err());
    assert!(named("a\u{7f}b", "").is_err());
    assert!(named("x", &"d".repeat(500)).is_ok());
    assert!(named("x", &"d".repeat(501)).is_err());
    assert!(named("x", &"é".repeat(500)).is_ok());
    assert!(named("x", "line one\nline two").is_ok());
    assert!(named("x", "a\u{0}b").is_err());
}

#[test]
fn mate_rules() {
    let mate = |by, min, max, nodes| {
        Condition::Mate(MateCond {
            by,
            moves_min: min,
            moves_max: max,
            mode: MateMode::Available,
            forcing_only: true,
            max_nodes: nodes,
        })
    };
    assert!(pattern(mate(Who::White, 1, 8, 1_000)).validate().is_ok());
    assert!(pattern(mate(Who::Any, 1, 1, 2_000_000)).validate().is_ok());
    assert!(pattern(mate(Who::Mover, 3, 3, 20_000)).validate().is_ok());
    assert_eq!(invalid_path(mate(Who::Opponent, 1, 1, 20_000)), "root.by");
    assert!(pattern(mate(Who::White, 0, 1, 20_000)).validate().is_err());
    assert!(pattern(mate(Who::White, 1, 9, 20_000)).validate().is_err());
    assert!(pattern(mate(Who::White, 3, 2, 20_000)).validate().is_err());
    assert_eq!(invalid_path(mate(Who::White, 1, 1, 999)), "root.max_nodes");
    assert_eq!(
        invalid_path(mate(Who::White, 1, 1, 2_000_001)),
        "root.max_nodes"
    );
}

#[test]
fn sacrifice_rules() {
    let sac = |min_net, within, roles: Vec<RoleName>| {
        Condition::Sacrifice(SacrificeCond {
            by: Who::Any,
            roles,
            min_net,
            outcome: Outcome::Either,
            within,
        })
    };
    assert!(pattern(sac(1, 1, vec![])).validate().is_ok());
    assert!(
        pattern(sac(20, 8, vec![RoleName::Queen]))
            .validate()
            .is_ok()
    );
    assert!(pattern(sac(0, 3, vec![])).validate().is_err());
    assert!(pattern(sac(21, 3, vec![])).validate().is_err());
    assert!(pattern(sac(2, 0, vec![])).validate().is_err());
    assert!(pattern(sac(2, 9, vec![])).validate().is_err());
}

fn relation(kind: RelKind) -> RelationCond {
    RelationCond {
        kind,
        actor: PieceSel {
            side: Who::White,
            roles: vec![],
            region: Region::Anywhere,
        },
        target: TargetSel {
            roles: vec![],
            region: Region::Anywhere,
        },
        behind: None,
        gain: Gain::Any,
        min_targets: 2,
        when: When::Any,
        new_this_move: false,
    }
}

#[test]
fn relation_coherence_rules() {
    let behind = Some(TargetSel {
        roles: vec![RoleName::King],
        region: Region::Anywhere,
    });
    for kind in [RelKind::Pins, RelKind::Skewers] {
        let mut r = relation(kind);
        r.behind = behind.clone();
        assert!(
            pattern(Condition::Relation(r)).validate().is_ok(),
            "{kind:?}"
        );
    }
    for kind in [RelKind::Attacks, RelKind::Defends, RelKind::Forks] {
        let mut r = relation(kind);
        r.behind = behind.clone();
        assert_eq!(
            invalid_path(Condition::Relation(r)),
            "root.behind",
            "{kind:?}"
        );
    }
    // min_targets: 2..=8 for forks, exactly 2 (the default) elsewhere.
    for n in [2u8, 5, 8] {
        let mut r = relation(RelKind::Forks);
        r.min_targets = n;
        assert!(pattern(Condition::Relation(r)).validate().is_ok());
    }
    for n in [0u8, 1, 9, 255] {
        let mut r = relation(RelKind::Forks);
        r.min_targets = n;
        assert_eq!(invalid_path(Condition::Relation(r)), "root.min_targets");
    }
    for kind in [
        RelKind::Attacks,
        RelKind::Defends,
        RelKind::Pins,
        RelKind::Skewers,
    ] {
        let mut r = relation(kind);
        r.min_targets = 3;
        assert_eq!(
            invalid_path(Condition::Relation(r)),
            "root.min_targets",
            "{kind:?}"
        );
    }
    // gain only for attacks.
    for gain in [Gain::Undefended, Gain::WinsMaterial] {
        let mut r = relation(RelKind::Attacks);
        r.gain = gain;
        assert!(pattern(Condition::Relation(r)).validate().is_ok());
        for kind in [
            RelKind::Defends,
            RelKind::Pins,
            RelKind::Skewers,
            RelKind::Forks,
        ] {
            let mut r = relation(kind);
            r.gain = gain;
            assert_eq!(
                invalid_path(Condition::Relation(r)),
                "root.gain",
                "{kind:?}"
            );
        }
    }
    // The regions of actor, target and behind are validated with their path.
    let mut r = relation(RelKind::Pins);
    r.actor.region = Region::File('z');
    assert_eq!(invalid_path(Condition::Relation(r)), "root.actor.region");
    let mut r = relation(RelKind::Pins);
    r.target.region = Region::Rank(0);
    assert_eq!(invalid_path(Condition::Relation(r)), "root.target.region");
    let mut r = relation(RelKind::Pins);
    r.behind = Some(TargetSel {
        roles: vec![],
        region: Region::Squares(vec![]),
    });
    assert_eq!(invalid_path(Condition::Relation(r)), "root.behind.region");
}

#[test]
fn paths_point_at_the_defective_card() {
    let tree = Condition::All {
        items: vec![
            leaf(),
            Condition::Any {
                items: vec![
                    leaf(),
                    Condition::Not {
                        item: Box::new(piece_with(Region::Rank(0), 1, None)),
                    },
                ],
            },
        ],
    };
    assert_eq!(invalid_path(tree), "root.items[1].items[1].item.region");
}

#[test]
fn move_feature_and_result_rules() {
    let mv = |promotion: Vec<RoleName>| {
        Condition::Move(MoveCond {
            which: WhichMove::Previous,
            by: Who::Any,
            roles: vec![],
            from: Region::Anywhere,
            to: Region::Anywhere,
            capture: Flag::Both,
            check: Flag::Both,
            promotion,
            castling: None,
        })
    };
    assert!(
        pattern(mv(vec![RoleName::Queen, RoleName::Knight]))
            .validate()
            .is_ok()
    );
    assert!(pattern(mv(vec![RoleName::King])).validate().is_err());
    assert!(pattern(mv(vec![RoleName::Pawn])).validate().is_err());
    let feature = |min| {
        Condition::Feature(FeatureCond {
            feature: Feature::PassedPawn,
            side: Who::White,
            region: Region::Anywhere,
            min,
        })
    };
    assert!(pattern(feature(1)).validate().is_ok());
    assert!(pattern(feature(0)).validate().is_err());
    assert!(
        pattern(Condition::Result { results: vec![] })
            .validate()
            .is_err()
    );
    assert!(
        pattern(Condition::Result {
            results: vec![ResultKind::Unknown]
        })
        .validate()
        .is_ok()
    );
}

#[test]
fn saving_validates_and_dates_are_checked() {
    let mut p = pattern(leaf());
    p.name = String::new();
    assert!(PatternFile::new(p).to_toml().is_err());
    let mut f = PatternFile::new(pattern(leaf()));
    f.created = Some("2026-10-27".into());
    let text = f.to_toml().expect("write");
    assert!(text.contains("created = \"2026-10-27\""), "{text}");
    assert_eq!(
        PatternFile::from_toml_str(&text)
            .expect("read")
            .created
            .as_deref(),
        Some("2026-10-27")
    );
    for bad in ["2026-1-27", "27/10/2026", "2026-10-2x", "", "2026-10-271"] {
        f.created = Some(bad.into());
        assert!(f.to_toml().is_err(), "{bad}");
    }
}

// ------------------------------------------------------------ proptest

fn arb_sq() -> impl Strategy<Value = Sq> {
    (0u32..64).prop_map(|i| Sq(mychess_core::Square::new(i)))
}

fn arb_region() -> impl Strategy<Value = Region> {
    prop_oneof![
        Just(Region::Anywhere),
        prop::collection::vec(arb_sq(), 1..=6).prop_map(Region::Squares),
        (b'a'..=b'h').prop_map(|c| Region::File(char::from(c))),
        (1u8..=8).prop_map(Region::Rank),
        (1u8..=8).prop_map(Region::OwnRank),
        (arb_sq(), arb_sq()).prop_map(|(from, to)| Region::Rect { from, to }),
        prop::sample::select(vec![
            NamedRegion::Center,
            NamedRegion::ExtendedCenter,
            NamedRegion::QueenSide,
            NamedRegion::KingSide,
            NamedRegion::WhiteHalf,
            NamedRegion::BlackHalf,
            NamedRegion::LightSquares,
            NamedRegion::DarkSquares,
            NamedRegion::Edge,
            NamedRegion::LongDiagonals,
        ])
        .prop_map(Region::Named),
    ]
}

fn arb_who() -> impl Strategy<Value = Who> {
    prop::sample::select(vec![
        Who::White,
        Who::Black,
        Who::Mover,
        Who::Opponent,
        Who::Any,
    ])
}

/// `Who` for conditions about the move just played: `mover` is rejected by
/// validation (D3299 item 10).
fn arb_who_moved() -> impl Strategy<Value = Who> {
    prop::sample::select(vec![Who::White, Who::Black, Who::Opponent, Who::Any])
}

fn arb_roles() -> impl Strategy<Value = Vec<RoleName>> {
    prop::collection::vec(
        prop::sample::select(vec![
            RoleName::King,
            RoleName::Queen,
            RoleName::Rook,
            RoleName::Bishop,
            RoleName::Knight,
            RoleName::Pawn,
        ]),
        0..=4,
    )
}

fn arb_promo() -> impl Strategy<Value = Vec<RoleName>> {
    prop::collection::vec(
        prop::sample::select(vec![
            RoleName::Queen,
            RoleName::Rook,
            RoleName::Bishop,
            RoleName::Knight,
        ]),
        0..=3,
    )
}

fn arb_flag() -> impl Strategy<Value = Flag> {
    prop::sample::select(vec![Flag::Yes, Flag::No, Flag::Both])
}

fn arb_range() -> impl Strategy<Value = Option<MinMax<u8>>> {
    prop::option::of((0u8..=8, 0u8..=8).prop_map(|(a, b)| MinMax {
        min: a.min(b),
        max: a.max(b),
    }))
}

fn arb_counts() -> impl Strategy<Value = Counts> {
    (
        arb_range(),
        arb_range(),
        arb_range(),
        arb_range(),
        arb_range(),
        arb_range(),
    )
        .prop_map(|(queens, rooks, bishops, knights, minors, pawns)| Counts {
            queens,
            rooks,
            bishops,
            knights,
            minors,
            pawns,
        })
}

fn arb_relation() -> impl Strategy<Value = RelationCond> {
    (
        prop::sample::select(vec![
            RelKind::Attacks,
            RelKind::Defends,
            RelKind::Pins,
            RelKind::Skewers,
            RelKind::Forks,
        ]),
        (arb_who(), arb_roles(), arb_region()),
        (arb_roles(), arb_region()),
        prop::option::of((arb_roles(), arb_region())),
        prop::sample::select(vec![Gain::Any, Gain::Undefended, Gain::WinsMaterial]),
        2u8..=8,
        prop::sample::select(vec![When::Any, When::ActorJustMoved, When::ActorToMove]),
        any::<bool>(),
    )
        .prop_map(|(kind, a, t, b, gain, n, when, new)| RelationCond {
            kind,
            actor: PieceSel {
                side: a.0,
                roles: a.1,
                region: a.2,
            },
            target: TargetSel {
                roles: t.0,
                region: t.1,
            },
            // Only pins and skewers have `behind`, only attacks have `gain`,
            // only forks have `min_targets` (the validator insists).
            behind: if matches!(kind, RelKind::Pins | RelKind::Skewers) {
                b.map(|(roles, region)| TargetSel { roles, region })
            } else {
                None
            },
            gain: if kind == RelKind::Attacks {
                gain
            } else {
                Gain::Any
            },
            min_targets: if kind == RelKind::Forks { n } else { 2 },
            when,
            new_this_move: new,
        })
}

fn arb_leaf() -> impl Strategy<Value = Condition> {
    prop_oneof![
        (
            arb_who(),
            arb_roles(),
            arb_region(),
            0u8..=3,
            prop::option::of(3u8..=9)
        )
            .prop_map(
                |(side, roles, region, min, max)| Condition::Piece(PieceCond {
                    side,
                    roles,
                    region,
                    min,
                    max,
                })
            ),
        (
            arb_counts(),
            arb_counts(),
            prop::option::of((-20i32..=0, 0i32..=20)),
            prop::sample::select(vec![
                BishopColors::Either,
                BishopColors::Same,
                BishopColors::Opposite
            ])
        )
            .prop_map(
                |(white, black, diff, bishops)| Condition::Material(MaterialCond {
                    white,
                    black,
                    diff: diff.map(|(min, max)| MinMax { min, max }),
                    bishops,
                })
            ),
        prop::sample::select(vec![Side::White, Side::Black])
            .prop_map(|side| Condition::SideToMove { side }),
        prop::sample::select(vec![Phase::Opening, Phase::Middlegame, Phase::Endgame])
            .prop_map(|phase| Condition::Phase { phase }),
        prop::sample::select(vec![Status::Check, Status::Checkmate, Status::Stalemate])
            .prop_map(|status| Condition::Status { status }),
        prop::collection::vec(
            prop::sample::select(vec![
                ResultKind::WhiteWins,
                ResultKind::Draw,
                ResultKind::BlackWins,
                ResultKind::Unknown
            ]),
            1..=4
        )
        .prop_map(|results| Condition::Result { results }),
        (
            prop::sample::select(vec![
                Feature::PassedPawn,
                Feature::IsolatedPawn,
                Feature::DoubledPawns,
                Feature::ExposedKing,
                Feature::BishopPair
            ]),
            arb_who(),
            arb_region(),
            1u8..=8
        )
            .prop_map(
                |(feature, side, region, min)| Condition::Feature(FeatureCond {
                    feature,
                    side,
                    region,
                    min
                })
            ),
        arb_relation().prop_map(Condition::Relation),
        (
            prop::sample::select(vec![
                DiscoveredKind::Attack,
                DiscoveredKind::Check,
                DiscoveredKind::DoubleCheck
            ]),
            arb_who_moved(),
            arb_roles()
        )
            .prop_map(
                |(discovery, by, roles)| Condition::Discovered(DiscoveredCond {
                    discovery,
                    by,
                    roles
                })
            ),
        arb_who_moved().prop_map(|by| Condition::DoubleAttack(DoubleAttackCond { by })),
        (
            arb_who(),
            arb_roles().prop_map(|r| r
                .into_iter()
                .filter(|x| *x != RoleName::King)
                .collect::<Vec<_>>()),
            1u8..=20,
            prop::sample::select(vec![
                Outcome::Recovered,
                Outcome::Mate,
                Outcome::Either,
                Outcome::Any
            ]),
            1u8..=8
        )
            .prop_map(
                |(by, roles, min_net, outcome, within)| Condition::Sacrifice(SacrificeCond {
                    by,
                    roles,
                    min_net,
                    outcome,
                    within
                })
            ),
        (
            prop::sample::select(vec![Who::White, Who::Black, Who::Mover, Who::Any]),
            1u8..=8,
            1u8..=8,
            prop::sample::select(vec![MateMode::Played, MateMode::Available]),
            any::<bool>(),
            1_000u32..=2_000_000
        )
            .prop_map(|(by, a, b, mode, forcing_only, max_nodes)| Condition::Mate(
                MateCond {
                    by,
                    moves_min: a.min(b),
                    moves_max: a.max(b),
                    mode,
                    forcing_only,
                    max_nodes,
                }
            )),
        (
            prop::sample::select(vec![WhichMove::Previous, WhichMove::Next]),
            arb_who(),
            arb_roles(),
            arb_region(),
            arb_region(),
            arb_flag(),
            arb_flag(),
            arb_promo(),
            prop::option::of(prop::sample::select(vec![
                CastleKind::Short,
                CastleKind::Long,
                CastleKind::Any
            ]))
        )
            .prop_map(
                |(which, by, roles, from, to, capture, check, promotion, castling)| {
                    let by = if which == WhichMove::Previous && by == Who::Mover {
                        Who::Opponent
                    } else {
                        by
                    };
                    Condition::Move(MoveCond {
                        which,
                        by,
                        roles,
                        from,
                        to,
                        capture,
                        check,
                        promotion,
                        castling,
                    })
                }
            ),
    ]
}

fn arb_tree() -> impl Strategy<Value = Condition> {
    arb_leaf().prop_recursive(3, 40, 3, |inner| {
        prop_oneof![
            prop::collection::vec(inner.clone(), 1..=3).prop_map(|items| Condition::All { items }),
            prop::collection::vec(inner.clone(), 1..=3).prop_map(|items| Condition::Any { items }),
            inner.prop_map(|c| Condition::Not { item: Box::new(c) }),
        ]
    })
}

fn arb_scope() -> impl Strategy<Value = Scope> {
    (
        0u16..=500,
        0u16..=500,
        1u8..=40,
        any::<bool>(),
        any::<bool>(),
    )
        .prop_map(|(a, b, hold, end_only, ignore_colors)| Scope {
            min_move: a.min(b),
            max_move: a.max(b),
            hold,
            end_only,
            ignore_colors,
        })
}

fn arb_pattern() -> impl Strategy<Value = Pattern> {
    (
        "[A-Za-zÀ-ÿ0-9 ._()-]{1,80}",
        "[A-Za-zÀ-ÿ0-9 ._()\\n\\t-]{0,200}",
        arb_scope(),
        arb_tree(),
    )
        .prop_map(|(name, description, scope, root)| Pattern {
            name,
            description,
            scope,
            root,
        })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn valid_patterns_survive_toml_and_the_text_is_stable(p in arb_pattern()) {
        prop_assert!(p.validate().is_ok(), "generator must only make valid patterns: {:?}", p.validate());
        let file = PatternFile::new(p);
        let text = file.to_toml().expect("write");
        prop_assert!(text.len() as u64 <= MAX_PATTERN_FILE_BYTES);
        let again = PatternFile::from_toml_str(&text).expect("read");
        prop_assert_eq!(&again, &file);
        prop_assert_eq!(again.to_toml().expect("write"), text.clone());
        // The same through the byte entry point, with and without a BOM.
        prop_assert_eq!(&PatternFile::from_bytes(text.as_bytes()).expect("bytes"), &file);
        let mut bom = b"\xEF\xBB\xBF".to_vec();
        bom.extend_from_slice(text.as_bytes());
        prop_assert_eq!(&PatternFile::from_bytes(&bom).expect("bom"), &file);
    }

    #[test]
    fn corrupted_files_never_panic(
        idx in 0usize..5,
        cut in 0usize..4000,
        flip in 0usize..4000,
        byte in any::<u8>(),
    ) {
        let text = SPEC_FILES[idx].1.as_bytes().to_vec();
        let mut mutated = text.clone();
        let at = flip % mutated.len();
        mutated[at] = byte;
        let r = PatternFile::from_bytes(&mutated);
        if let Ok(file) = r {
            prop_assert!(file.pattern.validate().is_ok());
        }
        let cut = cut % (text.len() + 1);
        let r = PatternFile::from_bytes(&text[..cut]);
        if let Ok(file) = r {
            prop_assert!(file.pattern.validate().is_ok());
        }
    }

    #[test]
    fn arbitrary_bytes_never_panic(b in prop::collection::vec(any::<u8>(), 0..2000)) {
        if let Ok(file) = PatternFile::from_bytes(&b) {
            prop_assert!(file.pattern.validate().is_ok());
        }
    }
}
