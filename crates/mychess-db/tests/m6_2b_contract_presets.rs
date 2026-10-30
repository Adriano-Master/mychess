//! Contract tests of the preset catalogue (spec `docs/specs/m6-2-taticas-e-padroes.md`,
//! "Catálogo dos presets", rules 22 and 27, D3279, D3280): ids, lookup,
//! parameter ranges, every preset builds / validates / round-trips, and each
//! one has a positive and a negative position written here.
#![allow(
    clippy::unwrap_used,
    clippy::needless_pass_by_value,
    clippy::too_many_lines,
    clippy::similar_names,
    unused_qualifications,
    clippy::assert_is_empty,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::cast_possible_truncation,
    clippy::trivially_copy_pass_by_ref,
    dead_code
)]

#[path = "common/m3_3a_support.rs"]
mod base;
#[path = "common/m6_1a_support.rs"]
mod support;

use std::collections::HashSet;

use mychess_db::pattern::model::{
    DiscoveredKind, MateMode, Outcome, Pattern, PatternError, PatternFile, RoleName, Who,
};
use mychess_db::pattern::presets::{self, Params};
use mychess_db::pattern::{Line, eval_position, matching_plies};
use pretty_assertions::assert_eq;
use proptest::prelude::*;
use support::{game, game_fen};

fn preset(id: &str, p: Params) -> Pattern {
    presets::build(id, &p).unwrap()
}

fn by(who: Who) -> Params {
    Params {
        by: who,
        ..Params::default()
    }
}

fn line_of(fen: Option<&str>, ucis: &str) -> Line {
    let g = match fen {
        Some(f) => game_fen(f, ucis),
        None => game(ucis),
    };
    Line::from_game(&g, 2048)
}

/// The pattern holds at position `i` of the line.
fn at(p: &Pattern, line: &Line, i: usize) -> bool {
    eval_position(&p.root, line, i, None).is_true()
}

/// The pattern holds at the position of the FEN (no previous move).
fn static_holds(p: &Pattern, fen: &str) -> bool {
    at(p, &line_of(Some(fen), ""), 0)
}

// ---------------------------------------------------------- catalogue

#[test]
fn the_catalogue_has_21_distinct_ids_and_find_returns_each() {
    let all = presets::all();
    assert_eq!(all.len(), 21);
    let ids: HashSet<_> = all.iter().map(|p| p.id).collect();
    assert_eq!(ids.len(), 21);
    let keys: HashSet<_> = all.iter().map(|p| p.name_key).collect();
    assert_eq!(keys.len(), 21, "every preset has its own Fluent key");
    for p in &all {
        assert!(!p.id.is_empty() && !p.id.contains(char::is_whitespace));
        assert!(p.name_key.starts_with("pattern-preset-"), "{}", p.name_key);
        assert_eq!(presets::find(p.id).unwrap().id, p.id);
    }
    for id in [
        "mate_in",
        "pin",
        "fork",
        "skewer",
        "discovered",
        "double_attack",
        "sacrifice",
        "hanging",
        "passed_pawn",
        "exposed_king",
        "back_rank_mate",
        "promotion",
        "iqp",
        "iqp_black",
        "iqp_both",
        "hanging_pawns",
        "maroczy",
        "fianchetto_light",
        "fianchetto_dark",
        "fianchetto_four",
        "opposite_castling",
    ] {
        assert!(ids.contains(id), "{id} is in the spec catalogue");
    }
}

#[test]
fn find_and_build_of_an_unknown_id_are_none_and_an_error() {
    for id in ["", "nope", "FORK", "fork ", "mate_in\0", "../fork"] {
        assert!(presets::find(id).is_none(), "{id:?}");
        assert!(
            matches!(
                presets::build(id, &Params::default()),
                Err(PatternError::InvalidValue { .. })
            ),
            "{id:?}"
        );
    }
}

// ------------------------------------------------ parameter ranges (22)

