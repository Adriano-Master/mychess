// Derived from Scid vs PC 4.27, tcl/windows/tree.tcl
// (`::tree::mask::performSearch` 2896-2960). Copyright (c) 1999-2002 Shane
// Hudson; Scid vs PC contributors. Scid is distributed under the GNU General
// Public License (see its COPYING); this file is part of MyChess,
// GPL-3.0-or-later (docs/adr/0004-licenca.md).

//! Searching a mask (decision D1832): moves and positions by NAG, markers,
//! color and comment text.

use super::model::{MAX_PREVIEW_BYTES, Marker, MaskColor, MaskKey, MaskNag, TreeMask, first_line};

/// The most rows a search returns (D1832).
pub const MAX_HITS: usize = 10_000;

/// The filters of a search. A filter that is `None` is off; all the filters
/// that are on must match a move for it to be listed.
// The outer `Option` is the filter being on or off; the inner one is the
// value looked for, where `None` means "no NAG" or "no marker".
#[allow(clippy::option_option)]
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MaskQuery {
    /// The NAG; `Some(None)` looks for moves with no NAG.
    pub nag: Option<Option<MaskNag>>,
    /// The first marker; `Some(None)` looks for moves with none.
    pub marker1: Option<Option<Marker>>,
    /// The second marker.
    pub marker2: Option<Option<Marker>>,
    /// The color.
    pub color: Option<MaskColor>,
    /// Text of the comment of the move (a literal substring, any case).
    pub move_text: Option<String>,
    /// Text of the comment of the position: positions that do not match are
    /// left out, and the ones that do get a row of their own.
    pub position_text: Option<String>,
}

/// A row of the result: a move of a position, or the position itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaskHit {
    /// The position.
    pub key: MaskKey,
    /// The move; `None` for the row of a position comment.
    pub san: Option<String>,
    /// The first line of the comment of the move, or of the position for its
    /// own row, at most [`MAX_PREVIEW_BYTES`] bytes (D1851).
    pub comment: String,
}

/// The result of [`run`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MaskHits {
    /// The rows, in file order (at most [`MAX_HITS`]).
    pub rows: Vec<MaskHit>,
    /// Positions that matched the position comment filter.
    pub positions_found: usize,
    /// Positions looked at.
    pub positions_total: usize,
    /// Moves listed.
    pub moves_found: usize,
    /// Moves looked at (those of the positions that passed the position
    /// filter are the ones that count in the Scid; here, all but `null`).
    pub moves_total: usize,
    /// Whether rows were left out because of the limit.
    pub truncated: bool,
}

/// Case-insensitive literal substring test (the empty text matches all).
fn contains_ci(haystack: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return true;
    }
    if haystack.is_ascii() && needle.is_ascii() {
        return haystack
            .as_bytes()
            .windows(needle.len())
            .any(|w| w.eq_ignore_ascii_case(needle.as_bytes()));
    }
    haystack.to_lowercase().contains(&needle.to_lowercase())
}

