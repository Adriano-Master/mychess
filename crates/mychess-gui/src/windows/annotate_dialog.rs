//! The "Configure annotation" dialog and the progress of a run (spec M4-6,
//! D461-D480, D910-D914).
//!
//! The application keeps the options being edited (`AppState::annotate_dialog`);
//! every control sends `AnnotateMessage::SetOptions` with the new value and
//! the application clamps it. The dialog disables what the engine cannot do
//! (`AppState::annotate_caps`, D477): an XBoard engine takes only a time
//! limit, no missed mates and no exercises. The book is shown disabled until
//! the book crate exists (M5).

use fluent_bundle::FluentArgs;
use iced::widget::{button, checkbox, column, container, row, scrollable, text};
use iced::{Alignment, Element, Length};
use mychess_app::engines::{EngineRunState, RunId};
use mychess_app::{
    AnnotateCaps, AnnotateDialog, AnnotateMessage, AnnotateNotice, AnnotateOptions, AnnotateSide,
    AppState, Control, DEFAULT_OPENING_MOVES, Informant, Message as AppMessage, ScoreFormat,
    WithScore, WithVars,
};

use crate::i18n::I18n;
use crate::shell::Message;
use crate::theme::{self, ButtonLook};
use crate::widgets::modal::{Card, modal};
use crate::windows::dialog_ui::DialogMessage;

/// Id of the box.
pub const ANNOTATE_BOX_ID: &str = "annotate-box";

const BODY_HEIGHT: f32 = 460.0;
pub(crate) const LABEL_WIDTH: f32 = 200.0;

fn send(message: AnnotateMessage) -> Message {
    Message::App(AppMessage::Annotate(message))
}

// ---- pure helpers ----

/// `value` moved by `steps` tenths, rounded to one decimal (the thresholds
/// move by 0.1; the cut-off by 0.5 is five of them).
pub(crate) fn step_tenths(value: f32, steps: i32) -> f32 {
    // Thresholds are a few pawns: far from the limits of the conversions.
    #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
    let tenths = (f64::from(value) * 10.0).round() as i64 + i64::from(steps);
    #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
    let moved = (tenths as f64 / 10.0) as f32;
    moved
}

/// The time per move after one step: 0.1 s below one second, 1 s above.
pub(crate) fn step_movetime(current: std::time::Duration, up: bool) -> std::time::Duration {
    let ms = u64::try_from(current.as_millis()).unwrap_or(u64::MAX);
    let step = if ms < 1000 || (!up && ms == 1000) {
        100
    } else {
        1000
    };
    let moved = if up {
        ms.saturating_add(step)
    } else {
        ms.saturating_sub(step)
    };
    std::time::Duration::from_millis(moved)
}

/// The options with the control the engine can really use: an engine
/// without a depth limit only takes time (D477).
pub(crate) fn effective_control(options: &AnnotateOptions, caps: AnnotateCaps) -> Control {
    if caps.depth {
        options.control
    } else {
        Control::Time
    }
}

/// The text of the progress in the status bar ("Annotating... move N of M").
pub fn progress_text(i18n: &I18n, progress: &mychess_app::AnnotationProgress) -> String {
    let mut args = FluentArgs::new();
    let batch = progress.game_total > 1;
    args.set("game", i64::from(progress.game_index));
    args.set("games", i64::from(progress.game_total));
    if progress.total_plies == 0 {
        return i18n.tr_args(
            if batch {
                "annotate-progress-batch-starting"
            } else {
                "annotate-progress-starting"
            },
            &args,
        );
    }
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
    let key = if batch {
        "annotate-progress-batch"
    } else {
        "annotate-progress"
    };
    i18n.tr_args(key, &args)
}

/// The fraction done, for the progress bar.
pub fn progress_fraction(progress: &mychess_app::AnnotationProgress) -> Option<f32> {
    if progress.total_plies == 0 {
        return None;
    }
    // Move counts of one game: exact in an `f32`.
    #[allow(clippy::cast_precision_loss)]
    let fraction = progress.done_plies as f32 / progress.total_plies as f32;
    Some(fraction.clamp(0.0, 1.0))
}

