//! The game report panel (spec M4-10b, D1421, D1433, D1436): the depth
//! presets and "Generate report", the progress, the accuracy of each side,
//! the count of moves per class, the curve with its markers and the list of
//! key moments. The data comes from [`mychess_app::AppState::report_data`]; this module
//! only lays it out.

use fluent_bundle::FluentArgs;
use iced::widget::{Column, button, checkbox, column, container, row, scrollable, text, tooltip};
use iced::{Alignment, Element, Length};
use mychess_app::report::{
    MoveClass, ReportData, ReportMessage, ReportPreset, Review, ReviewStep, SideFilter, key_moments,
};
use mychess_core::Color as Side;

use super::annotate_dialog::choice;
use super::graph::{move_label, report_curve};
use super::panes;
use crate::i18n::{I18n, Language};
use crate::shell::{Message, Shell};
use crate::theme::report_colors::{class_color, class_glyph};
use crate::theme::{self, Tokens};

/// Height of the list of moments.
const LIST_HEIGHT: f32 = 150.0;

fn send(message: ReportMessage) -> Message {
    Message::App(mychess_app::Message::Report(message))
}

/// An accuracy with one decimal, in the decimal mark of the language
/// (`73.6` or `73,6`); `-` when the side has no move to measure.
pub(crate) fn accuracy_text(i18n: &I18n, accuracy: Option<f32>) -> String {
    let Some(value) = accuracy else {
        return "-".to_owned();
    };
    let text = format!("{value:.1}");
    match i18n.language() {
        Language::PtBr => text.replace('.', ","),
        Language::EnUs => text,
    }
}

/// A duration as `m:ss`, or `h:mm:ss` from an hour on (the same in every
/// language, like a clock).
pub(crate) fn clock_text(seconds: u32) -> String {
    let (hours, minutes, secs) = (seconds / 3600, seconds / 60 % 60, seconds % 60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{secs:02}")
    } else {
        format!("{minutes}:{secs:02}")
    }
}

/// The text of the status bar while the report is analysed, with the time
/// left once the pace is known (D2121).
pub(crate) fn progress_text(
    i18n: &I18n,
    progress: &mychess_app::AnnotationProgress,
    eta: Option<u32>,
) -> String {
    if progress.total_plies == 0 {
        return i18n.tr("report-progress-starting");
    }
    let mut args = FluentArgs::new();
    args.set(
        "done",
        i64::from(
            progress
                .done_plies
                .saturating_add(1)
                .min(progress.total_plies),
        ),
    );
    args.set("total", i64::from(progress.total_plies));
    match eta {
        Some(seconds) => {
            args.set("time", clock_text(seconds));
            i18n.tr_args("report-progress-eta", &args)
        }
        None => i18n.tr_args("report-progress", &args),
    }
}

/// The class name, translated.
fn class_name(i18n: &I18n, class: MoveClass) -> String {
    i18n.tr(class.name_key())
}

fn class_tag<'a>(class: MoveClass) -> Element<'a, Message> {
    text(class_glyph(class))
        .size(13)
        .width(Length::Fixed(24.0))
        .style(move |theme| text::Style {
            color: Some(class_color(&Tokens::of(theme), class)),
        })
        .into()
}

fn section<'a>(label: String) -> Element<'a, Message> {
    text(label).size(12).style(theme::muted_text).into()
}

/// The box of the accuracy of one side; the side of the user is framed and
/// named "(you)" (D2115): the frame and the word, not only the hue.
fn accuracy_box<'a>(
    i18n: &I18n,
    side: Side,
    accuracy: Option<f32>,
    mine: bool,
) -> Element<'a, Message> {
    let mut name = match side {
        Side::White => i18n.tr("report-side-white"),
        Side::Black => i18n.tr("report-side-black"),
    };
    if mine {
        let mut args = FluentArgs::new();
        args.set("side", name);
        name = i18n.tr_args("report-side-you", &args);
    }
    container(
        column![
            text(name).size(12).style(theme::muted_text),
            text(accuracy_text(i18n, accuracy)).size(26),
            text(i18n.tr("report-accuracy"))
                .size(11)
                .style(theme::muted_text),
        ]
        .spacing(2)
        .align_x(Alignment::Center),
    )
    .padding([8, 14])
    .width(Length::FillPortion(1))
    .center_x(Length::FillPortion(1))
    .id(if mine {
        "report-accuracy-mine"
    } else {
        "report-accuracy-other"
    })
    .style(if mine {
        container::bordered_box
    } else {
        container::rounded_box
    })
    .into()
}

