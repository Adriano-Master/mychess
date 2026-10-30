//! The "Merge game" dialog (spec M7-6, slice c, D3853, D3864): the preview of
//! the current game with the merge done, the "up to move" buttons, the note
//! about the end of the game, and the confirmation of a batch.
//!
//! The application owns all the state ([`AppState::merge_view`]); nothing is
//! kept here, so there is no draft to forget on Esc. The window draws the
//! view, sends [`MergeMessage::SetEndPly`], [`MergeMessage::Apply`] and
//! [`MergeMessage::Cancel`], and translates the comment that goes before the
//! first merged move ([`localize`]).

use fluent_bundle::FluentArgs;
use iced::widget::scrollable::AbsoluteOffset;
use iced::widget::text::Span;
use iced::widget::{Id, operation};
use iced::widget::{button, column, container, rich_text, row, scrollable, span, text};
use iced::{Element, Length, Padding, Task};
use mychess_app::{
    AppState, END_MOVE_CHOICES, Inline, MergeMessage, MergeNotice, MergeView,
    Message as AppMessage, NotationDoc, end_ply_of_moves,
};
use mychess_core::MergeCommentParts;

use crate::i18n::I18n;
use crate::shell::{Message, Shell};
use crate::theme::{self, Tokens};
use crate::widgets::modal::{Card, modal};

/// Id of the box of the dialog.
pub const MERGE_BOX_ID: &str = "merge-box";
/// Id of the OK button.
pub const MERGE_APPLY_ID: &str = "merge-apply";
/// Id of the Cancel button.
pub const MERGE_CANCEL_ID: &str = "merge-cancel";
/// Id of the preview.
pub const MERGE_PREVIEW_ID: &str = "merge-preview";
/// Id of the "All" button.
pub const MERGE_ALL_ID: &str = "merge-end-all";

/// Width of the box.
const BOX_WIDTH: f32 = 600.0;
/// Height of the preview.
pub const PREVIEW_HEIGHT: f32 = 260.0;
/// Text size of the preview.
const PREVIEW_TEXT: f32 = 14.0;
/// Indentation of a variation block, per level.
const INDENT_STEP: f32 = 18.0;

/// Id of the button "up to move `moves`".
pub fn end_button_id(moves: u32) -> String {
    format!("merge-end-{moves}")
}

fn app_message(message: MergeMessage) -> AppMessage {
    AppMessage::Merge(message)
}

fn send(message: MergeMessage) -> Message {
    Message::App(app_message(message))
}

/// The message of OK and of Enter. The comment is translated on its way by
/// [`localize`].
pub fn apply_message_app() -> AppMessage {
    app_message(MergeMessage::Apply { comment: None })
}

/// The message of Cancel and of Esc.
pub fn cancel_message_app() -> AppMessage {
    app_message(MergeMessage::Cancel)
}

/// The message of the context menus and buttons that merge game `number` of
/// the database in `slot`.
pub fn open_message_app(slot: mychess_app::SlotId, number: u32) -> AppMessage {
    app_message(MergeMessage::Open { slot, number })
}

/// Whether the game of the editor may take a merge now: it has moves, it is
/// not in trial mode and nothing owns it (annotation, a game against an
/// engine, an online game). The one predicate of the menus.
pub fn can_merge(app: &AppState) -> bool {
    !app.session.in_trial()
        && !app.annotation_running()
        && !app.play_live()
        && !app.assist_locked()
        && app.session.game().mainline().next().is_some()
}

/// Whether the dialog is open. Cheap: it only asks for the view.
pub fn is_open(app: &AppState) -> bool {
    app.merge_view().is_some()
}

/// Adds the translated comment to an OK that has none: the application
/// writes the neutral English one otherwise. Any other message goes through.
pub fn localize(app: &AppState, i18n: &I18n, message: AppMessage) -> AppMessage {
    if let AppMessage::Merge(MergeMessage::Apply { comment: None }) = &message
        && let Some(view) = app.merge_view()
        && let Some(parts) = view.comment_parts
    {
        let comment = Some(comment_text(i18n, parts, view.truncated()));
        return app_message(MergeMessage::Apply { comment });
    }
    message
}

