//! The blocking work of the board image (spec M7-4, section 8, rules 26-29).

use std::io::Write as _;
use std::sync::Arc;

use super::{ImageFailure, ImageFormat, ImageJob, ImageOutcome};
use crate::board_image::{BoardImage, board_png, board_svg};
use crate::error::FileError;
use crate::fsutil::write_atomic;
use crate::games_export::is_protected;

/// The longest side of a preview, in pixels.
const PREVIEW_SIDE: u32 = 512;

fn board_image(job: &ImageJob, size_px: u32, margin_px: u32) -> BoardImage {
    let mut img = BoardImage::new(job.position.clone(), job.assets.pieces.clone(), size_px);
    img.flip = job.flip;
    img.margin_px = margin_px;
    img.coordinates = job.options.coordinates;
    img.last_move = if job.options.last_move {
        job.last_move
    } else {
        None
    };
    img.marks = if job.options.marks {
        job.marks.clone()
    } else {
        Vec::new()
    };
    img.palette = job.assets.palette.clone();
    img
}

/// The PNG of the preview: at most [`PREVIEW_SIDE`] pixels, with the
/// options of the job.
pub(crate) fn preview_png(job: &ImageJob) -> Result<Arc<Vec<u8>>, ImageFailure> {
    let size = job.options.size_px.min(PREVIEW_SIDE);
    // The margin keeps its proportion to the board.
    let margin = job
        .options
        .margin_px
        .saturating_mul(size)
        .checked_div(job.options.size_px.max(1))
        .unwrap_or(0);
    let img = board_image(
        job,
        size.max(*crate::board_image::SIZE_RANGE.start()),
        margin,
    );
    board_png(&img, job.options.transparent, &job.assets.font)
        .map(Arc::new)
        .map_err(ImageFailure::Image)
}

/// Makes the image of the job: a preview, or a file written atomically.
pub(crate) fn image_run(job: &ImageJob) -> Result<ImageOutcome, ImageFailure> {
    let Some(dest) = job.dest.as_deref() else {
        return preview_png(job).map(ImageOutcome::Preview);
    };
    if is_protected(dest, &job.protected) {
        return Err(ImageFailure::Protected);
    }
    if !job.confirmed && std::fs::symlink_metadata(dest).is_ok() {
        return Ok(ImageOutcome::NeedsConfirm(dest.to_path_buf()));
    }
    let img = board_image(job, job.options.size_px, job.options.margin_px);
    let bytes = match job.options.format {
        ImageFormat::Png => board_png(&img, job.options.transparent, &job.assets.font)
            .map_err(ImageFailure::Image)?,
        // The SVG has no transparent option: it never paints a page.
        ImageFormat::Svg => board_svg(&img).into_bytes(),
    };
    write_atomic(dest, |file| file.write_all(&bytes)).map_err(|error| {
        ImageFailure::File(FileError::from_io(&error, Some(dest.to_path_buf())))
    })?;
    Ok(ImageOutcome::Saved(dest.to_path_buf()))
}
