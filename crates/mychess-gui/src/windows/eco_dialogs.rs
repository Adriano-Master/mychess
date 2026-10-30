//! The "ECO-classify games" and "Find novelty" dialogs (spec M5-7b,
//! D1710-D1720). The application keeps what the user chose and what came out
//! (`AppState::eco_classify_view`, `eco_novelty_view`); the dialogs draw it
//! and send [`EcoMessage`]s. While a job runs they show its progress (the
//! one of the game list, `AppState::list_job`) and a button to stop it.

use fluent_bundle::FluentArgs;
use iced::widget::{
    button, checkbox, column, container, progress_bar, radio, row, text, text_input,
};
use iced::{Alignment, Element, Length};
use mychess_app::{
    AppState, ClassifyChoice, EcoClassifyView, EcoMessage, EcoNoveltyView, ListJobKind,
    ListMessage, Message as AppMessage, SlotId,
};

use super::annotate_dialog::choice;
use super::eco_text::{classify_result_lines, novelty_text, slot_label};
use crate::i18n::I18n;
use crate::shell::Message;
use crate::theme;
use crate::widgets::modal::{Card, modal};

/// Id of the classification box.
pub const ECO_CLASSIFY_BOX_ID: &str = "eco-classify-box";
/// Id of the novelty box.
pub const ECO_NOVELTY_BOX_ID: &str = "eco-novelty-box";
/// Id of the field of the date.
pub const ECO_SINCE_ID: &str = "eco-classify-since";

fn send(message: EcoMessage) -> Message {
    Message::App(AppMessage::Eco(message))
}

fn stop_job() -> Message {
    Message::App(AppMessage::Bases(mychess_app::BasesMessage::List(
        ListMessage::CancelJob,
    )))
}

/// The fraction done of the job of `kind`, and the percentage text.
fn job_progress(app: &AppState, kind: ListJobKind) -> Option<(f32, i64)> {
    let (running, control) = app.list_job()?;
    if running != kind {
        return None;
    }
    let (done, total) = control.progress();
    let fraction = if total > 0 {
        // A ratio to draw: the precision of an `f32` is plenty.
        #[allow(clippy::cast_precision_loss)]
        let ratio = (done as f32 / total as f32).clamp(0.0, 1.0);
        ratio
    } else {
        0.0
    };
    // Between 0 and 100 after the clamp.
    #[allow(clippy::cast_possible_truncation)]
    let percent = (fraction * 100.0).round() as i64;
    Some((fraction, percent))
}

fn progress_row<'a>(
    app: &AppState,
    kind: ListJobKind,
    key: &str,
    i18n: &I18n,
) -> Option<Element<'a, Message>> {
    let (fraction, percent) = job_progress(app, kind)?;
    let mut args = FluentArgs::new();
    args.set("percent", percent);
    Some(
        row![
            text(i18n.tr_args(key, &args)).size(12),
            progress_bar(0.0..=1.0, fraction)
                .length(Length::Fill)
                .girth(Length::Fixed(8.0)),
        ]
        .spacing(8)
        .align_y(Alignment::Center)
        .into(),
    )
}

fn muted<'a>(content: String) -> Element<'a, Message> {
    text(content).size(12).style(theme::muted_text).into()
}

// ---- classify ----

/// The box, when the application has the dialog open.
pub fn classify<'a>(app: &AppState, i18n: &I18n) -> Option<Element<'a, Message>> {
    let view = app.eco_classify_view()?;
    Some(classify_box(app, &view, i18n))
}

fn classify_choices<'a>(view: &EcoClassifyView<'_>, i18n: &I18n) -> Element<'a, Message> {
    let current = Some(view.choice);
    let pick = |label: String, value: ClassifyChoice| {
        radio(label, value, current, |v| {
            send(EcoMessage::SetClassifyChoice(v))
        })
        .size(15)
        .text_size(13)
    };
    let mut filter_args = FluentArgs::new();
    filter_args.set("count", i64::from(view.filter_games));
    let since = text_input(&i18n.tr("eco-classify-since-placeholder"), view.since)
        .id(ECO_SINCE_ID)
        .on_input(|t| send(EcoMessage::SetClassifySince(t)))
        .padding(4)
        .width(Length::Fixed(130.0));
    let mut list = column![
        pick(i18n.tr("eco-classify-new"), ClassifyChoice::NoCode),
        pick(i18n.tr("eco-classify-all"), ClassifyChoice::All),
        pick(
            i18n.tr_args("eco-classify-filter", &filter_args),
            ClassifyChoice::Filter
        ),
        pick(i18n.tr("eco-classify-year"), ClassifyChoice::LastYear),
        pick(i18n.tr("eco-classify-month"), ClassifyChoice::LastMonth),
        row![
            pick(i18n.tr("eco-classify-since"), ClassifyChoice::Since),
            since
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    ]
    .spacing(6);
    if view.invalid_date {
        list = list.push(
            text(i18n.tr("eco-classify-invalid-date"))
                .size(12)
                .style(text::danger),
        );
    }
    list.into()
}

