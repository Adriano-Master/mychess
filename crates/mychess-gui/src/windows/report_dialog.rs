//! The "Configure report" dialog (spec M4-10, D1422, D1425, D2100-D2105): the
//! engine, the limit of each search (depth or time), the limits of the
//! classes of move and the marks on the board.
//!
//! The application keeps the choices being edited
//! (`AppState::report.config_dialog`); every control sends
//! `ReportMessage::ConfigSet` with the new value and the application limits
//! it. OK saves the choices in the preferences and runs the report.

use iced::widget::{button, column, container, scrollable, text};
use iced::{Element, Length};
use mychess_app::engines::{EngineRunState, RunId};
use mychess_app::report::{ReportConfig, ReportMessage};
use mychess_app::{AppState, Message as AppMessage};

use crate::i18n::{I18n, Language};
use crate::shell::Message;
use crate::theme;
use crate::widgets::modal::{Card, modal};
use crate::windows::annotate_dialog::{check, choice, engine_name, group, labelled, stepper};

use fluent_bundle::FluentArgs;
use mychess_app::report::MoveClass;

/// Id of the box.
pub const REPORT_CONFIG_BOX_ID: &str = "report-config-box";

const BODY_HEIGHT: f32 = 430.0;

fn send(message: ReportMessage) -> Message {
    Message::App(AppMessage::Report(message))
}

/// A number with one decimal and the separator of the language (D2194).
fn decimal_text(i18n: &I18n, value: f64) -> String {
    let text = format!("{value:.1}");
    match i18n.language() {
        Language::PtBr => text.replace('.', ","),
        Language::EnUs => text,
    }
}

/// The seconds per search shown with one decimal.
fn seconds_text(i18n: &I18n, movetime_ms: u32) -> String {
    decimal_text(i18n, f64::from(movetime_ms) / 1000.0)
}

/// The time per search after one step: 0.1 s below one second, 1 s above
/// (like the annotation dialog).
fn step_ms(ms: u32, up: bool) -> u32 {
    let step = if ms < 1000 || (!up && ms == 1000) {
        100
    } else {
        1000
    };
    if up {
        ms.saturating_add(step)
    } else {
        ms.saturating_sub(step)
    }
}

fn engine_section<'a>(app: &AppState, engine: Option<RunId>, i18n: &I18n) -> Element<'a, Message> {
    let set = |run: Option<RunId>| send(ReportMessage::ConfigEngine(run));
    let mut items = vec![choice(
        i18n.tr("annotate-engine-default"),
        None,
        engine,
        true,
        set,
    )];
    for run in app.engine_runs() {
        if run.state != EngineRunState::Running {
            continue;
        }
        items.push(choice(
            engine_name(app, run.id),
            Some(run.id),
            engine,
            true,
            set,
        ));
    }
    labelled(i18n.tr("annotate-engine"), group(items))
}

fn effort_section<'a>(config: &ReportConfig, i18n: &I18n) -> Element<'a, Message> {
    let c = *config;
    let with = move |c: ReportConfig| send(ReportMessage::ConfigSet(c));
    column![
        labelled(
            i18n.tr("report-config-limit-label"),
            group(vec![
                choice(
                    i18n.tr("annotate-control-depth"),
                    false,
                    c.by_time,
                    true,
                    move |by_time| with(ReportConfig { by_time, ..c }),
                ),
                choice(
                    i18n.tr("annotate-control-time"),
                    true,
                    c.by_time,
                    true,
                    move |by_time| with(ReportConfig { by_time, ..c }),
                ),
            ]),
        ),
        stepper(
            i18n.tr("annotate-depth"),
            c.depth.to_string(),
            with(ReportConfig {
                depth: c.depth.saturating_sub(1),
                ..c
            }),
            with(ReportConfig {
                depth: c.depth.saturating_add(1),
                ..c
            }),
            !c.by_time,
        ),
        stepper(
            i18n.tr("annotate-seconds"),
            seconds_text(i18n, c.movetime_ms),
            with(ReportConfig {
                movetime_ms: step_ms(c.movetime_ms, false),
                ..c
            }),
            with(ReportConfig {
                movetime_ms: step_ms(c.movetime_ms, true),
                ..c
            }),
            c.by_time,
        ),
    ]
    .spacing(6)
    .into()
}

