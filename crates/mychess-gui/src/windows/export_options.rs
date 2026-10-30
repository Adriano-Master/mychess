//! The format selector and the options of each format of "Export games"
//! (spec M7-4c, decisions M7-4/D4300-D4310; Scid `exportOptions`).
//!
//! The application keeps the options in the preferences
//! (`AppState::export_docs_view`); every control sends an
//! [`ExportDocsMessage`], so a click is remembered and saved by the
//! application and there is no draft here.

use iced::widget::{column, container, row, text, text_input};
use iced::{Alignment, Element, Length};
use mychess_app::Message as AppMessage;
use mychess_app::export_docs::{
    CsvSeparator, DEFAULT_LIST_FORMAT, ExportDocsMessage, ExportDocsView, ExportEdit, ExportFormat,
    MAX_DIAGRAM_PX, MAX_LIST_FORMAT_CHARS, MAX_PER_FILE,
};

use crate::i18n::I18n;
use crate::shell::Message;
use crate::windows::panel_kit::{BODY_TEXT, action, check, choice, danger, muted};

/// Every format, in the order of the selector.
pub const FORMATS: [ExportFormat; 7] = [
    ExportFormat::Pgn,
    ExportFormat::Html,
    ExportFormat::Viewer,
    ExportFormat::Latex,
    ExportFormat::Epd,
    ExportFormat::List,
    ExportFormat::Csv,
];

/// Diagram sizes the HTML options offer (`0` is none).
pub const DIAGRAM_SIZES: [u16; 5] = [0, 150, 200, 300, 400];
/// Games per file the viewer options offer.
pub const PER_FILE_CHOICES: [u32; 5] = [100, 250, 500, 1000, 5000];

/// The message of a change of an option.
pub fn edit(change: ExportEdit) -> Message {
    docs(ExportDocsMessage::Edit(change))
}

/// A message of the export options.
pub fn docs(message: ExportDocsMessage) -> Message {
    Message::App(AppMessage::ExportDocs(message))
}

/// The id of the button of a format.
pub fn format_id(format: ExportFormat) -> String {
    format!("export-format-{}", format.name())
}

fn with_id(id: String, body: Element<'_, Message>) -> Element<'_, Message> {
    container(body).id(id).into()
}

fn toggle<'a>(
    i18n: &I18n,
    key: &str,
    id: &'static str,
    on: bool,
    change: impl Fn(bool) -> ExportEdit + 'a,
) -> Element<'a, Message> {
    with_id(
        id.to_owned(),
        check(i18n.tr(key), on, Some(move |v| edit(change(v)))),
    )
}

fn labelled(label: String, body: Element<'_, Message>) -> Element<'_, Message> {
    row![
        text(label).size(BODY_TEXT).width(Length::Fixed(190.0)),
        body
    ]
    .spacing(8)
    .align_y(Alignment::Center)
    .into()
}

/// The selector of the format.
pub fn format_selector<'a>(view: &ExportDocsView<'_>, i18n: &I18n) -> Element<'a, Message> {
    let mut formats = row![].spacing(4);
    for format in FORMATS {
        formats = formats.push(with_id(
            format_id(format),
            choice(
                i18n.tr(format.message_key()),
                view.prefs.format == format,
                Some(docs(ExportDocsMessage::SetFormat(format))),
            ),
        ));
    }
    column![
        text(i18n.tr("export-ui-format-title")).size(BODY_TEXT),
        formats.wrap()
    ]
    .spacing(6)
    .into()
}

