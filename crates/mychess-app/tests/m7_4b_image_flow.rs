//! M7-4b contract: the dialog of the board image - preview by request, save
//! with a file dialog, the question about an existing file, the position
//! shown (spec `docs/specs/m7-4-exportacao.md`, section 8, rules 27-29,
//! D3724, D3728, criteria 13 and 17).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::assert_is_empty)]

mod m7_4b_common;

use m7_4b_common::*;
use mychess_app::{ImageFailure, ImageStepView};
use tempfile::tempdir;

fn png_size(bytes: &[u8]) -> (u32, u32) {
    let pixmap = resvg::tiny_skia::Pixmap::decode_png(bytes).expect("a PNG");
    (pixmap.width(), pixmap.height())
}

fn render_jobs(effects: &[Effect]) -> Vec<(mychess_app::RequestId, mychess_app::ImageJob)> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::RenderImage { request, job } => Some((*request, (**job).clone())),
            _ => None,
        })
        .collect()
}

/// Runs the render effect and returns the answer message.
fn run(rig: &Rig, request: mychess_app::RequestId, job: mychess_app::ImageJob) -> Message {
    rig.perform(Effect::RenderImage {
        request,
        job: Box::new(job),
    })
}

fn pick_spec(effects: &[Effect]) -> mychess_app::PickSpec {
    effects
        .iter()
        .find_map(|e| match e {
            Effect::PickFile(spec) => Some(spec.clone()),
            _ => None,
        })
        .expect("a file dialog")
}

#[test]
fn without_piece_drawings_the_window_does_not_open_and_says_why() {
    let mut rig = Rig::new();
    let effects = rig.raw(ExportDocsMessage::OpenImage);
    assert!(effects.is_empty());
    assert!(rig.state.export_image_view().is_none());
    assert_eq!(
        docs_notice(&rig),
        Some(GamesExportNotice::Problem(ExportProblem::AssetsMissing))
    );
}

#[test]
fn opening_asks_for_a_preview_of_the_position_shown_and_the_answer_fills_it() {
    let mut rig = Rig::new();
    rig.set_assets();
    rig.state.board_flipped = true;
    let effects = rig.raw(ExportDocsMessage::OpenImage);
    let jobs = render_jobs(&effects);
    assert_eq!(jobs.len(), 1);
    let (request, job) = jobs.into_iter().next().unwrap();
    assert!(job.dest.is_none(), "a preview writes no file");
    assert!(job.flip, "follows the board");
    assert_eq!(job.position, rig.state.session.position().clone());
    assert!(rig.state.modal_open());
    assert!(rig.state.export_image_view().unwrap().preview.is_none());
    let answer = run(&rig, request, job);
    rig.raw(answer);
    let view = rig.state.export_image_view().unwrap();
    // The preview is never larger than 512 pixels.
    assert_eq!(png_size(view.preview.unwrap()), (512, 512));
    assert!(view.preview_error.is_none());
}

#[test]
fn an_older_preview_is_dropped_and_the_newest_one_stays() {
    let mut rig = Rig::new();
    rig.set_assets();
    let first = render_jobs(&rig.raw(ExportDocsMessage::OpenImage));
    let (req_a, job_a) = first.into_iter().next().unwrap();
    // Changing an option asks for a new preview and cancels the old one.
    let second = render_jobs(&rig.raw(ExportDocsMessage::Edit(ExportEdit::ImageSize(128))));
    let (req_b, job_b) = second.into_iter().next().unwrap();
    assert_ne!(req_a, req_b);
    // The newest answer arrives first, the old one afterwards.
    let b = run(&rig, req_b, job_b);
    rig.raw(b);
    let a = run(&rig, req_a, job_a);
    rig.raw(a);
    let view = rig.state.export_image_view().unwrap();
    assert_eq!(
        png_size(view.preview.unwrap()),
        (128, 128),
        "the old 512 preview was dropped"
    );
}

#[test]
fn a_failed_preview_shows_the_error_and_keeps_no_stale_image() {
    let mut rig = Rig::new();
    let mut broken = assets();
    broken.font = std::sync::Arc::new(b"not a font".to_vec());
    rig.state.set_export_assets(Some(broken));
    let (request, job) = render_jobs(&rig.raw(ExportDocsMessage::OpenImage))
        .into_iter()
        .next()
        .unwrap();
    let answer = run(&rig, request, job);
    rig.raw(answer);
    let view = rig.state.export_image_view().unwrap();
    assert!(view.preview.is_none());
    assert!(matches!(
        view.preview_error,
        Some(ImageFailure::Image(ImageError::InvalidFont))
    ));
}

