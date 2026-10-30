//! M6-2a contract tests for the tactical detectors, derived from
//! `docs/specs/m6-2-taticas-e-padroes.md` (D3260-D3269, rules 5, 11, 12, 28).
//! Positions are written by us; they only use the public API.

use mychess_core::tactics::{
    self, GamePhase, MateQuery, MateVerdict, Pin, Skewer, double_attack, doubled_pawns,
    isolated_pawns, king_shield, non_pawn_material,
};
use mychess_core::{CastlingMode, Color, Move, Position, Role, SanMode, Square};
use pretty_assertions::assert_eq;
use proptest::prelude::*;
use rstest::rstest;

fn pos(fen: &str) -> Position {
    Position::from_fen(fen).unwrap_or_else(|e| panic!("bad fen {fen}: {e}"))
}

fn touch<T>(_: T) {}

/// FEN without the move counters (mirroring keeps them untouched, so the
/// counters of `mirror(after(m))` and `after(mirror(m))` legitimately differ).
fn core_fen(p: &Position) -> String {
    p.fen().split(' ').take(4).collect::<Vec<_>>().join(" ")
}

fn sq(name: &str) -> Square {
    name.parse().unwrap_or_else(|_| panic!("bad square {name}"))
}

fn sqs(names: &[&str]) -> Vec<Square> {
    names.iter().map(|n| sq(n)).collect()
}

fn play(p: &Position, san: &str) -> (Move, Position) {
    let parsed = p
        .parse_san(san, SanMode::Tolerant)
        .unwrap_or_else(|e| panic!("san {san} in {}: {e}", p.fen()));
    let next = p.after(parsed.mv).expect("legal move");
    (parsed.mv, next)
}

// ---------------------------------------------------------------- pins

#[rstest]
// relative pin: behind is worth more than the pinned piece
#[case::relative_more_valuable("4q1k1/8/8/8/4n3/8/8/4RK2 w - - 0 1", Some(("e1", "e4", "e8")))]
// relative pin: behind is undefended even if it is not worth more
#[case::relative_undefended("4n1k1/8/8/8/4b3/8/8/4RK2 w - - 0 1", Some(("e1", "e4", "e8")))]
// behind defended by the king and not worth more: no pin
#[case::behind_defended("5k2/8/8/8/4b3/8/8/4RKn1 w - - 0 1", None)]
// a bishop does not pin along a file
#[case::bishop_on_a_file("4k3/8/8/4n3/8/8/8/4BK2 w - - 0 1", None)]
// two pieces between pinner and king: nothing is pinned
#[case::two_blockers("4k3/8/8/4n3/4P3/8/8/4RK2 w - - 0 1", None)]
// a queen pins along a diagonal
#[case::queen_diagonal("7k/8/5n2/8/3Q4/8/8/4K3 w - - 0 1", Some(("d4", "f6", "h8")))]
fn pin_table(#[case] fen: &str, #[case] expected: Option<(&str, &str, &str)>) {
    let want: Vec<Pin> = expected
        .map(|(a, b, c)| Pin {
            pinner: sq(a),
            pinned: sq(b),
            behind: sq(c),
        })
        .into_iter()
        .collect();
    assert_eq!(tactics::pins(&pos(fen), Color::White), want, "{fen}");
}

#[test]
fn only_sliders_pin_and_only_for_the_asked_colour() {
    // Knight and pawn "facing" the king never pin.
    let p = pos("4k3/8/3n4/4n3/8/3N4/4P3/4K3 w - - 0 1");
    assert_eq!(tactics::pins(&p, Color::White), Vec::new());
    assert_eq!(tactics::pins(&p, Color::Black), Vec::new());
    // The absolute pin of the spec belongs to White only.
    let q = pos("4k3/8/8/4n3/8/8/8/4RK2 w - - 0 1");
    assert_eq!(tactics::pins(&q, Color::Black), Vec::new());
}

#[test]
fn a_king_in_front_is_a_skewer_not_a_pin() {
    let p = pos("R3k2q/8/8/8/8/8/8/4K3 b - - 0 1");
    assert_eq!(tactics::pins(&p, Color::White), Vec::new());
}

// ------------------------------------------------------------ skewers

#[rstest]
// front queen, back undefended rook: the front piece is out of the board
// when the defence of the back piece is judged (D3262)
#[case::queen_before_undefended_rook("R3q2r/8/8/8/8/8/8/4K2k w - - 0 1", Some(("a8", "e8", "h8")))]
// bishop skewers a queen against a rook that is defended but worth more
#[case::back_worth_more_than_attacker("7k/6p1/5r2/8/3q4/8/8/B3K3 w - - 0 1", Some(("a1", "d4", "f6")))]
// back defended and not worth more than the attacker: not worth winning
#[case::back_defended_and_cheap("4k3/8/8/8/8/8/7p/R3q1nK w - - 0 1", None)]
// the back piece is never the king
#[case::back_is_king("4k3/8/8/4n3/8/8/8/4RK2 w - - 0 1", None)]
fn skewer_table(#[case] fen: &str, #[case] expected: Option<(&str, &str, &str)>) {
    let want: Vec<Skewer> = expected
        .map(|(a, f, b)| Skewer {
            attacker: sq(a),
            front: sq(f),
            back: sq(b),
        })
        .into_iter()
        .collect();
    assert_eq!(tactics::skewers(&pos(fen), Color::White), want, "{fen}");
}

#[test]
fn equal_value_front_and_back_is_not_a_skewer() {
    // Rook a1 attacks rook d1, rook h1 stands behind it.
    let p = pos("4k3/8/8/8/8/8/K7/R2r3r w - - 0 1");
    assert_eq!(tactics::skewers(&p, Color::White), Vec::new());
}

