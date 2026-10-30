// Scid: src/optable.cpp `OpLine::SetPositionalThemes` 184-235,
// `posHasIQP`/`posHasAdvancedPawn`/`posHasKPawnStorm`/`posHasOpenFyle`
// 41-82 and `endgameTheme` 20-39. Copyright (c) 2001-2003 Shane Hudson.
// Scid is distributed under the GNU General Public License (see its
// COPYING); this file is part of MyChess, GPL-3.0-or-later
// (docs/adr/0004-licenca.md).

//! The positional themes of a position and the class of the final material
//! (decisions D3606 and D3607).

use mychess_core::{Color, Piece, Role, Square};

use super::model::{EndTheme, NUM_THEMES, Theme};
use crate::si4::{ListPosition, MaterialSignature};

/// What the themes need to know about a position, read in one walk over the
/// pieces (no allocation).
#[derive(Default)]
struct Census {
    king_file: [Option<u8>; 2],
    queens: [u8; 2],
    bishops: [u8; 2],
    /// Pawns per file, per colour.
    pawn_files: [[u8; 8]; 2],
    /// Pawns per rank (0 = 1st rank), per colour.
    pawn_ranks: [[u8; 8]; 2],
}

fn slot(color: Color) -> usize {
    match color {
        Color::White => 0,
        Color::Black => 1,
    }
}

fn census(pos: &ListPosition) -> Census {
    let mut c = Census::default();
    for color in [Color::White, Color::Black] {
        let i = slot(color);
        for &sq in pos.piece_squares(color) {
            let Some(piece) = pos.piece_at(sq) else {
                continue;
            };
            match piece.role {
                Role::King => c.king_file[i] = Some(file_of(sq)),
                Role::Queen => c.queens[i] = c.queens[i].saturating_add(1),
                Role::Bishop => c.bishops[i] = c.bishops[i].saturating_add(1),
                Role::Pawn => {
                    let (f, r) = (usize::from(file_of(sq)), usize::from(rank_of(sq)));
                    if let Some(n) = c.pawn_files[i].get_mut(f) {
                        *n = n.saturating_add(1);
                    }
                    if let Some(n) = c.pawn_ranks[i].get_mut(r) {
                        *n = n.saturating_add(1);
                    }
                }
                _ => {}
            }
        }
    }
    c
}

fn file_of(sq: Square) -> u8 {
    u8::try_from(u32::from(sq) % 8).unwrap_or(0)
}

fn rank_of(sq: Square) -> u8 {
    u8::try_from(u32::from(sq) / 8).unwrap_or(0)
}

fn has_pawn(pos: &ListPosition, color: Color, sq: Square) -> bool {
    pos.piece_at(sq)
        == Some(Piece {
            color,
            role: Role::Pawn,
        })
}

/// A pawn storm: no pawn of the colour on the g or h file squares of its
/// second and third ranks, but one on the g or h file.
fn kingside_storm(pos: &ListPosition, c: &Census, color: Color) -> bool {
    let squares = match color {
        Color::White => [Square::G2, Square::H2, Square::G3, Square::H3],
        Color::Black => [Square::G7, Square::H7, Square::G6, Square::H6],
    };
    let files = &c.pawn_files[slot(color)];
    squares.iter().all(|&sq| !has_pawn(pos, color, sq))
        && (files.get(6).copied().unwrap_or(0) > 0 || files.get(7).copied().unwrap_or(0) > 0)
}

/// Isolated queen's pawn: no pawn on the c and e files and one on the d file.
fn isolated_d_pawn(c: &Census, color: Color) -> bool {
    let f = &c.pawn_files[slot(color)];
    f.get(2).copied().unwrap_or(0) == 0
        && f.get(4).copied().unwrap_or(0) == 0
        && f.get(3).copied().unwrap_or(0) > 0
}

fn advanced_pawn(c: &Census, color: Color) -> bool {
    let r = &c.pawn_ranks[slot(color)];
    let any = |ranks: [usize; 3]| ranks.iter().any(|&i| r.get(i).copied().unwrap_or(0) > 0);
    match color {
        // Ranks 5, 6 and 7 / ranks 4, 3 and 2.
        Color::White => any([4, 5, 6]),
        Color::Black => any([3, 2, 1]),
    }
}

