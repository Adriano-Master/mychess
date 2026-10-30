//! The window "Export position as image" (spec M7-4c, decisions
//! M7-4/D4311-D4320).
//!
//! The application owns the position, the options and the file jobs
//! (`AppState::export_image_view`); this module draws the preview as a SVG
//! that the front-end builds itself (the application's own preview is a PNG
//! and `iced` is built without image codecs, ADR 0002), once per change of
//! the options, never per frame.

use std::sync::Arc;

use fluent_bundle::FluentArgs;
use iced::widget::{column, container, row, svg, text};
use iced::{Alignment, Element, Length};
use mychess_app::Message as AppMessage;
use mychess_app::board_image::{BoardImage, PieceSvgs, SIZE_RANGE, board_svg};
use mychess_app::export_docs::{
    ExportAssets, ExportDocsMessage, ExportEdit, ExportImageView, ImageFormat, ImagePrefs,
    ImageStepView,
};
use mychess_core::Position;

use crate::shell::{Message, Shell};
use crate::theme;
use crate::widgets::modal::{Card, modal};
use crate::windows::export_options::{docs, edit};
use crate::windows::panel_kit::{BODY_TEXT, action, check, choice, danger, muted};

/// Id of the card of the window.
pub const EXPORT_IMAGE_BOX_ID: &str = "export-image-box";
/// Id of the card of the question about an existing file.
pub const EXPORT_IMAGE_CONFIRM_ID: &str = "export-image-confirm-box";

/// Sizes of the board the window offers, in pixels.
pub const SIZES: [u32; 7] = [256, 384, 512, 768, 1024, 2048, 4096];
/// Margins the window offers, in pixels.
pub const MARGINS: [u32; 4] = [0, 16, 32, 64];
/// Side of the preview on the screen, in logical pixels.
const PREVIEW_SIDE: f32 = 320.0;
/// Nominal side of the SVG of the preview.
const PREVIEW_NOMINAL: u32 = 512;

/// What a preview was drawn from.
#[derive(Debug, Clone, PartialEq)]
struct PreviewKey {
    prefs: ImagePrefs,
    flip: bool,
    position: Position,
    last_move: Option<(mychess_core::Square, mychess_core::Square)>,
    marks: usize,
    pieces: Arc<PieceSvgs>,
}

/// What the window keeps on its own: the assets the host gave the
/// application and the SVG of the preview.
#[derive(Default)]
pub struct ExportImageUi {
    pub(crate) assets: Option<Arc<ExportAssets>>,
    pub(crate) assets_key: Option<AssetsKey>,
    preview: Option<(PreviewKey, svg::Handle)>,
}

impl std::fmt::Debug for ExportImageUi {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExportImageUi")
            .field("has_assets", &self.assets.is_some())
            .field("has_preview", &self.preview.is_some())
            .finish_non_exhaustive()
    }
}

/// What the assets given to the application were built from: the piece set
/// and the board theme.
pub type AssetsKey = (
    crate::widgets::board::pieces::PieceSetId,
    theme::board_theme::BoardThemeId,
);

impl ExportImageUi {
    /// Whether the SVG of the preview is up to date with `view`.
    pub fn preview_ready(&self) -> bool {
        self.preview.is_some()
    }

    /// Draws the preview again when what it shows changed.
    pub(crate) fn sync_preview(&mut self, view: Option<&ExportImageView<'_>>) {
        let (Some(view), Some(assets)) = (view, self.assets.clone()) else {
            self.preview = None;
            return;
        };
        let key = PreviewKey {
            prefs: view.prefs.clone(),
            flip: view.flip,
            position: view.position.clone(),
            last_move: view.last_move,
            marks: view.marks.len(),
            pieces: assets.pieces.clone(),
        };
        if self.preview.as_ref().is_some_and(|(k, _)| *k == key) {
            return;
        }
        let nominal = PREVIEW_NOMINAL.clamp(*SIZE_RANGE.start(), *SIZE_RANGE.end());
        let mut img = BoardImage::new(view.position.clone(), assets.pieces.clone(), nominal);
        img.flip = view.flip;
        // The margin keeps its proportion to the board.
        img.margin_px = view
            .prefs
            .margin_px
            .saturating_mul(nominal)
            .checked_div(view.prefs.size_px.max(1))
            .unwrap_or(0);
        img.coordinates = view.prefs.coordinates;
        img.last_move = if view.prefs.last_move {
            view.last_move
        } else {
            None
        };
        img.marks = if view.prefs.marks {
            view.marks.to_vec()
        } else {
            Vec::new()
        };
        img.palette = assets.palette.clone();
        let handle = svg::Handle::from_memory(board_svg(&img).into_bytes());
        self.preview = Some((key, handle));
    }
}