/// The translated text of a remark of the annotation.
pub fn notice_text(i18n: &I18n, notice: &AnnotateNotice) -> String {
    let mut args = FluentArgs::new();
    match notice {
        AnnotateNotice::Finished { plies, variations } => {
            args.set("plies", i64::from(*plies));
            args.set("variations", i64::from(*variations));
        }
        AnnotateNotice::BatchFinished { games, skipped } => {
            args.set("games", i64::from(*games));
            args.set("skipped", i64::from(*skipped));
        }
        AnnotateNotice::ReportWritten { moves, variations } => {
            args.set("moves", i64::from(*moves));
            args.set("variations", i64::from(*variations));
        }
        AnnotateNotice::ReportStopped { plies, total } => {
            args.set("plies", i64::from(*plies));
            args.set("total", i64::from(*total));
        }
        AnnotateNotice::ReportKept { kept, plies } => {
            args.set("kept", i64::from(*kept));
            args.set("plies", i64::from(*plies));
        }
        AnnotateNotice::BatchStopped { games, total } => {
            args.set("games", i64::from(*games));
            args.set("total", i64::from(*total));
        }
        AnnotateNotice::Cancelled { plies }
        | AnnotateNotice::GameChanged { plies }
        | AnnotateNotice::EngineFailed { plies }
        | AnnotateNotice::NoScore { plies } => args.set("plies", i64::from(*plies)),
        _ => {}
    }
    i18n.tr_args(notice.message_key(), &args)
}

// ---- small widgets ----

pub(crate) fn muted_if<'a>(label: String, enabled: bool) -> iced::widget::Text<'a> {
    let t = text(label).size(13);
    if enabled {
        t
    } else {
        t.style(theme::muted_text)
    }
}

/// A button of a group of exclusive choices: pressed when it is the current
/// value.
pub(crate) fn choice<'a, T: Copy + PartialEq + 'a>(
    label: String,
    value: T,
    current: T,
    enabled: bool,
    on: impl Fn(T) -> Message,
) -> Element<'a, Message> {
    button(muted_if(label, enabled))
        .padding([3, 8])
        .style(theme::command_button(
            ButtonLook::Secondary,
            value == current,
        ))
        .on_press_maybe(enabled.then(|| on(value)))
        .into()
}

/// `label [-] value [+]`.
pub(crate) fn stepper<'a>(
    label: String,
    shown: String,
    down: Message,
    up: Message,
    enabled: bool,
) -> Element<'a, Message> {
    let step = |glyph: &'static str, message: Message| {
        button(text(glyph).size(13))
            .padding([2, 10])
            .style(button::secondary)
            .on_press_maybe(enabled.then_some(message))
    };
    row![
        muted_if(label, enabled).width(Length::Fixed(LABEL_WIDTH)),
        step("\u{2212}", down),
        muted_if(shown, enabled).width(Length::Fixed(56.0)),
        step("+", up),
    ]
    .spacing(6)
    .align_y(Alignment::Center)
    .into()
}

pub(crate) fn check<'a>(
    label: String,
    on: bool,
    enabled: bool,
    change: impl Fn(bool) -> Message + 'a,
) -> Element<'a, Message> {
    checkbox(on)
        .label(label)
        .size(15)
        .text_size(13)
        .on_toggle_maybe(enabled.then_some(change))
        .into()
}

pub(crate) fn labelled(label: String, body: Element<'_, Message>) -> Element<'_, Message> {
    row![text(label).size(13).width(Length::Fixed(LABEL_WIDTH)), body]
        .spacing(6)
        .align_y(Alignment::Center)
        .into()
}

pub(crate) fn group(items: Vec<Element<'_, Message>>) -> Element<'_, Message> {
    row(items).spacing(4).wrap().into()
}

pub(crate) fn engine_name(app: &AppState, run: RunId) -> String {
    let Some(run) = app.engine_runs().iter().find(|r| r.id == run) else {
        return "?".to_owned();
    };
    run.identity
        .as_ref()
        .map(|i| i.name.clone())
        .or_else(|| app.engines.get(run.engine).map(|e| e.name.clone()))
        .unwrap_or_else(|| "?".to_owned())
}

// ---- the sections of the form ----

fn engine_section<'a>(
    app: &AppState,
    dialog: &AnnotateDialog,
    i18n: &I18n,
) -> Element<'a, Message> {
    let mut items = vec![choice(
        i18n.tr("annotate-engine-default"),
        None,
        dialog.engine,
        true,
        |run: Option<RunId>| send(AnnotateMessage::SetEngine(run)),
    )];
    for run in app.engine_runs() {
        if run.state != EngineRunState::Running {
            continue;
        }
        items.push(choice(
            engine_name(app, run.id),
            Some(run.id),
            dialog.engine,
            true,
            |run: Option<RunId>| send(AnnotateMessage::SetEngine(run)),
        ));
    }
    labelled(i18n.tr("annotate-engine"), group(items))
}

