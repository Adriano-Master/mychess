//! M6-2a: the pattern model, its validation and its TOML file format.

#![allow(clippy::too_many_lines)]

use mychess_db::pattern::*;
use proptest::prelude::*;

const MATE_IN_2: &str = include_str!("m6_2a_fixtures/mate_in_2.toml");
const KNIGHT_FORK: &str = include_str!("m6_2a_fixtures/knight_fork.toml");
const ABSOLUTE_PIN: &str = include_str!("m6_2a_fixtures/absolute_pin.toml");
const QUEEN_SACRIFICE: &str = include_str!("m6_2a_fixtures/queen_sacrifice.toml");
const CUSTOM: &str = include_str!("m6_2a_fixtures/custom_any_not.toml");

fn load(text: &str) -> Pattern {
    PatternFile::from_toml_str(text).expect("fixture").pattern
}

fn sq(s: &str) -> Sq {
    Sq(s.parse().expect("square"))
}

fn actor(side: Who, roles: &[RoleName]) -> PieceSel {
    PieceSel {
        side,
        roles: roles.to_vec(),
        region: Region::Anywhere,
    }
}

fn pattern(root: Condition) -> Pattern {
    Pattern {
        name: "t".into(),
        description: String::new(),
        scope: Scope::default(),
        root,
    }
}

fn leaf() -> Condition {
    Condition::SideToMove { side: Side::White }
}

#[test]
fn the_fixtures_load_and_match_the_built_trees() {
    assert_eq!(
        load(MATE_IN_2).root,
        Condition::Mate(MateCond {
            by: Who::White,
            moves_min: 2,
            moves_max: 2,
            mode: MateMode::Available,
            forcing_only: false,
            max_nodes: 20_000
        })
    );
    assert_eq!(load(MATE_IN_2).scope.min_move, 0);
    let fork = load(KNIGHT_FORK);
    let Condition::Relation(r) = &fork.root else {
        panic!("relation expected");
    };
    assert_eq!(r.kind, RelKind::Forks);
    assert_eq!(r.when, When::ActorJustMoved);
    assert!(r.new_this_move);
    assert_eq!(r.actor, actor(Who::White, &[RoleName::Knight]));
    assert_eq!(r.behind, None);
    let pin = load(ABSOLUTE_PIN);
    let Condition::Relation(r) = &pin.root else {
        panic!("relation expected");
    };
    assert_eq!(r.kind, RelKind::Pins);
    assert_eq!(
        r.behind.as_ref().map(|b| b.roles.clone()),
        Some(vec![RoleName::King])
    );
    let Condition::All { items } = load(QUEEN_SACRIFICE).root else {
        panic!("all expected");
    };
    assert_eq!(items.len(), 2);
    assert_eq!(
        items[1],
        Condition::Sacrifice(SacrificeCond {
            by: Who::White,
            roles: vec![RoleName::Queen],
            min_net: 5,
            outcome: Outcome::Mate,
            within: 3
        })
    );
}

#[test]
fn the_custom_example_has_any_and_not() {
    let p = load(CUSTOM);
    let Condition::All { items } = &p.root else {
        panic!("all expected");
    };
    assert!(matches!(items[1], Condition::Any { .. }));
    assert!(matches!(items[2], Condition::Not { .. }));
    let Condition::Any { items: inner } = &items[1] else {
        panic!()
    };
    let Condition::Piece(q) = &inner[0] else {
        panic!("piece expected")
    };
    assert_eq!(q.region, Region::Named(NamedRegion::BlackHalf));
    assert_eq!((q.min, q.max), (0, Some(0)));
}

#[test]
fn fixtures_survive_a_roundtrip() {
    for text in [
        MATE_IN_2,
        KNIGHT_FORK,
        ABSOLUTE_PIN,
        QUEEN_SACRIFICE,
        CUSTOM,
    ] {
        let file = PatternFile::from_toml_str(text).expect("fixture");
        let out = file.to_toml().expect("serialize");
        assert!(
            out.starts_with("format = \"mychess-pattern\"\nversion = 1\n"),
            "{out}"
        );
        assert_eq!(PatternFile::from_toml_str(&out).expect("reload"), file);
    }
}