// -------------------------------------------------------------- forks

#[test]
fn fork_whose_forker_can_be_captured_for_free_does_not_count() {
    // Nc7 forks Ke8 and Ra8 but the queen on d8 takes it for nothing.
    let p = pos("r2qk3/2N5/8/8/8/8/8/4K3 b - - 1 1");
    assert_eq!(tactics::forks(&p, Color::White), Vec::new());
    // A pawn on b6 protects the knight: Qxc7 Pxc7 loses the queen.
    let defended = pos("r2qk3/2N5/1P6/8/8/8/8/4K3 b - - 1 1");
    let found = tactics::forks(&defended, Color::White);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].forker, sq("c7"));
}

#[test]
fn fork_victims_must_be_worth_it_when_defended() {
    // A rook attacking two knights, each defended by a pawn: Rxn loses.
    let defended = pos("4k3/4p3/3n4/6p1/3R3n/8/8/4K3 w - - 0 1");
    assert_eq!(tactics::forks(&defended, Color::White), Vec::new());
    // Without the defenders it is a fork.
    let loose = pos("4k3/8/3n4/8/3R3n/8/8/4K3 w - - 0 1");
    let found = tactics::forks(&loose, Color::White);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].forker, sq("d4"));
    assert_eq!(found[0].victims.len(), 2);
}

#[test]
fn pawn_fork() {
    let p = pos("4k3/8/2n1n3/3P4/8/8/8/4K3 b - - 0 1");
    let found = tactics::forks(&p, Color::White);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].forker, sq("d5"));
    let mut victims = found[0].victims.clone();
    victims.sort_by_key(|s| u32::from(*s));
    assert_eq!(victims, sqs(&["c6", "e6"]));
}

#[test]
fn a_defended_pawn_still_forks_two_cheap_defended_targets() {
    // Pawn takes a defended knight: +3 -1 = +2 > 0, so it is still worth it.
    let p = pos("4k3/3p1p2/2n1n3/3P4/8/8/8/4K3 b - - 0 1");
    let found = tactics::forks(&p, Color::White);
    assert_eq!(found.len(), 1, "{found:?}");
}

#[test]
fn king_fork_needs_unattacked_victims() {
    // Kd4 attacks the loose knights c5 and e5.
    let p = pos("7k/8/8/2n1n3/3K4/8/8/8 b - - 0 1");
    let found = tactics::forks(&p, Color::White);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].forker, sq("d4"));
    // A pawn on b6 defends c5: the king cannot take it, one victim is left.
    let guarded = pos("7k/8/1p6/2n1n3/3K4/8/8/8 b - - 0 1");
    assert_eq!(tactics::forks(&guarded, Color::White), Vec::new());
}

#[test]
fn pinned_forker_cannot_fork() {
    // The knight on c7 is pinned to the king on c1 by the rook on c8.
    let p = pos("r1r1k3/2N5/8/8/8/8/8/2K5 b - - 0 1");
    assert_eq!(tactics::forks(&p, Color::White), Vec::new());
}

#[test]
fn fork_by_black_is_found_for_black() {
    let p = pos("4k3/8/8/8/8/2n5/8/R3K3 w - - 0 1");
    assert_eq!(tactics::forks(&p, Color::White), Vec::new());
    let q = pos("4k3/8/8/8/8/8/2n5/R3K3 w - - 0 1");
    let found = tactics::forks(&q, Color::Black);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].forker, sq("c2"));
}

// ------------------------------------------------------ double attack

#[test]
fn double_attack_after_the_forking_move() {
    let prev = pos("r3k3/8/8/3N4/8/8/8/4K3 w - - 0 1");
    let (_, cur) = play(&prev, "Nc7+");
    let victims = double_attack(&prev, &cur, Color::White);
    assert_eq!(victims.len(), 2);
    assert!(victims.contains(&sq("a8")) && victims.contains(&sq("e8")));
}

#[test]
fn double_attack_needs_a_threat_created_by_the_move() {
    // The fork of the two rooks already stood: a king move creates no new
    // threat.
    let prev = pos("4k3/r3r3/2N5/8/8/8/8/4K3 w - - 0 1");
    assert_eq!(tactics::threatened_squares(&prev, Color::White).len(), 2);
    let (_, cur) = play(&prev, "Kd1");
    assert_eq!(double_attack(&prev, &cur, Color::White), Vec::new());
}

#[test]
fn a_single_threat_is_not_a_double_attack() {
    let prev = pos("r3k3/8/8/8/8/3N4/8/4K3 w - - 0 1");
    let (_, cur) = play(&prev, "Nb4");
    assert_eq!(double_attack(&prev, &cur, Color::White), Vec::new());
}

// ------------------------------------------------------------- discovered

#[test]
fn double_check_has_two_checkers() {
    let p = pos("4k3/8/8/1B6/8/8/8/4RK2 b - - 0 1");
    let mut c = tactics::checkers(&p);
    c.sort_by_key(|s| u32::from(*s));
    assert_eq!(c, sqs(&["e1", "b5"]));
    // A single check has one checker; a quiet position none.
    assert_eq!(
        tactics::checkers(&pos("4k3/8/8/8/8/8/8/4RK2 b - - 0 1")).len(),
        1
    );
    assert_eq!(tactics::checkers(&Position::new()), Vec::new());
}

#[test]
fn discovered_double_check_arises_from_a_move() {
    // Bc6+ checks with the bishop and uncovers the rook on the e-file.
    let prev = pos("4k3/8/8/8/4B3/8/8/4RK2 w - - 0 1");
    let (mv, cur) = play(&prev, "Bc6+");
    assert_eq!(tactics::checkers(&cur).len(), 2);
    assert!(tactics::discovered_check(&prev, mv, &cur));
}

