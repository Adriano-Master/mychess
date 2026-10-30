//! The panel under the board during a calculation exercise, and the arrow
//! of the missed move (spec M8-6c, D2327).
//!
//! Everything is drawn from the [`CalvarView`] that the application offers;
//! the buttons send messages of the application. The text of the field of
//! moves is the only state of the window.

use iced::widget::{button, column, container, row, scrollable, space, text, text_input, tooltip};
use iced::{Alignment, Element, Length};
use mychess_app::calvar::{Assessment, CalvarMessage, CalvarPhase, CalvarView, Report};
use mychess_app::{AppState, ArrowKind, EngineArrow, Message as AppMessage};

use crate::i18n::I18n;
use crate::shell::Message;
use crate::theme;
use crate::windows::calvar_dialog::{CALVAR_ENTRY_ID, CalvarUi, CalvarUiMessage};
use crate::windows::calvar_text::{
    clock_text, crucial_text, current_text, engine_text, eval_tip, found_text, line_result_text,
    lines_text, missed_text, notice_text, origin_text, parts_text, rejected_text, score_text,
    status_text, title_text,
};

/// Id of the panel under the board.
pub const CALVAR_BAR_ID: &str = "calvar-bar";

const LIST_HEIGHT: f32 = 96.0;
const RESULT_HEIGHT: f32 = 150.0;

fn send(message: CalvarMessage) -> Message {
    Message::App(AppMessage::Calvar(message))
}

fn tipped<'a>(
    label: String,
    tip: String,
    message: Option<Message>,
    primary: bool,
) -> Element<'a, Message> {
    let b = button(text(label).size(13))
        .padding([4, 10])
        .on_press_maybe(message);
    let b = if primary {
        b.style(button::primary)
    } else {
        b.style(button::secondary)
    };
    tooltip(b, text(tip).size(12), tooltip::Position::Top)
        .style(theme::tooltip_box)
        .into()
}

/// The arrow of the best move that the user missed (D2316), in the green of
/// the best move of the review.
pub fn arrows(app: &AppState) -> Vec<EngineArrow> {
    if !app.calvar_live() {
        return Vec::new();
    }
    let Some((from, to)) = app.calvar_view().arrow else {
        return Vec::new();
    };
    vec![EngineArrow {
        from,
        to,
        rank: 1,
        kind: ArrowKind::ReviewBest,
        side: mychess_core::Color::White,
        label: None,
    }]
}

/// The panel, when an exercise is on.
pub fn bar<'a>(app: &AppState, ui: &CalvarUi, i18n: &I18n) -> Option<Element<'a, Message>> {
    if !app.calvar_live() {
        return None;
    }
    Some(bar_of(
        &app.calvar_view(),
        app.calvar_can_add_to_game(),
        ui.entry(),
        i18n,
    ))
}

fn header<'a>(view: &CalvarView, i18n: &I18n) -> Element<'a, Message> {
    let mut meter = column![text(clock_text(view.elapsed_s)).size(18)]
        .spacing(2)
        .align_x(Alignment::Center);
    if view.time_limit_s > 0 {
        let mut args = fluent_bundle::FluentArgs::new();
        args.set("count", i64::from(view.time_limit_s));
        meter = meter.push(
            text(i18n.tr_args("calvar-time-limit", &args))
                .size(11)
                .style(theme::muted_text),
        );
    }
    let status = text(status_text(view, i18n)).size(13);
    let status = match view.phase {
        CalvarPhase::Failed(_) => status.style(text::danger),
        _ => status,
    };
    let mut info = column![text(title_text(view, i18n)).size(15), status].spacing(2);
    if let Some(origin) = origin_text(view, i18n) {
        info = info.push(text(origin).size(12).style(theme::muted_text));
    }
    if let Some(notice) = view.notice {
        info = info.push(text(notice_text(notice, i18n)).size(12).style(text::danger));
    }
    if let Some(issue) = view.store_issue {
        info = info.push(text(notice_text(issue, i18n)).size(12).style(text::danger));
    }
    if let Some(rejected) = rejected_text(view, i18n) {
        info = info.push(text(rejected).size(12).style(text::danger));
    }
    row![
        info,
        space::horizontal(),
        meter,
        tipped(
            i18n.tr("calvar-close"),
            i18n.tr("calvar-close-tip"),
            Some(send(CalvarMessage::Close)),
            false,
        ),
    ]
    .spacing(14)
    .align_y(Alignment::Center)
    .into()
}