#[test]
fn options_are_remembered_and_the_flip_belongs_to_the_window() {
    let mut rig = Rig::new();
    rig.set_assets();
    rig.raw(ExportDocsMessage::OpenImage);
    rig.raw(ExportDocsMessage::Edit(ExportEdit::ImageFormat(
        ImageFormat::Svg,
    )));
    rig.raw(ExportDocsMessage::Edit(ExportEdit::ImageCoordinates(false)));
    rig.raw(ExportDocsMessage::Edit(ExportEdit::ImageSize(10_000)));
    rig.raw(ExportDocsMessage::Edit(ExportEdit::ImageFlip(true)));
    let prefs = &rig.state.prefs.export_docs.image;
    assert_eq!(prefs.format, ImageFormat::Svg);
    assert!(!prefs.coordinates);
    assert_eq!(prefs.size_px, 4096, "limited to the range");
    let view = rig.state.export_image_view().unwrap();
    assert!(view.flip);
    assert!(!rig.state.board_flipped, "the board itself is not touched");
}

#[test]
fn saving_a_png_writes_the_decodable_file_and_closes_the_window() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    rig.set_assets();
    rig.raw(ExportDocsMessage::OpenImage);
    rig.raw(ExportDocsMessage::Edit(ExportEdit::ImageSize(256)));
    let effects = rig.raw(ExportDocsMessage::ImageSave);
    let spec = pick_spec(&effects);
    assert_eq!(
        spec.kind,
        PickKind::ExportImage {
            format: ImageFormat::Png
        }
    );
    assert_eq!(spec.file_name.as_deref(), Some("board.png"));
    assert!(matches!(
        rig.state.export_image_view().unwrap().step,
        ImageStepView::Picking
    ));
    let dest = dir.path().join("pic");
    let effects = rig.raw(Message::FilePicked(spec.request, Some(dest.clone())));
    let (request, job) = render_jobs(&effects).into_iter().next().unwrap();
    assert_eq!(
        job.dest.as_deref(),
        Some(dir.path().join("pic.png").as_path())
    );
    assert!(
        !dir.path().join("pic.png").exists(),
        "update writes nothing"
    );
    let answer = run(&rig, request, job);
    rig.raw(answer);
    let bytes = std::fs::read(dir.path().join("pic.png")).unwrap();
    assert_eq!(png_size(&bytes), (256, 256));
    assert!(rig.state.export_image_view().is_none());
    assert_eq!(
        docs_notice(&rig),
        Some(GamesExportNotice::ImageSaved {
            name: "pic.png".to_owned()
        })
    );
    assert!(!has_temp(dir.path()));
}

#[test]
fn saving_an_svg_writes_the_document() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    rig.set_assets();
    rig.raw(ExportDocsMessage::OpenImage);
    rig.raw(ExportDocsMessage::Edit(ExportEdit::ImageFormat(
        ImageFormat::Svg,
    )));
    let spec = pick_spec(&rig.raw(ExportDocsMessage::ImageSave));
    assert_eq!(
        spec.kind,
        PickKind::ExportImage {
            format: ImageFormat::Svg
        }
    );
    let effects = rig.raw(Message::FilePicked(
        spec.request,
        Some(dir.path().join("Pic.SVG")),
    ));
    let (request, job) = render_jobs(&effects).into_iter().next().unwrap();
    let answer = run(&rig, request, job);
    rig.raw(answer);
    let text = text_of(&dir.path().join("Pic.svg"));
    assert!(text.starts_with("<svg xmlns="));
    assert_eq!(text.matches("class=\"sq\"").count(), 64);
}

#[test]
fn a_name_completed_by_the_extension_that_exists_is_asked_about() {
    let dir = tempdir().unwrap();
    let existing = dir.path().join("pic.png");
    std::fs::write(&existing, "precious").unwrap();
    let mut rig = Rig::new();
    rig.set_assets();
    rig.raw(ExportDocsMessage::OpenImage);
    let spec = pick_spec(&rig.raw(ExportDocsMessage::ImageSave));
    let effects = rig.raw(Message::FilePicked(
        spec.request,
        Some(dir.path().join("pic")),
    ));
    let (request, job) = render_jobs(&effects).into_iter().next().unwrap();
    let answer = run(&rig, request, job);
    rig.raw(answer);
    assert_eq!(std::fs::read_to_string(&existing).unwrap(), "precious");
    assert!(matches!(
        rig.state.export_image_view().unwrap().step,
        ImageStepView::Confirm(path) if path == existing
    ));
    // No: back to the options, nothing replaced.
    let effects = rig.raw(ExportDocsMessage::Confirm(false));
    assert!(effects.is_empty());
    assert_eq!(std::fs::read_to_string(&existing).unwrap(), "precious");
    assert!(matches!(
        rig.state.export_image_view().unwrap().step,
        ImageStepView::Options
    ));
    // Yes, the second time.
    let spec = pick_spec(&rig.raw(ExportDocsMessage::ImageSave));
    let effects = rig.raw(Message::FilePicked(
        spec.request,
        Some(dir.path().join("pic")),
    ));
    let (request, job) = render_jobs(&effects).into_iter().next().unwrap();
    let answer = run(&rig, request, job);
    rig.raw(answer);
    let effects = rig.raw(ExportDocsMessage::Confirm(true));
    let (request, job) = render_jobs(&effects).into_iter().next().unwrap();
    assert!(job.confirmed);
    let answer = run(&rig, request, job);
    rig.raw(answer);
    assert_eq!(png_size(&std::fs::read(&existing).unwrap()), (512, 512));
}

