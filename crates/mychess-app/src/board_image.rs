//! The image of a board position: an SVG built as text and a PNG
//! rasterized from it (spec M7-4, D3723-D3727, ADR 0016).
//!
//! The application layer knows no toolkit: the piece drawings and the font
//! come from the host ([`PieceSvgs`], the font bytes), the module only
//! assembles text. The same assembler draws the diagrams of the HTML export
//! ([`BoardDiagrams`], a `DiagramSource`) and the board image of the dialog.
//!
//! Everything is deterministic: the same input gives the same bytes.

mod diagrams;
mod namespace;
mod png;
mod svg;

pub use diagrams::BoardDiagrams;
pub use namespace::namespace_svg_ids;
pub use png::board_png;
pub use svg::{BoardImage, BoardPalette, PieceSvgs, board_svg, piece_symbols};

/// Smallest and largest side of the board of an image, in pixels (D3725).
pub const SIZE_RANGE: std::ops::RangeInclusive<u32> = 64..=4096;

/// Largest margin around the board, in pixels.
pub const MAX_MARGIN_PX: u32 = 512;

/// Why an image could not be made.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ImageError {
    /// The side of the board is outside [`SIZE_RANGE`].
    #[error("the board size {0} is outside 64..=4096 pixels")]
    SizeOutOfRange(u32),
    /// The margin is larger than [`MAX_MARGIN_PX`].
    #[error("the margin {0} is larger than 512 pixels")]
    MarginTooLarge(u32),
    /// A piece drawing is missing or is not a usable SVG.
    #[error("the drawing of the piece {piece} is missing or not a usable SVG")]
    BadPiece {
        /// The piece, as `wK` or `bQ`.
        piece: &'static str,
    },
    /// The font bytes are not a font, or no font was given for the
    /// coordinates.
    #[error("the font for the coordinates is missing or invalid")]
    InvalidFont,
    /// The SVG could not be parsed or drawn.
    #[error("the SVG could not be rendered: {0}")]
    Render(String),
    /// The PNG could not be encoded.
    #[error("the PNG could not be encoded: {0}")]
    Encode(String),
}

impl ImageError {
    /// The Fluent key of the message.
    pub fn message_key(&self) -> &'static str {
        match self {
            ImageError::SizeOutOfRange(_) | ImageError::MarginTooLarge(_) => {
                "export-err-image-size"
            }
            ImageError::BadPiece { .. } => "export-err-image-piece",
            ImageError::InvalidFont => "export-err-image-font",
            ImageError::Render(_) | ImageError::Encode(_) => "export-err-image-render",
        }
    }
}