fn open_file(c: &Census, file: usize) -> bool {
    c.pawn_files[0].get(file).copied().unwrap_or(0) == 0
        && c.pawn_files[1].get(file).copied().unwrap_or(0) == 0
}

/// Adds the themes that `pos` shows to `counts`.
pub(crate) fn add_positional_themes(pos: &ListPosition, counts: &mut [u8; NUM_THEMES]) {
    let c = census(pos);
    let mut bump = |theme: Theme| {
        if let Some(n) = counts.get_mut(theme.index()) {
            *n = n.saturating_add(1);
        }
    };
    if let (Some(w), Some(b)) = (c.king_file[0], c.king_file[1]) {
        if (w <= 2 && b <= 2) || (w >= 6 && b >= 6) {
            bump(Theme::SameCastling);
        }
        if (w <= 2 && b >= 6) || (w >= 6 && b <= 2) {
            bump(Theme::OppositeCastling);
        }
    }
    if c.queens == [0, 0] {
        bump(Theme::QueensOff);
    }
    if (c.bishops[0] >= 2) != (c.bishops[1] >= 2) {
        bump(Theme::OneBishopPair);
    }
    if kingside_storm(pos, &c, Color::White) || kingside_storm(pos, &c, Color::Black) {
        bump(Theme::KingsidePawnStorm);
    }
    if isolated_d_pawn(&c, Color::White) {
        bump(Theme::WhiteIqp);
    }
    if isolated_d_pawn(&c, Color::Black) {
        bump(Theme::BlackIqp);
    }
    if advanced_pawn(&c, Color::White) {
        bump(Theme::WhiteAdvancedPawn);
    }
    if advanced_pawn(&c, Color::Black) {
        bump(Theme::BlackAdvancedPawn);
    }
    if open_file(&c, 2) || open_file(&c, 3) || open_file(&c, 4) {
        bump(Theme::OpenCdeFile);
    }
}

