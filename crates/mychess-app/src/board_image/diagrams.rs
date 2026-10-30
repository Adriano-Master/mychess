//! The diagrams of the HTML export (spec M7-4, D3702, rule 8).

use std::sync::Arc;

use mychess_core::Position;
use mychess_pgn::export::DiagramSource;

use super::svg::svg_document;
use super::{BoardImage, BoardPalette, PieceSvgs};

/// A [`DiagramSource`] that draws the position of a FEN as an inline SVG.
///
/// Every diagram has its own id prefix (`<salt>dg<n>-`), so any number of
/// them can share one page without their gradients clashing (D3726). The
/// `salt` makes the prefixes of an appended run differ from those already in
/// the file; an empty salt keeps the text deterministic.
#[derive(Debug, Clone)]
pub struct BoardDiagrams {
    pieces: Arc<PieceSvgs>,
    palette: BoardPalette,
    salt: String,
    drawn: u32,
}

impl BoardDiagrams {
    /// Diagrams with `pieces` and `palette`; `salt` is letters and digits.
    pub fn new(pieces: Arc<PieceSvgs>, palette: BoardPalette, salt: &str) -> BoardDiagrams {
        BoardDiagrams {
            pieces,
            palette,
            salt: salt.chars().filter(char::is_ascii_alphanumeric).collect(),
            drawn: 0,
        }
    }
}

impl DiagramSource for BoardDiagrams {
    fn svg(&mut self, fen: &str, flip: bool, px: u16) -> String {
        let Ok(position) = Position::from_fen(fen) else {
            return String::new();
        };
        self.drawn = self.drawn.saturating_add(1);
        let mut img = BoardImage::new(position, self.pieces.clone(), u32::from(px.max(1)));
        img.flip = flip;
        img.coordinates = false;
        img.palette = self.palette.clone();
        img.id_prefix = format!("{}dg{}-", self.salt, self.drawn);
        img.title = Some(fen.to_owned());
        svg_document(&img, true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board_image::svg::tests::fake_set;

    const START: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";

    #[test]
    fn two_diagrams_in_one_page_do_not_share_ids() {
        let mut source = BoardDiagrams::new(fake_set(), BoardPalette::default(), "");
        let one = source.svg(START, false, 200);
        let two = source.svg(START, true, 200);
        assert!(one.contains("<title>rnbqkbnr/"), "{one}");
        assert!(one.contains("width=\"200\""));
        let ids = |svg: &str| -> Vec<String> {
            svg.match_indices(" id=\"")
                .map(|(i, _)| svg[i + 5..].split('"').next().unwrap_or("").to_owned())
                .collect()
        };
        let (a, b) = (ids(&one), ids(&two));
        assert!(!a.is_empty() && !b.is_empty());
        assert!(a.iter().all(|id| !b.contains(id)), "{a:?} / {b:?}");
        assert!(a.iter().all(|id| id.starts_with("dg1-")));
    }

    #[test]
    fn a_bad_fen_draws_nothing_and_the_salt_changes_the_ids() {
        let mut source = BoardDiagrams::new(fake_set(), BoardPalette::default(), "x9");
        assert_eq!(source.svg("not a fen", false, 200), "");
        assert!(source.svg(START, false, 200).contains("id=\"x9dg1-wK\""));
    }
}