/// The first line of the description of a game: `White -- Black Result`.
pub fn description_line(i18n: &I18n, parts: &MergeCommentParts) -> String {
    let mut args = FluentArgs::new();
    args.set("white", parts.white.as_str());
    args.set("black", parts.black.as_str());
    args.set("result", parts.result);
    i18n.tr_args("merge-comment-line", &args)
}

/// The second line: `Event, year`.
pub fn event_line(i18n: &I18n, parts: &MergeCommentParts) -> String {
    let mut args = FluentArgs::new();
    args.set("event", parts.event.as_str());
    // A string: a number would be formatted with a group separator.
    args.set("year", parts.year.to_string());
    i18n.tr_args("merge-comment-event", &args)
}

/// The comment put before the first merged move, translated (Scid:
/// `sc_game merge`, 8193-8205): `White -- Black Result[ (N moves)]` and
/// `Event, year` on the next line. The `(N moves)` part appears when the
/// merge cut the game short.
pub fn comment_text(i18n: &I18n, parts: &MergeCommentParts, truncated: bool) -> String {
    let mut first = description_line(i18n, parts);
    if truncated {
        let mut args = FluentArgs::new();
        args.set("count", i64::from(parts.full_moves()));
        first.push(' ');
        first.push_str(&i18n.tr_args("merge-comment-moves", &args));
    }
    format!("{first}\n{}", event_line(i18n, parts))
}

/// The translated text of a notice: the reason wrapped in "Unable to merge
/// the selected game", or the summary of a batch.
pub fn notice_text(i18n: &I18n, notice: &MergeNotice) -> String {
    if let MergeNotice::BatchDone { merged, skipped } = notice {
        let mut args = FluentArgs::new();
        args.set("merged", i64::from(*merged));
        args.set("skipped", i64::from(*skipped));
        return i18n.tr_args(notice.message_key(), &args);
    }
    let mut args = FluentArgs::new();
    args.set("reason", i18n.tr(notice.message_key()));
    i18n.tr_args("merge-err-unable", &args)
}

/// Width of the text of the preview: the box less its padding and the bar.
const PREVIEW_TEXT_WIDTH: f32 = BOX_WIDTH - 64.0;
/// Average width of a character of the preview, in text sizes.
const CHAR_WIDTH: f32 = 0.55;
/// Height of a line of the preview, in text sizes (the iced default).
const LINE_HEIGHT: f32 = 1.3;
/// Space between the blocks of the preview.
const BLOCK_SPACING: f32 = 4.0;

/// How many characters an item of the preview takes.
fn inline_chars(inline: &Inline) -> usize {
    match inline {
        Inline::Number(number) => number.chars().count(),
        Inline::Move { san, .. } => san.chars().count(),
        Inline::Open | Inline::Close | Inline::Space => 1,
        _ => 0,
    }
}

/// Where the move at `(block, item)` of `doc` is drawn, from the top of the
/// preview, in pixels. An estimate: the text wraps like a paragraph of
/// `CHAR_WIDTH` wide characters, which is close enough to bring the line
/// into the 260 pixels of the window.
pub fn position_y(doc: &NotationDoc, block: usize, item: usize) -> f32 {
    let line = PREVIEW_TEXT * LINE_HEIGHT;
    let mut y = 0.0_f32;
    for (index, current) in doc.blocks.iter().enumerate() {
        let indent = f32::from(current.indent) * INDENT_STEP;
        let per_line = ((PREVIEW_TEXT_WIDTH - indent) / (PREVIEW_TEXT * CHAR_WIDTH))
            .floor()
            .max(1.0);
        let upto = if index == block {
            item
        } else {
            current.inlines.len()
        };
        let chars: usize = current.inlines.iter().take(upto).map(inline_chars).sum();
        // A paragraph of a few hundred characters: the cast is exact.
        #[allow(clippy::cast_precision_loss)]
        let lines = (chars as f32 / per_line).floor();
        if index == block {
            return y + lines * line;
        }
        #[allow(clippy::cast_precision_loss)]
        let height = ((chars as f32 / per_line).ceil().max(1.0)) * line;
        y += height + BLOCK_SPACING;
    }
    y
}

