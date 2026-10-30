//! Material accounting for the player strips (spec M2-5, D234).
//!
//! Scid: `::board::material` in `tcl/board.tcl:2735-2855`. The Scid draws one
//! column per colour; MyChess shows, per side, either the surplus of each
//! piece type ([`MaterialMode::Difference`]) or the pieces it really captured
//! on the way from the start of the game to the cursor
//! ([`MaterialMode::Captured`]).
//!
//! Deviation from the Scid (and from the M2-3 widget): `Captured` no longer
//! compares the board with the 32 initial pieces, which counted a promotion
//! as a captured pawn. It walks the moves of the path and credits the piece
//! each move took (en passant included) to the side that moved. A game that
//! starts from a FEN starts with nothing captured.
//!
//! Everything here is pure data: the glyphs and the `+N` text are drawn by
//! the front-end.

use mychess_core::{Color, Game, NodeId, Position, Role};
use serde::Serialize;

/// A list of `(role, count)` pairs in the order `Q R B N P`, only with
/// counts above zero.
pub type PieceCounts = Vec<(Role, u8)>;

/// What the strips show (Scid `gameInfo(showMaterial)`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize)]
pub enum MaterialMode {
    /// Nothing.
    #[serde(rename = "off")]
    Off,
    /// The surplus of each piece type.
    #[serde(rename = "difference")]
    Difference,
    /// The pieces each side has captured on the path to the cursor.
    #[default]
    #[serde(rename = "captured")]
    Captured,
}

impl MaterialMode {
    /// The spelling used in the TOML file.
    pub fn as_str(self) -> &'static str {
        match self {
            MaterialMode::Off => "off",
            MaterialMode::Difference => "difference",
            MaterialMode::Captured => "captured",
        }
    }

    /// Reads the TOML spelling.
    pub fn from_name(text: &str) -> Option<MaterialMode> {
        match text {
            "off" => Some(MaterialMode::Off),
            "difference" => Some(MaterialMode::Difference),
            "captured" => Some(MaterialMode::Captured),
            _ => None,
        }
    }

    /// The next mode in the cycle `Off > Difference > Captured > Off`.
    #[must_use]
    pub fn next(self) -> MaterialMode {
        match self {
            MaterialMode::Off => MaterialMode::Difference,
            MaterialMode::Difference => MaterialMode::Captured,
            MaterialMode::Captured => MaterialMode::Off,
        }
    }
}

/// The content of both strips, indexed by colour.
///
/// With `Difference` the pieces listed for a colour are the surplus pieces
/// *of that colour*; with `Captured` they are the pieces *that colour took*
/// (so they are of the opposite colour).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Material {
    /// White's strip.
    pub white: PieceCounts,
    /// Black's strip.
    pub black: PieceCounts,
    /// The side ahead in points and by how many (`None` when level or `Off`).
    pub advantage: Option<(Color, u32)>,
}

/// Roles of the strips in display order, with their point values.
const ORDER: [(Role, u32); 5] = [
    (Role::Queen, 9),
    (Role::Rook, 5),
    (Role::Bishop, 3),
    (Role::Knight, 3),
    (Role::Pawn, 1),
];

/// Counts per role in [`ORDER`].
type Counts = [u8; 5];

fn role_index(role: Role) -> Option<usize> {
    ORDER.iter().position(|(r, _)| *r == role)
}

fn side_index(color: Color) -> usize {
    match color {
        Color::White => 0,
        Color::Black => 1,
    }
}

fn to_list(counts: Counts) -> PieceCounts {
    ORDER
        .iter()
        .zip(&counts)
        .filter(|(_, n)| **n > 0)
        .map(|((role, _), n)| (*role, *n))
        .collect()
}

/// The pieces on the board per side: `[white, black]`.
fn board_counts(position: &Position) -> [Counts; 2] {
    let mut counts = [[0u8; 5]; 2];
    for (_, piece) in position.pieces() {
        if let Some(i) = role_index(piece.role) {
            let slot = &mut counts[side_index(piece.color)][i];
            *slot = slot.saturating_add(1);
        }
    }
    counts
}

fn points(counts: Counts) -> u32 {
    ORDER
        .iter()
        .zip(&counts)
        .map(|((_, value), n)| value * u32::from(*n))
        .sum()
}

/// The side ahead in points among the pieces on the board (P=1, N=B=3, R=5,
/// Q=9) and by how many points; `None` when level.
pub fn advantage(position: &Position) -> Option<(Color, u32)> {
    let [white, black] = board_counts(position);
    let (w, b) = (points(white), points(black));
    match w.cmp(&b) {
        std::cmp::Ordering::Greater => Some((Color::White, w - b)),
        std::cmp::Ordering::Less => Some((Color::Black, b - w)),
        std::cmp::Ordering::Equal => None,
    }
}