fn counts_table<'a>(
    i18n: &I18n,
    data: &ReportData,
    filter: Option<MoveClass>,
) -> Element<'a, Message> {
    let mut table = Column::new().spacing(2);
    table = table.push(
        row![
            text("").width(Length::Fixed(24.0)),
            text("").width(Length::Fill),
            text(i18n.tr("report-side-white"))
                .size(11)
                .width(Length::Fixed(56.0))
                .style(theme::muted_text),
            text(i18n.tr("report-side-black"))
                .size(11)
                .width(Length::Fixed(56.0))
                .style(theme::muted_text),
        ]
        .spacing(6),
    );
    for class in MoveClass::ALL {
        let (white, black) = (
            data.report.white.count(class),
            data.report.black.count(class),
        );
        let name = class_name(i18n, class);
        let label = if white + black == 0 {
            text(name).size(13).style(theme::muted_text)
        } else {
            text(name).size(13)
        };
        let selected = filter == Some(class);
        let line = row![
            class_tag(class),
            label.width(Length::Fill),
            text(white.to_string()).size(13).width(Length::Fixed(56.0)),
            text(black.to_string()).size(13).width(Length::Fixed(56.0)),
        ]
        .spacing(6)
        .align_y(Alignment::Center);
        let target = if selected { None } else { Some(class) };
        table = table.push(
            button(line)
                .padding([2, 4])
                .width(Length::Fill)
                .style(if selected {
                    button::primary
                } else {
                    button::text
                })
                .on_press(send(ReportMessage::Filter(target))),
        );
    }
    table.into()
}

fn moments<'a>(i18n: &I18n, data: &ReportData, filter: Option<MoveClass>) -> Element<'a, Message> {
    let plies = data.moments(filter);
    if plies.is_empty() {
        return text(i18n.tr("report-no-moments"))
            .size(12)
            .style(theme::muted_text)
            .into();
    }
    let mut list = Column::new().spacing(1);
    for ply in plies {
        let Some(verdict) = ply.checked_sub(1).and_then(|i| data.report.moves.get(i)) else {
            continue;
        };
        let san = data.sans.get(ply - 1).cloned().unwrap_or_default();
        let mut args = FluentArgs::new();
        args.set("loss", format!("{:.0}", verdict.loss));
        let loss = if verdict.loss >= 0.5 {
            i18n.tr_args("report-moment-loss", &args)
        } else {
            String::new()
        };
        let line = row![
            class_tag(verdict.class),
            text(format!("{} {san}", move_label(ply)))
                .size(13)
                .width(Length::Fill),
            text(loss).size(12).style(theme::muted_text),
        ]
        .spacing(6)
        .align_y(Alignment::Center);
        list = list.push(
            button(line)
                .padding([2, 4])
                .width(Length::Fill)
                .style(button::text)
                .on_press(send(ReportMessage::GoTo(ply))),
        );
    }
    scrollable(list)
        .height(Length::Fixed(LIST_HEIGHT))
        .width(Length::Fill)
        .into()
}

/// The ply of the cursor on the main line, from 1; `None` at the start or
/// off the main line.
fn cursor_ply(shell: &Shell) -> Option<usize> {
    let session = &shell.app.session;
    let cursor = session.cursor();
    session
        .game()
        .mainline()
        .position(|node| node == cursor)
        .map(|i| i + 1)
}

/// The depth presets; one is pressed only while the saved depth is exactly
/// its own (the dialog may have set another depth or a time limit, D2100).
fn preset_buttons<'a>(shell: &Shell, busy: bool) -> Element<'a, Message> {
    let i18n = &shell.i18n;
    let config = &shell.app.prefs.report_config;
    let preset = (!config.by_time)
        .then(|| ReportPreset::nearest(config.depth))
        .filter(|p| p.depth() == config.depth);
    let mut presets = row![].spacing(4).align_y(Alignment::Center);
    for p in ReportPreset::ALL {
        presets = presets.push(choice(
            i18n.tr(p.label_key()),
            Some(p),
            preset,
            !busy,
            |p: Option<ReportPreset>| send(ReportMessage::SetPreset(p.unwrap_or_default())),
        ));
    }
    presets.into()
}