/// Runs the search over the whole mask. `null` moves never appear.
pub fn run(mask: &TreeMask, query: &MaskQuery) -> MaskHits {
    let mut hits = MaskHits::default();
    let push = |hits: &mut MaskHits, row: MaskHit| {
        if hits.rows.len() < MAX_HITS {
            hits.rows.push(row);
        } else {
            hits.truncated = true;
        }
    };
    for (key, position) in mask.iter() {
        hits.positions_total += 1;
        if let Some(text) = &query.position_text {
            if !contains_ci(&position.comment, text) {
                continue;
            }
            hits.positions_found += 1;
            push(
                &mut hits,
                MaskHit {
                    key: key.clone(),
                    san: None,
                    comment: first_line(&position.comment, MAX_PREVIEW_BYTES),
                },
            );
        }
        for m in position.visible_moves() {
            hits.moves_total += 1;
            if query.nag.as_ref().is_some_and(|n| *n != m.nag)
                || query.marker1.as_ref().is_some_and(|k| *k != m.markers[0])
                || query.marker2.as_ref().is_some_and(|k| *k != m.markers[1])
                || query.color.as_ref().is_some_and(|c| *c != m.color)
                || query
                    .move_text
                    .as_ref()
                    .is_some_and(|t| !contains_ci(&m.comment, t))
            {
                continue;
            }
            hits.moves_found += 1;
            push(
                &mut hits,
                MaskHit {
                    key: key.clone(),
                    san: Some(m.san.clone()),
                    comment: first_line(&m.comment, MAX_PREVIEW_BYTES),
                },
            );
        }
    }
    hits
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::mask::{MarkerSlot, MaskOp};
    use mychess_core::Position;

    fn sample() -> TreeMask {
        let mut m = TreeMask::new();
        let p0 = Position::new();
        let mut p1 = p0.clone();
        let mv = p1.parse_uci("e2e4").unwrap();
        p1.play(mv).unwrap();
        for (p, san, nag, color, text) in [
            (
                &p0,
                "e4",
                Some(MaskNag::Good),
                MaskColor::Green,
                "Main [x] move",
            ),
            (&p0, "d4", None, MaskColor::White, ""),
            (&p0, "null", None, MaskColor::White, "hidden"),
            (&p1, "c5", Some(MaskNag::Good), MaskColor::Red, "Sicilian"),
        ] {
            m.apply(
                p,
                &MaskOp::SetNag {
                    san: san.into(),
                    nag,
                },
            );
            m.apply(
                p,
                &MaskOp::SetColor {
                    san: san.into(),
                    color,
                },
            );
            m.apply(
                p,
                &MaskOp::SetMoveComment {
                    san: san.into(),
                    text: text.into(),
                },
            );
        }
        m.apply(
            &p0,
            &MaskOp::SetMarker {
                san: "d4".into(),
                slot: MarkerSlot::First,
                marker: Some(Marker::Include),
            },
        );
        m.apply(
            &p1,
            &MaskOp::SetPositionComment {
                text: "after e4".into(),
            },
        );
        m
    }

    fn sans(h: &MaskHits) -> Vec<(Option<&str>, &str)> {
        h.rows
            .iter()
            .map(|r| (r.san.as_deref(), r.comment.as_str()))
            .collect()
    }

    #[test]
    fn no_filter_lists_every_visible_move() {
        let h = run(&sample(), &MaskQuery::default());
        assert_eq!(h.moves_found, 3);
        assert_eq!((h.moves_total, h.positions_total), (3, 2));
        assert!(!h.truncated);
        assert!(h.rows.iter().all(|r| r.san.as_deref() != Some("null")));
    }

    #[test]
    fn each_filter_and_combinations() {
        let m = sample();
        let q = |f: fn(&mut MaskQuery)| {
            let mut q = MaskQuery::default();
            f(&mut q);
            run(&m, &q)
        };
        let h = q(|q| q.nag = Some(Some(MaskNag::Good)));
        assert_eq!(h.moves_found, 2, "a one-character NAG is found");
        let h = q(|q| q.nag = Some(None));
        assert_eq!(h.moves_found, 1);
        let h = q(|q| q.marker1 = Some(Some(Marker::Include)));
        assert_eq!(sans(&h), [(Some("d4"), "")]);
        let h = q(|q| q.marker2 = Some(None));
        assert_eq!(h.moves_found, 3);
        let h = q(|q| q.color = Some(MaskColor::Red));
        assert_eq!(sans(&h), [(Some("c5"), "Sicilian")]);
        let h = q(|q| q.move_text = Some("[X]".into()));
        assert_eq!(h.moves_found, 1, "literal, case-insensitive");
        let h = q(|q| q.move_text = Some("*".into()));
        assert_eq!(h.moves_found, 0, "no wildcards");
        let h = q(|q| {
            q.nag = Some(Some(MaskNag::Good));
            q.color = Some(MaskColor::Green);
        });
        assert_eq!(h.moves_found, 1);
    }

    #[test]
    fn position_filter_excludes_positions_and_adds_a_row() {
        let m = sample();
        let q = MaskQuery {
            position_text: Some("AFTER".into()),
            ..MaskQuery::default()
        };
        let h = run(&m, &q);
        assert_eq!((h.positions_found, h.positions_total), (1, 2));
        assert_eq!(sans(&h), [(None, "after e4"), (Some("c5"), "Sicilian")]);
        assert_eq!(h.moves_found, 1);
    }

    #[test]
    fn result_is_limited() {
        let mut m = TreeMask::new();
        for i in 0..(MAX_HITS / 400 + 2) {
            let key = MaskKey::Opaque(format!("k{i}"));
            let p = m.entry(&key).unwrap();
            for j in 0..400 {
                p.moves
                    .push(crate::tree::mask::MaskMove::new(format!("m{j}")));
            }
        }
        let h = run(&m, &MaskQuery::default());
        assert_eq!(h.rows.len(), MAX_HITS);
        assert!(h.truncated);
        assert_eq!(h.moves_found, (MAX_HITS / 400 + 2) * 400);
    }
}