/// The surplus of each piece type per side on `position` (the
/// [`MaterialMode::Difference`] content).
pub fn difference(position: &Position) -> Material {
    let [white, black] = board_counts(position);
    let mut surplus = [[0u8; 5]; 2];
    for i in 0..ORDER.len() {
        let (w, b) = (white[i], black[i]);
        if w > b {
            surplus[0][i] = w - b;
        } else if b > w {
            surplus[1][i] = b - w;
        }
    }
    Material {
        white: to_list(surplus[0]),
        black: to_list(surplus[1]),
        advantage: advantage(position),
    }
}

/// The pieces each side captured on the path from the root to `cursor`:
/// `white` holds what White took, `black` what Black took.
///
/// Each move credits the piece it captured (a pawn for en passant, only the
/// captured piece for a promotion with capture) to the side that moved. An
/// unknown `cursor` gives empty lists.
pub fn captured_along(game: &Game, cursor: NodeId) -> (PieceCounts, PieceCounts) {
    let counts = captured_counts(game, cursor);
    (to_list(counts[0]), to_list(counts[1]))
}

/// [`captured_along`] as raw counts, walking from the cursor to the root
/// (no allocation).
fn captured_counts(game: &Game, cursor: NodeId) -> [Counts; 2] {
    let mut counts = [[0u8; 5]; 2];
    let mut node = cursor;
    while let Ok(Some(parent)) = game.parent(node) {
        let captured = game.mv(node).ok().and_then(mychess_core::Move::capture);
        let mover = game.position_at(parent).map(Position::turn);
        if let (Some(role), Ok(mover)) = (captured, mover)
            && let Some(i) = role_index(role)
        {
            let slot = &mut counts[side_index(mover)][i];
            *slot = slot.saturating_add(1);
        }
        node = parent;
    }
    counts
}