#[test]
fn castling_never_counts_as_a_discovered_attack() {
    // O-O gives a direct check with the rook on f1: not a discovery.
    let prev = pos("5k2/8/8/8/8/8/8/4K2R w K - 0 1");
    let (mv, cur) = play(&prev, "O-O");
    assert!(matches!(mv, Move::Castle { .. }));
    assert_eq!(tactics::discovered_attacks(&prev, mv, &cur), Vec::new());
    assert!(!tactics::discovered_check(&prev, mv, &cur));
    assert!(cur.is_check());
}

#[test]
fn the_piece_that_moved_is_not_the_discoverer() {
    // The rook itself moves with check: not a discovered one.
    let prev = pos("4k3/8/8/8/8/8/8/R3K3 w - - 0 1");
    let (mv, cur) = play(&prev, "Ra8+");
    assert!(!tactics::discovered_check(&prev, mv, &cur));
}

#[test]
fn discovered_attack_on_a_piece_that_is_not_worth_it_does_not_count() {
    // The rook uncovered against a knight guarded by a pawn: Rxn loses.
    let guarded = pos("4k3/8/3p4/4n3/4B3/8/8/4RK2 w - - 0 1");
    let (mv, cur) = play(&guarded, "Bd3");
    assert_eq!(tactics::discovered_attacks(&guarded, mv, &cur), Vec::new());
    // The same against a loose knight is a discovered attack, not a check.
    let loose = pos("4k3/8/8/4n3/4B3/8/8/4RK2 w - - 0 1");
    let (mv, cur) = play(&loose, "Bd3");
    let found = tactics::discovered_attacks(&loose, mv, &cur);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].slider, sq("e1"));
    assert_eq!(found[0].target, sq("e5"));
    assert_eq!(found[0].moved, Role::Bishop);
    assert!(!tactics::discovered_check(&loose, mv, &cur));
}

// ------------------------------------------------------------- structure

#[test]
fn passed_pawns_with_en_passant_and_promotion_squares() {
    // En passant square set: both d5 and e5 are passed (neighbours on the
    // same rank are not "in front").
    let p = pos("4k3/8/8/3pP3/8/8/8/4K3 w - d6 0 1");
    assert_eq!(tactics::passed_pawns(&p, Color::White), vec![sq("e5")]);
    assert_eq!(tactics::passed_pawns(&p, Color::Black), vec![sq("d5")]);
    // Pawn on the seventh rank, enemy pawn behind it: passed.
    let q = pos("7k/3P4/8/8/8/8/3p4/7K w - - 0 1");
    assert_eq!(tactics::passed_pawns(&q, Color::White), vec![sq("d7")]);
    // A pawn of the same colour in front does not stop being passed.
    let r = pos("4k3/8/3P4/3P4/8/8/8/4K3 w - - 0 1");
    assert_eq!(tactics::passed_pawns(&r, Color::White).len(), 2);
    // Black's direction is the opposite: a black pawn is blocked by the
    // white pawn on the file below it.
    let s = pos("4k3/3p4/8/8/8/8/3P4/4K3 w - - 0 1");
    assert_eq!(tactics::passed_pawns(&s, Color::Black), Vec::new());
    assert_eq!(tactics::passed_pawns(&s, Color::White), Vec::new());
}

#[rstest]
#[case("4k3/8/8/8/8/8/P1P5/4K3 w - - 0 1", &["a2", "c2"])]
#[case("4k3/8/8/8/8/8/PP6/4K3 w - - 0 1", &[])]
#[case("4k3/8/8/8/8/2P5/P1P5/4K3 w - - 0 1", &["a2", "c2", "c3"])]
fn isolated_pawn_table(#[case] fen: &str, #[case] expected: &[&str]) {
    let mut got = isolated_pawns(&pos(fen), Color::White);
    got.sort_by_key(|s| u32::from(*s));
    let mut want = sqs(expected);
    want.sort_by_key(|s| u32::from(*s));
    assert_eq!(got, want, "{fen}");
}

#[test]
fn doubled_pawns_count_both_and_ignore_the_enemy() {
    let p = pos("4k3/3p4/8/8/8/2P5/2P5/4K3 w - - 0 1");
    let mut d = doubled_pawns(&p, Color::White);
    d.sort_by_key(|s| u32::from(*s));
    assert_eq!(d, sqs(&["c2", "c3"]));
    assert_eq!(doubled_pawns(&p, Color::Black), Vec::new());
    // Tripled: all three.
    let t = pos("4k3/8/8/8/2P5/2P5/2P5/4K3 w - - 0 1");
    assert_eq!(doubled_pawns(&t, Color::White).len(), 3);
}

#[test]
fn bishop_pair_needs_both_square_colours() {
    assert!(tactics::bishop_pair(&Position::new(), Color::White));
    // c1 and f1: dark + light.
    assert!(tactics::bishop_pair(
        &pos("4k3/8/8/8/8/8/8/2B1KB2 w - - 0 1"),
        Color::White
    ));
    // c1 and a3: both dark.
    assert!(!tactics::bishop_pair(
        &pos("4k3/8/8/8/8/B7/8/2B1K3 w - - 0 1"),
        Color::White
    ));
    // Two knights are not a pair.
    assert!(!tactics::bishop_pair(
        &pos("4k3/8/8/8/8/8/8/1N2K1N1 w - - 0 1"),
        Color::White
    ));
    // Three bishops (a promotion) with both colours.
    assert!(tactics::bishop_pair(
        &pos("4k3/8/8/8/8/B7/8/2B1KB2 w - - 0 1"),
        Color::White
    ));
}