/// The class of the final material of a game: queens, then rooks, then
/// minor pieces decide (`endgameTheme`).
pub(crate) fn end_theme(final_mat_sig: u32) -> EndTheme {
    let m = MaterialSignature::from_bits(final_mat_sig);
    let queens = m.white_queens > 0 || m.black_queens > 0;
    let rooks = m.white_rooks > 0 || m.black_rooks > 0;
    let minors =
        m.white_bishops > 0 || m.black_bishops > 0 || m.white_knights > 0 || m.black_knights > 0;
    match (queens, rooks, minors) {
        (false, false, false) => EndTheme::Pawns,
        (false, false, true) => EndTheme::Minor,
        (false, true, false) => EndTheme::Rook,
        (false, true, true) => EndTheme::RookMinor,
        (true, false, false) => EndTheme::Queen,
        (true, false, true) => EndTheme::QueenMinor,
        (true, true, false) => EndTheme::QueenRook,
        (true, true, true) => EndTheme::QueenRookMinor,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn themes(fen: &str) -> [u8; NUM_THEMES] {
        let pos = ListPosition::from_fen(fen).expect("test fen");
        let mut out = [0; NUM_THEMES];
        add_positional_themes(&pos, &mut out);
        out
    }

    const START: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";

    #[test]
    fn start_position_has_same_wing_kings_and_no_other_theme() {
        let t = themes(START);
        // Kings on the e file: neither a-c nor g-h.
        assert_eq!(t, [0; NUM_THEMES]);
    }

    #[test]
    fn castled_kings_same_and_opposite() {
        let same = themes("r1bq1rk1/pppp1ppp/2n5/8/8/8/PPPP1PPP/R1BQ1RK1 w - - 0 1");
        assert_eq!((same[0], same[1]), (1, 0));
        let opposite = themes("2kr4/pppppppp/8/8/8/8/PPPPPPPP/R3K3 w - - 0 1");
        assert_eq!((opposite[0], opposite[1]), (0, 0), "king on e1 is no wing");
        let opposite = themes("2kr4/pppppppp/8/8/8/8/PPPPPPPP/5RK1 w - - 0 1");
        assert_eq!((opposite[0], opposite[1]), (0, 1));
    }

    #[test]
    fn queens_off_needs_no_queen_at_all() {
        assert_eq!(themes(START)[2], 0);
        let t = themes("rnb1kbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w - - 0 1");
        assert_eq!(t[2], 0, "White still has a queen");
        let t = themes("rnb1kbnr/pppppppp/8/8/8/8/PPPPPPPP/RNB1KBNR w - - 0 1");
        assert_eq!(t[2], 1);
    }

    #[test]
    fn bishop_pair_only_one_side() {
        let t = themes("rn1qkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w - - 0 1");
        assert_eq!(t[3], 1);
        let both = themes("rn1qkbnr/pppppppp/8/8/8/8/PPPPPPPP/RN1QKBNR w - - 0 1");
        assert_eq!(both[3], 0, "neither side has the pair");
    }

    #[test]
    fn pawn_storm_needs_the_pawns_gone_and_one_on_g_or_h() {
        // White pawn on g4, none on g2/h2/g3/h3.
        let t = themes("4k3/pppppppp/8/8/6P1/8/PPPPPP2/4K3 w - - 0 1");
        assert_eq!(t[4], 1);
        // Pawn still on h2.
        let t = themes("4k3/pppppppp/8/8/6P1/8/PPPPPP1P/4K3 w - - 0 1");
        assert_eq!(t[4], 0);
        // Black: no pawn on g7/h7/g6/h6, one on h5.
        let t = themes("4k3/pppppp2/8/7p/8/8/PPPPPPPP/4K3 w - - 0 1");
        assert_eq!(t[4], 1);
    }

    #[test]
    fn isolated_queens_pawn_by_colour() {
        let t = themes("4k3/pp3ppp/8/8/8/8/PP1P1PPP/4K3 w - - 0 1");
        assert_eq!((t[5], t[6]), (1, 0));
        let t = themes("4k3/pp1p1ppp/8/8/8/8/PP3PPP/4K3 w - - 0 1");
        assert_eq!((t[5], t[6]), (0, 1));
    }

    #[test]
    fn advanced_pawns_by_rank() {
        let t = themes("4k3/8/8/4P3/8/8/8/4K3 w - - 0 1");
        assert_eq!((t[7], t[8]), (1, 0));
        let t = themes("4k3/8/8/8/8/4p3/8/4K3 w - - 0 1");
        assert_eq!((t[7], t[8]), (0, 1));
        let t = themes("4k3/8/8/8/4P3/8/8/4K3 w - - 0 1");
        assert_eq!(t[7], 0, "4th rank is not advanced for White");
    }

    #[test]
    fn open_file_any_of_c_d_e() {
        assert_eq!(themes(START)[9], 0);
        let t = themes("rnbqkbnr/pp1ppppp/8/8/8/8/PP1PPPPP/RNBQKBNR w - - 0 1");
        assert_eq!(t[9], 1);
        let t = themes("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w - - 0 1");
        assert_eq!(t[9], 0);
    }

    #[test]
    fn end_theme_priority_queen_rook_minor() {
        let sig =
            |wq: u32, wr: u32, wb: u32, wn: u32| (wq << 22) | (wr << 20) | (wb << 18) | (wn << 16);
        assert_eq!(end_theme(0), EndTheme::Pawns);
        assert_eq!(end_theme(sig(0, 0, 1, 0)), EndTheme::Minor);
        assert_eq!(end_theme(sig(0, 0, 0, 2)), EndTheme::Minor);
        assert_eq!(end_theme(sig(0, 1, 0, 0)), EndTheme::Rook);
        assert_eq!(end_theme(sig(0, 1, 1, 0)), EndTheme::RookMinor);
        assert_eq!(end_theme(sig(1, 0, 0, 0)), EndTheme::Queen);
        assert_eq!(end_theme(sig(1, 0, 0, 1)), EndTheme::QueenMinor);
        assert_eq!(end_theme(sig(1, 1, 0, 0)), EndTheme::QueenRook);
        assert_eq!(end_theme(sig(1, 1, 1, 1)), EndTheme::QueenRookMinor);
        // Black pieces count too (bits 10, 8, 6, 4).
        assert_eq!(end_theme(1 << 10), EndTheme::Queen);
    }
}
