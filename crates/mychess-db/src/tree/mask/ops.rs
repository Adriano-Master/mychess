// Derived from Scid vs PC 4.27, tcl/windows/tree.tcl (`::tree::mask::op` 1977,
// `addToMask` 2078, `removeFromMask` 2099, colors, NAGs, comments and markers
// 2154-2341). Copyright (c) 1999-2002 Shane Hudson; Scid vs PC contributors.
// Scid is distributed under the GNU General Public License (see its
// COPYING); this file is part of MyChess, GPL-3.0-or-later
// (docs/adr/0004-licenca.md).

//! Editing and querying a mask by position (decisions D1821, D1828).

use mychess_core::{Move, Position};

use super::matching::{MoveMatcher, resolve_move};
use super::model::{
    MAX_FIELD_BYTES, MAX_MOVES_PER_POSITION, Marker, MaskColor, MaskKey, MaskMove, MaskNag,
    MaskPosition, TreeMask, clean_comment,
};

/// Which of the two markers of a move.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MarkerSlot {
    /// The first marker.
    First,
    /// The second marker.
    Second,
}

impl MarkerSlot {
    fn index(self) -> usize {
        match self {
            MarkerSlot::First => 0,
            MarkerSlot::Second => 1,
        }
    }
}

/// An edit of the mask at one position. The `san` of every move operation is
/// the text of the move (use [`mask_san`](super::mask_san) for a new one); an
/// existing equivalent entry (D1805) is reused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MaskOp {
    /// Adds the move (nothing if it is there).
    AddMove {
        /// The move.
        san: String,
    },
    /// Removes the move; the position goes away when it is left with no move
    /// and no comment.
    RemoveMove {
        /// The move.
        san: String,
    },
    /// Sets the comment of the move.
    SetMoveComment {
        /// The move.
        san: String,
        /// The text.
        text: String,
    },
    /// Sets the comment of the position (creates the position).
    SetPositionComment {
        /// The text.
        text: String,
    },
    /// Sets the NAG of the move.
    SetNag {
        /// The move.
        san: String,
        /// The NAG, `None` for none.
        nag: Option<MaskNag>,
    },
    /// Sets the color of the move.
    SetColor {
        /// The move.
        san: String,
        /// The color.
        color: MaskColor,
    },
    /// Sets one of the markers of the move.
    SetMarker {
        /// The move.
        san: String,
        /// Which marker.
        slot: MarkerSlot,
        /// The marker, `None` for none.
        marker: Option<Marker>,
    },
}

/// A move of the mask that the tree does not have (D1828).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OnlyMaskMove<'a> {
    /// The entry.
    pub entry: &'a MaskMove,
    /// The legal move it names in the position; `None` for an illegal entry
    /// (case 12 of the spec).
    pub legal: Option<Move>,
}

/// What the mask says about a position, laid over the rows of the tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaskOverlay<'a> {
    /// The comment of the position (empty if none).
    pub comment: &'a str,
    /// For each tree row (same order as asked), the entry of the mask.
    pub rows: Vec<Option<&'a MaskMove>>,
    /// The visible moves of the mask that no row has, in mask order.
    pub only_mask: Vec<OnlyMaskMove<'a>>,
}

impl TreeMask {
    /// The entry of the move `san` in `position`, using D1805.
    pub fn lookup_move(&self, position: &Position, san: &str) -> Option<&MaskMove> {
        let entry = self.position(position)?;
        let matcher = MoveMatcher::new(Some(position));
        find_move(entry, &matcher, san).and_then(|i| entry.moves.get(i))
    }

