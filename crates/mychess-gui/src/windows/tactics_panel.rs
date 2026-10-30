//! The panel under the board during a tactics session, and the arrow of the
//! hint (spec M8-4c, D1939, D1940).
//!
//! Everything is drawn from the [`TacticsView`] that the application offers;
//! the buttons send messages of the application.

use iced::widget::{button, column, container, row, space, text, tooltip};
use iced::{Alignment, Element, Length};
use mychess_app::tactics::{TacticsMessage, TacticsNotice, TacticsPhase, TacticsView};
use mychess_app::{AppState, ArrowKind, EngineArrow, Message as AppMessage};

use crate::i18n::I18n;
use crate::shell::Message;
use crate::theme;
use crate::windows::tactics_text::{
    beyond_text, clock_text, hint_text, notice_text, stats_text, status_text, title_text,
};

/// Id of the panel under the board.
pub const TACTICS_BAR_ID: &str = "tactics-bar";

fn send(message: TacticsMessage) -> Message {
    Message::App(AppMessage::Tactics(message))
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

/// The arrow of the second level of the hint (D1919): the move to play, in
/// the green of the best move of the review.
pub fn hint_arrows(app: &AppState) -> Vec<EngineArrow> {
    if !app.tactics_live() {
        return Vec::new();
    }
    let view = app.tactics_view();
    let Some(hint) = view.hint else {
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
    if !app.tactics_live() {
        return None;
    }
    Some(bar_of(
        &app.tactics_view(),
        app.calvar_can_start_from_tactics(),
        i18n,
    ))
}

fn status_line<'a>(view: &TacticsView, i18n: &I18n) -> Element<'a, Message> {
    let line = text(status_text(view, i18n)).size(14);
    match view.phase {
        TacticsPhase::Solved(_) => line.style(text::success).into(),
        TacticsPhase::Failed(_) => line.style(text::danger).into(),
        _ => line.into(),
    }
}

/// The buttons of the panel.
fn buttons_of<'a>(
    view: &TacticsView,
    can_calvar: bool,
    i18n: &I18n,
) -> iced::widget::Row<'a, Message> {
    row![
        tipped(
            i18n.tr("tactics-hint"),
            i18n.tr("tactics-hint-tip"),
            view.can_hint.then(|| send(TacticsMessage::Hint)),
            false,
        ),
        tipped(
            i18n.tr("tactics-show-solution"),
            i18n.tr("tactics-show-solution-tip"),
            view.can_show_solution
                .then(|| send(TacticsMessage::ShowSolution)),
            false,
        ),
        tipped(
            i18n.tr("tactics-retry"),
            i18n.tr("tactics-retry-tip"),
            view.can_retry.then(|| send(TacticsMessage::Retry)),
            false,
        ),
        tipped(
            i18n.tr("tactics-continue"),
            i18n.tr("tactics-continue-tip"),
            view.can_continue.then(|| send(TacticsMessage::Continue)),
            false,
        ),
        tipped(
            i18n.tr("tactics-next"),
            i18n.tr("tactics-next-tip"),
            view.can_next.then(|| send(TacticsMessage::Next)),
            matches!(
                view.phase,
                TacticsPhase::Solved(_) | TacticsPhase::Failed(_)
            ),
        ),
        tipped(
            i18n.tr("calvar-from-tactics"),
            i18n.tr("calvar-from-tactics-tip"),
            can_calvar.then_some(Message::CalvarUi(
                crate::windows::calvar_dialog::CalvarUiMessage::FromTactics,
            )),
            false,
        ),
        tipped(
            i18n.tr("tactics-close"),
            i18n.tr("tactics-close-tip"),
            Some(send(TacticsMessage::Close)),
            false,
        ),
    ]
    .spacing(6)
    .align_y(Alignment::Center)
}

fn bar_of<'a>(view: &TacticsView, can_calvar: bool, i18n: &I18n) -> Element<'a, Message> {
    let mut info = column![
        text(title_text(view, i18n)).size(15),
        status_line(view, i18n),
    ]
    .spacing(2);
    if !view.title.is_empty() {
        info = info.push(text(view.title.clone()).size(12).style(theme::muted_text));
    }
    if let Some(hint) = hint_text(view, i18n) {
        info = info.push(text(hint).size(12));
    }
    if let Some(notice) = view.notice {
        info = info.push(text(notice_text(notice, i18n)).size(12).style(text::danger));
    }
    // Why the score is not being saved, for as long as that holds (D1954).
    if let Some(issue) = view.store_issue {
        info = info.push(text(notice_text(issue, i18n)).size(12).style(text::danger));
    }
    if view.opinion_missing {
        info = info.push(
            text(notice_text(TacticsNotice::NoEngine, i18n))
                .size(12)
                .style(theme::muted_text),
        );
    }
    if view.beyond_limit > 0 {
        info = info.push(
            text(beyond_text(view, i18n))
                .size(12)
                .style(theme::muted_text),
        );
    }
    info = info.push(
        text(stats_text(&view.stats, i18n))
            .size(12)
            .style(theme::muted_text),
    );
    let mut meter = column![text(clock_text(view.elapsed_s)).size(18)]
        .spacing(2)
        .align_x(Alignment::Center);
    if view.time_limit_s > 0 {
        let mut args = fluent_bundle::FluentArgs::new();
        args.set("count", i64::from(view.time_limit_s));
        meter = meter.push(
            text(i18n.tr_args("tactics-time-limit", &args))
                .size(11)
                .style(theme::muted_text),
        );
    }
    let mut args = fluent_bundle::FluentArgs::new();
    args.set("count", i64::from(view.attempts_left));
    args.set("max", i64::from(view.attempts_max));
    meter = meter.push(
        text(i18n.tr_args("tactics-attempts", &args))
            .size(11)
            .style(theme::muted_text),
    );
    let buttons = buttons_of(view, can_calvar, i18n);
    container(
        row![
            info,
            space::horizontal(),
            meter,
            // Wraps in a narrow pane: the buttons are many (M8-6d added one).
            buttons.wrap().align_x(iced::alignment::Horizontal::Right)
        ]
        .spacing(14)
        .align_y(Alignment::Center),
    )
    .id(TACTICS_BAR_ID)
    .padding([6, 12])
    .width(Length::Fill)
    .style(theme::status_bar)
    .into()
}
