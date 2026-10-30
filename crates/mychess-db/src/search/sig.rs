// Derived from Scid vs PC 4.27, src/matsig.cpp (`matsig_isReachable`,
// `hpSig_PossibleMatch`, `hpSig_Final`) and src/matsig.h. Copyright (c)
// 1999-2002 Shane Hudson. Scid is distributed under the GNU General Public
// License (see its COPYING); this file is part of MyChess,
// GPL-3.0-or-later (docs/adr/0004-licenca.md).

//! The index shortcuts of the searches (decision D3207 of
//! `docs/specs/m6-1-buscas-avancadas.md`): tests on the final material
//! signature and on the home-pawn data of an [`IndexEntry`].
//!
//! They only **accelerate** a search: a game they discard could not have
//! matched. They are valid only for games with the standard start position
//! (flag `START` clear), because Scid leaves stale values in the index of the
//! others (`docs/formatos/si4.md`, section 3).

use crate::si4::{GameFlags, IndexEntry, MaterialSignature};

/// The home-pawn signature of the standard start position.
pub(crate) const HPSIG_STD_START: u16 = 0xFFFF;

/// Whether the shortcuts may be used for this game.
pub(crate) fn index_is_trusted(entry: &IndexEntry) -> bool {
    !entry.flags.contains(GameFlags::START)
}

/// `matsig_isReachable`: whether `start` could turn into `target` with
/// captures and (maybe) promotions. Signatures are the 24-bit `FinalMatSig`
/// layout.
pub(crate) fn is_reachable(start: u32, target: u32, promos: bool, upromo: bool) -> bool {
    let s = MaterialSignature::from_bits(start);
    let t = MaterialSignature::from_bits(target);
    if s.white_pawns < t.white_pawns || s.black_pawns < t.black_pawns {
        return false;
    }
    if upromo {
        return true;
    }
    if s.white_rooks < t.white_rooks
        || s.black_rooks < t.black_rooks
        || s.white_bishops < t.white_bishops
        || s.black_bishops < t.black_bishops
        || s.white_knights < t.white_knights
        || s.black_knights < t.black_knights
    {
        return false;
    }
    if promos {
        return true;
    }
    s.white_queens >= t.white_queens && s.black_queens >= t.black_queens
}

/// Whether the entry says the game has a promotion / an under-promotion.
pub(crate) fn promotion_flags(entry: &IndexEntry) -> (bool, bool) {
    (
        entry.flags.contains(GameFlags::PROMOTION),
        entry.flags.contains(GameFlags::UNDER_PROMOTION),
    )
}

/// `MATSIG_FlipColor`: swaps the white and black halves of a signature.
pub(crate) fn flip_signature(sig: u32) -> u32 {
    ((sig >> 12) & 0xFFF) | ((sig & 0xFFF) << 12)
}

/// `hpSig_PossibleMatch`: whether a game whose home-pawn changes are in
/// `entry` passes through a position with home-pawn signature `sig`.
pub(crate) fn hp_possible_match(sig: u16, entry: &IndexEntry) -> bool {
    if sig == HPSIG_STD_START {
        return true;
    }
    let mut current = HPSIG_STD_START;
    for change in entry.home_pawn_changes() {
        current &= !(1u16 << (change & 15));
        if current == sig {
            return true;
        }
        // A pawn never comes back to its home square.
        if current & sig != sig {
            return false;
        }
    }
    false
}

/// `hpSig_Final`: the home-pawn signature at the end of the main line.
pub(crate) fn hp_final(entry: &IndexEntry) -> u16 {
    let mut sig = HPSIG_STD_START;
    for change in entry.home_pawn_changes() {
        sig &= !(1u16 << (change & 15));
    }
    sig
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reachable_follows_the_promotion_flags() {
        // Start: white queen (1), nothing else; target: two white queens.
        let one_queen = 1 << 22;
        let two_queens = 2 << 22;
        assert!(!is_reachable(one_queen, two_queens, false, false));
        assert!(is_reachable(one_queen, two_queens, true, false));
        // Rooks can only come from an under-promotion.
        let one_rook = 1 << 20;
        let two_rooks = 2 << 20;
        assert!(!is_reachable(one_rook, two_rooks, true, false));
        assert!(is_reachable(one_rook, two_rooks, true, true));
        // Pawns are always checked.
        assert!(!is_reachable(1 << 12, 2 << 12, true, true));
    }

    #[test]
    fn flip_swaps_the_halves() {
        assert_eq!(flip_signature(0x00AB_C123), 0x0012_3ABC);
    }
}