#[rstest]
#[case::three_pawns("4k3/8/8/8/8/8/5PPP/6K1 w - - 0 1", Color::White, 3)]
#[case::only_f2("4k3/8/8/8/8/8/5P2/6K1 w - - 0 1", Color::White, 1)]
#[case::g3_counts_two_ranks_ahead("4k3/8/8/8/8/6P1/8/6K1 w - - 0 1", Color::White, 1)]
#[case::g4_is_too_far("4k3/8/8/8/6P1/8/8/6K1 w - - 0 1", Color::White, 0)]
#[case::corner_two_files("4k3/8/8/8/8/8/6PP/7K w - - 0 1", Color::White, 2)]
#[case::last_rank_has_no_front("6K1/5PPP/8/8/8/8/8/4k3 w - - 0 1", Color::White, 0)]
#[case::black_mirror("6k1/5ppp/8/8/8/8/8/4K3 w - - 0 1", Color::Black, 3)]
#[case::black_behind_does_not_count("4K3/8/8/8/8/8/5ppp/6k1 w - - 0 1", Color::Black, 0)]
fn king_shield_table(#[case] fen: &str, #[case] color: Color, #[case] expected: u8) {
    assert_eq!(king_shield(&pos(fen), color), expected, "{fen}");
}

#[test]
fn exposed_king_boundary_shield_one_and_two() {
    // Same middlegame material; only the shield differs.
    let base = |pawns: &str| format!("r1bq1rk1/ppp2ppp/2n5/8/8/8/{pawns}/RNBQ1RK1 w - - 0 1");
    let one = pos(&base("5P2"));
    let two = pos(&base("5PP1"));
    assert_eq!(tactics::phase(&one), GamePhase::Middlegame);
    assert!(tactics::exposed_king(&one, Color::White));
    assert!(!tactics::exposed_king(&two, Color::White));
    // The black king is judged on its own pawns.
    assert!(!tactics::exposed_king(&one, Color::Black));
    // No pawns at all: shield 0, exposed in the middlegame.
    assert!(tactics::exposed_king(&pos(&base("8")), Color::White));
}

// ------------------------------------------------------------------ phase

#[test]
fn phase_thresholds_56_and_24_are_exact() {
    assert_eq!(tactics::PHASE_OPENING_MIN, 56);
    assert_eq!(tactics::PHASE_ENDGAME_MAX, 24);
    // 62 at the start.
    assert_eq!(non_pawn_material(&Position::new()), 62);
    // 56: White loses both knights (62 - 6): the opening limit itself.
    let t56 = pos("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/R1BQKB1R w KQkq - 0 1");
    assert_eq!(non_pawn_material(&t56), 56);
    assert_eq!(tactics::phase(&t56), GamePhase::Opening);
    // 57: one rook short.
    let t57 = pos("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBN1 w Qkq - 0 1");
    assert_eq!(non_pawn_material(&t57), 57);
    assert_eq!(tactics::phase(&t57), GamePhase::Opening);
    // 54 = 62 - 5 - 3 (a rook and a knight): a middlegame.
    let t54 = pos("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKB2 w Qkq - 0 1");
    assert_eq!(non_pawn_material(&t54), 54);
    assert_eq!(tactics::phase(&t54), GamePhase::Middlegame);
    // 24: Q+R against R+R.
    let t24 = pos("r3k2r/8/8/8/8/8/8/R2QK3 w - - 0 1");
    assert_eq!(non_pawn_material(&t24), 24);
    assert_eq!(tactics::phase(&t24), GamePhase::Endgame);
    // 25: Q+R against R+N+N.
    let t25 = pos("rn2k1n1/8/8/8/8/8/8/R2QK3 w - - 0 1");
    assert_eq!(non_pawn_material(&t25), 25);
    assert_eq!(tactics::phase(&t25), GamePhase::Middlegame);
    // Kings and pawns only: endgame.
    assert_eq!(
        tactics::phase(&pos("4k3/pppp4/8/8/8/8/PPPP4/4K3 w - - 0 1")),
        GamePhase::Endgame
    );
}

#[test]
fn exposed_king_flips_exactly_at_the_endgame_limit() {
    // T = 25 (middlegame): the lone king with no pawns is exposed.
    let t25 = pos("rn2k1n1/8/8/8/8/8/8/R2QK3 w - - 0 1");
    assert!(tactics::exposed_king(&t25, Color::White));
    // T = 24 (endgame): it is not.
    let t24 = pos("r3k2r/8/8/8/8/8/8/R2QK3 w - - 0 1");
    assert!(!tactics::exposed_king(&t24, Color::White));
}

// ------------------------------------------------------ values / exchange

#[test]
fn king_value_is_only_for_ordering() {
    assert_eq!(tactics::value(Role::King), tactics::KING_VALUE);
    assert_eq!(tactics::KING_VALUE, 1000);
    assert_eq!(tactics::material_value(Role::King), 0);
    // The kings never enter the balance.
    assert_eq!(tactics::material_balance(&Position::new()), 0);
    assert_eq!(
        tactics::material_balance(&pos("4k3/8/8/8/8/8/8/Q3K3 w - - 0 1")),
        9
    );
    assert_eq!(
        tactics::material_balance(&pos("4k3/pp6/8/8/8/8/8/R3K3 w - - 0 1")),
        3
    );
}

