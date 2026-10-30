//! The PNG of a position (spec M7-4, rule 27, D3723): the SVG of
//! [`board_svg`](super::board_svg) rasterized by `resvg`, with the font given
//! by the host and no system fonts.

use std::sync::Arc;

use resvg::{tiny_skia, usvg};

use super::svg::svg_document;
use super::{BoardImage, ImageError, MAX_MARGIN_PX, SIZE_RANGE};

/// Renders `img` to PNG bytes (RGBA, 8 bits). With `transparent` the squares
/// and the margin are not painted, so the empty squares have alpha 0.
///
/// `font` is a TrueType or OpenType file for the coordinates; it is only
/// needed (and checked) when the image has coordinates.
///
/// # Errors
///
/// [`ImageError::SizeOutOfRange`] / [`ImageError::MarginTooLarge`] for the
/// sizes, [`ImageError::InvalidFont`] when the coordinates need a font and
/// `font` is not one, [`ImageError::Render`] / [`ImageError::Encode`] when
/// `resvg` or the encoder refuse.
pub fn board_png(img: &BoardImage, transparent: bool, font: &[u8]) -> Result<Vec<u8>, ImageError> {
    if !SIZE_RANGE.contains(&img.size_px) {
        return Err(ImageError::SizeOutOfRange(img.size_px));
    }
    if img.margin_px > MAX_MARGIN_PX {
        return Err(ImageError::MarginTooLarge(img.margin_px));
    }
    let mut options = usvg::Options::default();
    let mut fonts = usvg::fontdb::Database::new();
    if img.coordinates
        || img
            .marks
            .iter()
            .any(|m| matches!(m.kind, mychess_core::MarkKind::Text(_)))
    {
        fonts.load_font_data(font.to_vec());
        let family = fonts
            .faces()
            .next()
            .and_then(|face| face.families.first().map(|(name, _)| name.clone()))
            .ok_or(ImageError::InvalidFont)?;
        fonts.set_sans_serif_family(family.clone());
        fonts.set_serif_family(family);
    }
    options.fontdb = Arc::new(fonts);
    let svg = svg_document(img, !transparent);
    let tree =
        usvg::Tree::from_str(&svg, &options).map_err(|e| ImageError::Render(e.to_string()))?;
    let side = img.size_px + 2 * img.margin_px;
    let mut pixmap = tiny_skia::Pixmap::new(side, side)
        .ok_or_else(|| ImageError::Render("empty canvas".to_owned()))?;
    resvg::render(
        &tree,
        tiny_skia::Transform::identity(),
        &mut pixmap.as_mut(),
    );
    pixmap
        .encode_png()
        .map_err(|e| ImageError::Encode(e.to_string()))
}
