//! The premove: one move made while the opponent thinks, kept on the client
//! and sent when it becomes legal (spec M8-3, D1321).

use mychess_core::{Color, Move, Position, Role, Square};

/// A premove: squares and promotion piece, nothing else is remembered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Premove {
    /// Origin.
    pub from: Square,
    /// Destination.
    pub to: Square,
    /// Promotion piece.
    pub promotion: Option<Role>,
}

/// Whether the piece on `from` of `color` could ever go to `to` by the way
/// its kind moves, ignoring what stands in between (the board will change
/// before the move is sent). Castling is accepted by shape (the king two
/// files, or onto its own rook); its legality is decided when the premove is
/// played (D4450).
pub fn is_pseudo_legal(position: &Position, color: Color, from: Square, to: Square) -> bool {
    let Some(piece) = position.piece_at(from) else {
        return false;
    };
    if piece.color != color || from == to {
        return false;
    }
    let file = |s: Square| i32::from(u8::from(s.file()));
    let rank = |s: Square| i32::from(u8::from(s.rank()));
    let (df, dr) = (file(to) - file(from), rank(to) - rank(from));
    let (af, ar) = (df.abs(), dr.abs());
    let back_rank = if color == Color::White { 0 } else { 7 };
    // Castling: the king goes two files, or onto its own rook (the way
    // Chess960 and "king takes rook" gestures say it). Only the shape is
    // checked here; legality is decided when the premove is played (D4451).
    if piece.role == Role::King && ar == 0 && rank(from) == back_rank {
        let onto_own_rook = position
            .piece_at(to)
            .is_some_and(|t| t.color == color && t.role == Role::Rook);
        if af == 2 || onto_own_rook {
            return true;
        }
    }
    // A piece of one's own colour on the target is not necessarily there
    // later, but a premove onto it is never meant; Lichess refuses it too.
    if position
        .piece_at(to)
        .is_some_and(|target| target.color == color)
    {
        return false;
    }
    match piece.role {
        Role::Knight => (af, ar) == (1, 2) || (af, ar) == (2, 1),
        Role::Bishop => af == ar,
        Role::Rook => df == 0 || dr == 0,
        Role::Queen => df == 0 || dr == 0 || af == ar,
        Role::King => af <= 1 && ar <= 1,
        Role::Pawn => {
            let forward = if color == Color::White { 1 } else { -1 };
            let home = if color == Color::White { 1 } else { 6 };
            let straight = df == 0 && (dr == forward || (dr == 2 * forward && rank(from) == home));
            let capture = af == 1 && dr == forward;
            straight || capture
        }
    }
}

impl Premove {
    /// Builds the premove of `color` from `from` to `to`, or `None` when the
    /// piece could never make that move (see [`is_pseudo_legal`]).
    ///
    /// A pawn that reaches the last rank promotes: to `chosen`, or to a
    /// queen when none was chosen. `chosen` is ignored for any other move.
    pub fn enter(
        position: &Position,
        color: Color,
        from: Square,
        to: Square,
        chosen: Option<Role>,
    ) -> Option<Premove> {
        if !is_pseudo_legal(position, color, from, to) {
            return None;
        }
        let piece = position.piece_at(from)?;
        let last_rank = if color == Color::White { 7 } else { 0 };
        let promotes = piece.role == Role::Pawn && i32::from(u8::from(to.rank())) == last_rank;
        let promotion = promotes.then_some(match chosen {
            Some(role @ (Role::Rook | Role::Bishop | Role::Knight)) => role,
            _ => Role::Queen,
        });
        Some(Premove {
            from,
            to,
            promotion,
        })
    }

    /// The premove as a legal move of `position`, or `None` (it is then
    /// dropped without a word, D1321).
    pub fn resolve(self, position: &Position) -> Option<Move> {
        position.find_move(self.from, self.to, self.promotion)
    }
}

/// The single premove the client keeps (D1321): pure state, no I/O.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PremoveSlot {
    premove: Option<Premove>,
}

impl PremoveSlot {
    /// An empty slot.
    pub fn new() -> PremoveSlot {
        PremoveSlot::default()
    }