fn classify_box<'a>(
    app: &AppState,
    view: &EcoClassifyView<'_>,
    i18n: &I18n,
) -> Element<'a, Message> {
    let mut base = FluentArgs::new();
    base.set("name", slot_label(app, i18n, view.slot));
    base.set("games", i64::from(view.total_games));
    let codes = row![
        choice(
            i18n.tr("eco-classify-basic"),
            false,
            view.extended,
            true,
            |v| send(EcoMessage::SetClassifyExtended(v)),
        ),
        choice(
            i18n.tr("eco-classify-extended"),
            true,
            view.extended,
            true,
            |v| send(EcoMessage::SetClassifyExtended(v)),
        ),
    ]
    .spacing(4);
    let mut body = column![
        muted(i18n.tr_args("eco-classify-base", &base)),
        text(i18n.tr("eco-classify-which")).size(13),
        classify_choices(view, i18n),
        text(i18n.tr("eco-classify-codes")).size(13),
        codes,
    ]
    .spacing(8);
    if !view.book_loaded {
        body = body.push(
            text(i18n.tr("eco-classify-no-book"))
                .size(12)
                .style(text::danger),
        );
    }
    if !view.can_write {
        body = body.push(muted(i18n.tr("eco-classify-read-only")));
    }
    if let Some(progress) =
        progress_row(app, ListJobKind::EcoClassify, "eco-classify-progress", i18n)
    {
        body = body.push(progress);
    }
    if let Some(result) = view.result {
        for line in classify_result_lines(i18n, result) {
            body = body.push(text(line).size(13));
        }
    }
    let ready = view.book_loaded && !view.running;
    let buttons = if view.running {
        vec![
            button(text(i18n.tr("eco-stop")))
                .style(button::secondary)
                .on_press(stop_job())
                .into(),
        ]
    } else {
        vec![
            button(text(i18n.tr("eco-close")))
                .style(button::secondary)
                .on_press(send(EcoMessage::CloseClassify))
                .into(),
            button(text(i18n.tr("eco-classify-preview")))
                .style(button::secondary)
                .on_press_maybe(ready.then(|| send(EcoMessage::ClassifyPreview)))
                .into(),
            button(text(i18n.tr("eco-classify-run")))
                .on_press_maybe((ready && view.can_write).then(|| send(EcoMessage::ClassifyRun)))
                .into(),
        ]
    };
    modal(Card {
        id: ECO_CLASSIFY_BOX_ID,
        title: i18n.tr("eco-classify-title"),
        body: body.into(),
        buttons,
        width: 520.0,
    })
}

// ---- novelty ----

/// The box, when the application has the dialog open.
pub fn novelty<'a>(app: &AppState, i18n: &I18n) -> Option<Element<'a, Message>> {
    let view = app.eco_novelty_view()?;
    Some(novelty_box(app, &view, i18n))
}

fn novelty_box<'a>(app: &AppState, view: &EcoNoveltyView<'_>, i18n: &I18n) -> Element<'a, Message> {
    let references: Vec<Element<'a, Message>> = view
        .references
        .iter()
        .map(|&id: &SlotId| {
            choice(
                slot_label(app, i18n, id),
                id,
                view.reference,
                !view.running,
                |id| send(EcoMessage::SetNoveltyReference(id)),
            )
        })
        .collect();
    let mut older = FluentArgs::new();
    older.set("date", view.game_date.short());
    let mut body = column![
        muted(i18n.tr("eco-novelty-help")),
        text(i18n.tr("eco-novelty-database")).size(13),
        row(references).spacing(4).wrap(),
        checkbox(view.older_only)
            .label(i18n.tr_args("eco-novelty-older", &older))
            .size(15)
            .text_size(13)
            .on_toggle_maybe(
                (!view.running).then_some(|on| send(EcoMessage::SetNoveltyOlderOnly(on)))
            ),
    ]
    .spacing(8);
    if !view.has_moves {
        body = body.push(muted(i18n.tr("eco-novelty-empty")));
    }
    if let Some(progress) =
        progress_row(app, ListJobKind::EcoNovelty, "eco-novelty-searching", i18n)
    {
        body = body.push(progress);
    }
    if let Some(result) = view.result {
        body = body.push(container(text(novelty_text(i18n, result)).size(14)).padding([4, 0]));
    }
    let buttons = if view.running {
        vec![
            button(text(i18n.tr("eco-stop")))
                .style(button::secondary)
                .on_press(stop_job())
                .into(),
        ]
    } else {
        vec![
            button(text(i18n.tr("eco-close")))
                .style(button::secondary)
                .on_press(send(EcoMessage::CloseNovelty))
                .into(),
            button(text(i18n.tr("eco-novelty-find")))
                .on_press_maybe(view.has_moves.then(|| send(EcoMessage::FindNovelty)))
                .into(),
        ]
    };
    modal(Card {
        id: ECO_NOVELTY_BOX_ID,
        title: i18n.tr("eco-novelty-title"),
        body: body.into(),
        buttons,
        width: 520.0,
    })
}