#[test]
fn mate_n_outside_one_to_five_is_invalid_and_inside_builds() {
    let mate = presets::find("mate_in").unwrap();
    for n in [0u8, 6, 9, 10, 11, 255] {
        let r = presets::check_params(
            &mate,
            &Params {
                n,
                ..Params::default()
            },
        );
        assert!(matches!(r, Err(PatternError::InvalidValue { .. })), "n={n}");
        assert!(
            presets::build(
                "mate_in",
                &Params {
                    n,
                    ..Params::default()
                }
            )
            .is_err()
        );
    }
    for n in 1..=5u8 {
        presets::check_params(
            &mate,
            &Params {
                n,
                ..Params::default()
            },
        )
        .unwrap();
        presets::build(
            "mate_in",
            &Params {
                n,
                ..Params::default()
            },
        )
        .unwrap();
    }
}

#[test]
fn an_invalid_colour_is_refused_by_every_preset() {
    for preset in presets::all() {
        for who in [Who::Mover, Who::Opponent] {
            let r = presets::check_params(&preset, &by(who));
            assert!(
                matches!(r, Err(PatternError::InvalidValue { .. })),
                "{} {who:?}",
                preset.id
            );
            assert!(preset.build_checked(&by(who)).is_err(), "{}", preset.id);
        }
    }
}

#[test]
fn other_numeric_ranges_are_checked_at_both_ends() {
    let d = Params::default;
    let ok = |id: &str, p: Params| {
        presets::build(id, &p).unwrap_or_else(|e| panic!("{id}: {e}"));
    };
    let no = |id: &str, p: Params| {
        assert!(
            matches!(
                presets::build(id, &p),
                Err(PatternError::InvalidValue { .. })
            ),
            "{id}"
        );
    };
    ok(
        "mate_in",
        Params {
            max_nodes: 1_000,
            ..d()
        },
    );
    ok(
        "mate_in",
        Params {
            max_nodes: 2_000_000,
            ..d()
        },
    );
    no(
        "mate_in",
        Params {
            max_nodes: 999,
            ..d()
        },
    );
    no(
        "mate_in",
        Params {
            max_nodes: 2_000_001,
            ..d()
        },
    );
    no(
        "mate_in",
        Params {
            max_nodes: 0,
            ..d()
        },
    );
    no(
        "mate_in",
        Params {
            max_nodes: u32::MAX,
            ..d()
        },
    );
    ok(
        "sacrifice",
        Params {
            min_net: 1,
            within: 1,
            ..d()
        },
    );
    ok(
        "sacrifice",
        Params {
            min_net: 20,
            within: 8,
            ..d()
        },
    );
    no("sacrifice", Params { min_net: 21, ..d() });
    no("sacrifice", Params { within: 0, ..d() });
    no("sacrifice", Params { within: 9, ..d() });
    no(
        "sacrifice",
        Params {
            min_net: 255,
            ..d()
        },
    );
    ok(
        "passed_pawn",
        Params {
            min_own_rank: Some(2),
            ..d()
        },
    );
    ok(
        "passed_pawn",
        Params {
            min_own_rank: Some(7),
            ..d()
        },
    );
    no(
        "passed_pawn",
        Params {
            min_own_rank: Some(1),
            ..d()
        },
    );
    no(
        "passed_pawn",
        Params {
            min_own_rank: Some(8),
            ..d()
        },
    );
    no(
        "passed_pawn",
        Params {
            min_own_rank: Some(255),
            ..d()
        },
    );
}

#[test]
fn require_win_needs_a_colour_and_a_king_cannot_be_sacrificed() {
    let d = Params::default;
    assert!(
        presets::build(
            "sacrifice",
            &Params {
                require_win: true,
                ..d()
            }
        )
        .is_err()
    );
    presets::build(
        "sacrifice",
        &Params {
            require_win: true,
            by: Who::Black,
            ..d()
        },
    )
    .unwrap();
    assert!(
        presets::build(
            "sacrifice",
            &Params {
                roles: vec![RoleName::King],
                ..d()
            }
        )
        .is_err()
    );
    for r in [RoleName::Pawn, RoleName::King] {
        assert!(
            presets::build(
                "promotion",
                &Params {
                    roles: vec![r],
                    ..d()
                }
            )
            .is_err()
        );
    }
}