#[rstest]
#[case::free_pawn("4k3/8/8/3p4/8/5N2/8/4K3 w - - 0 1", "f3", "d5", 1)]
#[case::pawn_takes_defended_pawn("4k3/8/2p5/3p4/4P3/8/8/4K3 w - - 0 1", "e4", "d5", 0)]
#[case::rook_takes_defended_pawn("4k3/8/2p5/3p4/8/8/3R4/4K3 w - - 0 1", "d2", "d5", -4)]
#[case::queen_takes_pawn_defended_by_pawn("4k3/8/2p5/3p4/8/8/3Q4/4K3 w - - 0 1", "d2", "d5", -8)]
#[case::xray_battery_wins("3rk3/8/8/3p4/8/8/3R4/3RK3 w - - 0 1", "d2", "d5", 1)]
#[case::xray_single_rook_loses("3rk3/8/8/3p4/8/8/3R4/4K3 w - - 0 1", "d2", "d5", -4)]
#[case::king_cannot_recapture_a_defended_square(
    "8/8/4k3/3p4/8/1B6/8/3RK3 w - - 0 1",
    "d1",
    "d5",
    1
)]
#[case::king_recaptures_when_alone("8/8/4k3/3p4/8/8/8/3RK3 w - - 0 1", "d1", "d5", -4)]
#[case::own_piece_is_zero("4k3/8/8/3p4/8/8/8/3RK3 w - - 0 1", "d1", "e1", 0)]
#[case::empty_origin_is_zero("4k3/8/8/3p4/8/8/8/3RK3 w - - 0 1", "a1", "d5", 0)]
#[case::empty_target_is_zero("4k3/8/8/3p4/8/8/8/3RK3 w - - 0 1", "d1", "d4", 0)]
fn see_table(#[case] fen: &str, #[case] from: &str, #[case] to: &str, #[case] expected: i32) {
    assert_eq!(tactics::see(&pos(fen), sq(from), sq(to)), expected, "{fen}");
}

#[test]
fn see_ignores_pins_after_the_first_capture_documented_limit() {
    // The knight on b6 is pinned to the king on b8 by the rook on b1, so it
    // cannot really recapture on d5, but the SEE still counts it (rule 11).
    let p = pos("1k6/8/1n6/3p4/4P3/8/8/1R2K3 w - - 0 1");
    assert_eq!(tactics::see(&p, sq("e4"), sq("d5")), 0);
    // The honest detectors know about the pin:
    assert!(!tactics::legal_attack(&p, sq("b6"), sq("d5")));
    assert!(tactics::attacks(&p, sq("b6"), sq("d5")));
    assert_eq!(tactics::defenders(&p, sq("d5")), Vec::new());
}

#[test]
fn geometric_versus_legal_attack() {
    // Pinned knight "attacks" geometrically but not legally.
    let p = pos("4k3/8/8/4n3/8/8/8/4RK2 b - - 0 1");
    assert!(tactics::attacks(&p, sq("e5"), sq("d3")));
    assert!(!tactics::legal_attack(&p, sq("e5"), sq("d3")));
    // A pinned rook may capture along the line of the pin.
    let q = pos("4r1k1/8/8/8/4R3/8/8/4K3 w - - 0 1");
    // (Re4 is pinned by Re8 against Ke1; it can capture e8 along the line.)
    assert!(tactics::legal_attack(&q, sq("e4"), sq("e8")));
    assert!(!tactics::legal_attack(&q, sq("e4"), sq("a4")));
    assert!(tactics::attacks(&q, sq("e4"), sq("a4")));
    // Empty origin: never an attack.
    assert!(!tactics::attacks(&q, sq("a1"), sq("a2")));
    assert!(!tactics::legal_attack(&q, sq("a1"), sq("a2")));
}

#[test]
fn king_attack_honours_the_xray_through_itself() {
    // The rook on e8 attacks through the king: e3 is not a legal square.
    let p = pos("4r1k1/8/8/8/4K3/4p3/8/8 w - - 0 1");
    assert!(tactics::attacks(&p, sq("e4"), sq("e3")));
    assert!(!tactics::legal_attack(&p, sq("e4"), sq("e3")));
}

#[test]
fn en_passant_does_not_make_the_neighbour_pawn_a_target() {
    // White may take d5 en passant, but that is not an "attack" on d5.
    let p = pos("4k3/8/8/3pP3/8/8/8/4K3 w - d6 0 1");
    assert!(!tactics::attacks(&p, sq("e5"), sq("d5")));
    assert!(tactics::attacks(&p, sq("e5"), sq("d6")));
    assert!(tactics::attacks(&p, sq("e5"), sq("f6")));
    assert!(!tactics::threatened_squares(&p, Color::White).contains(&sq("d5")));
}

#[test]
fn defenders_of_king_defended_piece() {
    // Rook and bishop both attack d5: the king cannot defend it.
    let two = pos("8/8/4k3/3n4/8/1B6/8/3RK3 w - - 0 1");
    assert_eq!(tactics::defenders(&two, sq("d5")), Vec::new());
    // With only the rook, the king defends.
    let one = pos("8/8/4k3/3n4/8/8/8/3RK3 w - - 0 1");
    assert_eq!(tactics::defenders(&one, sq("d5")), vec![sq("e6")]);
    // Empty square: no defenders.
    assert_eq!(tactics::defenders(&one, sq("a5")), Vec::new());
}