fn control_section<'a>(
    options: &AnnotateOptions,
    caps: AnnotateCaps,
    i18n: &I18n,
) -> Element<'a, Message> {
    let control = effective_control(options, caps);
    let set = move |control: Control| {
        send(AnnotateMessage::SetOptions(AnnotateOptions {
            control,
            ..*options
        }))
    };
    let with = |o: AnnotateOptions| send(AnnotateMessage::SetOptions(o.normalized()));
    let depth_on = control == Control::Depth;
    let seconds = options.movetime.as_secs_f64();
    let mut page = column![
        labelled(
            i18n.tr("annotate-control"),
            group(vec![
                choice(
                    i18n.tr("annotate-control-depth"),
                    Control::Depth,
                    control,
                    caps.depth,
                    set,
                ),
                choice(
                    i18n.tr("annotate-control-time"),
                    Control::Time,
                    control,
                    true,
                    set,
                ),
            ]),
        ),
        stepper(
            i18n.tr("annotate-depth"),
            options.depth.to_string(),
            with(AnnotateOptions {
                depth: options.depth.saturating_sub(1),
                ..*options
            }),
            with(AnnotateOptions {
                depth: options.depth.saturating_add(1),
                ..*options
            }),
            depth_on,
        ),
        stepper(
            i18n.tr("annotate-seconds"),
            format!("{seconds:.1}"),
            with(AnnotateOptions {
                movetime: step_movetime(options.movetime, false),
                ..*options
            }),
            with(AnnotateOptions {
                movetime: step_movetime(options.movetime, true),
                ..*options
            }),
            !depth_on,
        ),
    ]
    .spacing(6);
    if !caps.depth {
        page = page.push(
            text(i18n.tr("annotate-xboard-note"))
                .size(12)
                .style(theme::muted_text),
        );
    }
    page.into()
}

fn thresholds_section<'a>(options: &AnnotateOptions, i18n: &I18n) -> Element<'a, Message> {
    let with = |o: AnnotateOptions| send(AnnotateMessage::SetOptions(o.normalized()));
    column![
        stepper(
            i18n.tr("annotate-threshold"),
            format!("{:.1}", options.blunder),
            with(AnnotateOptions {
                blunder: step_tenths(options.blunder, -1),
                ..*options
            }),
            with(AnnotateOptions {
                blunder: step_tenths(options.blunder, 1),
                ..*options
            }),
            true,
        ),
        stepper(
            i18n.tr("annotate-cutoff"),
            format!("{:.1}", options.cutoff),
            with(AnnotateOptions {
                cutoff: step_tenths(options.cutoff, -5),
                ..*options
            }),
            with(AnnotateOptions {
                cutoff: step_tenths(options.cutoff, 5),
                ..*options
            }),
            true,
        ),
    ]
    .spacing(6)
    .into()
}

#[allow(clippy::too_many_lines)] // one flat list of the groups of choices
fn choices_section<'a>(
    options: &AnnotateOptions,
    caps: AnnotateCaps,
    i18n: &I18n,
) -> Element<'a, Message> {
    let o = *options;
    let score = |label: &str, value: WithScore| {
        choice(i18n.tr(label), value, o.with_score, true, move |v| {
            send(AnnotateMessage::SetOptions(AnnotateOptions {
                with_score: v,
                ..o
            }))
        })
    };
    let vars = |label: &str, value: WithVars| {
        choice(i18n.tr(label), value, o.with_vars, true, move |v| {
            send(AnnotateMessage::SetOptions(AnnotateOptions {
                with_vars: v,
                ..o
            }))
        })
    };
    let side = |label: &str, value: AnnotateSide| {
        choice(i18n.tr(label), value, o.side, true, move |v| {
            send(AnnotateMessage::SetOptions(AnnotateOptions {
                side: v,
                ..o
            }))
        })
    };
    let format = |label: &str, value: ScoreFormat| {
        choice(i18n.tr(label), value, o.score_format, true, move |v| {
            send(AnnotateMessage::SetOptions(AnnotateOptions {
                score_format: v,
                ..o
            }))
        })
    };
    let flag = |label: &str, on: bool, enabled: bool, set: fn(&mut AnnotateOptions, bool)| {
        check(i18n.tr(label), on, enabled, move |value| {
            let mut next = o;
            set(&mut next, value);
            send(AnnotateMessage::SetOptions(next))
        })
    };
    column![
        labelled(
            i18n.tr("annotate-add-score"),
            group(vec![
                score("annotate-score-all", WithScore::AllMoves),
                score("annotate-score-blunders", WithScore::Blunders),
                score("annotate-score-variations", WithScore::Variations),
                score("annotate-score-no", WithScore::No),
            ]),
        ),
        labelled(
            i18n.tr("annotate-add-vars"),
            group(vec![
                vars("annotate-vars-blunders", WithVars::Blunders),
                vars("annotate-vars-notbest", WithVars::NotBest),
                vars("annotate-vars-all", WithVars::AllMoves),
                vars("annotate-vars-no", WithVars::No),
            ]),
        ),
        row![
            flag(
                "annotate-missed-mates",
                o.missed_mates,
                caps.missed_mates,
                |o, v| {
                    o.missed_mates = v;
                }
            ),
            flag("annotate-end", o.var_at_end, true, |o, v| o.var_at_end = v),
        ]
        .spacing(20),
        labelled(
            i18n.tr("annotate-side"),
            group(vec![
                side("annotate-side-all", AnnotateSide::All),
                side("annotate-side-white", AnnotateSide::White),
                side("annotate-side-black", AnnotateSide::Black),
            ]),
        ),
        labelled(
            i18n.tr("annotate-score-format"),
            group(vec![
                format("annotate-format-plain", ScoreFormat::Plain),
                format("annotate-format-bracket", ScoreFormat::Bracket),
                format("annotate-format-eval", ScoreFormat::Eval),
            ]),
        ),
    ]
    .spacing(8)
    .into()
}