    /// Lays the mask of `position` over the rows of the tree (`tree_sans`,
    /// the SAN of each row): the entry of each row, the position comment, and
    /// the moves only the mask has (D1828). `null` is never returned.
    pub fn overlay<'a>(&'a self, position: &Position, tree_sans: &[&str]) -> MaskOverlay<'a> {
        let Some(entry) = self.position(position) else {
            return MaskOverlay {
                comment: "",
                rows: vec![None; tree_sans.len()],
                only_mask: Vec::new(),
            };
        };
        let matcher = MoveMatcher::new(Some(position));
        let mut used = vec![false; entry.moves.len()];
        let rows = tree_sans
            .iter()
            .map(|san| {
                let i = find_move(entry, &matcher, san)?;
                if let Some(u) = used.get_mut(i) {
                    *u = true;
                }
                entry.moves.get(i)
            })
            .collect();
        let only_mask = entry
            .moves
            .iter()
            .zip(&used)
            .filter(|(m, used)| !**used && !m.is_null())
            .map(|(m, _)| OnlyMaskMove {
                entry: m,
                legal: resolve_move(position, &m.san),
            })
            .collect();
        MaskOverlay {
            comment: &entry.comment,
            rows,
            only_mask,
        }
    }

    /// Applies `op` at `position` (D1821). Returns whether the mask changed;
    /// an operation with no effect (same value, absent move to remove) does
    /// not count. Every operation on a move that is not in the mask adds it
    /// first, except removal.
    pub fn apply(&mut self, position: &Position, op: &MaskOp) -> bool {
        let key = MaskKey::from_position(position);
        self.apply_at(&key, Some(position), op)
    }

    pub(crate) fn apply_at(
        &mut self,
        key: &MaskKey,
        position: Option<&Position>,
        op: &MaskOp,
    ) -> bool {
        let matcher = MoveMatcher::new(position);
        match op {
            MaskOp::RemoveMove { san } => self.remove_move(key, &matcher, san),
            MaskOp::SetPositionComment { text } => self.set_position_comment(key, text),
            MaskOp::AddMove { san } => self.ensure_move(key, &matcher, san).is_some_and(|c| c.1),
            MaskOp::SetMoveComment { san, text } => {
                let (clean, _) = clean_comment(text);
                self.edit_move(key, &matcher, san, |m| {
                    replace(&mut m.comment, clean.clone())
                })
            }
            // The values are normalized to what the file gives back (D1852);
            // a text too long to be stored makes the operation a no-op.
            MaskOp::SetNag { san, nag } => {
                let nag = match nag.as_ref().map(MaskNag::normalized).transpose() {
                    Ok(n) => n.flatten(),
                    Err(_) => return false,
                };
                self.edit_move(key, &matcher, san, |m| replace(&mut m.nag, nag))
            }
            MaskOp::SetColor { san, color } => {
                let Ok(color) = color.normalized() else {
                    return false;
                };
                self.edit_move(key, &matcher, san, |m| replace(&mut m.color, color))
            }
            MaskOp::SetMarker { san, slot, marker } => {
                let marker = match marker.as_ref().map(Marker::normalized).transpose() {
                    Ok(m) => m.flatten(),
                    Err(_) => return false,
                };
                self.edit_move(key, &matcher, san, |m| {
                    m.markers
                        .get_mut(slot.index())
                        .is_some_and(|cell| replace(cell, marker))
                })
            }
        }
    }

    /// Finds the move or adds it. `None` when the text is unusable or the
    /// position is full; otherwise `(index, added)`.
    fn ensure_move(
        &mut self,
        key: &MaskKey,
        matcher: &MoveMatcher<'_>,
        san: &str,
    ) -> Option<(usize, bool)> {
        let san = san.trim();
        if san.is_empty() || san.len() > MAX_FIELD_BYTES {
            return None;
        }
        if let Some(entry) = self.get(key)
            && let Some(i) = find_move(entry, matcher, san)
        {
            return Some((i, false));
        }
        let entry = self.entry(key)?;
        if entry.moves.len() >= MAX_MOVES_PER_POSITION {
            if entry.moves.is_empty() && entry.comment.is_empty() {
                self.remove(key);
            }
            return None;
        }
        entry.moves.push(MaskMove::new(san));
        Some((entry.moves.len() - 1, true))
    }

    fn edit_move(
        &mut self,
        key: &MaskKey,
        matcher: &MoveMatcher<'_>,
        san: &str,
        change: impl FnOnce(&mut MaskMove) -> bool,
    ) -> bool {
        let Some((i, added)) = self.ensure_move(key, matcher, san) else {
            return false;
        };
        let changed = self
            .get_mut(key)
            .and_then(|e| e.moves.get_mut(i))
            .is_some_and(change);
        added || changed
    }

    fn remove_move(&mut self, key: &MaskKey, matcher: &MoveMatcher<'_>, san: &str) -> bool {
        let Some(entry) = self.get_mut(key) else {
            return false;
        };
        let Some(i) = find_move(entry, matcher, san) else {
            return false;
        };
        entry.moves.remove(i);
        if entry.moves.is_empty() && entry.comment.is_empty() {
            self.remove(key);
        }
        true
    }

    fn set_position_comment(&mut self, key: &MaskKey, text: &str) -> bool {
        let (clean, _) = clean_comment(text);
        let Some(entry) = self.get_mut(key) else {
            if clean.is_empty() {
                return false;
            }
            let Some(entry) = self.entry(key) else {
                return false;
            };
            entry.comment = clean;
            return true;
        };
        if entry.comment == clean {
            return false;
        }
        entry.comment = clean;
        if entry.moves.is_empty() && entry.comment.is_empty() {
            self.remove(key);
        }
        true
    }
}