#[test]
fn a_file_the_dialog_confirmed_is_replaced_at_once() {
    let dir = tempdir().unwrap();
    let existing = dir.path().join("same.png");
    std::fs::write(&existing, "old").unwrap();
    let mut rig = Rig::new();
    rig.set_assets();
    rig.raw(ExportDocsMessage::OpenImage);
    let spec = pick_spec(&rig.raw(ExportDocsMessage::ImageSave));
    let effects = rig.raw(Message::FilePicked(spec.request, Some(existing.clone())));
    let (request, job) = render_jobs(&effects).into_iter().next().unwrap();
    assert!(job.confirmed);
    let answer = run(&rig, request, job);
    rig.raw(answer);
    assert_eq!(png_size(&std::fs::read(&existing).unwrap()), (512, 512));
}

#[test]
fn an_open_database_file_is_never_overwritten_by_an_image() {
    let dir = tempdir().unwrap();
    let base = write(dir.path(), "shot.png", &numbered(2));
    let mut rig = Rig::new();
    rig.set_assets();
    rig.send(BasesMessage::Open(base.clone()));
    assert_eq!(
        rig.state.bases.current_slot().and_then(|s| s.path.clone()),
        Some(base.clone()),
        "the premise: the PNG-named file is open as a database"
    );
    let before = std::fs::read(&base).unwrap();
    rig.raw(ExportDocsMessage::OpenImage);
    let spec = pick_spec(&rig.raw(ExportDocsMessage::ImageSave));
    let effects = rig.raw(Message::FilePicked(spec.request, Some(base.clone())));
    let (request, job) = render_jobs(&effects).into_iter().next().unwrap();
    let answer = run(&rig, request, job);
    rig.raw(answer);
    assert_eq!(std::fs::read(&base).unwrap(), before);
    assert_eq!(
        docs_notice(&rig),
        Some(GamesExportNotice::ImageFailed(ImageFailure::Protected))
    );
}

#[test]
fn cancelling_the_file_dialog_goes_back_to_the_options_and_a_stale_answer_is_ignored() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    rig.set_assets();
    rig.raw(ExportDocsMessage::OpenImage);
    let spec = pick_spec(&rig.raw(ExportDocsMessage::ImageSave));
    rig.raw(Message::FilePicked(spec.request, None));
    assert!(matches!(
        rig.state.export_image_view().unwrap().step,
        ImageStepView::Options
    ));
    // The same answer again, now stale, starts nothing.
    let effects = rig.raw(Message::FilePicked(
        spec.request,
        Some(dir.path().join("late.png")),
    ));
    assert!(render_jobs(&effects).is_empty());
    // Closing the window drops everything.
    rig.raw(ExportDocsMessage::ImageClose);
    assert!(rig.state.export_image_view().is_none());
    assert!(rig.raw(ExportDocsMessage::ImageSave).is_empty());
}

#[test]
fn a_rendering_that_fails_leaves_no_file_and_reports_the_cause() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    let mut broken = assets();
    broken.font = std::sync::Arc::new(Vec::new());
    rig.state.set_export_assets(Some(broken));
    rig.raw(ExportDocsMessage::OpenImage);
    let spec = pick_spec(&rig.raw(ExportDocsMessage::ImageSave));
    let effects = rig.raw(Message::FilePicked(
        spec.request,
        Some(dir.path().join("bad.png")),
    ));
    let (request, job) = render_jobs(&effects).into_iter().next().unwrap();
    let answer = run(&rig, request, job);
    rig.raw(answer);
    assert!(!dir.path().join("bad.png").exists());
    assert!(!has_temp(dir.path()));
    assert_eq!(
        docs_notice(&rig),
        Some(GamesExportNotice::ImageFailed(ImageFailure::Image(
            ImageError::InvalidFont
        )))
    );
    // The window stays so the user can change the options.
    assert!(rig.state.export_image_view().is_some());
}