#[test]
fn names_and_descriptions_given_by_the_caller_are_used_and_checked() {
    let p = preset(
        "fork",
        Params {
            name: Some("Garfo".into()),
            description: Some("descrição".into()),
            ..Params::default()
        },
    );
    assert_eq!(
        (p.name.as_str(), p.description.as_str()),
        ("Garfo", "descrição")
    );
    assert!(
        presets::build(
            "fork",
            &Params {
                name: Some("x".repeat(80)),
                ..Params::default()
            }
        )
        .is_ok()
    );
    assert!(
        presets::build(
            "fork",
            &Params {
                name: Some("x".repeat(81)),
                ..Params::default()
            }
        )
        .is_err()
    );
    assert!(
        presets::build(
            "fork",
            &Params {
                name: Some(String::new()),
                ..Params::default()
            }
        )
        .is_err()
    );
    // The default name is not empty and fits.
    for pr in presets::all() {
        let p = pr.build_checked(&Params::default()).unwrap();
        let n = p.name.chars().count();
        assert!((1..=80).contains(&n), "{}", pr.id);
    }
}

// ---------------------------------- every preset builds, validates, round-trips

fn roles_strategy() -> impl Strategy<Value = Vec<RoleName>> {
    proptest::sample::subsequence(
        vec![
            RoleName::Knight,
            RoleName::Bishop,
            RoleName::Rook,
            RoleName::Queen,
        ],
        0..=4,
    )
}

fn params_strategy() -> impl Strategy<Value = Params> {
    let colour = prop_oneof![Just(Who::White), Just(Who::Black), Just(Who::Any)];
    let kind = prop_oneof![
        Just(DiscoveredKind::Attack),
        Just(DiscoveredKind::Check),
        Just(DiscoveredKind::DoubleCheck)
    ];
    let outcome = prop_oneof![
        Just(Outcome::Recovered),
        Just(Outcome::Mate),
        Just(Outcome::Either),
        Just(Outcome::Any)
    ];
    let mode = prop_oneof![Just(MateMode::Played), Just(MateMode::Available)];
    (
        (colour, 1u8..=5, mode, any::<bool>(), 1_000u32..=2_000_000),
        (roles_strategy(), roles_strategy(), any::<bool>(), kind),
        (
            1u8..=20,
            outcome,
            1u8..=8,
            proptest::option::of(2u8..=7),
            proptest::option::of("[A-Za-z][A-Za-z ]{0,78}"),
            proptest::option::of("[a-z ]{0,200}"),
        ),
        any::<bool>(),
    )
        .prop_map(
            |(
                (by, n, mate_mode, forcing_only, max_nodes),
                (roles, target_roles, against_king, discovery),
                (min_net, outcome, within, min_own_rank, name, description),
                win,
            )| {
                Params {
                    require_win: win && by != Who::Any,
                    by,
                    n,
                    mate_mode,
                    forcing_only,
                    max_nodes,
                    roles,
                    target_roles,
                    against_king,
                    discovery,
                    min_net,
                    outcome,
                    within,
                    min_own_rank,
                    name,
                    description,
                }
            },
        )
}

proptest! {
    #[test]
    fn every_preset_with_any_in_range_parameters_builds_validates_and_round_trips(
        params in params_strategy()
    ) {
        for preset in presets::all() {
            let pattern = preset.build_checked(&params).unwrap();
            pattern.validate().unwrap();
            let again = presets::build(preset.id, &params).unwrap();
            prop_assert_eq!(&again, &pattern, "build is pure ({})", preset.id);
            let text = PatternFile::new(pattern.clone()).to_toml().unwrap();
            let back = PatternFile::from_toml_str(&text).unwrap().pattern;
            prop_assert_eq!(back, pattern, "{}", preset.id);
        }
    }
}