fn limit_stepper<'a>(
    i18n: &I18n,
    class: MoveClass,
    tenths: u16,
    step: u16,
    set: impl Fn(u16) -> ReportConfig,
) -> Element<'a, Message> {
    let mut args = FluentArgs::new();
    args.set("class", i18n.tr(class.name_key()));
    stepper(
        i18n.tr_args("report-config-limit", &args),
        decimal_text(i18n, f64::from(tenths) / 10.0),
        send(ReportMessage::ConfigSet(set(tenths.saturating_sub(step)))),
        send(ReportMessage::ConfigSet(set(tenths.saturating_add(step)))),
        true,
    )
}

fn limits_section<'a>(config: &ReportConfig, i18n: &I18n) -> Element<'a, Message> {
    let c = *config;
    let mut args = FluentArgs::new();
    args.set("class", i18n.tr(MoveClass::Blunder.name_key()));
    column![
        text(i18n.tr("report-config-limits")).size(13),
        limit_stepper(i18n, MoveClass::Best, c.best_tenths, 1, move |v| {
            ReportConfig {
                best_tenths: v,
                ..c
            }
        }),
        limit_stepper(
            i18n,
            MoveClass::Excellent,
            c.excellent_tenths,
            1,
            move |v| {
                ReportConfig {
                    excellent_tenths: v,
                    ..c
                }
            }
        ),
        limit_stepper(i18n, MoveClass::Good, c.good_tenths, 5, move |v| {
            ReportConfig {
                good_tenths: v,
                ..c
            }
        }),
        limit_stepper(
            i18n,
            MoveClass::Inaccuracy,
            c.inaccuracy_tenths,
            5,
            move |v| {
                ReportConfig {
                    inaccuracy_tenths: v,
                    ..c
                }
            }
        ),
        limit_stepper(i18n, MoveClass::Mistake, c.mistake_tenths, 5, move |v| {
            ReportConfig {
                mistake_tenths: v,
                ..c
            }
        }),
        text(i18n.tr_args("report-config-last", &args))
            .size(12)
            .style(theme::muted_text),
        text(i18n.tr("report-config-note"))
            .size(12)
            .style(theme::muted_text),
    ]
    .spacing(6)
    .into()
}

/// The box, when the application has the dialog open.
pub fn view<'a>(app: &'a AppState, i18n: &I18n) -> Option<Element<'a, Message>> {
    let dialog = app.report.config_dialog?;
    let c = dialog.config;
    let body = column![
        engine_section(app, dialog.engine, i18n),
        effort_section(&c, i18n),
        limits_section(&c, i18n),
        check(
            i18n.tr("report-config-marks"),
            c.board_marks,
            true,
            move |on| {
                send(ReportMessage::ConfigSet(ReportConfig {
                    board_marks: on,
                    ..c
                }))
            }
        ),
    ]
    .spacing(12);
    Some(modal(Card {
        id: REPORT_CONFIG_BOX_ID,
        title: i18n.tr("report-config-title"),
        body: container(scrollable(body.padding([0, 12])).height(Length::Fixed(BODY_HEIGHT)))
            .into(),
        buttons: vec![
            button(text(i18n.tr("report-config-defaults")))
                .style(button::text)
                .on_press(send(ReportMessage::ConfigDefaults))
                .into(),
            button(text(i18n.tr("dialog-cancel")))
                .style(button::secondary)
                .on_press(send(ReportMessage::CloseConfig))
                .into(),
            button(text(i18n.tr("report-generate")))
                .style(button::primary)
                .on_press(send(ReportMessage::ConfigStart))
                .into(),
        ],
        width: 640.0,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_time_steps_by_tenths_below_a_second_and_by_seconds_above() {
        assert_eq!(step_ms(500, true), 600);
        assert_eq!(step_ms(1000, false), 900);
        assert_eq!(step_ms(1000, true), 2000);
        assert_eq!(step_ms(2000, false), 1000);
        assert_eq!(step_ms(0, false), 0);
    }

    #[test]
    fn the_seconds_are_shown_with_one_decimal() {
        let i18n = I18n::new(Language::EnUs);
        assert_eq!(seconds_text(&i18n, 2000), "2.0");
        assert_eq!(seconds_text(&i18n, 1500), "1.5");
        let pt = I18n::new(Language::PtBr);
        assert_eq!(seconds_text(&pt, 1500), "1,5");
        assert_eq!(decimal_text(&pt, 0.5), "0,5");
    }
}