/// "Continue analysis", with its tip.
fn continue_button<'a>(i18n: &I18n) -> Element<'a, Message> {
    container(tooltip(
        button(text(i18n.tr("report-continue")).size(13))
            .padding([3, 10])
            .style(button::secondary)
            .on_press(send(ReportMessage::Continue)),
        container(text(i18n.tr("report-continue-tip")).size(12))
            .padding([4, 8])
            .style(theme::tooltip_box),
        tooltip::Position::Bottom,
    ))
    .id("report-continue")
    .into()
}

fn controls<'a>(shell: &Shell, has_data: bool, resumable: bool) -> Element<'a, Message> {
    let i18n = &shell.i18n;
    let running = shell.app.report_running();
    let busy = shell.app.annotation_running();
    let presets = preset_buttons(shell, busy);
    let has_moves = shell.app.session.game().mainline().next().is_some();
    let generate_label = if has_data {
        i18n.tr("report-regenerate")
    } else {
        i18n.tr("report-generate")
    };
    let generate = container(
        button(text(generate_label).size(13))
            .padding([3, 10])
            .style(button::primary)
            .on_press_maybe((!busy && has_moves).then(|| send(ReportMessage::Generate))),
    )
    .id("report-generate");
    let configure = container(
        button(text(i18n.tr("report-configure")).size(13))
            .padding([3, 10])
            .style(button::secondary)
            .on_press_maybe((!busy).then(|| send(ReportMessage::OpenConfig))),
    )
    .id("report-configure");
    let mut line = row![presets, generate, configure]
        .spacing(10)
        .align_y(Alignment::Center);
    // A stopped run is taken up again where it ended (D2110).
    if !busy && resumable {
        line = line.push(continue_button(i18n));
    }
    if running {
        line = line.push(
            container(
                button(text(i18n.tr("annotate-stop")).size(13))
                    .padding([3, 10])
                    .style(button::secondary)
                    .on_press(send(ReportMessage::Cancel)),
            )
            .id("report-cancel"),
        );
    } else if has_data {
        line = line.push(
            container(
                button(text(i18n.tr("report-clear")).size(13))
                    .padding([3, 10])
                    .style(button::secondary)
                    .on_press(send(ReportMessage::Clear)),
            )
            .id("report-clear"),
        );
    }
    if !busy {
        let tip = |label: String, tip: String, message: ReportMessage| {
            tooltip(
                button(text(label).size(13))
                    .padding([3, 10])
                    .style(button::secondary)
                    .on_press(send(message)),
                container(text(tip).size(12))
                    .padding([4, 8])
                    .style(theme::tooltip_box),
                tooltip::Position::Bottom,
            )
        };
        if has_data {
            line = line.push(tip(
                i18n.tr("report-write"),
                i18n.tr("report-write-tip"),
                ReportMessage::WriteToGame,
            ));
        } else if has_moves {
            line = line.push(tip(
                i18n.tr("report-from-game"),
                i18n.tr("report-from-game-tip"),
                ReportMessage::FromGame,
            ));
        }
    }
    line.wrap().into()
}

fn small_button<'a>(label: String, message: Option<ReportMessage>) -> Element<'a, Message> {
    button(text(label).size(13))
        .padding([3, 10])
        .style(button::secondary)
        .on_press_maybe(message.map(send))
        .into()
}

/// The buttons that start a review, one per side (D1435); the one of the
/// user's own side stands out (D2115).
fn review_start<'a>(i18n: &I18n, data: &ReportData, user: Option<Side>) -> Element<'a, Message> {
    let mut line = row![text(i18n.tr("report-review-start")).size(13)]
        .spacing(8)
        .align_y(Alignment::Center);
    for (key, id, side) in [
        (
            "report-side-white",
            "report-review-white",
            SideFilter::White,
        ),
        (
            "report-side-black",
            "report-review-black",
            SideFilter::Black,
        ),
        ("report-review-both", "report-review-both", SideFilter::Both),
    ] {
        let any = !key_moments(&data.report, side, false).is_empty();
        let mine = matches!(
            (side, user),
            (SideFilter::White, Some(Side::White)) | (SideFilter::Black, Some(Side::Black))
        );
        let message = any.then_some(ReportMessage::StartReview {
            side,
            highlights: false,
        });
        let element: Element<'a, Message> = if mine {
            button(text(i18n.tr(key)).size(13))
                .padding([3, 10])
                .style(button::primary)
                .on_press_maybe(message.map(send))
                .into()
        } else {
            small_button(i18n.tr(key), message)
        };
        line = line.push(container(element).id(id));
    }
    line.wrap().into()
}