#[test]
fn the_created_date_is_kept_and_checked() {
    let mut file = PatternFile::new(load(MATE_IN_2));
    file.created = Some("2026-10-27".into());
    let text = file.to_toml().expect("serialize");
    assert_eq!(
        PatternFile::from_toml_str(&text).expect("reload").created,
        file.created
    );
    file.created = Some("27/10/2026".into());
    assert!(matches!(
        file.to_toml(),
        Err(PatternError::InvalidValue { .. })
    ));
}

#[test]
fn unknown_fields_wrong_format_and_versions_are_typed_errors() {
    let extra = MATE_IN_2.replace("max_nodes = 20000", "max_nodes = 20000\nbogus = 1");
    assert!(matches!(
        PatternFile::from_toml_str(&extra),
        Err(PatternError::Toml(_))
    ));
    let fmt = MATE_IN_2.replace("mychess-pattern", "other");
    assert!(matches!(
        PatternFile::from_toml_str(&fmt),
        Err(PatternError::WrongFormat { found }) if found == "other"
    ));
    // A future version is reported as such even if it has new fields.
    let future = MATE_IN_2
        .replace("version = 1", "version = 2")
        .replace("max_nodes = 20000", "max_nodes = 20000\nnew_thing = true");
    assert!(matches!(
        PatternFile::from_toml_str(&future),
        Err(PatternError::UnsupportedVersion { found: 2 })
    ));
    let no_format = MATE_IN_2.replace("format = \"mychess-pattern\"\n", "");
    assert!(matches!(
        PatternFile::from_toml_str(&no_format),
        Err(PatternError::WrongFormat { .. })
    ));
    let bad_kind = MATE_IN_2.replace("kind = \"mate\"", "kind = \"cql\"");
    assert!(matches!(
        PatternFile::from_toml_str(&bad_kind),
        Err(PatternError::Toml(_))
    ));
    let bad_square =
        KNIGHT_FORK.replace("region = \"anywhere\"", "region = { squares = [\"z9\"] }");
    assert!(matches!(
        PatternFile::from_toml_str(&bad_square),
        Err(PatternError::Toml(_))
    ));
}

#[test]
fn file_limits_and_encodings() {
    let big = format!("{MATE_IN_2}\n# {}\n", "x".repeat(300 * 1024));
    assert!(matches!(
        PatternFile::from_bytes(big.as_bytes()),
        Err(PatternError::TooLarge { .. })
    ));
    assert!(matches!(
        PatternFile::from_bytes(b"format = \"mychess-pattern\"\0"),
        Err(PatternError::NotText)
    ));
    // BOM is accepted.
    let mut bom = b"\xEF\xBB\xBF".to_vec();
    bom.extend_from_slice(MATE_IN_2.as_bytes());
    assert!(PatternFile::from_bytes(&bom).is_ok());
    // Windows-1252 text: the name has "ç" as a single 0xE7 byte.
    let latin1 = MATE_IN_2.replace("Mate em 2 (brancas)", "Mate em 2 ç");
    let (bytes, _, _) = encoding_rs::WINDOWS_1252.encode(&latin1);
    let file = PatternFile::from_bytes(&bytes).expect("latin1");
    assert_eq!(file.pattern.name, "Mate em 2 ç");
}

#[test]
fn hostile_inputs_never_panic() {
    let deep_tables = format!("[root{}]\nkind = \"not\"\n", ".item".repeat(20_000));
    let deep_inline = format!("x = {}1{}", "[".repeat(5_000), "]".repeat(5_000));
    let inputs: Vec<String> = vec![
        String::new(),
        "$".into(),
        "[".into(),
        "[[root.items]]".into(),
        "format = [".into(),
        "format = \"mychess-pattern\"\nversion = 1\nname = \"x\"\n".into(),
        "\u{0}".into(),
        deep_tables,
        deep_inline,
        format!(
            "format = \"mychess-pattern\"\nversion = 1\nname = \"x\"\n[root]\nkind = \"not\"\n{}",
            "[root.item]\nkind = \"not\"\n"
        ),
    ];
    for text in inputs {
        drop(PatternFile::from_toml_str(&text));
        drop(PatternFile::from_bytes(text.as_bytes()));
    }
    for bytes in [
        &b"\xff\xfe\x00"[..],
        &b"\x80\x81\x82"[..],
        &[0xE7u8; 100][..],
    ] {
        drop(PatternFile::from_bytes(bytes));
    }
}

