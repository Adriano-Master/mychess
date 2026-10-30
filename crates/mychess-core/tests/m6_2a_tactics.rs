//! M6-2a: the tactical detectors, checked against the positions of the
//! spec (`docs/specs/m6-2-taticas-e-padroes.md`, acceptance criteria).

use mychess_core::tactics::{
    self, GamePhase, MateQuery, MateVerdict, Pin, Skewer, discovered_attacks, discovered_check,
};
use mychess_core::{Color, Position, Role, Square};

fn pos(fen: &str) -> Position {
    Position::from_fen(fen).expect("fen of the spec")
}

#[test]
fn fork_positions_of_the_spec() {
    let forked = pos("r3k3/2N5/8/8/8/8/8/4K3 b - - 1 1");
    let f = tactics::forks(&forked, Color::White);
    assert_eq!(f.len(), 1);
    assert_eq!(f[0].forker, Square::C7);
    assert!(f[0].victims.contains(&Square::E8) && f[0].victims.contains(&Square::A8));
    assert_eq!(
        tactics::forks(&pos("r3k3/8/8/3N4/8/8/8/4K3 w - - 0 1"), Color::White),
        Vec::new()
    );
}

#[test]
fn absolute_pin_and_skewer_of_the_spec() {
    let p = pos("4k3/8/8/4n3/8/8/8/4RK2 w - - 0 1");
    assert_eq!(
        tactics::pins(&p, Color::White),
        vec![Pin {
            pinner: Square::E1,
            pinned: Square::E5,
            behind: Square::E8
        }]
    );
    let s = pos("R3k2q/8/8/8/8/8/8/4K3 b - - 0 1");
    assert_eq!(
        tactics::skewers(&s, Color::White),
        vec![Skewer {
            attacker: Square::A8,
            front: Square::E8,
            back: Square::H8
        }]
    );
}

#[test]
fn discovered_check_of_the_spec() {
    let prev = pos("4k3/7p/8/8/4B3/8/8/4RK2 w - - 0 1");
    assert!(!tactics::attacks(&prev, Square::E1, Square::E8));
    let mv = prev.parse_uci("e4h7").expect("legal");
    let cur = prev.after(mv).expect("play");
    assert!(discovered_check(&prev, mv, &cur));
    assert_eq!(discovered_attacks(&prev, mv, &cur).len(), 1);
    // The quiet bishop move e4-d3 uncovers the same check.
    let mv2 = prev.parse_uci("e4d3").expect("legal");
    let cur2 = prev.after(mv2).expect("play");
    assert!(discovered_check(&prev, mv2, &cur2));
    assert!(tactics::checkers(&cur2).contains(&Square::E1));
}

#[test]
fn mate_in_one_positions_of_the_spec() {
    let q = MateQuery {
        min_moves: 1,
        max_moves: 1,
        forcing_only: false,
        max_nodes: 100_000,
    };
    for fen in [
        "r1bqkb1r/pppp1ppp/2n2n2/4p2Q/2B1P3/8/PPPP1PPP/RNB1K1NR w KQkq - 4 4",
        "6k1/5ppp/8/8/8/8/8/R5K1 w - - 0 1",
    ] {
        assert_eq!(
            tactics::mate_in_available(&pos(fen), q),
            MateVerdict::Mate,
            "{fen}"
        );
    }
    assert_eq!(
        tactics::mate_in_available(&Position::new(), q),
        MateVerdict::NoMate
    );
}

#[test]
fn structure_of_the_spec() {
    let p = pos("4k3/8/8/3P4/8/8/8/4K3 w - - 0 1");
    assert_eq!(tactics::passed_pawns(&p, Color::White), vec![Square::D5]);
    let q = pos("4k3/2p5/8/3P4/8/8/8/4K3 w - - 0 1");
    assert_eq!(tactics::passed_pawns(&q, Color::White), Vec::new());
}

#[test]
fn exposed_king_of_the_spec() {
    let thin = pos("r1bq1rk1/ppp2ppp/2n5/8/8/8/5P2/RNBQ1RK1 w - - 0 1");
    assert_eq!(tactics::phase(&thin), GamePhase::Middlegame);
    assert!(tactics::exposed_king(&thin, Color::White));
    let full = pos("r1bq1rk1/ppp2ppp/2n5/8/8/8/5PPP/RNBQ1RK1 w - - 0 1");
    assert!(!tactics::exposed_king(&full, Color::White));
    let end = pos("4k3/8/8/8/8/8/5P2/6KR w - - 0 1");
    assert_eq!(tactics::phase(&end), GamePhase::Endgame);
    assert!(!tactics::exposed_king(&end, Color::White));
}

#[test]
fn values_and_exchange() {
    assert_eq!(
        [
            Role::Pawn,
            Role::Knight,
            Role::Bishop,
            Role::Rook,
            Role::Queen
        ]
        .map(tactics::value),
        [1, 3, 3, 5, 9]
    );
    let p = pos("4k3/8/2p5/3p4/8/8/3R4/4K3 w - - 0 1");
    assert_eq!(tactics::see(&p, Square::D2, Square::D5), -4);
}

#[test]
fn mirror_roundtrips_and_swaps_the_colours() {
    let p = pos("r1bqkbnr/pppp1ppp/2n5/4p3/4P3/5N2/PPPP1PPP/RNBQKB1R w KQkq - 2 3");
    let m = tactics::mirror(&p).expect("mirror");
    assert_eq!(m.turn(), Color::Black);
    assert_eq!(tactics::mirror(&m).expect("mirror"), p);
    assert_eq!(
        tactics::material_balance(&m),
        -tactics::material_balance(&p)
    );
}

#[test]
fn mate_interrupt_hook_stops_the_solver_and_is_neutral_otherwise() {
    use std::cell::Cell;
    // A quiet middlegame: a mate in 3 search needs far more than 4096 nodes.
    let p = pos("r1bq1rk1/ppp2ppp/2np1n2/2b1p3/2B1P3/2NP1N2/PPP2PPP/R1BQ1RK1 w - - 0 7");
    let q = MateQuery {
        min_moves: 1,
        max_moves: 3,
        forcing_only: false,
        max_nodes: 5_000_000,
    };
    let calls = Cell::new(0u32);
    let stop = || {
        calls.set(calls.get() + 1);
        true
    };
    assert_eq!(
        tactics::mate_in_available_with(&p, q, Some(&stop)),
        MateVerdict::Unknown
    );
    assert_eq!(calls.get(), 1, "stops at the first consultation");
    let forcing = MateQuery {
        forcing_only: true,
        ..q
    };
    assert_eq!(
        tactics::mate_in_available_with(&p, forcing, Some(&stop)),
        MateVerdict::Unknown
    );
    // Never cancelled: same answer as the plain function.
    let go = || false;
    let tiny = MateQuery {
        max_nodes: 20_000,
        ..q
    };
    assert_eq!(
        tactics::mate_in_available_with(&p, tiny, Some(&go)),
        tactics::mate_in_available(&p, tiny)
    );
    assert_eq!(
        tactics::mate_in_played_with(&p, 5, tiny, Some(&stop)),
        MateVerdict::Unknown
    );
}