/// The sentence of the moment on show, from facts only (D1448).
fn review_sentence(shell: &Shell, data: &ReportData, review: &Review) -> Option<String> {
    let i18n = &shell.i18n;
    let moment = review.current()?;
    let verdict = data.report.moves.get(moment.ply.checked_sub(1)?)?;
    let game = shell.app.session.game();
    let best = data.best_san(game, moment.ply);
    let mut args = FluentArgs::new();
    args.set(
        "played",
        data.sans.get(moment.ply - 1).cloned().unwrap_or_default(),
    );
    args.set("loss", format!("{:.0}", verdict.loss));
    args.set("class", class_name(i18n, verdict.class));
    if let Some(best) = &best {
        args.set("best", best.clone());
    }
    let key = match review.step {
        ReviewStep::Looking if best.is_some() => "report-review-looking-best",
        ReviewStep::Trying if review.hint_due() => {
            let from = verdict
                .best
                .and_then(mychess_core::Move::from)
                .map(|sq| sq.to_string());
            args.set("square", from.unwrap_or_default());
            "report-review-hint"
        }
        ReviewStep::Trying => "report-review-trying",
        ReviewStep::Solved => "report-review-solved",
        ReviewStep::Revealed if best.is_some() => "report-review-revealed",
        // Without a known best move there is only the verdict to repeat.
        ReviewStep::Looking | ReviewStep::Revealed => "report-review-looking",
    };
    Some(i18n.tr_args(key, &args))
}

/// The card of the review in progress.
fn review_card<'a>(
    shell: &'a Shell,
    data: &'a ReportData,
    review: &'a Review,
) -> Element<'a, Message> {
    let i18n = &shell.i18n;
    let mut args = FluentArgs::new();
    args.set("index", i64::try_from(review.index + 1).unwrap_or(i64::MAX));
    args.set(
        "total",
        i64::try_from(review.moments.len()).unwrap_or(i64::MAX),
    );
    let class = review
        .current()
        .and_then(|m| data.report.moves.get(m.ply.checked_sub(1)?))
        .map(|v| v.class);
    let mut head = row![text(i18n.tr_args("report-review-title", &args)).size(13)]
        .spacing(8)
        .align_y(Alignment::Center);
    if let Some(class) = class {
        head = head.push(class_tag(class));
        head = head.push(text(class_name(i18n, class)).size(13));
    }
    let known_best = review
        .current()
        .and_then(|m| data.report.moves.get(m.ply.checked_sub(1)?))
        .is_some_and(|v| v.best.is_some());
    let trying = review.step == ReviewStep::Trying;
    let buttons = row![
        small_button(
            i18n.tr("report-review-try"),
            (known_best && !trying).then_some(ReportMessage::ReviewTry)
        ),
        small_button(
            i18n.tr("report-review-show-best"),
            known_best.then_some(ReportMessage::ReviewShowBest)
        ),
        small_button(
            i18n.tr("report-review-previous"),
            (review.index > 0).then_some(ReportMessage::ReviewPrevious)
        ),
        small_button(
            i18n.tr("report-review-next"),
            (review.index + 1 < review.moments.len()).then_some(ReportMessage::ReviewNext)
        ),
        small_button(
            i18n.tr("report-review-exit"),
            Some(ReportMessage::ReviewExit)
        ),
    ]
    .spacing(6);
    let arrow_box = checkbox(shell.app.prefs.report.review_arrow)
        .label(i18n.tr("report-review-arrow"))
        .size(16)
        .text_size(13)
        .on_toggle(move |on| {
            Message::App(crate::app_api::set_report_prefs(
                mychess_app::report::ReportPrefs { review_arrow: on },
            ))
        });
    container(
        column![
            head,
            text(review_sentence(shell, data, review).unwrap_or_default()).size(13),
            buttons.wrap(),
            arrow_box,
            text(i18n.tr("report-review-keys"))
                .size(11)
                .style(theme::muted_text),
        ]
        .spacing(6),
    )
    .id("report-review")
    .padding(8)
    .width(Length::Fill)
    .style(container::rounded_box)
    .into()
}

/// "I played: White / Black / Not set": asks for the side when the names
/// of the game did not tell it (D2115).
fn user_side_row<'a>(i18n: &I18n, user: Option<Side>) -> Element<'a, Message> {
    let mut line = row![text(i18n.tr("report-user-side")).size(13)]
        .spacing(6)
        .align_y(Alignment::Center);
    for (key, side) in [
        ("report-side-white", Some(Side::White)),
        ("report-side-black", Some(Side::Black)),
        ("report-user-none", None),
    ] {
        line = line.push(choice(
            i18n.tr(key),
            side,
            user,
            true,
            |side: Option<Side>| send(ReportMessage::SetUserSide(side)),
        ));
    }
    let mut column = Column::new().spacing(4).push(line.wrap());
    if user.is_none() {
        column = column.push(
            text(i18n.tr("report-user-ask"))
                .size(11)
                .style(theme::muted_text),
        );
    }
    container(column).id("report-user-side").into()
}