// ------------------------------------------------------ positive/negative

#[test]
fn mate_in_one_positive_and_negative() {
    let p = preset(
        "mate_in",
        Params {
            n: 1,
            ..by(Who::White)
        },
    );
    // Spec acceptance: both positions have a mate in one for White.
    for fen in [
        "r1bqkb1r/pppp1ppp/2n2n2/4p2Q/2B1P3/8/PPPP1PPP/RNB1K1NR w KQkq - 4 4",
        "6k1/5ppp/8/8/8/8/8/R5K1 w - - 0 1",
    ] {
        let l = line_of(Some(fen), "");
        assert!(at(&p, &l, 0), "{fen}");
    }
    let start = line_of(None, "");
    assert!(!at(&p, &start, 0));
    // Black to move there: not White's mate.
    assert!(!at(
        &p,
        &line_of(Some("6k1/5ppp/8/8/8/8/8/R5K1 b - - 0 1"), ""),
        0
    ));
    // Mate in 2 is not mate in exactly 1.
    let mate2 = preset(
        "mate_in",
        Params {
            n: 1,
            mate_mode: MateMode::Played,
            ..by(Who::White)
        },
    );
    assert!(!at(
        &mate2,
        &line_of(Some("6k1/5ppp/8/8/8/8/8/R5K1 w - - 0 1"), ""),
        0
    ));
}

#[test]
fn fork_positive_negative_and_rule_27() {
    let p = preset(
        "fork",
        Params {
            roles: vec![RoleName::Knight],
            ..by(Who::White)
        },
    );
    let from = "r3k3/8/8/3N4/8/8/8/4K3 w - - 0 1";
    let fork = line_of(Some(from), "d5c7");
    assert!(at(&p, &fork, 1), "Nc7+ forks king and rook");
    assert!(!at(&p, &fork, 0), "rule 27: not before the fork");
    let quiet = line_of(Some(from), "d5b6");
    assert!(!at(&p, &quiet, 1), "Nb6 attacks only the rook");
    // The fork is credited to White only.
    assert!(!at(&preset("fork", by(Who::Black)), &fork, 1));
    assert!(at(&preset("fork", by(Who::Any)), &fork, 1));
}

#[test]
fn pin_positive_negative_absolute_and_relative() {
    let abs = preset("pin", by(Who::White));
    assert!(static_holds(&abs, "4k3/8/8/4n3/8/8/8/4RK2 w - - 0 1"));
    assert!(!static_holds(&abs, "4k3/8/8/8/8/5n2/8/3R1K2 w - - 0 1"));
    assert!(!static_holds(&abs, "4k3/8/8/8/8/8/8/3R1K2 w - - 0 1"));
    // Only the pinner's colour counts.
    assert!(!static_holds(
        &preset("pin", by(Who::Black)),
        "4k3/8/8/4n3/8/8/8/4RK2 w - - 0 1"
    ));
    // Against a queen: not absolute.
    let rel = preset(
        "pin",
        Params {
            against_king: false,
            ..by(Who::White)
        },
    );
    let queen = "4q2k/8/8/4n3/8/8/8/4R2K w - - 0 1";
    assert!(static_holds(&rel, queen));
    assert!(!static_holds(&abs, queen));
}

#[test]
fn skewer_positive_and_negative() {
    let p = preset("skewer", by(Who::White));
    let from = "4k2q/8/8/8/8/8/8/R3K3 w - - 0 1";
    let l = line_of(Some(from), "a1a8");
    assert!(at(&p, &l, 1), "Ra8+ skewers king and queen");
    assert!(!at(&p, &l, 0));
    // Ra2 attacks nothing.
    assert!(!at(&p, &line_of(Some(from), "a1a2"), 1));
}