#[test]
fn worth_and_safe() {
    // Attacking the king is always worth it.
    let p = pos("r3k3/2N5/8/8/8/8/8/4K3 b - - 1 1");
    assert!(tactics::worth(&p, sq("c7"), sq("e8")));
    assert!(tactics::worth(&p, sq("c7"), sq("a8")));
    // Attacking own piece or an empty square is not.
    assert!(!tactics::worth(&p, sq("c7"), sq("e1")));
    assert!(!tactics::worth(&p, sq("c7"), sq("d5")));
    // A knight that stands en prise to an equal trade is not safe.
    let trade = pos("4k3/8/8/3p4/4N3/8/8/4K3 b - - 0 1");
    assert!(!tactics::safe(&trade, sq("e4")) || tactics::see(&trade, sq("d5"), sq("e4")) < 0);
    let hang = pos("4k3/8/8/3p4/4Q3/8/8/4K3 b - - 0 1");
    assert!(!tactics::safe(&hang, sq("e4")));
    assert!(tactics::safe(&hang, sq("e1")));
    // Empty square is trivially safe.
    assert!(tactics::safe(&hang, sq("a8")));
}

#[test]
fn safe_treats_an_equal_trade_as_unsafe() {
    // Black pawn takes the knight and the pawn on d3 retakes: see = 3 - 1 > 0 for
    // the pawn, so unsafe. With a bishop trade: B takes B, B retakes: 0 -> unsafe.
    let p = pos("4k3/8/8/8/3b4/8/1B6/4K3 w - - 0 1");
    assert!(tactics::attacks(&p, sq("b2"), sq("d4")));
    assert!(!tactics::safe(&p, sq("b2")));
}

// ------------------------------------------------------------------ mate

fn q(min: u8, max: u8, forcing: bool, nodes: u32) -> MateQuery {
    MateQuery {
        min_moves: min,
        max_moves: max,
        forcing_only: forcing,
        max_nodes: nodes,
    }
}

/// White to play and mate in exactly 2 with a quiet first move (Ra7).
const LADDER: &str = "7k/8/8/8/8/8/R7/1R4K1 w - - 0 1";

#[test]
fn available_mate_uses_the_shortest_length_for_the_range() {
    let p = pos(LADDER);
    assert_eq!(
        tactics::mate_in_available(&p, q(2, 2, false, 100_000)),
        MateVerdict::Mate
    );
    assert_eq!(
        tactics::mate_in_available(&p, q(1, 2, false, 100_000)),
        MateVerdict::Mate
    );
    // The shortest mate is 2: a range of 1..=1 or 3..=3 does not match.
    assert_eq!(
        tactics::mate_in_available(&p, q(1, 1, false, 100_000)),
        MateVerdict::NoMate
    );
    assert_eq!(
        tactics::mate_in_available(&p, q(3, 3, false, 100_000)),
        MateVerdict::NoMate
    );
}

#[test]
fn available_mate_with_a_tiny_budget_is_unknown_and_with_a_big_one_is_mate() {
    let p = pos(LADDER);
    assert_eq!(
        tactics::mate_in_available(&p, q(2, 2, false, 1)),
        MateVerdict::Unknown
    );
    assert_eq!(
        tactics::mate_in_available(&p, q(2, 2, false, 2_000_000)),
        MateVerdict::Mate
    );
}

#[test]
fn forcing_only_skips_mates_that_start_quietly() {
    let p = pos(LADDER);
    assert_eq!(
        tactics::mate_in_available(&p, q(2, 2, true, 100_000)),
        MateVerdict::NoMate
    );
    // A mate in 1 starts with a check by definition.
    let one = pos("6k1/5ppp/8/8/8/8/8/R5K1 w - - 0 1");
    assert_eq!(
        tactics::mate_in_available(&one, q(1, 1, true, 100_000)),
        MateVerdict::Mate
    );
    assert_eq!(
        tactics::mate_in_available(&one, q(1, 1, false, 100_000)),
        MateVerdict::Mate
    );
}

#[test]
fn available_mate_is_false_without_material_or_when_the_game_is_over() {
    // The side to move cannot mate with a lone king.
    let lone = pos("4k3/8/8/8/8/8/8/4K3 w - - 0 1");
    assert_eq!(
        tactics::mate_in_available(&lone, q(1, 3, false, 100_000)),
        MateVerdict::NoMate
    );
    // Checkmated side to move (fool's mate): it cannot give mate.
    let mated = pos("rnb1kbnr/pppp1ppp/8/4p3/6Pq/5P2/PPPPP2P/RNBQKBNR w KQkq - 1 3");
    assert!(mated.is_checkmate());
    assert_eq!(
        tactics::mate_in_available(&mated, q(1, 3, false, 100_000)),
        MateVerdict::NoMate
    );
    // Stalemate too.
    let stale = pos("7k/5Q2/6K1/8/8/8/8/8 b - - 0 1");
    assert!(stale.is_stalemate());
    assert_eq!(
        tactics::mate_in_available(&stale, q(1, 3, false, 100_000)),
        MateVerdict::NoMate
    );
}

#[test]
fn played_mate_looks_only_at_the_distance_of_the_game() {
    // Fool's mate: 1.f3 e5 2.g4 Qh4#. The position after 2.g4 has mate in 1.
    let start = Position::new();
    let (_, p1) = play(&start, "f3");
    let (_, p2) = play(&p1, "e5");
    let (_, p3) = play(&p2, "g4");
    let (_, end) = play(&p3, "Qh4#");
    assert!(end.is_checkmate());
    assert_eq!(
        tactics::mate_in_played(&p3, 1, q(1, 1, false, 100_000)),
        MateVerdict::Mate
    );
    // Even distance (the other side to move): never.
    assert_eq!(
        tactics::mate_in_played(&p2, 2, q(1, 2, false, 100_000)),
        MateVerdict::NoMate
    );
    // Distance 3 is a mate in 2: outside 1..=1.
    assert_eq!(
        tactics::mate_in_played(&p1, 3, q(1, 1, false, 100_000)),
        MateVerdict::NoMate
    );
    // Zero plies is not a distance.
    assert_eq!(
        tactics::mate_in_played(&p3, 0, q(1, 1, false, 100_000)),
        MateVerdict::NoMate
    );
}