fn extras_section<'a>(
    options: &AnnotateOptions,
    caps: AnnotateCaps,
    book_available: bool,
    i18n: &I18n,
) -> Element<'a, Message> {
    let o = *options;
    let set = move |next: AnnotateOptions| send(AnnotateMessage::SetOptions(next));
    let opening = o.opening_only;
    let moves = opening.unwrap_or(DEFAULT_OPENING_MOVES);
    let moved = move |delta: i16| {
        let next = i16::from(moves).saturating_add(delta);
        set(AnnotateOptions {
            opening_only: Some(u8::try_from(next.max(0)).unwrap_or(u8::MAX)),
            ..o
        }
        .normalized())
    };
    column![
        // Needs an opening book, chosen in the book window (spec M5-6,
        // D1664).
        row![
            check(
                i18n.tr("annotate-use-book"),
                o.use_book && book_available,
                book_available,
                move |on| set(AnnotateOptions { use_book: on, ..o })
            ),
            text(if book_available {
                String::new()
            } else {
                i18n.tr("annotate-book-needs-book")
            })
            .size(12)
            .style(theme::muted_text),
        ]
        .spacing(10)
        .align_y(Alignment::Center),
        check(
            i18n.tr("annotate-opening-only"),
            opening.is_some(),
            true,
            move |on| {
                set(AnnotateOptions {
                    opening_only: on.then_some(DEFAULT_OPENING_MOVES),
                    ..o
                })
            }
        ),
        stepper(
            i18n.tr("annotate-opening-moves"),
            moves.to_string(),
            moved(-1),
            moved(1),
            opening.is_some(),
        ),
        check(i18n.tr("annotate-batch"), o.batch, true, move |on| {
            set(AnnotateOptions { batch: on, ..o })
        }),
        check(i18n.tr("annotate-add-tag"), o.add_tag, true, move |on| {
            set(AnnotateOptions { add_tag: on, ..o })
        }),
        check(
            i18n.tr("annotate-variations"),
            o.variations,
            true,
            move |on| {
                set(AnnotateOptions {
                    variations: on,
                    ..o
                })
            }
        ),
        check(
            i18n.tr("annotate-exercises"),
            o.mark_exercises,
            caps.exercises,
            move |on| set(AnnotateOptions {
                mark_exercises: on,
                ..o
            }),
        ),
    ]
    .spacing(6)
    .into()
}

/// The eight thresholds of the informant symbols (`configInformant`, D474).
fn informant_section<'a>(informant: &Informant, i18n: &I18n) -> Element<'a, Message> {
    type Edit = fn(Informant, f32) -> Informant;
    let i = *informant;
    let rows: [(&str, f32, Edit); 8] = [
        ("annotate-informant-interesting", i.interesting, |i, v| {
            Informant {
                interesting: v,
                ..i
            }
        }),
        ("annotate-informant-poor", i.poor, |i, v| Informant {
            poor: v,
            ..i
        }),
        ("annotate-informant-blunder", i.blunder, |i, v| Informant {
            blunder: v,
            ..i
        }),
        ("annotate-informant-dubious", i.dubious, |i, v| Informant {
            dubious: v,
            ..i
        }),
        ("annotate-informant-slight", i.slight, |i, v| Informant {
            slight: v,
            ..i
        }),
        ("annotate-informant-moderate", i.moderate, |i, v| {
            Informant { moderate: v, ..i }
        }),
        ("annotate-informant-decisive", i.decisive, |i, v| {
            Informant { decisive: v, ..i }
        }),
        ("annotate-informant-won", i.won, |i, v| Informant {
            won: v,
            ..i
        }),
    ];
    let mut page = column![].spacing(6);
    for (key, value, edit) in rows {
        let to = move |steps: i32| {
            send(AnnotateMessage::SetInformant(
                edit(i, step_tenths(value, steps)).normalized(),
            ))
        };
        page = page.push(stepper(
            i18n.tr(key),
            format!("{value:.1}"),
            to(-1),
            to(1),
            true,
        ));
    }
    page.push(
        button(text(i18n.tr("annotate-informant-defaults")).size(13))
            .style(button::secondary)
            .on_press(send(AnnotateMessage::SetInformant(Informant::default()))),
    )
    .into()
}