/// The offset that brings the first merged move into the window of the
/// preview, in its middle (the position is an estimate, so it may be off by
/// half a window either way); `None` when there is none (a batch).
pub fn preview_offset(view: &MergeView<'_>) -> Option<f32> {
    let (block, item) = view.first_position()?;
    Some((position_y(view.preview_doc, block, item) - PREVIEW_HEIGHT / 2.0).max(0.0))
}

/// The task that scrolls the preview to the first merged move, if the dialog
/// is open. `scroll_to` is safe with a menu open: the menu bars go through
/// `widgets::menu_guard`.
pub fn reveal_first(app: &AppState) -> Option<Task<Message>> {
    let offset = preview_offset(&app.merge_view()?)?;
    Some(operation::scroll_to(
        Id::new(MERGE_PREVIEW_ID),
        AbsoluteOffset {
            x: None,
            y: Some(offset),
        },
    ))
}

/// Whether `message` opens the dialog or changes its preview, so that the
/// shell scrolls to the new moves.
pub fn moves_preview(message: &AppMessage) -> bool {
    matches!(
        message,
        AppMessage::Merge(MergeMessage::Loaded { .. } | MergeMessage::SetEndPly(_))
    )
}

/// The preview: the game with the merge done, a block per line, the moves
/// that the merge adds highlighted with the accent of the theme.
fn preview<'a>(view: &MergeView<'_>, tokens: &Tokens) -> Element<'a, Message> {
    // The notation is cached by the application (built once per outcome).
    let doc = view.preview_doc;
    let mut blocks = column![].spacing(4).width(Length::Fill);
    for block in &doc.blocks {
        let mut spans: Vec<Span<'a, ()>> = Vec::with_capacity(block.inlines.len());
        for inline in &block.inlines {
            match inline {
                Inline::Number(number) => {
                    spans.push(span(number.to_string()).color(tokens.muted));
                }
                Inline::Move {
                    node,
                    san,
                    in_variation,
                    ..
                } => {
                    let item = span(san.to_string());
                    spans.push(if view.is_new(*node) {
                        item.color(tokens.on_accent)
                            .background(tokens.accent)
                            .border(iced::Border {
                                radius: tokens.radius.chip.into(),
                                ..iced::Border::default()
                            })
                            .padding([0, 3])
                    } else if *in_variation {
                        item.color(tokens.variation)
                    } else {
                        item.color(tokens.text)
                    });
                }
                Inline::Open => spans.push(span("(").color(tokens.muted)),
                Inline::Close => spans.push(span(")").color(tokens.muted)),
                Inline::Space => spans.push(span(" ")),
                _ => {}
            }
        }
        let indent = f32::from(block.indent) * INDENT_STEP;
        blocks = blocks.push(
            container(rich_text(spans).size(PREVIEW_TEXT).width(Length::Fill)).padding(Padding {
                left: indent,
                ..Padding::ZERO
            }),
        );
    }
    scrollable(blocks)
        .id(MERGE_PREVIEW_ID)
        .height(Length::Fixed(PREVIEW_HEIGHT))
        .width(Length::Fill)
        .into()
}

/// A button of the row "up to move".
fn end_button<'a>(label: String, id: &str, on: bool, end_ply: Option<u32>) -> Element<'a, Message> {
    let body = button(text(label).size(13))
        .padding([3, 8])
        .on_press(send(MergeMessage::SetEndPly(end_ply)));
    let body = if on {
        body.style(button::primary)
    } else {
        body.style(button::secondary)
    };
    // A button takes no id: the container carries it (for tests and focus).
    container(body).id(id.to_owned()).into()
}