    /// The premove kept, if any.
    pub fn get(self) -> Option<Premove> {
        self.premove
    }

    /// Whether a premove is kept.
    pub fn is_set(self) -> bool {
        self.premove.is_some()
    }

    /// Keeps `premove`, replacing the previous one (one at a time).
    pub fn set(&mut self, premove: Premove) {
        self.premove = Some(premove);
    }

    /// Drops the premove; reports whether there was one.
    pub fn cancel(&mut self) -> bool {
        self.premove.take().is_some()
    }

    /// Empties the slot and returns the premove as a legal move of
    /// `position` (the position after the opponent's move), or `None` when it
    /// is illegal there. Either way the slot ends empty: an illegal premove
    /// is dropped silently (D1321).
    pub fn take_if_legal(&mut self, position: &Position) -> Option<Move> {
        self.premove.take()?.resolve(position)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sq(s: &str) -> Square {
        s.parse().expect("square")
    }

    #[test]
    fn pieces_move_as_their_kind_does() {
        let p = Position::new();
        let w = Color::White;
        assert!(is_pseudo_legal(&p, w, sq("e2"), sq("e4")));
        assert!(is_pseudo_legal(&p, w, sq("g1"), sq("f3")));
        // Through own pieces is fine for a premove (the board changes) ...
        assert!(is_pseudo_legal(&p, w, sq("a1"), sq("a5")));
        // ... but not a wrong shape, an own piece as target or the other
        // side's piece.
        assert!(!is_pseudo_legal(&p, w, sq("g1"), sq("g3")));
        assert!(!is_pseudo_legal(&p, w, sq("a1"), sq("a2")));
        assert!(!is_pseudo_legal(&p, w, sq("e7"), sq("e5")));
        assert!(!is_pseudo_legal(&p, w, sq("e2"), sq("e5")));
        assert!(!is_pseudo_legal(&p, w, sq("e3"), sq("e4")));
    }

    #[test]
    fn castling_is_a_premove_by_king_target_or_onto_the_rook() {
        let p = Position::from_fen("r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1").expect("fen");
        assert!(is_pseudo_legal(&p, Color::White, sq("e1"), sq("g1")));
        assert!(is_pseudo_legal(&p, Color::White, sq("e1"), sq("c1")));
        assert!(is_pseudo_legal(&p, Color::White, sq("e1"), sq("h1")));
        assert!(is_pseudo_legal(&p, Color::White, sq("e1"), sq("f1")));
        // Not off the back rank, not three files, not onto another piece.
        assert!(!is_pseudo_legal(&p, Color::White, sq("e1"), sq("b1")));
        let mid = Position::from_fen("4k3/8/8/8/8/4K3/8/8 w - - 0 1").expect("fen");
        assert!(!is_pseudo_legal(&mid, Color::White, sq("e3"), sq("g3")));
    }

    #[test]
    fn a_castling_premove_is_played_only_when_castling_is_legal_then() {
        let free = Position::from_fen("r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1").expect("fen");
        let pm = Premove::enter(&free, Color::White, sq("e1"), sq("g1"), None).expect("pm");
        assert!(pm.resolve(&free).expect("legal").is_castle());
        let onto = Premove::enter(&free, Color::White, sq("e1"), sq("a1"), None).expect("pm");
        assert!(onto.resolve(&free).expect("legal").is_castle());
        // The rook was captured meanwhile: the premove is dropped.
        let gone = Position::from_fen("r3k2r/8/8/8/8/8/8/R3K3 w Qkq - 0 1").expect("fen");
        assert!(pm.resolve(&gone).is_none());
        // Castling through an attacked square is illegal.
        let attacked = Position::from_fen("r3k2r/8/8/8/8/8/5r2/R3K2R w KQkq - 0 1").expect("fen");
        assert!(pm.resolve(&attacked).is_none());
    }

    #[test]
    fn en_passant_is_a_premove_and_resolves_when_legal() {
        let before = Position::from_fen("4k3/8/8/3pP3/8/8/8/4K3 w - d6 0 1").expect("fen");
        let pm = Premove::enter(&before, Color::White, sq("e5"), sq("d6"), None).expect("pm");
        let mv = pm.resolve(&before).expect("en passant is legal");
        assert_eq!(mv.to(), Some(sq("d6")));
        let none = Position::from_fen("4k3/8/8/3pP3/8/8/8/4K3 w - - 0 1").expect("fen");
        assert!(pm.resolve(&none).is_none(), "no en passant square");
    }

    #[test]
    fn a_premove_becomes_a_move_only_if_legal_then() {
        let p = Position::new();
        let ok = Premove {
            from: sq("e2"),
            to: sq("e4"),
            promotion: None,
        };
        assert!(ok.resolve(&p).is_some());
        let bad = Premove {
            from: sq("e2"),
            to: sq("e5"),
            promotion: None,
        };
        assert!(bad.resolve(&p).is_none());
    }

    #[test]
    fn a_pawn_reaching_the_last_rank_promotes_to_a_queen_by_default() {
        let pos = Position::from_fen("4k3/P7/8/8/8/8/8/4K3 w - - 0 1").expect("fen");
        let enter = |chosen| {
            Premove::enter(&pos, Color::White, sq("a7"), sq("a8"), chosen).expect("premove")
        };
        assert_eq!(enter(None).promotion, Some(Role::Queen));
        let knight = enter(Some(Role::Knight));
        assert_eq!(knight.promotion, Some(Role::Knight));
        assert_eq!(
            enter(Some(Role::King)).promotion,
            Some(Role::Queen),
            "a pawn cannot become a king"
        );
        let mv = knight.resolve(&pos).expect("legal");
        assert_eq!(mv.promotion(), Some(Role::Knight));
    }

    #[test]
    fn a_choice_is_ignored_when_the_move_does_not_promote() {
        let p = Position::new();
        let e4 = Premove::enter(&p, Color::White, sq("e2"), sq("e4"), Some(Role::Rook));
        assert_eq!(e4.expect("premove").promotion, None);
        assert!(Premove::enter(&p, Color::White, sq("g1"), sq("g3"), None).is_none());
    }

    #[test]
    fn the_slot_keeps_one_premove_and_cancels_it() {
        let p = Position::new();
        let mut slot = PremoveSlot::new();
        assert!(!slot.is_set() && !slot.cancel());
        let a = Premove::enter(&p, Color::White, sq("e2"), sq("e4"), None).expect("a");
        let b = Premove::enter(&p, Color::White, sq("d2"), sq("d4"), None).expect("b");
        slot.set(a);
        slot.set(b);
        assert_eq!(slot.get(), Some(b), "the newer replaces the older");
        assert!(slot.cancel());
        assert!(slot.get().is_none());
    }

    #[test]
    fn take_if_legal_empties_the_slot_and_drops_the_illegal_one() {
        // White premoves Nf3 while it is Black's turn; Black answers.
        let start = Position::new();
        let mut slot = PremoveSlot::new();
        slot.set(Premove::enter(&start, Color::White, sq("g1"), sq("f3"), None).expect("pm"));
        let legal = slot.take_if_legal(&start).expect("legal");
        assert_eq!(legal.to(), Some(sq("f3")));
        assert!(!slot.is_set());

        // A rook premove through a square the opponent then blocks is
        // illegal; so is a premove whose piece was captured.
        let p = Position::from_fen("4k3/8/8/8/8/8/r7/R3K3 w - - 0 1").expect("fen");
        slot.set(Premove::enter(&p, Color::White, sq("a1"), sq("a5"), None).expect("pm"));
        assert!(slot.take_if_legal(&p).is_none(), "a2 blocks the file");
        assert!(!slot.is_set(), "the illegal premove is dropped");
        assert!(slot.take_if_legal(&p).is_none(), "empty slot");
    }

    #[test]
    fn a_premove_that_exposes_the_king_is_dropped() {
        let p = Position::from_fen("4r1k1/8/8/8/8/8/4B3/4K3 w - - 0 1").expect("fen");
        let mut slot = PremoveSlot::new();
        slot.set(Premove::enter(&p, Color::White, sq("e2"), sq("d3"), None).expect("pm"));
        assert!(slot.take_if_legal(&p).is_none(), "the bishop is pinned");
    }
}