#[test]
fn a_deep_not_chain_is_rejected_as_too_deep() {
    // Depth 7: six nots around a leaf.
    let mut root = leaf();
    for _ in 0..6 {
        root = Condition::Not {
            item: Box::new(root),
        };
    }
    assert!(matches!(
        pattern(root).validate(),
        Err(PatternError::TooDeep { .. })
    ));
    let mut ok = leaf();
    for _ in 0..5 {
        ok = Condition::Not { item: Box::new(ok) };
    }
    assert!(pattern(ok).validate().is_ok());
}

#[test]
fn too_many_nodes_and_empty_groups() {
    let wide = |n| Condition::All {
        items: vec![leaf(); n],
    };
    let mut items: Vec<Condition> = (0..3).map(|_| wide(20)).collect();
    items.push(leaf());
    let big = Condition::All { items };
    assert!(matches!(
        pattern(big).validate(),
        Err(PatternError::TooManyNodes { .. })
    ));
    assert!(pattern(wide(24)).validate().is_ok());
    assert!(matches!(
        pattern(wide(25)).validate(),
        Err(PatternError::InvalidValue { .. })
    ));
    let empty = Condition::All {
        items: vec![leaf(), Condition::Any { items: vec![] }],
    };
    match pattern(empty).validate() {
        Err(PatternError::EmptyGroup { path }) => assert_eq!(path, "root.items[1].items"),
        other => panic!("{other:?}"),
    }
}

fn bad_path(root: Condition) -> String {
    match pattern(root).validate() {
        Err(PatternError::InvalidValue { path, .. }) => path,
        other => panic!("expected InvalidValue, got {other:?}"),
    }
}

fn piece(region: Region, min: u8, max: Option<u8>) -> Condition {
    Condition::Piece(PieceCond {
        side: Who::White,
        roles: vec![],
        region,
        min,
        max,
    })
}

#[test]
fn field_validation_reports_a_path() {
    assert_eq!(bad_path(piece(Region::File('i'), 1, None)), "root.region");
    assert_eq!(bad_path(piece(Region::Rank(0), 1, None)), "root.region");
    assert_eq!(bad_path(piece(Region::OwnRank(9), 1, None)), "root.region");
    assert_eq!(
        bad_path(piece(Region::Squares(vec![]), 1, None)),
        "root.region"
    );
    assert_eq!(
        bad_path(piece(Region::Squares(vec![sq("a1"); 65]), 1, None)),
        "root.region"
    );
    assert_eq!(bad_path(piece(Region::Anywhere, 3, Some(1))), "root");
    let nested = Condition::All {
        items: vec![leaf(), leaf(), piece(Region::File('z'), 1, None)],
    };
    assert_eq!(bad_path(nested), "root.items[2].region");
    // Squares with repeats are fine (a set); rect corners in any order too.
    assert!(
        pattern(piece(Region::Squares(vec![sq("a1"); 64]), 1, None))
            .validate()
            .is_ok()
    );
    assert!(
        pattern(piece(
            Region::Rect {
                from: sq("f6"),
                to: sq("c3")
            },
            1,
            None
        ))
        .validate()
        .is_ok()
    );
}

#[test]
fn scope_name_and_description_limits() {
    let mut p = pattern(leaf());
    p.scope.hold = 0;
    assert!(p.validate().is_err());
    p.scope.hold = 41;
    assert!(p.validate().is_err());
    p.scope.hold = 40;
    assert!(p.validate().is_ok());
    p.scope.min_move = 10;
    p.scope.max_move = 9;
    assert!(p.validate().is_err());
    let mut p = pattern(leaf());
    p.name = String::new();
    assert!(p.validate().is_err());
    p.name = "x".repeat(81);
    assert!(p.validate().is_err());
    p.name = "a\nb".into();
    assert!(p.validate().is_err());
    p.name = "x".repeat(80);
    assert!(p.validate().is_ok());
    p.description = "d".repeat(501);
    assert!(p.validate().is_err());
}