/// The content of the strips for the position at `cursor`.
///
/// `Off` gives an empty [`Material`] without advantage. An unknown `cursor`
/// gives the same.
pub fn material(game: &Game, cursor: NodeId, mode: MaterialMode) -> Material {
    let Ok(position) = game.position_at(cursor) else {
        return Material::default();
    };
    match mode {
        MaterialMode::Off => Material::default(),
        MaterialMode::Difference => difference(position),
        MaterialMode::Captured => {
            let (white, black) = captured_along(game, cursor);
            Material {
                white,
                black,
                advantage: advantage(position),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pos(fen: &str) -> Position {
        Position::from_fen(fen).expect("valid test FEN")
    }

    /// Plays SAN-free UCI moves from `game`'s root; returns the last node.
    fn play(game: &mut Game, from: NodeId, ucis: &[&str]) -> NodeId {
        let mut node = from;
        for uci in ucis {
            let position = game.position_at(node).expect("node").clone();
            let mv = position.parse_uci(uci).expect("legal uci");
            node = game.add_move(node, mv).expect("add");
        }
        node
    }

    fn game_after(ucis: &[&str]) -> (Game, NodeId) {
        let mut game = Game::new();
        let root = game.root();
        let end = play(&mut game, root, ucis);
        (game, end)
    }

    #[test]
    fn start_position_has_empty_strips_in_every_mode() {
        let game = Game::new();
        for mode in [
            MaterialMode::Off,
            MaterialMode::Difference,
            MaterialMode::Captured,
        ] {
            assert_eq!(material(&game, game.root(), mode), Material::default());
        }
    }

    #[test]
    fn captured_counts_the_real_captures_on_the_path() {
        // 1.e4 d5 2.exd5 Qxd5
        let (game, end) = game_after(&["e2e4", "d7d5", "e4d5", "d8d5"]);
        let m = material(&game, end, MaterialMode::Captured);
        assert_eq!(m.white, vec![(Role::Pawn, 1)]);
        assert_eq!(m.black, vec![(Role::Pawn, 1)]);
        assert_eq!(m.advantage, None);
    }

    #[test]
    fn captured_follows_the_cursor_not_the_end_of_the_game() {
        let (game, end) = game_after(&["e2e4", "d7d5", "e4d5", "d8d5"]);
        let mid = game.path_to(end).expect("path")[2]; // after exd5
        let m = material(&game, mid, MaterialMode::Captured);
        assert_eq!(m.white, vec![(Role::Pawn, 1)]);
        assert!(m.black.is_empty());
        assert_eq!(m.advantage, Some((Color::White, 1)));
    }

    #[test]
    fn en_passant_counts_one_pawn_for_the_mover() {
        // 1.e4 a6 2.e5 d5 3.exd6 (en passant)
        let (game, end) = game_after(&["e2e4", "a7a6", "e4e5", "d7d5", "e5d6"]);
        let m = material(&game, end, MaterialMode::Captured);
        assert_eq!(m.white, vec![(Role::Pawn, 1)]);
        assert!(m.black.is_empty());
    }

    #[test]
    fn promotion_without_capture_does_not_count_as_a_capture() {
        let mut game = Game::from_position(pos("4k3/P7/8/8/8/8/8/4K3 w - - 0 1"));
        let root = game.root();
        let end = play(&mut game, root, &["a7a8q"]);
        let m = material(&game, end, MaterialMode::Captured);
        assert!(m.white.is_empty() && m.black.is_empty());
        // The new queen shows in the points, though.
        assert_eq!(m.advantage, Some((Color::White, 9)));
    }

    #[test]
    fn promotion_with_capture_counts_only_the_captured_piece() {
        let mut game = Game::from_position(pos("1r2k3/P7/8/8/8/8/8/4K3 w - - 0 1"));
        let root = game.root();
        let end = play(&mut game, root, &["a7b8q"]);
        let m = material(&game, end, MaterialMode::Captured);
        assert_eq!(m.white, vec![(Role::Rook, 1)]);
        assert!(m.black.is_empty());
    }

    #[test]
    fn game_started_from_fen_does_not_count_missing_pieces() {
        let game = Game::from_position(pos("4k3/8/8/8/8/8/PPPPPPPP/4K3 w - - 0 1"));
        let m = material(&game, game.root(), MaterialMode::Captured);
        assert!(m.white.is_empty() && m.black.is_empty());
        assert_eq!(m.advantage, Some((Color::White, 8)));
    }

    #[test]
    fn captures_in_a_variation_follow_that_line_only() {
        let mut game = Game::new();
        let root = game.root();
        let main = play(&mut game, root, &["e2e4", "d7d5", "e4d5"]);
        let e4 = game.path_to(main).expect("path")[0];
        let var = play(&mut game, e4, &["e7e5", "f2f4", "e5f4"]);
        let m = material(&game, var, MaterialMode::Captured);
        assert!(m.white.is_empty());
        assert_eq!(m.black, vec![(Role::Pawn, 1)]);
        let m = material(&game, main, MaterialMode::Captured);
        assert_eq!(m.white, vec![(Role::Pawn, 1)]);
        assert!(m.black.is_empty());
    }

    #[test]
    fn difference_shows_the_surplus_and_ignores_the_path() {
        let p = pos("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNB1KBNR w KQkq - 0 1");
        let m = difference(&p);
        assert!(m.white.is_empty());
        assert_eq!(m.black, vec![(Role::Queen, 1)]);
        assert_eq!(m.advantage, Some((Color::Black, 9)));
    }

    #[test]
    fn difference_orders_queen_rook_bishop_knight_pawn() {
        let p = pos("4k3/8/8/8/8/8/P7/QRBN2K1 w - - 0 1");
        let roles: Vec<Role> = difference(&p).white.iter().map(|(r, _)| *r).collect();
        assert_eq!(
            roles,
            [
                Role::Queen,
                Role::Rook,
                Role::Bishop,
                Role::Knight,
                Role::Pawn
            ]
        );
    }

    #[test]
    fn difference_merges_equal_types_into_one_count() {
        let p = pos("4k3/8/8/8/8/8/PPP5/4K3 w - - 0 1");
        assert_eq!(difference(&p).white, vec![(Role::Pawn, 3)]);
    }

    #[test]
    fn bare_kings_have_no_material() {
        let game = Game::from_position(pos("4k3/8/8/8/8/8/8/4K3 w - - 0 1"));
        for mode in [MaterialMode::Difference, MaterialMode::Captured] {
            assert_eq!(material(&game, game.root(), mode), Material::default());
        }
    }

    #[test]
    fn off_has_no_advantage_even_when_unbalanced() {
        let game = Game::from_position(pos("4k3/8/8/8/8/8/8/Q3K3 w - - 0 1"));
        assert_eq!(
            material(&game, game.root(), MaterialMode::Off),
            Material::default()
        );
        assert_eq!(
            material(&game, game.root(), MaterialMode::Captured).advantage,
            Some((Color::White, 9))
        );
    }

    #[test]
    fn stale_node_id_gives_empty_material() {
        let (_, end) = game_after(&["e2e4"]);
        let other = Game::new();
        assert_eq!(
            material(&other, end, MaterialMode::Captured),
            Material::default()
        );
        assert_eq!(captured_along(&other, end), (vec![], vec![]));
    }

    #[test]
    fn mode_cycles_and_round_trips_names() {
        assert_eq!(MaterialMode::Off.next(), MaterialMode::Difference);
        assert_eq!(MaterialMode::Difference.next(), MaterialMode::Captured);
        assert_eq!(MaterialMode::Captured.next(), MaterialMode::Off);
        for mode in [
            MaterialMode::Off,
            MaterialMode::Difference,
            MaterialMode::Captured,
        ] {
            assert_eq!(MaterialMode::from_name(mode.as_str()), Some(mode));
        }
        assert_eq!(MaterialMode::from_name("all"), None);
        assert_eq!(MaterialMode::default(), MaterialMode::Captured);
    }
}