#[test]
fn discovered_check_attack_and_double_check() {
    let from = "4k3/7p/8/8/4B3/8/8/4RK2 w - - 0 1";
    let check = preset(
        "discovered",
        Params {
            discovery: DiscoveredKind::Check,
            ..by(Who::White)
        },
    );
    let l = line_of(Some(from), "e4h7");
    assert!(at(&check, &l, 1), "Bxh7 uncovers the rook's check");
    assert!(!at(&check, &l, 0));
    assert!(
        !at(&check, &line_of(Some(from), "f1g1"), 1),
        "a king move discovers nothing"
    );
    let double = preset(
        "discovered",
        Params {
            discovery: DiscoveredKind::DoubleCheck,
            ..by(Who::White)
        },
    );
    assert!(
        !at(&double, &l, 1),
        "a single discovered check is not a double check"
    );
    let dfrom = "4k3/8/8/8/4N3/8/8/4RK2 w - - 0 1";
    assert!(at(&double, &line_of(Some(dfrom), "e4f6"), 1), "Nf6++");
    // Black cannot claim White's discovery.
    assert!(!at(
        &preset(
            "discovered",
            Params {
                discovery: DiscoveredKind::Check,
                ..by(Who::Black)
            }
        ),
        &l,
        1
    ));
}

#[test]
fn double_attack_positive_and_negative() {
    let p = preset("double_attack", by(Who::White));
    let l = line_of(Some("r3k3/8/8/3N4/8/8/8/4K3 w - - 0 1"), "d5c7");
    assert!(at(&p, &l, 1));
    assert!(!at(&p, &l, 0));
    assert!(!at(&p, &line_of(None, "e2e4"), 1));
}

#[test]
fn sacrifice_positive_and_negative() {
    let legal = "e2e4 e7e5 g1f3 d7d6 f1c4 c8g4 b1c3 g7g6 f3e5 g4d1 c4f7 e8e7 c3d5";
    let l = line_of(None, legal);
    let p = preset(
        "sacrifice",
        Params {
            roles: vec![RoleName::Queen],
            min_net: 5,
            outcome: Outcome::Mate,
            ..by(Who::White)
        },
    );
    assert_eq!(matching_plies(&p, &l, None).first(), Some(&8));
    // By Black, nothing; equal trades are not sacrifices.
    let black = preset(
        "sacrifice",
        Params {
            roles: vec![RoleName::Queen],
            min_net: 5,
            outcome: Outcome::Mate,
            ..by(Who::Black)
        },
    );
    assert!(matching_plies(&black, &l, None).is_empty());
    let trade = line_of(None, "e2e4 d7d5 e4d5 d8d5");
    assert!(matching_plies(&preset("sacrifice", by(Who::Any)), &trade, None).is_empty());
    // With require_win the result of the game matters.
    let win = preset(
        "sacrifice",
        Params {
            roles: vec![RoleName::Queen],
            min_net: 5,
            outcome: Outcome::Mate,
            require_win: true,
            ..by(Who::White)
        },
    );
    assert!(
        matching_plies(&win, &l, None).is_empty(),
        "the game has no result"
    );
}

#[test]
fn hanging_positive_and_negative() {
    let p = preset("hanging", by(Who::White));
    let loose = "4k3/8/8/2b5/8/8/8/R3K3 w - - 0 1";
    assert!(
        at(&p, &line_of(Some(loose), "a1a5"), 1),
        "Ra5 attacks the loose bishop"
    );
    let guarded = "4k3/8/3p4/2b5/8/8/8/R3K3 w - - 0 1";
    assert!(
        !at(&p, &line_of(Some(guarded), "a1a5"), 1),
        "defended by the d6 pawn"
    );
    assert!(!at(&p, &line_of(Some(loose), "a1a5"), 0));
    assert!(!at(
        &preset("hanging", by(Who::Black)),
        &line_of(Some(loose), "a1a5"),
        1
    ));
}