/// The row "up to move": 5 to 40 and All (D3853).
fn end_row<'a>(view: &MergeView<'_>, i18n: &I18n) -> Element<'a, Message> {
    let mut buttons = row![text(i18n.tr("merge-up-to")).size(13)]
        .spacing(6)
        .align_y(iced::Alignment::Center);
    for moves in END_MOVE_CHOICES {
        let ply = end_ply_of_moves(moves);
        buttons = buttons.push(end_button(
            moves.to_string(),
            &end_button_id(moves),
            view.end_ply == Some(ply),
            Some(ply),
        ));
    }
    buttons = buttons.push(end_button(
        i18n.tr("merge-all"),
        MERGE_ALL_ID,
        view.end_ply.is_none(),
        None,
    ));
    buttons.wrap().into()
}

fn muted<'a>(content: String) -> Element<'a, Message> {
    text(content).size(12).style(theme::muted_text).into()
}

fn count_line(i18n: &I18n, key: &str, count: u32) -> Option<Element<'static, Message>> {
    (count > 0).then(|| {
        let mut args = FluentArgs::new();
        args.set("count", i64::from(count));
        muted(i18n.tr_args(key, &args))
    })
}

/// The question and the counts of a batch (D3864).
fn batch_body<'a>(view: &MergeView<'_>, i18n: &I18n) -> iced::widget::Column<'a, Message> {
    let report = view.batch_report;
    let mut confirm = FluentArgs::new();
    confirm.set(
        "count",
        i64::try_from(view.numbers.len()).unwrap_or(i64::MAX),
    );
    let mut summary = FluentArgs::new();
    summary.set("merged", i64::from(report.merged));
    summary.set("skipped", i64::from(report.skipped()));
    let mut body = column![
        text(i18n.tr_args("merge-batch-confirm", &confirm)).size(14),
        text(i18n.tr_args("merge-batch-summary", &summary)).size(13),
    ]
    .spacing(6);
    for (key, count) in [
        ("merge-batch-skip-no-match", report.skipped_no_match),
        ("merge-batch-skip-nothing", report.skipped_nothing),
        ("merge-batch-skip-same", report.skipped_same),
        ("merge-batch-skip-unreadable", report.skipped_unreadable),
    ] {
        if let Some(line) = count_line(i18n, key, count) {
            body = body.push(line);
        }
    }
    body
}

/// The box, when the application has the dialog open.
pub fn view(shell: &Shell) -> Option<Element<'_, Message>> {
    let merge = shell.app.merge_view()?;
    let i18n = &shell.i18n;
    let tokens = Tokens::of(&shell.theme());
    let mut body = if merge.batch {
        batch_body(&merge, i18n)
    } else {
        let mut single = column![].spacing(8);
        if let Some(parts) = merge.comment_parts {
            single = single.push(
                column![
                    muted(i18n.tr("merge-source-label")),
                    text(description_line(i18n, parts)).size(14),
                    muted(event_line(i18n, parts)),
                ]
                .spacing(2),
            );
        }
        single = single.push(end_row(&merge, i18n));
        if merge.extends_mainline() {
            single = single.push(text(i18n.tr("merge-note-extends")).size(13));
        }
        single
    };
    body = body
        .push(muted(i18n.tr("merge-preview-label")))
        .push(preview(&merge, &tokens))
        .push(muted(i18n.tr("merge-legend")));
    let buttons: Vec<Element<'_, Message>> = vec![
        container(
            button(text(i18n.tr("merge-cancel")))
                .style(button::secondary)
                .on_press(Message::App(cancel_message_app())),
        )
        .id(MERGE_CANCEL_ID)
        .into(),
        container(button(text(i18n.tr("merge-ok"))).on_press(Message::App(apply_message_app())))
            .id(MERGE_APPLY_ID)
            .into(),
    ];
    let title = if merge.batch {
        "merge-title-batch"
    } else {
        "merge-title"
    };
    Some(modal(Card {
        id: MERGE_BOX_ID,
        title: i18n.tr(title),
        body: body.into(),
        buttons,
        width: BOX_WIDTH,
    }))
}