/// The options of the format the application has chosen.
#[allow(clippy::too_many_lines)] // one arm per format
pub fn format_options<'a>(view: &ExportDocsView<'_>, i18n: &I18n) -> Element<'a, Message> {
    let prefs = view.prefs;
    let mut col = column![].spacing(6);
    match prefs.format {
        ExportFormat::Pgn => {}
        ExportFormat::Html => {
            let html = &prefs.html;
            col = col
                .push(toggle(
                    i18n,
                    "export-html-comments",
                    "export-html-comments",
                    html.comments,
                    ExportEdit::HtmlComments,
                ))
                .push(toggle(
                    i18n,
                    "export-html-variations",
                    "export-html-variations",
                    html.variations,
                    ExportEdit::HtmlVariations,
                ))
                .push(toggle(
                    i18n,
                    "export-html-symbols",
                    "export-html-symbols",
                    html.symbols,
                    ExportEdit::HtmlSymbols,
                ));
            let mut sizes = row![].spacing(4);
            for px in DIAGRAM_SIZES.into_iter().filter(|p| *p <= MAX_DIAGRAM_PX) {
                let label = if px == 0 {
                    i18n.tr("export-ui-diagram-none")
                } else {
                    px.to_string()
                };
                sizes = sizes.push(with_id(
                    format!("export-html-diagram-{px}"),
                    choice(
                        label,
                        html.diagram_px == px,
                        Some(edit(ExportEdit::HtmlDiagramPx(px))),
                    ),
                ));
            }
            col = col.push(labelled(
                i18n.tr("export-html-diagram-size"),
                sizes.wrap().into(),
            ));
        }
        ExportFormat::Viewer => {
            let viewer = &prefs.viewer;
            let mut sizes = row![].spacing(4);
            for n in PER_FILE_CHOICES.into_iter().filter(|n| *n <= MAX_PER_FILE) {
                sizes = sizes.push(with_id(
                    format!("export-viewer-per-file-{n}"),
                    choice(
                        n.to_string(),
                        viewer.per_file == n,
                        Some(edit(ExportEdit::ViewerPerFile(n))),
                    ),
                ));
            }
            col = col
                .push(labelled(
                    i18n.tr("export-viewer-per-file"),
                    sizes.wrap().into(),
                ))
                .push(muted(i18n.tr("export-ui-per-file-hint")))
                .push(toggle(
                    i18n,
                    "export-viewer-include-pgn",
                    "export-viewer-include-pgn",
                    viewer.include_pgn,
                    ExportEdit::ViewerIncludePgn,
                ))
                .push(toggle(
                    i18n,
                    "export-viewer-open-after",
                    "export-viewer-open-after",
                    viewer.open_after,
                    ExportEdit::ViewerOpenAfter,
                ));
        }
        ExportFormat::Latex => {
            let latex = &prefs.latex;
            col = col
                .push(toggle(
                    i18n,
                    "export-latex-comments",
                    "export-latex-comments",
                    latex.comments,
                    ExportEdit::LatexComments,
                ))
                .push(toggle(
                    i18n,
                    "export-latex-variations",
                    "export-latex-variations",
                    latex.variations,
                    ExportEdit::LatexVariations,
                ))
                .push(toggle(
                    i18n,
                    "export-latex-flip",
                    "export-latex-flip",
                    latex.flip,
                    ExportEdit::LatexFlip,
                ))
                .push(toggle(
                    i18n,
                    "export-latex-null-moves",
                    "export-latex-null-moves",
                    latex.convert_null_moves,
                    ExportEdit::LatexNullMoves,
                ));
        }
        ExportFormat::Epd => {
            let epd = &prefs.epd;
            col = col
                .push(toggle(
                    i18n,
                    "export-epd-dedupe",
                    "export-epd-dedupe",
                    epd.dedupe,
                    ExportEdit::EpdDedupe,
                ))
                .push(toggle(
                    i18n,
                    "export-epd-final-position",
                    "export-epd-final-position",
                    epd.final_position,
                    ExportEdit::EpdFinalPosition,
                ))
                .push(toggle(
                    i18n,
                    "export-epd-counters",
                    "export-epd-counters",
                    epd.hmvc_fmvn,
                    ExportEdit::EpdHmvcFmvn,
                ))
                .push(toggle(
                    i18n,
                    "export-epd-id",
                    "export-epd-id",
                    epd.id,
                    ExportEdit::EpdId,
                ))
                .push(toggle(
                    i18n,
                    "export-epd-game-number",
                    "export-epd-game-number",
                    epd.game_number,
                    ExportEdit::EpdGameNumber,
                ));
        }
        ExportFormat::List => {
            let field = text_input(DEFAULT_LIST_FORMAT, &prefs.list_format)
                .id("export-list-format-field")
                .on_input(|t| {
                    // The limit is enforced here too, so the field never
                    // holds more than the application keeps.
                    let kept: String = t.chars().take(MAX_LIST_FORMAT_CHARS).collect();
                    edit(ExportEdit::ListFormat(kept))
                })
                .padding(4)
                .size(BODY_TEXT)
                .width(Length::Fill);
            col = col
                .push(labelled(i18n.tr("export-list-format"), field.into()))
                .push(muted(i18n.tr("export-ui-list-hint")))
                .push(muted(i18n.tr("export-list-legend")))
                .push(with_id(
                    "export-list-default".to_owned(),
                    action(
                        i18n.tr("export-list-default"),
                        Some(edit(ExportEdit::ResetListFormat)),
                    ),
                ));
        }
        ExportFormat::Csv => {
            let csv = &prefs.csv;
            col = col
                .push(toggle(
                    i18n,
                    "export-csv-header",
                    "export-csv-header",
                    csv.header,
                    ExportEdit::CsvHeader,
                ))
                .push(labelled(
                    i18n.tr("export-csv-separator"),
                    row![
                        with_id(
                            "export-csv-sep-comma".to_owned(),
                            choice(
                                i18n.tr("export-csv-separator-comma"),
                                csv.separator == CsvSeparator::Comma,
                                Some(edit(ExportEdit::CsvSeparator(CsvSeparator::Comma))),
                            ),
                        ),
                        with_id(
                            "export-csv-sep-semicolon".to_owned(),
                            choice(
                                i18n.tr("export-csv-separator-semicolon"),
                                csv.separator == CsvSeparator::Semicolon,
                                Some(edit(ExportEdit::CsvSeparator(CsvSeparator::Semicolon))),
                            ),
                        ),
                    ]
                    .spacing(4)
                    .into(),
                ))
                .push(toggle(
                    i18n,
                    "export-csv-bom",
                    "export-csv-bom",
                    csv.bom,
                    ExportEdit::CsvBom,
                ));
        }
    }
    let needs_assets =
        matches!(prefs.format, ExportFormat::Viewer | ExportFormat::Html) && !view.has_assets;
    if needs_assets {
        col = col.push(danger(i18n.tr("export-ui-needs-assets")));
    }
    col.into()
}