#[test]
fn played_mate_in_two_and_the_budget() {
    let p = pos(LADDER);
    // Ra7 Kg8 Rb8#: 3 plies.
    assert_eq!(
        tactics::mate_in_played(&p, 3, q(2, 2, false, 100_000)),
        MateVerdict::Mate
    );
    assert_eq!(
        tactics::mate_in_played(&p, 3, q(1, 2, false, 100_000)),
        MateVerdict::Mate
    );
    assert_eq!(
        tactics::mate_in_played(&p, 3, q(2, 2, false, 1)),
        MateVerdict::Unknown
    );
    // Out of range or even distances.
    assert_eq!(
        tactics::mate_in_played(&p, 3, q(1, 1, false, 100_000)),
        MateVerdict::NoMate
    );
    assert_eq!(
        tactics::mate_in_played(&p, 5, q(2, 2, false, 100_000)),
        MateVerdict::NoMate
    );
    assert_eq!(
        tactics::mate_in_played(&p, 4, q(2, 2, false, 100_000)),
        MateVerdict::NoMate
    );
}

#[test]
fn played_mate_requires_the_shortest_mate_in_the_range() {
    // There is a mate in 1, but the game took 3 plies and the range is 2..=2:
    // the shortest forced mate (1) is outside the range.
    let one = pos("6k1/5ppp/8/8/8/8/8/R5K1 w - - 0 1");
    assert_eq!(
        tactics::mate_in_played(&one, 3, q(2, 2, false, 100_000)),
        MateVerdict::NoMate
    );
    assert_eq!(
        tactics::mate_in_played(&one, 3, q(1, 2, false, 100_000)),
        MateVerdict::Mate
    );
}

// ---------------------------------------------------------------- mirror

#[test]
fn mirror_moves_castling_rights_and_en_passant() {
    let p = pos("r3k2r/pppp1ppp/8/3Pp3/8/8/PPP2PPP/R3K2R w Kq e6 0 1");
    let m = tactics::mirror(&p).expect("mirror");
    assert_eq!(
        m.fen(),
        "r3k2r/ppp2ppp/8/8/3pP3/8/PPPP1PPP/R3K2R b Qk e3 0 1"
    );
    assert_eq!(m.turn(), Color::Black);
    assert_eq!(m.ep_square(), Some(sq("e3")));
    // Involution.
    assert_eq!(tactics::mirror(&m).expect("mirror").fen(), p.fen());
}

#[test]
fn mirror_square_flips_the_ranks_only() {
    assert_eq!(tactics::mirror_square(sq("a1")), sq("a8"));
    assert_eq!(tactics::mirror_square(sq("e4")), sq("e5"));
    assert_eq!(tactics::mirror_square(sq("h7")), sq("h2"));
}

#[test]
fn mirror_move_of_promotions_castling_and_en_passant() {
    let promo = Move::Normal {
        role: Role::Pawn,
        from: sq("a7"),
        to: sq("a8"),
        capture: None,
        promotion: Some(Role::Queen),
    };
    assert_eq!(
        tactics::mirror_move(promo),
        Move::Normal {
            role: Role::Pawn,
            from: sq("a2"),
            to: sq("a1"),
            capture: None,
            promotion: Some(Role::Queen),
        }
    );
    let capture = Move::Normal {
        role: Role::Pawn,
        from: sq("g7"),
        to: sq("h8"),
        capture: Some(Role::Rook),
        promotion: Some(Role::Knight),
    };
    assert_eq!(
        tactics::mirror_move(capture),
        Move::Normal {
            role: Role::Pawn,
            from: sq("g2"),
            to: sq("h1"),
            capture: Some(Role::Rook),
            promotion: Some(Role::Knight),
        }
    );
    assert_eq!(
        tactics::mirror_move(Move::Castle {
            king_from: sq("e1"),
            rook_from: sq("h1")
        }),
        Move::Castle {
            king_from: sq("e8"),
            rook_from: sq("h8")
        }
    );
    assert_eq!(
        tactics::mirror_move(Move::EnPassant {
            from: sq("e5"),
            to: sq("d6")
        }),
        Move::EnPassant {
            from: sq("e4"),
            to: sq("d3")
        }
    );
    assert_eq!(tactics::mirror_move(Move::Null), Move::Null);
}

fn mirror_commutes_along(san: &[&str]) {
    let mut p = Position::new();
    for s in san {
        let (mv, next) = play(&p, s);
        let mp = tactics::mirror(&p).expect("mirror");
        let mm = tactics::mirror_move(mv);
        assert!(mp.is_legal(mm), "{s}: mirrored move must be legal");
        let after_mirror = mp.after(mm).expect("play mirrored");
        assert_eq!(
            core_fen(&after_mirror),
            core_fen(&tactics::mirror(&next).expect("mirror")),
            "after {s}"
        );
        // The UCI text of the mirrored move parses in the mirrored position.
        let uci = mp.uci(mm);
        assert_eq!(mp.parse_uci(&uci).expect("uci"), mm, "{s}");
        p = next;
    }
}