#[test]
fn passed_pawn_positive_negative_and_rank() {
    let p = preset("passed_pawn", by(Who::White));
    assert!(static_holds(&p, "4k3/8/8/3P4/8/8/8/4K3 w - - 0 1"));
    assert!(!static_holds(&p, "4k3/2p5/8/3P4/8/8/8/4K3 w - - 0 1"));
    let far = preset(
        "passed_pawn",
        Params {
            min_own_rank: Some(6),
            ..by(Who::White)
        },
    );
    assert!(
        !static_holds(&far, "4k3/8/8/3P4/8/8/8/4K3 w - - 0 1"),
        "d5 is on rank 5"
    );
    assert!(static_holds(&far, "4k3/8/3P4/8/8/8/8/4K3 w - - 0 1"));
}

#[test]
fn exposed_king_positive_negative_and_endgame() {
    let p = preset("exposed_king", by(Who::White));
    let open = "r1bq1rk1/pppp1ppp/2n2n2/4p3/4P3/2N2N2/PPPP1P2/R1BQ1RK1 w - - 0 1";
    assert!(static_holds(&p, open), "only f2 shields the king");
    let shield = "r1bq1rk1/pppp1ppp/2n2n2/4p3/4P3/2N2N2/PPPP1PPP/R1BQ1RK1 w - - 0 1";
    assert!(!static_holds(&p, shield));
    assert!(
        !static_holds(&p, "6k1/5ppp/8/8/8/8/5P2/6K1 w - - 0 1"),
        "endgame"
    );
    assert!(!static_holds(&preset("exposed_king", by(Who::Black)), open));
}

#[test]
fn back_rank_mate_for_each_colour() {
    let white_mates = "R5k1/5ppp/8/8/8/8/8/4K3 b - - 0 1";
    let black_mates = "4k3/8/8/8/8/8/5PPP/r5K1 w - - 0 1";
    let side_mate = "8/7k/8/8/8/8/6R1/K6R b - - 0 1";
    for f in [white_mates, black_mates, side_mate] {
        assert!(
            mychess_core::Position::from_fen(f).unwrap().is_checkmate(),
            "{f}"
        );
    }
    let w = preset("back_rank_mate", by(Who::White));
    let b = preset("back_rank_mate", by(Who::Black));
    let a = preset("back_rank_mate", by(Who::Any));
    assert_eq!(
        [&w, &b, &a].map(|p| static_holds(p, white_mates)),
        [true, false, true]
    );
    assert_eq!(
        [&w, &b, &a].map(|p| static_holds(p, black_mates)),
        [false, true, true]
    );
    // A mate with the king on the 7th rank is not a back-rank mate.
    assert_eq!(
        [&w, &b, &a].map(|p| static_holds(p, side_mate)),
        [false, false, false]
    );
    // Not even checkmate.
    assert!(!static_holds(&a, "R5k1/6pp/8/8/8/8/8/4K3 b - - 0 1"));
}

#[test]
fn promotion_positive_and_negative() {
    let from = "7k/P7/8/8/8/8/8/K7 w - - 0 1";
    let any = preset("promotion", Params::default());
    let queen = line_of(Some(from), "a7a8q");
    let knight = line_of(Some(from), "a7a8n");
    assert!(at(&any, &queen, 1) && at(&any, &knight, 1));
    assert!(!at(&any, &queen, 0));
    let under = preset(
        "promotion",
        Params {
            roles: vec![RoleName::Rook, RoleName::Bishop, RoleName::Knight],
            ..Params::default()
        },
    );
    assert!(!at(&under, &queen, 1));
    assert!(at(&under, &knight, 1));
    // The promotion is White's.
    assert!(!at(&preset("promotion", by(Who::Black)), &queen, 1));
    assert!(at(&preset("promotion", by(Who::White)), &queen, 1));
    // An ordinary pawn move is no promotion.
    assert!(!at(&any, &line_of(None, "e2e4"), 1));
}