fn with_id(id: String, body: Element<'_, Message>) -> Element<'_, Message> {
    container(body).id(id).into()
}

fn labelled(label: String, body: Element<'_, Message>) -> Element<'_, Message> {
    row![
        text(label).size(BODY_TEXT).width(Length::Fixed(150.0)),
        body
    ]
    .spacing(8)
    .align_y(Alignment::Center)
    .into()
}

fn toggle<'a>(
    shell: &Shell,
    key: &str,
    id: &'static str,
    on: bool,
    enabled: bool,
    change: impl Fn(bool) -> ExportEdit + 'a,
) -> Element<'a, Message> {
    with_id(
        id.to_owned(),
        check(
            shell.i18n.tr(key),
            on,
            enabled.then_some(move |v| edit(change(v))),
        ),
    )
}

fn options<'a>(shell: &Shell, view: &ExportImageView<'_>, editable: bool) -> Element<'a, Message> {
    let i18n = &shell.i18n;
    let prefs = view.prefs;
    let on = |m: Message| editable.then_some(m);
    let formats = row![
        with_id(
            "export-image-png".to_owned(),
            choice(
                i18n.tr("export-image-format-png"),
                prefs.format == ImageFormat::Png,
                on(edit(ExportEdit::ImageFormat(ImageFormat::Png))),
            ),
        ),
        with_id(
            "export-image-svg".to_owned(),
            choice(
                i18n.tr("export-image-format-svg"),
                prefs.format == ImageFormat::Svg,
                on(edit(ExportEdit::ImageFormat(ImageFormat::Svg))),
            ),
        ),
    ]
    .spacing(4);
    let mut sizes = row![].spacing(4);
    for px in SIZES {
        sizes = sizes.push(with_id(
            format!("export-image-size-{px}"),
            choice(
                px.to_string(),
                prefs.size_px == px,
                on(edit(ExportEdit::ImageSize(px))),
            ),
        ));
    }
    let mut margins = row![].spacing(4);
    for px in MARGINS {
        let label = if px == 0 {
            i18n.tr("export-image-margin-none")
        } else {
            px.to_string()
        };
        margins = margins.push(with_id(
            format!("export-image-margin-{px}"),
            choice(
                label,
                prefs.margin_px == px,
                on(edit(ExportEdit::ImageMargin(px))),
            ),
        ));
    }
    column![
        labelled(i18n.tr("export-image-format"), formats.into()),
        labelled(i18n.tr("export-image-size"), sizes.wrap().into()),
        toggle(
            shell,
            "export-image-coordinates",
            "export-image-coordinates",
            prefs.coordinates,
            editable,
            ExportEdit::ImageCoordinates,
        ),
        toggle(
            shell,
            "export-image-flip",
            "export-image-flip",
            view.flip,
            editable,
            ExportEdit::ImageFlip,
        ),
        toggle(
            shell,
            "export-image-last-move",
            "export-image-last-move",
            prefs.last_move,
            editable,
            ExportEdit::ImageLastMove,
        ),
        toggle(
            shell,
            "export-image-marks",
            "export-image-marks",
            prefs.marks,
            editable,
            ExportEdit::ImageMarks,
        ),
        // The SVG never paints a page: the box only matters for the PNG.
        toggle(
            shell,
            "export-image-transparent",
            "export-image-transparent",
            prefs.transparent,
            editable && prefs.format == ImageFormat::Png,
            ExportEdit::ImageTransparent,
        ),
        labelled(i18n.tr("export-image-margin"), margins.wrap().into()),
    ]
    .spacing(8)
    .into()
}