fn entry_rows<'a>(view: &CalvarView, entry: &str, i18n: &I18n) -> Element<'a, Message> {
    let field = text_input(&i18n.tr("calvar-entry-placeholder"), entry)
        .id(CALVAR_ENTRY_ID)
        .on_input(|v| Message::CalvarUi(CalvarUiMessage::EntryChanged(v)))
        .on_submit(Message::CalvarUi(CalvarUiMessage::EntrySubmit))
        .size(13)
        .padding([3, 6]);
    let field = tooltip(
        field,
        text(i18n.tr("calvar-entry-tip")).size(12),
        tooltip::Position::Top,
    )
    .style(theme::tooltip_box);
    let controls = row![
        field,
        tipped(
            i18n.tr("calvar-undo"),
            i18n.tr("calvar-undo-tip"),
            view.can_undo.then(|| send(CalvarMessage::Undo)),
            false,
        ),
        tipped(
            i18n.tr("calvar-done"),
            i18n.tr("calvar-done-tip"),
            view.can_done.then(|| send(CalvarMessage::Done)),
            view.can_done,
        ),
    ]
    .spacing(6)
    .align_y(Alignment::Center);
    let mut evals = row![text(i18n.tr("calvar-end-line")).size(12)]
        .spacing(4)
        .align_y(Alignment::Center);
    for assessment in Assessment::ALL {
        evals = evals.push(tipped(
            assessment.symbol().to_owned(),
            eval_tip(assessment, i18n),
            view.can_end_line
                .then(|| send(CalvarMessage::EndLine(assessment))),
            false,
        ));
    }
    let mut lines = column![].spacing(2);
    for (index, line) in view.lines.iter().enumerate() {
        lines = lines.push(
            row![
                text(format!("{}  {}", line.san, line.assessment.symbol()))
                    .size(12)
                    .width(Length::Fill),
                tipped(
                    i18n.tr("calvar-remove-line"),
                    i18n.tr("calvar-remove-line-tip"),
                    Some(send(CalvarMessage::RemoveLine(index))),
                    false,
                ),
            ]
            .spacing(6)
            .align_y(Alignment::Center),
        );
    }
    let counters = row![
        text(lines_text(view, i18n)).size(12),
        text(engine_text(view, i18n))
            .size(12)
            .style(theme::muted_text),
    ]
    .spacing(14);
    column![
        controls,
        text(current_text(view, i18n)).size(12),
        evals,
        counters,
        scrollable(lines).height(Length::Fixed(LIST_HEIGHT)),
    ]
    .spacing(4)
    .into()
}

fn result_rows<'a>(
    view: &CalvarView,
    report: &Report,
    can_add: bool,
    i18n: &I18n,
) -> Element<'a, Message> {
    let mut list = column![
        text(score_text(report, i18n)).size(16),
        text(found_text(report, i18n)).size(13),
    ]
    .spacing(3);
    match crucial_text(report, i18n) {
        Some(crucial) => list = list.push(text(crucial).size(13).style(text::danger)),
        None => {
            list = list.push(text(i18n.tr("calvar-result-all-found")).size(13));
        }
    }
    if let Some(missed) = missed_text(report, i18n) {
        list = list.push(text(missed).size(12));
    }
    list = list.push(
        text(parts_text(report, i18n))
            .size(12)
            .style(theme::muted_text),
    );
    for line in &report.lines {
        list = list.push(text(line_result_text(line, i18n)).size(12));
    }
    let buttons = row![
        tipped(
            i18n.tr("calvar-add-to-game"),
            i18n.tr("calvar-add-to-game-tip"),
            can_add.then(|| send(CalvarMessage::AddToGame)),
            true,
        ),
        tipped(
            i18n.tr("calvar-new"),
            i18n.tr("calvar-new-tip"),
            Some(Message::CalvarUi(CalvarUiMessage::New)),
            false,
        ),
    ]
    .spacing(6);
    let _ = view;
    column![
        scrollable(list).height(Length::Fixed(RESULT_HEIGHT)),
        buttons
    ]
    .spacing(4)
    .into()
}

fn bar_of<'a>(view: &CalvarView, can_add: bool, entry: &str, i18n: &I18n) -> Element<'a, Message> {
    let mut body = column![header(view, i18n)].spacing(4);
    match (view.phase, view.report.as_ref()) {
        (CalvarPhase::Entering, _) => body = body.push(entry_rows(view, entry, i18n)),
        (CalvarPhase::Reported(_), Some(report)) => {
            body = body.push(result_rows(view, report, can_add, i18n));
        }
        _ => {}
    }
    container(body)
        .id(CALVAR_BAR_ID)
        .padding([6, 12])
        .width(Length::Fill)
        .style(theme::status_bar)
        .into()
}