#[test]
fn iqp_variants_positive_and_negative() {
    let iqp = "r1bq1rk1/pp3ppp/2n1pn2/8/3P4/2N2N2/PP3PPP/R1BQ1RK1 w - - 0 1";
    let with_e = "r1bq1rk1/pp3ppp/2n2n2/8/3PP3/2N2N2/PP3PPP/R1BQ1RK1 w - - 0 1";
    let with_c = "r1bq1rk1/pp3ppp/2n1pn2/8/2PP4/2N2N2/PP3PPP/R1BQ1RK1 w - - 0 1";
    let w = preset("iqp", Params::default());
    assert!(static_holds(&w, iqp));
    assert!(!static_holds(&w, with_e), "an e pawn supports d4");
    assert!(!static_holds(&w, with_c), "a c pawn supports d4");
    let b = preset("iqp_black", Params::default());
    assert!(!static_holds(&b, iqp));
    assert!(static_holds(
        &b,
        "r1bq1rk1/pp3ppp/2n2n2/3p4/8/2N1PN2/PP3PPP/R1BQ1RK1 w - - 0 1"
    ));
}

#[test]
fn maroczy_positive_and_negative() {
    let p = preset("maroczy", Params::default());
    assert!(static_holds(
        &p,
        "rnbqkb1r/pp2pp1p/3p1np1/8/2P1P3/2N1BN2/PP3PPP/R2QKB1R w KQkq - 0 1"
    ));
    assert!(!static_holds(
        &p,
        "rnbqkb1r/pp2pp1p/3p1np1/8/2P1P3/2N1BN2/PP1P1PPP/R2QKB1R w KQkq - 0 1"
    ));
    // Only e4 without c4.
    assert!(!static_holds(
        &p,
        "rnbqkb1r/pp2pp1p/3p1np1/8/4P3/2N1BN2/PP3PPP/R2QKB1R w KQkq - 0 1"
    ));
}

#[test]
fn fianchetto_presets_positive_and_negative() {
    let white_light = "rnbqkbnr/pppppppp/8/8/8/6P1/PPPPPPBP/RNBQK1NR b KQkq - 0 1";
    let l = preset("fianchetto_light", by(Who::White));
    assert!(static_holds(&l, white_light));
    assert!(!static_holds(
        &l,
        "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1"
    ));
    assert!(!static_holds(
        &preset("fianchetto_dark", by(Who::White)),
        white_light
    ));
    assert!(!static_holds(
        &preset("fianchetto_light", by(Who::Black)),
        white_light
    ));
}

#[test]
fn opposite_castling_positive_and_negative() {
    let p = preset("opposite_castling", Params::default());
    assert!(static_holds(
        &p,
        "2kr3r/ppp2ppp/8/8/8/8/PPP2PPP/R4RK1 w - - 0 1"
    ));
    assert!(static_holds(
        &p,
        "r4rk1/ppp2ppp/8/8/8/8/PPP2PPP/2KR3R w - - 0 1"
    ));
    assert!(
        !static_holds(&p, "r4rk1/ppp2ppp/8/8/8/8/PPP2PPP/R4RK1 w - - 0 1"),
        "same wing"
    );
    assert!(
        !static_holds(&p, "2kr3r/ppp2ppp/8/8/8/8/PPP2PPP/2KR3R w - - 0 1"),
        "same wing"
    );
}

#[test]
fn hanging_pawns_positive_and_negative() {
    let p = preset("hanging_pawns", by(Who::White));
    let yes = "r2q1rk1/pp3ppp/2n1bn2/8/2PP4/2N1BN2/P4PPP/R2Q1RK1 w - - 0 1";
    let with_b = "r2q1rk1/pp3ppp/2n1bn2/8/2PP4/1PN1BN2/5PPP/R2Q1RK1 w - - 0 1";
    assert!(static_holds(&p, yes));
    assert!(!static_holds(&p, with_b), "a b pawn spoils it");
}