fn report_view<'a>(shell: &'a Shell, data: &'a ReportData) -> Element<'a, Message> {
    let i18n = &shell.i18n;
    let mut body = Column::new().spacing(10);
    if !data.complete {
        let mut args = FluentArgs::new();
        args.set(
            "done",
            i64::try_from(data.report.moves.len()).unwrap_or(i64::MAX),
        );
        args.set("total", i64::try_from(data.plies).unwrap_or(i64::MAX));
        body = body.push(
            text(i18n.tr_args("report-partial", &args))
                .size(12)
                .style(theme::muted_text),
        );
    }
    let user = shell.app.report.user_side;
    body = body.push(user_side_row(i18n, user));
    body = body.push(
        row![
            accuracy_box(
                i18n,
                Side::White,
                data.report.white.accuracy,
                user == Some(Side::White)
            ),
            accuracy_box(
                i18n,
                Side::Black,
                data.report.black.accuracy,
                user == Some(Side::Black)
            ),
        ]
        .spacing(10),
    );
    body = match shell.app.report.review.as_ref() {
        Some(review) => body.push(review_card(shell, data, review)),
        None => body.push(review_start(i18n, data, user)),
    };
    body = body.push(report_curve(
        &data.graph,
        &shell.ui.report_graph,
        cursor_ply(shell),
        i18n,
    ));
    body = body.push(counts_table(i18n, data, shell.app.report.filter));
    let key = if shell.app.report.filter.is_some() {
        "report-moments-filtered"
    } else {
        "report-moments"
    };
    body = body.push(section(i18n.tr(key)));
    body = body.push(moments(i18n, data, shell.app.report.filter));
    body = body.push(
        text(i18n.tr("report-estimate-note"))
            .size(11)
            .style(theme::muted_text),
    );
    scrollable(body.padding([0, 4])).height(Length::Fill).into()
}

/// The report panel.
pub fn view(shell: &Shell) -> Element<'_, Message> {
    let i18n = &shell.i18n;
    let data = shell.app.report_data();
    let mut page = Column::new().spacing(10).padding(8);
    page = page.push(controls(
        shell,
        data.is_some(),
        data.is_some_and(ReportData::resumable),
    ));
    if shell.app.report_running() {
        page = page.push(
            text(i18n.tr("report-running"))
                .size(12)
                .style(theme::muted_text),
        );
    }
    match data {
        Some(data) => page = page.push(report_view(shell, data)),
        None if !shell.app.report_running() => {
            page = page.push(
                text(i18n.tr("report-empty"))
                    .size(13)
                    .style(theme::muted_text),
            );
        }
        None => {}
    }
    panes::body(page, "report-pane")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_empty_panel_offers_to_generate_and_the_pane_toggles() {
        let mut shell = Shell::new(None);
        let label = shell.i18n.tr("report-generate");
        let mut ui = iced_test::simulator(view(&shell));
        assert!(ui.find(label.as_str()).is_ok(), "the Generate button");
        drop(ui);
        assert!(!shell.ui.is_open(crate::windows::layout::PaneKind::Report));
        drop(
            shell.update(Message::Ui(crate::shell::UiMessage::TogglePane(
                crate::windows::layout::PaneKind::Report,
            ))),
        );
        assert!(shell.ui.is_open(crate::windows::layout::PaneKind::Report));
    }

    #[test]
    fn accuracy_uses_one_decimal_and_the_decimal_mark_of_the_language() {
        let en = I18n::new(Language::EnUs);
        let pt = I18n::new(Language::PtBr);
        assert_eq!(accuracy_text(&en, Some(73.64)), "73.6");
        assert_eq!(accuracy_text(&pt, Some(73.64)), "73,6");
        assert_eq!(accuracy_text(&en, None), "-");
    }

    #[test]
    fn the_clock_text_is_minutes_and_seconds_then_hours() {
        assert_eq!(clock_text(0), "0:00");
        assert_eq!(clock_text(75), "1:15");
        assert_eq!(clock_text(3600 + 5 * 60 + 9), "1:05:09");
    }
}
