//! The panel under the board during an opening-trainer session, and the
//! arrow of the hint (spec M8-5, D2240, D2241).
//!
//! Everything is drawn from the [`OpeningView`] that the application
//! offers; the buttons send messages of the application.

use iced::widget::{button, column, container, row, space, text, tooltip};
use iced::{Alignment, Element, Length};
use mychess_app::opening_trainer::{OpeningMessage, OpeningPhase, OpeningView};
use mychess_app::{AppState, ArrowKind, EngineArrow, Message as AppMessage};

use crate::i18n::I18n;
use crate::shell::Message;
use crate::theme;
use crate::windows::opening_trainer_text::{
    candidates_text, hint_text, notice_text, report_text, session_text, status_text, title_text,
};

/// Id of the panel under the board.
pub const OPENING_BAR_ID: &str = "opening-bar";

fn send(message: OpeningMessage) -> Message {
    Message::App(AppMessage::OpeningTrainer(message))
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

/// The arrow of the second level of the hint (D2241): the move to play, in
/// the green of the best move of the review.
pub fn hint_arrows(app: &AppState) -> Vec<EngineArrow> {
    if !app.opening_live() {
        return Vec::new();
    }
    let Some(hint) = app.opening_view().hint else {
        return Vec::new();
    };
    let Some(to) = hint.to else {
        return Vec::new();
    };
    vec![EngineArrow {
        from: hint.from,
        to,
        rank: 1,
        kind: ArrowKind::ReviewBest,
        side: mychess_core::Color::White,
        label: None,
    }]
}

/// The panel, when a session is on.
pub fn bar<'a>(app: &AppState, i18n: &I18n) -> Option<Element<'a, Message>> {
    if !app.opening_live() {
        return None;
    }
    Some(bar_of(&app.opening_view(), i18n))
}

fn buttons_of<'a>(view: &OpeningView, i18n: &I18n) -> iced::widget::Row<'a, Message> {
    let on_turn = view.phase == OpeningPhase::UserTurn;
    row![
        tipped(
            i18n.tr("opening-hint"),
            i18n.tr("opening-hint-tip"),
            on_turn.then(|| send(OpeningMessage::Hint)),
            false,
        ),
        tipped(
            i18n.tr("opening-retry"),
            i18n.tr("opening-retry-tip"),
            view.can_next.then(|| send(OpeningMessage::Retry)),
            false,
        ),
        tipped(
            i18n.tr("opening-report-button"),
            i18n.tr("opening-report-tip"),
            Some(send(OpeningMessage::Report)),
            false,
        ),
        tipped(
            i18n.tr("opening-next"),
            i18n.tr("opening-next-tip"),
            view.can_next.then(|| send(OpeningMessage::Next)),
            matches!(view.phase, OpeningPhase::LineDone(_)),
        ),
        tipped(
            i18n.tr("opening-close"),
            i18n.tr("opening-close-tip"),
            Some(send(OpeningMessage::Close)),
            false,
        ),
    ]
    .spacing(6)
    .align_y(Alignment::Center)
}

fn bar_of<'a>(view: &OpeningView, i18n: &I18n) -> Element<'a, Message> {
    let status = text(status_text(view, i18n)).size(14);
    let status = if view.phase == OpeningPhase::Exhausted {
        status.style(text::success)
    } else {
        status
    };
    let mut info = column![text(title_text(view, i18n)).size(15), status].spacing(2);
    if let Some(hint) = hint_text(view, i18n) {
        info = info.push(text(hint).size(12));
    }
    if let Some(moves) = candidates_text(&view.candidates, i18n) {
        info = info.push(text(moves).size(12));
    }
    if let Some(notice) = view.notice {
        info = info.push(text(notice_text(notice, i18n)).size(12).style(text::danger));
    }
    // Why the score is not being saved, for as long as that holds (D2225).
    if let Some(issue) = view.store_issue {
        info = info.push(text(notice_text(issue, i18n)).size(12).style(text::danger));
    }
    if view.truncated {
        info = info.push(
            text(notice_text(
                mychess_app::opening_trainer::OpeningNotice::Truncated,
                i18n,
            ))
            .size(12)
            .style(theme::muted_text),
        );
    }
    info = info.push(
        text(session_text(view, i18n))
            .size(12)
            .style(theme::muted_text),
    );
    if let Some(report) = &view.report {
        info = info.push(text(report_text(report, i18n)).size(12));
    }
    let buttons = buttons_of(view, i18n);
    container(
        row![info, space::horizontal(), buttons]
            .spacing(14)
            .align_y(Alignment::Center),
    )
    .id(OPENING_BAR_ID)
    .padding([6, 12])
    .width(Length::Fill)
    .style(theme::status_bar)
    .into()
}