fn relation(kind: RelKind) -> RelationCond {
    RelationCond {
        kind,
        actor: actor(Who::White, &[]),
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
fn relation_coherence() {
    let ok = |r: RelationCond| pattern(Condition::Relation(r)).validate().is_ok();
    assert!(ok(relation(RelKind::Attacks)));
    let mut r = relation(RelKind::Attacks);
    r.behind = Some(TargetSel {
        roles: vec![],
        region: Region::Anywhere,
    });
    assert!(!ok(r.clone()));
    r.kind = RelKind::Pins;
    assert!(ok(r));
    let mut r = relation(RelKind::Pins);
    r.min_targets = 3;
    assert!(!ok(r));
    let mut r = relation(RelKind::Forks);
    r.min_targets = 9;
    assert!(!ok(r.clone()));
    r.min_targets = 1;
    assert!(!ok(r.clone()));
    r.min_targets = 4;
    assert!(ok(r));
    let mut r = relation(RelKind::Defends);
    r.gain = Gain::Undefended;
    assert!(!ok(r));
    let mut r = relation(RelKind::Attacks);
    r.gain = Gain::WinsMaterial;
    assert!(ok(r));
}

#[test]
fn mate_sacrifice_material_and_move_validation() {
    let mate = |by, lo, hi, nodes| {
        pattern(Condition::Mate(MateCond {
            by,
            moves_min: lo,
            moves_max: hi,
            mode: MateMode::Played,
            forcing_only: true,
            max_nodes: nodes,
        }))
        .validate()
    };
    assert!(mate(Who::Any, 1, 8, 1_000).is_ok());
    assert!(mate(Who::Opponent, 1, 2, 5_000).is_err());
    assert!(mate(Who::White, 0, 2, 5_000).is_err());
    assert!(mate(Who::White, 3, 2, 5_000).is_err());
    assert!(mate(Who::White, 1, 9, 5_000).is_err());
    assert!(mate(Who::White, 1, 2, 999).is_err());
    assert!(mate(Who::White, 1, 2, 2_000_001).is_err());
    let sac = |min_net, within, roles: Vec<RoleName>| {
        pattern(Condition::Sacrifice(SacrificeCond {
            by: Who::White,
            roles,
            min_net,
            outcome: Outcome::Any,
            within,
        }))
        .validate()
    };
    assert!(sac(1, 1, vec![]).is_ok());
    assert!(sac(0, 1, vec![]).is_err());
    assert!(sac(21, 1, vec![]).is_err());
    assert!(sac(2, 0, vec![]).is_err());
    assert!(sac(2, 9, vec![]).is_err());
    assert!(sac(2, 3, vec![RoleName::King]).is_err());
    let mut material = MaterialCond::default();
    material.white.queens = Some(MinMax { min: 2, max: 1 });
    assert_eq!(bad_path(Condition::Material(material)), "root.white.queens");
    let material = MaterialCond {
        diff: Some(MinMax { min: 3, max: -3 }),
        ..MaterialCond::default()
    };
    assert_eq!(bad_path(Condition::Material(material)), "root.diff");
    let promo = Condition::Move(MoveCond {
        which: WhichMove::Previous,
        by: Who::Any,
        roles: vec![],
        from: Region::Anywhere,
        to: Region::Anywhere,
        capture: Flag::Both,
        check: Flag::Both,
        promotion: vec![RoleName::Pawn],
        castling: None,
    });
    assert_eq!(bad_path(promo), "root.promotion");
    assert!(
        pattern(Condition::Result { results: vec![] })
            .validate()
            .is_err()
    );
}

// --- property: valid patterns survive TOML ------------------------------------

fn arb_sq() -> impl Strategy<Value = Sq> {
    (0u8..64).prop_map(|i| Sq(mychess_core::Square::new(u32::from(i))))
}

fn arb_region() -> impl Strategy<Value = Region> {
    prop_oneof![
        Just(Region::Anywhere),
        prop::collection::vec(arb_sq(), 1..5).prop_map(Region::Squares),
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
            NamedRegion::LongDiagonals
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

/// `Who` for conditions about the move just played: `mover` is rejected.
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
        0..4,
    )
}

fn arb_minmax() -> impl Strategy<Value = Option<MinMax<u8>>> {
    prop::option::of((0u8..5, 0u8..5).prop_map(|(a, b)| MinMax {
        min: a.min(b),
        max: a.max(b),
    }))
}

fn arb_counts() -> impl Strategy<Value = Counts> {
    (
        arb_minmax(),
        arb_minmax(),
        arb_minmax(),
        arb_minmax(),
        arb_minmax(),
        arb_minmax(),
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
        arb_who(),
        arb_roles(),
        arb_region(),
        arb_roles(),
        arb_region(),
        prop::option::of((arb_roles(), arb_region())),
        2u8..=8,
        prop::sample::select(vec![Gain::Any, Gain::Undefended, Gain::WinsMaterial]),
        prop::sample::select(vec![When::Any, When::ActorJustMoved, When::ActorToMove]),
        any::<bool>(),
    )
        .prop_map(
            |(kind, side, ar, areg, tr, treg, behind, mt, gain, when, new)| RelationCond {
                kind,
                actor: PieceSel {
                    side,
                    roles: ar,
                    region: areg,
                },
                target: TargetSel {
                    roles: tr,
                    region: treg,
                },
                behind: if matches!(kind, RelKind::Pins | RelKind::Skewers) {
                    behind.map(|(roles, region)| TargetSel { roles, region })
                } else {
                    None
                },
                gain: if kind == RelKind::Attacks {
                    gain
                } else {
                    Gain::Any
                },
                min_targets: if kind == RelKind::Forks { mt } else { 2 },
                when,
                new_this_move: new,
            },
        )
}

fn arb_leaf() -> impl Strategy<Value = Condition> {
    let piece = (
        arb_who(),
        arb_roles(),
        arb_region(),
        0u8..4,
        prop::option::of(4u8..9),
    )
        .prop_map(|(side, roles, region, min, max)| {
            Condition::Piece(PieceCond {
                side,
                roles,
                region,
                min,
                max,
            })
        });
    let material = (
        arb_counts(),
        arb_counts(),
        prop::option::of(-5i32..5),
        any::<u8>(),
    )
        .prop_map(|(white, black, d, b)| MaterialCond {
            white,
            black,
            diff: d.map(|d| MinMax { min: d, max: d + 3 }),
            bishops: match b % 3 {
                0 => BishopColors::Either,
                1 => BishopColors::Same,
                _ => BishopColors::Opposite,
            },
        });
    let mv = (
        arb_who(),
        arb_roles(),
        arb_region(),
        arb_region(),
        any::<bool>(),
    )
        .prop_map(|(by, roles, from, to, next)| {
            // `mover` never matches the previous move and is rejected.
            let by = if !next && by == Who::Mover {
                Who::Opponent
            } else {
                by
            };
            Condition::Move(MoveCond {
                which: if next {
                    WhichMove::Next
                } else {
                    WhichMove::Previous
                },
                by,
                roles,
                from,
                to,
                capture: Flag::Yes,
                check: Flag::No,
                promotion: vec![RoleName::Queen, RoleName::Knight],
                castling: Some(CastleKind::Short),
            })
        });
    let feature = (
        prop::sample::select(vec![
            Feature::PassedPawn,
            Feature::IsolatedPawn,
            Feature::DoubledPawns,
            Feature::ExposedKing,
            Feature::BishopPair,
        ]),
        arb_who(),
        arb_region(),
        1u8..5,
    )
        .prop_map(|(feature, side, region, min)| {
            Condition::Feature(FeatureCond {
                feature,
                side,
                region,
                min,
            })
        });
    let mate = (
        prop::sample::select(vec![Who::White, Who::Black, Who::Mover, Who::Any]),
        1u8..=4,
        0u8..=4,
        any::<bool>(),
        any::<bool>(),
        1_000u32..2_000_000,
    )
        .prop_map(|(by, lo, extra, played, forcing, nodes)| {
            Condition::Mate(MateCond {
                by,
                moves_min: lo,
                moves_max: lo + extra,
                mode: if played {
                    MateMode::Played
                } else {
                    MateMode::Available
                },
                forcing_only: forcing,
                max_nodes: nodes,
            })
        });
    let sacrifice = (arb_who(), 1u8..=20, 1u8..=8, 0usize..4).prop_map(|(by, net, within, o)| {
        Condition::Sacrifice(SacrificeCond {
            by,
            roles: vec![RoleName::Queen],
            min_net: net,
            outcome: [
                Outcome::Recovered,
                Outcome::Mate,
                Outcome::Either,
                Outcome::Any,
            ][o],
            within,
        })
    });
    prop_oneof![
        piece,
        material.prop_map(Condition::Material),
        prop::sample::select(vec![Side::White, Side::Black])
            .prop_map(|side| Condition::SideToMove { side }),
        prop::sample::select(vec![Phase::Opening, Phase::Middlegame, Phase::Endgame])
            .prop_map(|phase| Condition::Phase { phase }),
        prop::sample::select(vec![Status::Check, Status::Checkmate, Status::Stalemate])
            .prop_map(|status| Condition::Status { status }),
        mv,
        prop::collection::vec(
            prop::sample::select(vec![
                ResultKind::WhiteWins,
                ResultKind::Draw,
                ResultKind::BlackWins,
                ResultKind::Unknown
            ]),
            1..4
        )
        .prop_map(|results| Condition::Result { results }),
        feature,
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
        sacrifice,
        mate,
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

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    #[test]
    fn every_valid_pattern_survives_toml(
        root in arb_tree(),
        hold in 1u8..=40,
        min in 0u16..20,
        extra in 0u16..100,
        flags in any::<(bool, bool)>(),
        desc in "[a-zA-Z ,.]{0,40}",
    ) {
        let mut p = pattern(root);
        p.description = desc;
        p.scope = Scope { min_move: min, max_move: min + extra, hold, end_only: flags.0, ignore_colors: flags.1 };
        prop_assume!(p.validate().is_ok());
        let file = PatternFile::new(p);
        let text = file.to_toml().expect("serialize");
        let back = PatternFile::from_toml_str(&text).expect("reload");
        prop_assert_eq!(back, file);
    }

    #[test]
    fn arbitrary_text_never_panics(s in "\\PC{0,200}") {
        drop(PatternFile::from_toml_str(&s));
        drop(PatternFile::from_bytes(s.as_bytes()));
    }

    #[test]
    fn arbitrary_bytes_never_panic(b in prop::collection::vec(any::<u8>(), 0..300)) {
        drop(PatternFile::from_bytes(&b));
    }
}

#[test]
fn mover_is_rejected_for_conditions_about_the_move_just_played() {
    let discovered = Condition::Discovered(DiscoveredCond {
        discovery: DiscoveredKind::Check,
        by: Who::Mover,
        roles: vec![],
    });
    let double = Condition::DoubleAttack(DoubleAttackCond { by: Who::Mover });
    let prev = |which, by| {
        Condition::Move(MoveCond {
            which,
            by,
            roles: vec![],
            from: Region::Anywhere,
            to: Region::Anywhere,
            capture: Flag::Both,
            check: Flag::Both,
            promotion: vec![],
            castling: None,
        })
    };
    for cond in [discovered, double, prev(WhichMove::Previous, Who::Mover)] {
        match pattern(cond).validate() {
            Err(PatternError::MoverNotAllowed { path }) => assert_eq!(path, "root.by"),
            other => panic!("expected MoverNotAllowed, got {other:?}"),
        }
    }
    // `opponent` (the side that just moved) and `mover` for the *next* move are fine.
    assert!(
        pattern(prev(WhichMove::Previous, Who::Opponent))
            .validate()
            .is_ok()
    );
    assert!(
        pattern(prev(WhichMove::Next, Who::Mover))
            .validate()
            .is_ok()
    );
    assert!(
        pattern(Condition::DoubleAttack(DoubleAttackCond {
            by: Who::Opponent
        }))
        .validate()
        .is_ok()
    );
}
