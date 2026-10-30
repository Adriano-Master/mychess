//! What a point of the notation panel points at (spec M2-4, rules 14-16).

use mychess_app::{CommentTarget, Inline};
use mychess_core::NodeId;

/// A clickable item of the document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hit {
    /// A move.
    Move(NodeId),
    /// The annotation symbol of the move of this node.
    Nag(NodeId),
    /// A comment.
    Comment(CommentTarget),
    /// The header of the variation card whose first move is this node:
    /// a click folds or unfolds the card (spec M2-5, D248).
    Card(NodeId),
}

impl Hit {
    /// The item an inline stands for; numbers, parentheses and blanks are
    /// not clickable (rule 9).
    pub fn of(inline: &Inline) -> Option<Hit> {
        match inline {
            Inline::Move { node, .. } => Some(Hit::Move(*node)),
            Inline::Nag { node, .. } => Some(Hit::Nag(*node)),
            Inline::Comment { target, .. } => Some(Hit::Comment(*target)),
            _ => None,
        }
    }

    /// The node the item belongs to: a click, a hover preview and a menu act
    /// on its position. The initial comment belongs to no move.
    pub fn node(self) -> Option<NodeId> {
        match self {
            Hit::Move(node)
            | Hit::Nag(node)
            | Hit::Card(node)
            | Hit::Comment(CommentTarget::After(node) | CommentTarget::Before(node)) => Some(node),
            Hit::Comment(CommentTarget::Initial) => None,
        }
    }
}

/// The index of the row that contains the vertical coordinate `y`, for rows
/// sorted by their top (`bounds` gives `(top, height)`).
pub fn row_at<T>(rows: &[T], y: f32, bounds: impl Fn(&T) -> (f32, f32)) -> Option<usize> {
    if !y.is_finite() {
        return None;
    }
    let after = rows.partition_point(|row| bounds(row).0 <= y);
    let index = after.checked_sub(1)?;
    let (top, height) = bounds(rows.get(index)?);
    (y < top + height).then_some(index)
}

#[cfg(test)]
mod tests {
    use mychess_core::{Game, Nag};

    use super::*;

    fn node() -> NodeId {
        Game::new().root()
    }

    #[test]
    fn only_moves_nags_and_comments_are_clickable() {
        let n = node();
        let mv = Inline::Move {
            node: n,
            san: "e4".into(),
            depth: 0,
            in_variation: false,
        };
        assert_eq!(Hit::of(&mv), Some(Hit::Move(n)));
        let nag = Inline::Nag {
            node: n,
            nag: Nag::new(1).expect("nag"),
        };
        assert_eq!(Hit::of(&nag), Some(Hit::Nag(n)));
        let comment = Inline::Comment {
            target: CommentTarget::After(n),
            text: "x".into(),
        };
        assert_eq!(
            Hit::of(&comment),
            Some(Hit::Comment(CommentTarget::After(n)))
        );
        for inline in [
            Inline::Number("1.".into()),
            Inline::Open,
            Inline::Close,
            Inline::Space,
            Inline::Pending { after: n },
        ] {
            assert_eq!(Hit::of(&inline), None);
        }
    }

    #[test]
    fn hits_name_the_node_they_act_on() {
        let n = node();
        assert_eq!(Hit::Move(n).node(), Some(n));
        assert_eq!(Hit::Nag(n).node(), Some(n));
        assert_eq!(Hit::Comment(CommentTarget::Before(n)).node(), Some(n));
        assert_eq!(Hit::Comment(CommentTarget::Initial).node(), None);
        // A card header acts on the first move of the variation.
        assert_eq!(Hit::Card(n).node(), Some(n));
    }

    #[test]
    fn rows_are_found_by_binary_search_and_gaps_miss() {
        let rows = [(0.0_f32, 10.0_f32), (12.0, 10.0), (30.0, 5.0)];
        let find = |y| row_at(&rows, y, |r| *r);
        assert_eq!(find(0.0), Some(0));
        assert_eq!(find(9.9), Some(0));
        assert_eq!(find(10.0), None, "the gap between rows");
        assert_eq!(find(12.0), Some(1));
        assert_eq!(find(34.9), Some(2));
        assert_eq!(find(35.0), None);
        assert_eq!(find(-1.0), None);
        assert_eq!(find(f32::NAN), None);
        assert_eq!(row_at::<(f32, f32)>(&[], 1.0, |r| *r), None);
    }
}
