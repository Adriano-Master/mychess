//! Tests of the catalogue of presets (spec `docs/specs/m6-2-taticas-e-padroes.md`,
//! "Catálogo dos presets", rule 22, D3279, D3280): the four examples of the
//! spec equal the TOML fixtures of slice a, every preset builds and
//! validates, parameters out of range are errors, and the structures hold on
//! positions written here.
#![allow(clippy::unwrap_used, clippy::too_many_lines)]

use std::path::PathBuf;

use mychess_core::Position;
use mychess_db::pattern::model::{
    DiscoveredKind, MateMode, Outcome, Pattern, PatternError, PatternFile, ResultKind, RoleName,
    Who,
};
use mychess_db::pattern::presets::{self, Params};
use mychess_db::pattern::{Frame, Line, eval_position};
use pretty_assertions::assert_eq;

fn fixture(name: &str) -> Pattern {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("m6_2a_fixtures")
        .join(name);
    PatternFile::from_bytes(&std::fs::read(path).unwrap())
        .unwrap()
        .pattern
}

fn named(fx: &Pattern, mut p: Params) -> Params {
    p.name = Some(fx.name.clone());
    p.description = Some(fx.description.clone());
    p
}

#[test]
fn the_four_examples_of_the_spec_are_what_build_returns() {
    let fx = fixture("mate_in_2.toml");
    let params = named(
        &fx,
        Params {
            by: Who::White,
            n: 2,
            mate_mode: MateMode::Available,
            forcing_only: false,
            max_nodes: 20_000,
            ..Params::default()
        },
    );
    assert_eq!(presets::build("mate_in", &params).unwrap(), fx);

    let fx = fixture("knight_fork.toml");
    let params = named(
        &fx,
        Params {
            by: Who::White,
            roles: vec![RoleName::Knight],
            ..Params::default()
        },
    );
    assert_eq!(presets::build("fork", &params).unwrap(), fx);

    let fx = fixture("absolute_pin.toml");
    let params = named(
        &fx,
        Params {
            by: Who::White,
            roles: vec![RoleName::Bishop, RoleName::Rook, RoleName::Queen],
            target_roles: vec![
                RoleName::Knight,
                RoleName::Bishop,
                RoleName::Rook,
                RoleName::Queen,
                RoleName::Pawn,
            ],
            against_king: true,
            ..Params::default()
        },
    );
    assert_eq!(presets::build("pin", &params).unwrap(), fx);

    let fx = fixture("queen_sacrifice.toml");
    let params = named(
        &fx,
        Params {
            by: Who::White,
            roles: vec![RoleName::Queen],
            min_net: 5,
            outcome: Outcome::Mate,
            within: 3,
            require_win: true,
            ..Params::default()
        },
    );
    assert_eq!(presets::build("sacrifice", &params).unwrap(), fx);
}

#[test]
fn every_preset_builds_validates_and_roundtrips_for_every_colour() {
    for preset in presets::all() {
        for by in [Who::White, Who::Black, Who::Any] {
            let params = Params {
                by,
                ..Params::default()
            };
            let pattern = preset.build_checked(&params).unwrap();
            let text = PatternFile::new(pattern.clone()).to_toml().unwrap();
            assert_eq!(
                PatternFile::from_toml_str(&text).unwrap().pattern,
                pattern,
                "{} {by:?}",
                preset.id
            );
        }
    }
}

#[test]
fn extremes_of_the_parameters_build() {
    for n in [1, 5] {
        for mode in [MateMode::Played, MateMode::Available] {
            let p = Params {
                n,
                mate_mode: mode,
                forcing_only: true,
                max_nodes: 1_000,
                ..Params::default()
            };
            presets::build("mate_in", &p).unwrap();
        }
    }
    for rank in [None, Some(2), Some(7)] {
        let p = Params {
            min_own_rank: rank,
            ..Params::default()
        };
        presets::build("passed_pawn", &p).unwrap();
    }
    for kind in [
        DiscoveredKind::Attack,
        DiscoveredKind::Check,
        DiscoveredKind::DoubleCheck,
    ] {
        let p = Params {
            discovery: kind,
            ..Params::default()
        };
        presets::build("discovered", &p).unwrap();
    }
    let p = Params {
        min_net: 20,
        within: 8,
        ..Params::default()
    };
    presets::build("sacrifice", &p).unwrap();
}

#[test]
fn parameters_out_of_range_are_invalid_values() {
    let bad = |id: &str, p: Params| {
        assert!(
            matches!(
                presets::build(id, &p),
                Err(PatternError::InvalidValue { .. })
            ),
            "{id}"
        );
    };
    let d = Params::default;
    bad("mate_in", Params { n: 0, ..d() });
    bad("mate_in", Params { n: 9, ..d() });
    bad(
        "mate_in",
        Params {
            max_nodes: 10,
            ..d()
        },
    );
    bad(
        "mate_in",
        Params {
            by: Who::Mover,
            ..d()
        },
    );
    bad(
        "mate_in",
        Params {
            by: Who::Opponent,
            ..d()
        },
    );
    bad(
        "passed_pawn",
        Params {
            min_own_rank: Some(0),
            ..d()
        },
    );
    bad(
        "passed_pawn",
        Params {
            min_own_rank: Some(8),
            ..d()
        },
    );
    bad("sacrifice", Params { min_net: 0, ..d() });
    bad("sacrifice", Params { within: 9, ..d() });
    bad(
        "sacrifice",
        Params {
            roles: vec![RoleName::King],
            ..d()
        },
    );
    bad(
        "sacrifice",
        Params {
            require_win: true,
            ..d()
        },
    );
    bad(
        "promotion",
        Params {
            roles: vec![RoleName::Pawn],
            ..d()
        },
    );
    bad(
        "fork",
        Params {
            name: Some(String::new()),
            ..d()
        },
    );
    bad(
        "fork",
        Params {
            name: Some("x".repeat(81)),
            ..d()
        },
    );
    bad("no_such_preset", d());
}