/// Sets `*slot = value`; true when it differed.
fn replace<T: PartialEq>(slot: &mut T, value: T) -> bool {
    if *slot == value {
        false
    } else {
        *slot = value;
        true
    }
}

/// The index of the move of `entry` that `san` names.
pub(crate) fn find_move(
    entry: &MaskPosition,
    matcher: &MoveMatcher<'_>,
    san: &str,
) -> Option<usize> {
    entry.moves.iter().position(|m| matcher.same(&m.san, san))
}

#[cfg(test)]
mod tests {
    use super::*;
    use mychess_core::SanMode;

    fn add(san: &str) -> MaskOp {
        MaskOp::AddMove { san: san.into() }
    }

    #[test]
    fn add_creates_position_and_is_idempotent() {
        let pos = Position::new();
        let mut mask = TreeMask::new();
        assert!(mask.apply(&pos, &add("e4")));
        assert!(!mask.apply(&pos, &add("e4")));
        assert!(!mask.apply(&pos, &add("e4+")));
        assert!(mask.apply(&pos, &add("d4")));
        let e = mask.position(&pos).unwrap();
        assert_eq!(e.moves.len(), 2);
        assert_eq!(e.moves[0], MaskMove::new("e4"));
        assert!(!mask.apply(&pos, &add("")));
    }

    #[test]
    fn edits_add_the_move_first_and_ignore_no_ops() {
        let pos = Position::new();
        let mut mask = TreeMask::new();
        let nag = MaskOp::SetNag {
            san: "e4".into(),
            nag: Some(MaskNag::Good),
        };
        assert!(mask.apply(&pos, &nag));
        assert!(!mask.apply(&pos, &nag));
        // The default color on an existing move changes nothing.
        let white = MaskOp::SetColor {
            san: "e4".into(),
            color: MaskColor::White,
        };
        assert!(!mask.apply(&pos, &white));
        // Setting a value on a missing move adds it, even with a default value.
        assert!(mask.apply(
            &pos,
            &MaskOp::SetColor {
                san: "d4".into(),
                color: MaskColor::White
            }
        ));
        let marker = |slot, marker| MaskOp::SetMarker {
            san: "e4".into(),
            slot,
            marker,
        };
        assert!(mask.apply(&pos, &marker(MarkerSlot::Second, Some(Marker::Include))));
        assert!(!mask.apply(&pos, &marker(MarkerSlot::Second, Some(Marker::Include))));
        assert!(mask.apply(&pos, &marker(MarkerSlot::Second, None)));
        let comment = |t: &str| MaskOp::SetMoveComment {
            san: "e4".into(),
            text: t.into(),
        };
        assert!(mask.apply(&pos, &comment("  main \r\n move ")));
        assert!(!mask.apply(&pos, &comment("main \n move")));
        let e = mask.position(&pos).unwrap();
        assert_eq!(e.moves[0].comment, "main \n move");
        assert_eq!(e.moves[0].nag, Some(MaskNag::Good));
    }