#[test]
fn mirror_commutes_with_castling_en_passant_and_promotion() {
    mirror_commutes_along(&[
        "e4", "e5", "Nf3", "Nc6", "Bc4", "Bc5", "O-O", "Nf6", "d3", "O-O",
    ]);
    mirror_commutes_along(&["e4", "a6", "e5", "d5", "exd6", "Nf6", "d7+", "Bxd7"]);
    mirror_commutes_along(&[
        "h4", "g5", "hxg5", "h6", "gxh6", "Bg7", "hxg7", "Nf6", "gxh8=Q+",
    ]);
    // Long castling by Black.
    mirror_commutes_along(&[
        "d4", "d5", "Nc3", "Nc6", "Bf4", "Bf5", "Qd2", "Qd7", "O-O-O", "O-O-O",
    ]);
}

#[test]
fn mirror_of_a_chess960_position_swaps_the_castling_files() {
    let p = pos("bbqnnrkr/pppppppp/8/8/8/8/PPPPPPPP/BBQNNRKR w Hf - 0 1");
    assert_eq!(p.castling_mode(), CastlingMode::Chess960);
    let m = tactics::mirror(&p).expect("mirror");
    assert_eq!(m.castling_mode(), CastlingMode::Chess960);
    assert_eq!(m.turn(), Color::Black);
    assert_eq!(
        m.fen(),
        "bbqnnrkr/pppppppp/8/8/8/8/PPPPPPPP/BBQNNRKR b Qk - 0 1"
    );
    assert_eq!(tactics::mirror(&m).expect("mirror").fen(), p.fen());
}

#[test]
fn mirror_keeps_the_castling_mode() {
    let p = pos("r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1");
    let m = tactics::mirror(&p).expect("mirror");
    assert_eq!(m.castling_mode(), CastlingMode::Standard);
    assert_eq!(m.fen(), "r3k2r/8/8/8/8/8/8/R3K2R b KQkq - 0 1");
}

#[test]
fn symmetric_detectors_agree_on_the_mirror() {
    // Rule 28 for the detectors: a fact about White in P is the same fact
    // about Black in mirror(P).
    for fen in [
        "r3k3/2N5/8/8/8/8/8/4K3 b - - 1 1",
        "4k3/8/8/4n3/8/8/8/4RK2 w - - 0 1",
        "R3k2q/8/8/8/8/8/8/4K3 b - - 0 1",
        "4k3/8/8/3P4/8/8/8/4K3 w - - 0 1",
        "r1bq1rk1/ppp2ppp/2n5/8/8/8/5P2/RNBQ1RK1 w - - 0 1",
    ] {
        let p = pos(fen);
        let m = tactics::mirror(&p).expect("mirror");
        let flip = |v: Vec<Square>| -> Vec<Square> {
            let mut out: Vec<Square> = v.into_iter().map(tactics::mirror_square).collect();
            out.sort_by_key(|s| u32::from(*s));
            out
        };
        assert_eq!(
            flip(
                tactics::forks(&p, Color::White)
                    .iter()
                    .map(|f| f.forker)
                    .collect()
            ),
            tactics::forks(&m, Color::Black)
                .iter()
                .map(|f| f.forker)
                .collect::<Vec<_>>(),
            "{fen}"
        );
        assert_eq!(
            tactics::pins(&p, Color::White).len(),
            tactics::pins(&m, Color::Black).len(),
            "{fen}"
        );
        assert_eq!(
            tactics::skewers(&p, Color::White).len(),
            tactics::skewers(&m, Color::Black).len(),
            "{fen}"
        );
        assert_eq!(
            flip(tactics::passed_pawns(&p, Color::White)),
            {
                let mut v = tactics::passed_pawns(&m, Color::Black);
                v.sort_by_key(|s| u32::from(*s));
                v
            },
            "{fen}"
        );
        assert_eq!(
            tactics::exposed_king(&p, Color::White),
            tactics::exposed_king(&m, Color::Black),
            "{fen}"
        );
        assert_eq!(tactics::phase(&p), tactics::phase(&m), "{fen}");
    }
}

proptest! {
    #[test]
    fn mirror_is_an_involution_along_random_games(choices in proptest::collection::vec(0usize..64, 0..40)) {
        let mut p = Position::new();
        for c in choices {
            let moves = p.legal_moves();
            if moves.is_empty() { break; }
            let mv = moves[c % moves.len()];
            let mp = tactics::mirror(&p).expect("mirror");
            let mm = tactics::mirror_move(mv);
            prop_assert!(mp.is_legal(mm));
            p = p.after(mv).expect("legal");
            prop_assert_eq!(core_fen(&mp.after(mm).expect("legal")), core_fen(&tactics::mirror(&p).expect("m")));
            prop_assert_eq!(tactics::mirror(&tactics::mirror(&p).expect("m")).expect("m").fen(), p.fen());
            prop_assert_eq!(tactics::material_balance(&tactics::mirror(&p).expect("m")), -tactics::material_balance(&p));
        }
    }

    #[test]
    fn detectors_never_panic_on_random_positions(choices in proptest::collection::vec(0usize..64, 0..60)) {
        let mut p = Position::new();
        let mut prev: Option<Position> = None;
        for c in choices {
            let moves = p.legal_moves();
            if moves.is_empty() { break; }
            let mv = moves[c % moves.len()];
            let next = p.after(mv).expect("legal");
            for color in [Color::White, Color::Black] {
                touch(tactics::pins(&next, color));
                touch(tactics::skewers(&next, color));
                touch(tactics::forks(&next, color));
                touch(tactics::passed_pawns(&next, color));
                touch(tactics::exposed_king(&next, color));
                touch(double_attack(&p, &next, color));
            }
            touch(tactics::discovered_attacks(&p, mv, &next));
            touch(tactics::checkers(&next));
            touch(tactics::mate_in_available(&next, q(1, 2, true, 2_000)));
            prev = Some(p);
            p = next;
        }
        let _ = prev;
    }
}