fn preview<'a>(shell: &'a Shell, view: &ExportImageView<'_>) -> Element<'a, Message> {
    let i18n = &shell.i18n;
    let body: Element<'a, Message> =
        match (&shell.ui.dialogs.export_image.preview, view.preview_error) {
            (_, Some(failure)) => column![
                danger(i18n.tr("export-image-no-preview")),
                muted(i18n.tr(failure.message_key())),
            ]
            .spacing(4)
            .into(),
            (Some((_, handle)), None) => svg(handle.clone())
                .width(Length::Fixed(PREVIEW_SIDE))
                .height(Length::Fixed(PREVIEW_SIDE))
                .into(),
            (None, None) => muted(i18n.tr("export-image-preview")),
        };
    column![
        text(i18n.tr("export-image-preview")).size(BODY_TEXT),
        container(body)
            .id("export-image-preview")
            .width(Length::Fixed(PREVIEW_SIDE))
            .height(Length::Fixed(PREVIEW_SIDE))
            .style(theme::pane_body),
    ]
    .spacing(6)
    .into()
}

fn main_card<'a>(shell: &'a Shell, view: &ExportImageView<'_>) -> Element<'a, Message> {
    let i18n = &shell.i18n;
    let editable = matches!(view.step, ImageStepView::Options);
    let busy = matches!(view.step, ImageStepView::Saving);
    let status: Element<'a, Message> = match view.step {
        ImageStepView::Picking => muted(i18n.tr("export-image-picking")),
        ImageStepView::Saving => muted(i18n.tr("export-image-saving")),
        _ => muted(String::new()),
    };
    let body = column![
        row![preview(shell, view), options(shell, view, editable)].spacing(20),
        status,
    ]
    .spacing(10);
    modal(Card {
        id: EXPORT_IMAGE_BOX_ID,
        title: i18n.tr("export-image-title"),
        body: body.into(),
        buttons: vec![
            with_id(
                "export-image-close".to_owned(),
                action(
                    i18n.tr("export-image-close"),
                    (!busy).then(|| docs(ExportDocsMessage::ImageClose)),
                ),
            ),
            with_id(
                "export-image-save".to_owned(),
                action(
                    i18n.tr("export-image-save"),
                    editable.then(|| docs(ExportDocsMessage::ImageSave)),
                ),
            ),
        ],
        width: 760.0,
    })
}

fn confirm_card<'a>(shell: &'a Shell, path: &std::path::Path) -> Element<'a, Message> {
    let i18n = &shell.i18n;
    let mut args = FluentArgs::new();
    args.set(
        "name",
        path.file_name().map_or_else(
            || path.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        ),
    );
    modal(Card {
        id: EXPORT_IMAGE_CONFIRM_ID,
        title: i18n.tr("export-ui-confirm-title"),
        body: text(i18n.tr_args("export-image-confirm", &args))
            .size(BODY_TEXT)
            .into(),
        buttons: vec![
            with_id(
                "export-image-confirm-no".to_owned(),
                action(
                    i18n.tr("export-games-cancel"),
                    Some(docs(ExportDocsMessage::Confirm(false))),
                ),
            ),
            with_id(
                "export-image-confirm-yes".to_owned(),
                action(
                    i18n.tr("export-ui-replace"),
                    Some(docs(ExportDocsMessage::Confirm(true))),
                ),
            ),
        ],
        width: 460.0,
    })
}

/// The layer to put over the window, if the image window is open.
pub fn layer(shell: &Shell) -> Option<Element<'_, Message>> {
    let view = shell.app.export_image_view()?;
    let main = main_card(shell, &view);
    if let ImageStepView::Confirm(path) = view.step {
        return Some(iced::widget::stack![main, confirm_card(shell, path)].into());
    }
    Some(main)
}

/// The message that closes the window or answers "no" to its question.
pub fn escape_message(shell: &Shell) -> Option<AppMessage> {
    let view = shell.app.export_image_view()?;
    Some(AppMessage::ExportDocs(match view.step {
        ImageStepView::Confirm(_) => ExportDocsMessage::Confirm(false),
        _ => ExportDocsMessage::ImageClose,
    }))
}

/// The message of Enter: saves, or answers "no" to the question: a
/// replacement is destructive, so "yes" needs a click (D4301).
pub fn enter_message(shell: &Shell) -> Option<AppMessage> {
    let view = shell.app.export_image_view()?;
    match view.step {
        ImageStepView::Confirm(_) => {
            Some(AppMessage::ExportDocs(ExportDocsMessage::Confirm(false)))
        }
        ImageStepView::Options => Some(AppMessage::ExportDocs(ExportDocsMessage::ImageSave)),
        _ => None,
    }
}