    #[test]
    fn removal_drops_empty_positions_but_keeps_commented_ones() {
        let pos = Position::new();
        let mut mask = TreeMask::new();
        mask.apply(&pos, &add("e4"));
        assert!(!mask.apply(&pos, &MaskOp::RemoveMove { san: "d4".into() }));
        assert!(mask.apply(&pos, &MaskOp::RemoveMove { san: "e4".into() }));
        assert!(mask.is_empty());
        assert!(!mask.apply(&pos, &MaskOp::RemoveMove { san: "e4".into() }));

        let set = |t: &str| MaskOp::SetPositionComment { text: t.into() };
        assert!(!mask.apply(&pos, &set("  ")));
        assert!(mask.apply(&pos, &set("hello")));
        mask.apply(&pos, &add("e4"));
        mask.apply(&pos, &MaskOp::RemoveMove { san: "e4".into() });
        assert_eq!(mask.len(), 1, "comment keeps the position");
        assert!(mask.apply(&pos, &set("")));
        assert!(mask.is_empty(), "empty comment without moves removes it");
    }

    #[test]
    fn equivalent_texts_are_the_same_entry() {
        let mut pos = Position::new();
        for m in ["e4", "e5", "Qh5", "Nc6", "Bc4", "Nf6"] {
            let mv = pos.parse_san(m, SanMode::Strict).unwrap().mv;
            pos.play(mv).unwrap();
        }
        let mut mask = TreeMask::new();
        assert!(mask.apply(&pos, &add("Qf7")));
        assert!(!mask.apply(&pos, &add("Qxf7+")));
        assert!(mask.lookup_move(&pos, "Qxf7#").is_some());
        assert!(mask.lookup_move(&pos, "Qxg7").is_none());
    }

    #[test]
    fn position_is_full_at_500_moves() {
        let pos = Position::new();
        let mut mask = TreeMask::new();
        for i in 0..MAX_MOVES_PER_POSITION {
            assert!(mask.apply(&pos, &add(&format!("m{i}"))));
        }
        assert!(!mask.apply(&pos, &add("one-more")));
    }

    #[test]
    fn overlay_matches_rows_and_lists_the_rest() {
        let pos = Position::new();
        let mut mask = TreeMask::new();
        for s in ["e4", "null", "Nf3", "Qd4"] {
            mask.apply(&pos, &add(s));
        }
        mask.apply(&pos, &MaskOp::SetPositionComment { text: "hi".into() });
        let o = mask.overlay(&pos, &["d4", "Nf3", "e4"]);
        assert_eq!(o.comment, "hi");
        assert_eq!(o.rows[0], None);
        assert_eq!(o.rows[1].map(|m| m.san.as_str()), Some("Nf3"));
        assert_eq!(o.rows[2].map(|m| m.san.as_str()), Some("e4"));
        assert_eq!(o.only_mask.len(), 1);
        assert_eq!(o.only_mask[0].entry.san, "Qd4");
        assert!(o.only_mask[0].legal.is_none(), "illegal entry flagged");
        let blank = TreeMask::new();
        let empty = blank.overlay(&pos, &["e4"]);
        assert_eq!(empty.rows, vec![None]);
    }
}