fn holds(pattern: &Pattern, fen: &str) -> bool {
    let pos = Position::from_fen(fen).unwrap();
    let line = Line::new(
        vec![Frame {
            pos,
            mv: None,
            balance: 0,
        }],
        ResultKind::Unknown,
        false,
    );
    eval_position(&pattern.root, &line, 0, None).is_true()
}

fn build(id: &str, by: Who) -> Pattern {
    presets::build(
        id,
        &Params {
            by,
            ..Params::default()
        },
    )
    .unwrap()
}

#[test]
fn isolated_queen_pawn_presets() {
    let white = "r1bq1rk1/pp3ppp/2n1pn2/8/3P4/2N2N2/PP3PPP/R1BQ1RK1 w - - 0 1";
    let black = "r1bq1rk1/pp3ppp/2n2n2/3p4/8/2N1PN2/PP3PPP/R1BQ1RK1 w - - 0 1";
    let both = "r1bq1rk1/pp3ppp/2n2n2/3p4/3P4/2N2N2/PP3PPP/R1BQ1RK1 w - - 0 1";
    let start = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";
    let w = build("iqp", Who::Any);
    let b = build("iqp_black", Who::Any);
    let a = build("iqp_both", Who::Any);
    assert_eq!(
        [white, black, both, start].map(|f| holds(&w, f)),
        [true, false, true, false]
    );
    assert_eq!(
        [white, black, both, start].map(|f| holds(&b, f)),
        [false, true, true, false]
    );
    assert_eq!(
        [white, black, both, start].map(|f| holds(&a, f)),
        [false, false, true, false]
    );
}

#[test]
fn maroczy_fianchetto_castling_and_hanging_pawns() {
    let maroczy = build("maroczy", Who::Any);
    assert!(holds(
        &maroczy,
        "rnbqkb1r/pp2pp1p/3p1np1/8/2P1P3/2N1BN2/PP3PPP/R2QKB1R w KQkq - 0 1"
    ));
    assert!(!holds(
        &maroczy,
        "rnbqkb1r/pp2pp1p/3p1np1/8/2P1P3/2N1BN2/PP1P1PPP/R2QKB1R w KQkq - 0 1"
    ));

    let one = "rnbqk2r/ppppppbp/5np1/8/8/5NP1/PPPPPPBP/RNBQK2R w KQkq - 0 1";
    let two = "rnbqk2r/ppppppbp/5np1/8/8/1P3NP1/PBPPPPBP/RN1QK2R w Kkq - 0 1";
    assert!(holds(&build("fianchetto_light", Who::White), one));
    assert!(!holds(&build("fianchetto_light", Who::Black), one));
    assert!(holds(&build("fianchetto_dark", Who::Black), one));
    assert!(!holds(&build("fianchetto_dark", Who::White), one));
    assert!(holds(&build("fianchetto_light", Who::Any), one));
    assert!(!holds(&build("fianchetto_four", Who::White), one));
    assert!(holds(&build("fianchetto_four", Who::White), two));
    assert!(holds(&build("fianchetto_four", Who::Any), two));

    let castling = build("opposite_castling", Who::Any);
    assert!(holds(
        &castling,
        "2kr3r/ppp2ppp/8/8/8/8/PPP2PPP/R4RK1 w - - 0 1"
    ));
    assert!(!holds(
        &castling,
        "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1"
    ));

    let hanging = "r2q1rk1/pp3ppp/2n1bn2/8/2PP4/2N1BN2/P4PPP/R2Q1RK1 w - - 0 1";
    assert!(holds(&build("hanging_pawns", Who::White), hanging));
    assert!(!holds(&build("hanging_pawns", Who::Black), hanging));
    assert!(holds(&build("hanging_pawns", Who::Any), hanging));
}

#[test]
fn passed_pawn_rank_and_back_rank_mate() {
    let fen = "4k3/8/P7/8/8/8/8/4K3 w - - 0 1";
    let at = |k| {
        presets::build(
            "passed_pawn",
            &Params {
                by: Who::White,
                min_own_rank: Some(k),
                ..Params::default()
            },
        )
        .unwrap()
    };
    assert!(holds(&at(5), fen));
    assert!(holds(&at(6), fen));
    assert!(!holds(&at(7), fen));
    let black_pawn = "4k3/8/8/8/8/p7/8/4K3 w - - 0 1";
    assert!(holds(&build("passed_pawn", Who::Black), black_pawn));
    assert!(!holds(&build("passed_pawn", Who::White), black_pawn));

    let mated = "R5k1/5ppp/8/8/8/8/8/4K3 b - - 0 1";
    assert!(holds(&build("back_rank_mate", Who::White), mated));
    assert!(holds(&build("back_rank_mate", Who::Any), mated));
    assert!(!holds(&build("back_rank_mate", Who::Black), mated));
}