/// The line that tells how long the run takes: the number of moves and,
/// under a time limit, the estimated duration (rule 22).
pub(crate) fn estimate_text(i18n: &I18n, dialog: &AnnotateDialog, caps: AnnotateCaps) -> String {
    let mut args = FluentArgs::new();
    args.set("moves", i64::from(dialog.moves));
    let moves = i18n.tr_args("annotate-estimate-moves", &args);
    let time_limited = effective_control(&dialog.options, caps) == Control::Time;
    if !time_limited {
        return moves;
    }
    let total = dialog
        .options
        .movetime
        .checked_mul(dialog.moves.max(1))
        .unwrap_or_default()
        .as_secs();
    args.set("minutes", i64::try_from(total / 60).unwrap_or(i64::MAX));
    args.set("seconds", i64::try_from(total % 60).unwrap_or(0));
    format!("{moves}  {}", i18n.tr_args("annotate-estimate-time", &args))
}

/// The box, when the application has the dialog open.
pub fn view<'a>(
    app: &'a AppState,
    informant_open: bool,
    i18n: &I18n,
) -> Option<Element<'a, Message>> {
    let dialog = app.annotate_dialog.as_ref()?;
    let caps = app.annotate_caps(dialog.engine);
    let options = dialog.options;

    let mut body = column![
        engine_section(app, dialog, i18n),
        control_section(&options, caps, i18n),
        thresholds_section(&options, i18n),
        choices_section(&options, caps, i18n),
        extras_section(&options, caps, dialog.book_available, i18n),
        button(
            text(i18n.tr(if informant_open {
                "annotate-informant-hide"
            } else {
                "annotate-informant-show"
            }))
            .size(13),
        )
        .style(button::text)
        .on_press(Message::Dialog(DialogMessage::ToggleAnnotateInformant)),
    ]
    .spacing(12);
    if informant_open {
        body = body.push(container(informant_section(&dialog.informant, i18n)).padding([0, 12]));
    }
    body = body.push(
        text(estimate_text(i18n, dialog, caps))
            .size(12)
            .style(theme::muted_text),
    );
    Some(modal(Card {
        id: ANNOTATE_BOX_ID,
        title: i18n.tr("annotate-title"),
        body: scrollable(body.padding([0, 12]))
            .height(Length::Fixed(BODY_HEIGHT))
            .into(),
        buttons: vec![
            button(text(i18n.tr("dialog-cancel")))
                .style(button::secondary)
                .on_press(send(AnnotateMessage::Close))
                .into(),
            button(text(i18n.tr("annotate-start")))
                .style(button::primary)
                .on_press(send(AnnotateMessage::Start))
                .into(),
        ],
        width: 680.0,
    }))
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use iced::{Settings, Size};
    use iced_test::Simulator;
    use mychess_app::{AnnotationProgress, update};

    use super::*;
    use crate::i18n::Language;

    fn open_state() -> AppState {
        let mut app = AppState::default();
        drop(update(
            &mut app,
            AppMessage::Annotate(AnnotateMessage::Open),
        ));
        app
    }

    fn annotates(messages: &[Message]) -> Vec<AnnotateMessage> {
        messages
            .iter()
            .filter_map(|m| match m {
                Message::App(AppMessage::Annotate(a)) => Some(a.clone()),
                _ => None,
            })
            .collect()
    }

    fn sim(app: &AppState, language: Language) -> Simulator<'_, Message> {
        let i18n = I18n::new(language);
        let element = view(app, false, &i18n).expect("the dialog is open");
        Simulator::with_size(Settings::default(), Size::new(900.0, 900.0), element)
    }

    fn click(app: &AppState, label: &str) -> Vec<Message> {
        let mut s = sim(app, Language::EnUs);
        s.click(label)
            .unwrap_or_else(|e| panic!("a clickable {label:?}: {e:?}"));
        s.into_messages().collect()
    }

    #[test]
    fn tenths_move_without_float_noise() {
        assert!((step_tenths(1.0, 1) - 1.1).abs() < 1e-6);
        assert!((step_tenths(1.0, -1) - 0.9).abs() < 1e-6);
        assert!((step_tenths(5.0, 5) - 5.5).abs() < 1e-6);
        assert!(step_tenths(f32::NAN, 1).is_finite());
    }

    #[test]
    fn the_time_per_move_steps_by_a_tenth_below_a_second_and_a_second_above() {
        let ms = Duration::from_millis;
        assert_eq!(step_movetime(ms(500), true), ms(600));
        assert_eq!(step_movetime(ms(1000), true), ms(2000));
        assert_eq!(step_movetime(ms(1000), false), ms(900));
        assert_eq!(step_movetime(ms(5000), false), ms(4000));
        assert_eq!(step_movetime(ms(0), false), ms(0));
    }

    #[test]
    fn progress_says_the_move_being_annotated() {
        let en = I18n::new(Language::EnUs);
        let pt = I18n::new(Language::PtBr);
        let at = |done, total| AnnotationProgress {
            done_plies: done,
            total_plies: total,
            game_index: 1,
            game_total: 1,
        };
        assert_eq!(progress_text(&en, &at(4, 40)), "Annotating... move 5 of 40");
        assert_eq!(progress_text(&pt, &at(4, 40)), "Anotando... lance 5 de 40");
        // Never past the end.
        assert_eq!(
            progress_text(&en, &at(40, 40)),
            "Annotating... move 40 of 40"
        );
        assert!(progress_text(&en, &at(0, 0)).contains("engine"));
        assert_eq!(progress_fraction(&at(0, 0)), None);
        assert_eq!(progress_fraction(&at(10, 40)), Some(0.25));
    }

    #[test]
    fn the_dialog_shows_the_defaults_of_the_scid() {
        let app = open_state();
        let mut s = sim(&app, Language::EnUs);
        for text in [
            "Configure annotation",
            "Depth per move",
            "13",
            "Threshold",
            "1.0",
            "Cut off",
            "5.0",
            "Missed/shorter mates",
            "At end of game",
            "Choose an opening book in Tools > Opening books",
            "Start",
            "Cancel",
        ] {
            assert!(s.find(text).is_ok(), "{text}");
        }
    }

    #[test]
    fn the_dialog_is_translated() {
        let app = open_state();
        let mut s = sim(&app, Language::PtBr);
        for text in [
            "Configurar anotação",
            "Profundidade por lance",
            "Iniciar",
            "Cancelar",
        ] {
            assert!(s.find(text).is_ok(), "{text}");
        }
    }

    #[test]
    fn start_and_cancel_send_their_messages() {
        let app = open_state();
        let started = click(&app, "Start");
        assert!(
            annotates(&started).contains(&AnnotateMessage::Start),
            "{started:?}"
        );
        let closed = click(&app, "Cancel");
        assert!(
            annotates(&closed).contains(&AnnotateMessage::Close),
            "{closed:?}"
        );
    }

    #[test]
    fn a_choice_sends_the_options_with_that_value() {
        let app = open_state();
        let defaults = AnnotateOptions::default();
        let sent = click(&app, "Time");
        assert!(
            annotates(&sent).contains(
                &(AnnotateMessage::SetOptions(AnnotateOptions {
                    control: Control::Time,
                    ..defaults
                }))
            ),
            "{sent:?}"
        );
    }

    #[test]
    fn the_depth_stepper_sends_one_level_more_or_less() {
        let app = open_state();
        let defaults = AnnotateOptions::default();
        // The depth is the first stepper: its "+" and "-" come first.
        for (glyph, depth) in [("+", defaults.depth + 1), ("\u{2212}", defaults.depth - 1)] {
            let mut s = sim(&app, Language::EnUs);
            s.click(glyph).expect("a stepper button");
            let sent: Vec<Message> = s.into_messages().collect();
            assert!(
                annotates(&sent).contains(&AnnotateMessage::SetOptions(AnnotateOptions {
                    depth,
                    ..defaults
                })),
                "{glyph}: {sent:?}"
            );
        }
    }

    #[test]
    fn a_stepper_at_its_limit_sends_the_clamped_value() {
        let defaults = AnnotateOptions::default();
        let mut beyond = defaults;
        beyond.depth = beyond.depth.saturating_mul(1000);
        let max = beyond.normalized().depth;
        assert!(max < beyond.depth, "the range has an upper limit");
        let mut app = open_state();
        drop(update(
            &mut app,
            AppMessage::Annotate(AnnotateMessage::SetOptions(AnnotateOptions {
                depth: max,
                ..defaults
            })),
        ));
        let mut s = sim(&app, Language::EnUs);
        s.click("+").expect("a stepper button");
        let sent: Vec<Message> = s.into_messages().collect();
        assert!(
            annotates(&sent).contains(&AnnotateMessage::SetOptions(AnnotateOptions {
                depth: max,
                ..defaults
            })),
            "the value stays at the maximum: {sent:?}"
        );
    }

    #[test]
    fn the_book_is_disabled_without_a_book_and_says_why() {
        let app = open_state();
        let sent = click(&app, "Use book");
        assert!(sent.is_empty(), "{sent:?}");
    }

    #[test]
    fn the_estimate_needs_a_time_limit() {
        let i18n = I18n::new(Language::EnUs);
        let mut dialog = AnnotateDialog {
            options: AnnotateOptions::default(),
            informant: Informant::default(),
            engine: None,
            moves: 40,
            book_available: false,
        };
        let caps = AnnotateCaps {
            depth: true,
            missed_mates: true,
            exercises: true,
        };
        assert_eq!(estimate_text(&i18n, &dialog, caps), "40 moves to annotate");
        dialog.options.control = Control::Time;
        dialog.options.movetime = Duration::from_secs(5);
        assert!(estimate_text(&i18n, &dialog, caps).contains("3 min 20 s"));
        // An engine without a depth limit is always under a time limit.
        dialog.options.control = Control::Depth;
        let xboard = AnnotateCaps {
            depth: false,
            missed_mates: false,
            exercises: false,
        };
        assert!(estimate_text(&i18n, &dialog, xboard).contains("3 min 20 s"));
        assert_eq!(effective_control(&dialog.options, xboard), Control::Time);
    }

    #[test]
    fn xboard_disables_depth_mates_and_exercises() {
        let xboard = AnnotateCaps {
            depth: false,
            missed_mates: false,
            exercises: false,
        };
        let i18n = I18n::new(Language::EnUs);
        let options = AnnotateOptions::default();
        let element: Element<'_, Message> = iced::widget::column![
            control_section(&options, xboard, &i18n),
            choices_section(&options, xboard, &i18n),
            extras_section(&options, xboard, false, &i18n),
        ]
        .into();
        let mut s = Simulator::with_size(Settings::default(), Size::new(900.0, 900.0), element);
        for label in [
            "Depth",
            "Missed/shorter mates",
            "Mark tactical exercises",
            "Depth per move",
        ] {
            s.click(label).expect("the label is on screen");
        }
        // The depth stepper comes first: its buttons are off without depth.
        for glyph in ["+", "\u{2212}"] {
            s.click(glyph).expect("a stepper button");
        }
        assert_eq!(
            s.into_messages().count(),
            0,
            "the depth, its stepper, the mates and the exercises are disabled"
        );
    }

    #[test]
    fn notices_carry_their_counts_in_both_languages() {
        let en = I18n::new(Language::EnUs);
        let pt = I18n::new(Language::PtBr);
        let done = AnnotateNotice::Finished {
            plies: 42,
            variations: 3,
        };
        assert!(notice_text(&en, &done).contains("42"));
        assert!(notice_text(&en, &done).contains('3'));
        assert!(notice_text(&pt, &done).contains("42"));
        let stopped = AnnotateNotice::Cancelled { plies: 7 };
        assert!(notice_text(&en, &stopped).contains('7'));
        assert!(notice_text(&pt, &AnnotateNotice::Busy).len() > 3);
    }

    // ---- the texts ----

    fn every_annotate_notice() -> Vec<AnnotateNotice> {
        vec![
            AnnotateNotice::AlreadyRunning,
            AnnotateNotice::NoEngine,
            AnnotateNotice::TrialMode,
            AnnotateNotice::NothingToAnnotate,
            AnnotateNotice::Busy,
            AnnotateNotice::Finished {
                plies: 31,
                variations: 4,
            },
            AnnotateNotice::Cancelled { plies: 31 },
            AnnotateNotice::GameChanged { plies: 31 },
            AnnotateNotice::EngineFailed { plies: 31 },
            AnnotateNotice::NoScore { plies: 31 },
            AnnotateNotice::BatchUnavailable,
            AnnotateNotice::BatchDirty,
            AnnotateNotice::BatchFinished {
                games: 5,
                skipped: 1,
            },
            AnnotateNotice::BatchStopped { games: 2, total: 9 },
        ]
    }

    fn every_stop_reason() -> [mychess_app::StopReason; 9] {
        use mychess_app::StopReason as R;
        [
            R::Checkmate,
            R::Stalemate,
            R::InsufficientMaterial,
            R::FiftyMoves,
            R::Repetition,
            R::NoMove,
            R::EngineError,
            R::UserStopped,
            R::GameChanged,
        ]
    }

    /// Whether the files of `language` itself (not the en-US fallback)
    /// define `key`.
    fn defined(language: Language, key: &str) -> bool {
        crate::assets::ftl_sources(language)
            .iter()
            .any(|(_, text)| ids(text).iter().any(|id| id == key))
    }

    #[test]
    fn every_annotate_and_mode_notice_is_translated_with_its_arguments() {
        use mychess_app::{AppNotice, ModeKind, ModeNotice};
        let mut notices: Vec<AppNotice> = every_annotate_notice()
            .into_iter()
            .map(AppNotice::Annotate)
            .collect();
        for simple in [
            ModeNotice::NullMoveInCheck,
            ModeNotice::Conflict,
            ModeNotice::TrialMode,
            ModeNotice::ShootOutBusy,
        ] {
            notices.push(AppNotice::Mode(simple));
        }
        for reason in every_stop_reason() {
            for kind in [ModeKind::Threat, ModeKind::ShootOut, ModeKind::Automove] {
                notices.push(AppNotice::Mode(ModeNotice::Ended { kind, reason }));
            }
        }
        for language in Language::ALL {
            let i18n = I18n::new(language);
            for notice in &notices {
                let key = notice.message_key();
                assert!(defined(language, key), "{key} missing in {language:?}");
                let shown = crate::windows::status_bar::app_notice_text(&i18n, notice);
                assert!(!shown.is_empty() && shown != key, "{key}: {shown:?}");
                assert!(!shown.contains('{'), "{key}: unresolved {shown:?}");
            }
        }
        // The counts reach the text.
        let en = I18n::new(Language::EnUs);
        let done = AppNotice::Annotate(AnnotateNotice::Finished {
            plies: 31,
            variations: 4,
        });
        let text = crate::windows::status_bar::app_notice_text(&en, &done);
        assert!(
            text.contains("31 moves") && text.contains("4 variations"),
            "{text}"
        );
        let one = AppNotice::Annotate(AnnotateNotice::Finished {
            plies: 1,
            variations: 1,
        });
        let text = crate::windows::status_bar::app_notice_text(&en, &one);
        assert!(
            text.contains("1 move,") && text.contains("1 variation."),
            "{text}"
        );
    }

    fn ids(source: &str) -> Vec<String> {
        source
            .lines()
            .filter(|l| !l.starts_with([' ', '#', '\t']))
            .filter_map(|l| l.split_once(" =").map(|(id, _)| id.trim().to_owned()))
            .filter(|id| !id.is_empty())
            .collect()
    }

    #[test]
    fn the_annotate_texts_have_the_same_ids_in_both_languages() {
        let find = |language: Language| {
            crate::assets::ftl_sources(language)
                .iter()
                .find(|(name, _)| *name == "annotate.ftl")
                .map(|(_, text)| ids(text))
                .expect("annotate.ftl is registered")
        };
        let (mut en, mut pt) = (find(Language::EnUs), find(Language::PtBr));
        assert!(en.len() > 40);
        en.sort();
        pt.sort();
        assert_eq!(en, pt);
    }

    #[test]
    fn no_id_is_defined_twice_in_a_language() {
        for language in Language::ALL {
            let mut seen = std::collections::HashSet::new();
            for (file, text) in crate::assets::ftl_sources(language) {
                for id in ids(text) {
                    assert!(
                        seen.insert(id.clone()),
                        "{id} repeats in {file} ({language:?})"
                    );
                }
            }
        }
    }

    #[test]
    fn the_keys_of_the_dialog_and_the_buttons_exist_in_both_languages() {
        let keys = [
            "annotate-title",
            "annotate-start",
            "annotate-stop",
            "annotate-progress",
            "menu-tools-annotate",
            "analysis-btn-annotate",
            "analysis-btn-threat",
            "analysis-btn-finish-game",
            "analysis-btn-training",
            "analysis-threat-label",
            "analysis-training-thinking",
            "analysis-training-your-move",
            "analysis-training-title",
            "analysis-mode-shootout-busy",
            "status-notice-null-move-in-check",
        ];
        for language in Language::ALL {
            for key in keys {
                assert!(defined(language, key), "{key} missing in {language:?}");
            }
        }
    }
}
