//! The dialogs of the tournament between engines: the form that sets it up,
//! the list of tournaments that can be resumed, the confirmation boxes and
//! the question about a participant that failed too often (spec M8-2b,
//! D1263-D1289, D1360-D1379).
//!
//! Everything is drawn from the [`TournamentDraft`] (the form as data) and
//! from the application; the controls send [`TournamentUiMessage`]s.

use iced::widget::{button, column, container, row, scrollable, space, text, text_input};
use iced::{Alignment, Element, Length};
use mychess_app::AppState;
use mychess_app::tournament::{
    BookPick, ExtraDestination, MAX_PARTICIPANTS, OnTimeLoss, OpeningSource, Phase, Scheduling,
    ScoreComment, SuiteOrder,
};
use mychess_core::GameResult;

use crate::i18n::I18n;
use crate::shell::Message;
use crate::theme;
use crate::widgets::modal::{Card, modal};
use crate::windows::annotate_dialog::{LABEL_WIDTH, check, choice, group, labelled};
use crate::windows::tournament_draft::{
    DraftEdit, TField, TimeKind, TournamentDraft, playable_engines,
};
use crate::windows::tournament_text::{config_error_text, duration_text, name_of};
use crate::windows::tournament_ui::{
    DecisionChoice, TournamentConfirm, TournamentUi, TournamentUiMessage,
};

/// Id of the form.
pub const TOURNAMENT_BOX_ID: &str = "tournament-box";
/// Id of the list of tournaments that can be resumed.
pub const TOURNAMENT_RESUME_BOX_ID: &str = "tournament-resume-box";
/// Id of the confirmation boxes.
pub const TOURNAMENT_CONFIRM_BOX_ID: &str = "tournament-confirm-box";
/// Id of the question about a participant.
pub const TOURNAMENT_DECISION_BOX_ID: &str = "tournament-decision-box";

const BODY_HEIGHT: f32 = 520.0;
const FIELD_WIDTH: f32 = 72.0;
const NAME_WIDTH: f32 = 170.0;

fn send(message: TournamentUiMessage) -> Message {
    Message::Tournament(message)
}

fn edit(edit: DraftEdit) -> Message {
    send(TournamentUiMessage::Edit(edit))
}

fn section_title<'a>(i18n: &I18n, key: &str) -> Element<'a, Message> {
    text(i18n.tr(key))
        .size(14)
        .style(theme::accent_text)
        .width(Length::Fixed(LABEL_WIDTH * 2.0))
        .into()
}

fn number_field<'a>(draft: &TournamentDraft, field: TField) -> Element<'a, Message> {
    text_input("", draft.text(field))
        .on_input(move |t| edit(DraftEdit::Number(field, t)))
        .on_submit(send(TournamentUiMessage::Submit))
        .padding(4)
        .width(Length::Fixed(FIELD_WIDTH))
        .into()
}

fn number_row<'a>(
    i18n: &I18n,
    draft: &TournamentDraft,
    label_key: &str,
    unit_key: Option<&str>,
    field: TField,
) -> Element<'a, Message> {
    let mut line = row![number_field(draft, field)]
        .spacing(8)
        .align_y(Alignment::Center);
    if let Some(unit) = unit_key {
        line = line.push(text(i18n.tr(unit)).size(13).style(theme::muted_text));
    }
    labelled(i18n.tr(label_key), line.into())
}

fn small_button<'a>(label: String, message: Option<Message>) -> Element<'a, Message> {
    button(text(label).size(13))
        .padding([3, 8])
        .style(button::secondary)
        .on_press_maybe(message)
        .into()
}

// ---- the sections of the form ----

fn participants_section<'a>(
    app: &AppState,
    draft: &'a TournamentDraft,
    i18n: &I18n,
) -> Element<'a, Message> {
    let mut page = column![].spacing(6);
    let full = draft.participants().len() >= MAX_PARTICIPANTS;
    for (index, participant) in draft.participants().iter().enumerate() {
        let engine = app
            .engines
            .get(mychess_app::engines::EngineId(participant.engine))
            .map_or_else(|| i18n.tr("tournament-engine-gone"), |e| e.name.clone());
        let line = row![
            text_input("", &participant.name)
                .on_input(move |t| edit(DraftEdit::ParticipantName(index, t)))
                .padding(4)
                .width(Length::Fixed(NAME_WIDTH)),
            text(engine)
                .size(12)
                .style(theme::muted_text)
                .width(Length::Fixed(120.0)),
            text_input(&i18n.tr("tournament-options-hint"), draft.options_of(index))
                .on_input(move |t| edit(DraftEdit::ParticipantOptions(index, t)))
                .padding(4)
                .width(Length::Fill),
            small_button(
                i18n.tr("tournament-duplicate"),
                (!full).then(|| edit(DraftEdit::Duplicate(index))),
            ),
            small_button(
                i18n.tr("tournament-remove"),
                Some(edit(DraftEdit::Remove(index))),
            ),
        ]
        .spacing(6)
        .align_y(Alignment::Center);
        page = page.push(line);
    }
    let engines: Vec<_> = playable_engines(app).collect();
    if engines.is_empty() {
        page = page.push(text(i18n.tr("tournament-no-engine")).style(text::danger));
    } else {
        let mut add: Vec<Element<'a, Message>> = engines
            .iter()
            .map(|e| {
                small_button(
                    e.name.clone(),
                    (!full).then(|| edit(DraftEdit::AddEngine(e.id))),
                )
            })
            .collect();
        add.push(small_button(
            i18n.tr("tournament-configure-engines"),
            Some(send(TournamentUiMessage::OpenEngines)),
        ));
        page = page.push(labelled(i18n.tr("tournament-add"), group(add)));
    }
    page.into()
}

fn tournament_section<'a>(draft: &'a TournamentDraft, i18n: &I18n) -> Element<'a, Message> {
    let config = draft.config();
    let pick = |s: Scheduling| edit(DraftEdit::Scheduling(s));
    let mut page = column![
        labelled(
            i18n.tr("tournament-name"),
            text_input("", &config.name)
                .on_input(|t| edit(DraftEdit::Name(t)))
                .padding(4)
                .into(),
        ),
        labelled(
            i18n.tr("tournament-site"),
            text_input("", &config.site)
                .on_input(|t| edit(DraftEdit::Site(t)))
                .padding(4)
                .into(),
        ),
        labelled(
            i18n.tr("tournament-scheduling"),
            group(vec![
                choice(
                    i18n.tr("tournament-sched-round-robin"),
                    Scheduling::RoundRobin,
                    config.scheduling,
                    true,
                    pick,
                ),
                choice(
                    i18n.tr("tournament-sched-carousel"),
                    Scheduling::Carousel,
                    config.scheduling,
                    true,
                    pick,
                ),
                choice(
                    i18n.tr("tournament-sched-gauntlet"),
                    Scheduling::Gauntlet,
                    config.scheduling,
                    true,
                    pick,
                ),
            ]),
        ),
    ]
    .spacing(6);
    if config.scheduling == Scheduling::Gauntlet {
        let tested: Vec<Element<'a, Message>> = draft
            .participants()
            .iter()
            .enumerate()
            .map(|(i, p)| {
                choice(p.name.clone(), i, config.tested, true, |i| {
                    edit(DraftEdit::Tested(i))
                })
            })
            .collect();
        page = page.push(labelled(i18n.tr("tournament-tested"), group(tested)));
    }
    page = page.push(number_row(
        i18n,
        draft,
        "tournament-cycles",
        None,
        TField::Cycles,
    ));
    page.into()
}

#[allow(clippy::too_many_lines)] // one flat list of the controls
fn time_section<'a>(draft: &'a TournamentDraft, i18n: &I18n) -> Element<'a, Message> {
    let kind = draft.time_kind();
    let pick = |k: TimeKind| edit(DraftEdit::Time(k));
    let mut page = column![labelled(
        i18n.tr("tournament-time-mode"),
        group(vec![
            choice(
                i18n.tr("tournament-time-clock"),
                TimeKind::Clock,
                kind,
                true,
                pick
            ),
            choice(
                i18n.tr("tournament-time-movetime"),
                TimeKind::MoveTime,
                kind,
                true,
                pick,
            ),
            choice(
                i18n.tr("tournament-time-depth"),
                TimeKind::Depth,
                kind,
                true,
                pick
            ),
            choice(
                i18n.tr("tournament-time-nodes"),
                TimeKind::Nodes,
                kind,
                true,
                pick
            ),
        ]),
    )]
    .spacing(6);
    let seconds = Some("tournament-unit-seconds");
    match kind {
        TimeKind::Clock => {
            page = page
                .push(number_row(
                    i18n,
                    draft,
                    "tournament-base",
                    seconds,
                    TField::Base,
                ))
                .push(number_row(
                    i18n,
                    draft,
                    "tournament-increment",
                    seconds,
                    TField::Increment,
                ));
        }
        TimeKind::MoveTime => {
            page = page
                .push(number_row(
                    i18n,
                    draft,
                    "tournament-movetime",
                    seconds,
                    TField::MoveTime,
                ))
                .push(number_row(
                    i18n,
                    draft,
                    "tournament-leeway",
                    Some("tournament-unit-percent"),
                    TField::Leeway,
                ));
        }
        TimeKind::Depth => {
            page = page
                .push(number_row(
                    i18n,
                    draft,
                    "tournament-depth",
                    None,
                    TField::Depth,
                ))
                .push(number_row(
                    i18n,
                    draft,
                    "tournament-depth-limit",
                    seconds,
                    TField::DepthLimit,
                ));
        }
        TimeKind::Nodes => {
            page = page
                .push(number_row(
                    i18n,
                    draft,
                    "tournament-nodes",
                    None,
                    TField::Nodes,
                ))
                .push(number_row(
                    i18n,
                    draft,
                    "tournament-depth-limit",
                    seconds,
                    TField::DepthLimit,
                ));
        }
    }
    page = page.push(number_row(
        i18n,
        draft,
        "tournament-margin",
        seconds,
        TField::Margin,
    ));
    page.into()
}

fn file_label(i18n: &I18n, path: Option<&std::path::Path>) -> String {
    match path {
        Some(path) => path
            .file_name()
            .unwrap_or(path.as_os_str())
            .to_string_lossy()
            .into_owned(),
        None => i18n.tr("tournament-no-file"),
    }
}

#[allow(clippy::too_many_lines)] // one flat list of the controls
fn opening_section<'a>(draft: &'a TournamentDraft, i18n: &I18n) -> Element<'a, Message> {
    let config = draft.config();
    let pick = |s: OpeningSource| edit(DraftEdit::Opening(s));
    let source = config.opening;
    let mut page = column![labelled(
        i18n.tr("tournament-opening"),
        group(vec![
            choice(
                i18n.tr("tournament-open-initial"),
                OpeningSource::Initial,
                source,
                true,
                pick,
            ),
            choice(
                i18n.tr("tournament-open-current"),
                OpeningSource::CurrentPosition,
                source,
                true,
                pick,
            ),
            choice(
                i18n.tr("tournament-open-current-first"),
                OpeningSource::CurrentPositionFirstGame,
                source,
                true,
                pick,
            ),
            choice(
                i18n.tr("tournament-open-book"),
                OpeningSource::Book,
                source,
                true,
                pick,
            ),
            choice(
                i18n.tr("tournament-open-suite"),
                OpeningSource::Suite,
                source,
                true,
                pick,
            ),
        ]),
    )]
    .spacing(6);
    match source {
        OpeningSource::Book => {
            let path = config.book.as_ref().map(|b| b.path.as_path());
            page = page.push(labelled(
                i18n.tr("tournament-book-file"),
                row![
                    small_button(
                        i18n.tr("tournament-choose-file"),
                        Some(send(TournamentUiMessage::PickBook)),
                    ),
                    text(file_label(i18n, path)).size(13),
                    small_button(
                        i18n.tr("tournament-clear-file"),
                        path.map(|_| edit(DraftEdit::Book(None))),
                    ),
                ]
                .spacing(8)
                .align_y(Alignment::Center)
                .into(),
            ));
            let current = config.book.as_ref().map_or(BookPick::default(), |b| b.pick);
            let on = config.book.is_some();
            page = page
                .push(labelled(
                    i18n.tr("tournament-book-pick"),
                    group(vec![
                        choice(
                            i18n.tr("tournament-book-weighted"),
                            BookPick::Weighted,
                            current,
                            on,
                            |p| edit(DraftEdit::BookPick(p)),
                        ),
                        choice(
                            i18n.tr("tournament-book-best"),
                            BookPick::Best,
                            current,
                            on,
                            |p| edit(DraftEdit::BookPick(p)),
                        ),
                    ]),
                ))
                .push(number_row(
                    i18n,
                    draft,
                    "tournament-book-limit",
                    Some("tournament-unit-plies"),
                    TField::BookLimit,
                ));
        }
        OpeningSource::Suite => {
            let path = config.suite.as_ref().map(|s| s.path.as_path());
            page = page.push(labelled(
                i18n.tr("tournament-suite-file"),
                row![
                    small_button(
                        i18n.tr("tournament-choose-file"),
                        Some(send(TournamentUiMessage::PickSuite)),
                    ),
                    text(file_label(i18n, path)).size(13),
                    small_button(
                        i18n.tr("tournament-clear-file"),
                        path.map(|_| edit(DraftEdit::Suite(None))),
                    ),
                ]
                .spacing(8)
                .align_y(Alignment::Center)
                .into(),
            ));
            let current = config
                .suite
                .as_ref()
                .map_or(SuiteOrder::default(), |s| s.order);
            let on = config.suite.is_some();
            page = page
                .push(labelled(
                    i18n.tr("tournament-suite-order"),
                    group(vec![
                        choice(
                            i18n.tr("tournament-suite-sequential"),
                            SuiteOrder::Sequential,
                            current,
                            on,
                            |o| edit(DraftEdit::SuiteOrder(o)),
                        ),
                        choice(
                            i18n.tr("tournament-suite-random"),
                            SuiteOrder::Random,
                            current,
                            on,
                            |o| edit(DraftEdit::SuiteOrder(o)),
                        ),
                    ]),
                ))
                .push(number_row(
                    i18n,
                    draft,
                    "tournament-suite-plies",
                    Some("tournament-unit-plies"),
                    TField::SuitePlies,
                ));
        }
        _ => {}
    }
    if matches!(source, OpeningSource::Book | OpeningSource::Suite) {
        page = page
            .push(check(
                i18n.tr("tournament-pair-openings"),
                config.pair_openings,
                true,
                |on| edit(DraftEdit::PairOpenings(on)),
            ))
            .push(number_row(
                i18n,
                draft,
                "tournament-seed",
                None,
                TField::Seed,
            ));
    }
    page.into()
}

fn engines_section<'a>(
    app: &AppState,
    draft: &'a TournamentDraft,
    i18n: &I18n,
) -> Element<'a, Message> {
    let config = draft.config();
    let limits = app.tournament_limits(config);
    let mut hint_args = fluent_bundle::FluentArgs::new();
    hint_args.set("recommended", i64::from(limits.recommended));
    hint_args.set("max", i64::from(limits.hard_max));
    let over = config.concurrency > limits.hard_max;
    let hint = text(i18n.tr_args("tournament-concurrency-hint", &hint_args))
        .size(13)
        .style(if over {
            text::danger
        } else {
            theme::muted_text
        });
    column![
        labelled(
            i18n.tr("tournament-concurrency"),
            row![number_field(draft, TField::Concurrency), hint]
                .spacing(8)
                .align_y(Alignment::Center)
                .into(),
        ),
        check(i18n.tr("tournament-ponder"), config.ponder, true, |on| {
            edit(DraftEdit::Ponder(on))
        }),
        check(
            i18n.tr("tournament-restart-each"),
            config.restart_each_game,
            true,
            |on| edit(DraftEdit::RestartEach(on)),
        ),
        number_row(
            i18n,
            draft,
            "tournament-hash",
            Some("tournament-unit-mb"),
            TField::Hash,
        ),
        number_row(i18n, draft, "tournament-threads", None, TField::Threads),
    ]
    .spacing(6)
    .into()
}

fn adjudication_section<'a>(
    draft: &'a TournamentDraft,
    i18n: &I18n,
    app: &AppState,
) -> Element<'a, Message> {
    let adj = &draft.config().adjudication;
    let mut page = column![check(
        i18n.tr("tournament-adj-resign"),
        adj.resign.is_some(),
        true,
        |on| edit(DraftEdit::Resign(on)),
    )]
    .spacing(6);
    if let Some(rule) = adj.resign {
        page = page
            .push(number_row(
                i18n,
                draft,
                "tournament-adj-cp",
                Some("tournament-unit-cp"),
                TField::ResignCp,
            ))
            .push(number_row(
                i18n,
                draft,
                "tournament-adj-moves",
                None,
                TField::ResignMoves,
            ))
            .push(check(
                i18n.tr("tournament-adj-both"),
                rule.require_both,
                true,
                |on| edit(DraftEdit::ResignBoth(on)),
            ));
    }
    page = page.push(check(
        i18n.tr("tournament-adj-draw"),
        adj.draw.is_some(),
        true,
        |on| edit(DraftEdit::Draw(on)),
    ));
    if adj.draw.is_some() {
        page = page
            .push(number_row(
                i18n,
                draft,
                "tournament-adj-after",
                None,
                TField::DrawAfter,
            ))
            .push(number_row(
                i18n,
                draft,
                "tournament-adj-cp",
                Some("tournament-unit-cp"),
                TField::DrawCp,
            ))
            .push(number_row(
                i18n,
                draft,
                "tournament-adj-moves",
                None,
                TField::DrawMoves,
            ));
    }
    page = page.push(check(
        i18n.tr("tournament-adj-max-plies"),
        adj.max_plies.is_some(),
        true,
        |on| edit(DraftEdit::MaxPlies(on)),
    ));
    if adj.max_plies.is_some() {
        page = page.push(number_row(
            i18n,
            draft,
            "tournament-adj-plies",
            Some("tournament-unit-plies"),
            TField::MaxPlies,
        ));
    }
    if app.tournament_tablebase_available() {
        page = page.push(check(
            i18n.tr("tournament-adj-tablebase"),
            adj.tablebase,
            true,
            |on| edit(DraftEdit::Tablebase(on)),
        ));
        if adj.tablebase {
            page = page.push(number_row(
                i18n,
                draft,
                "tournament-adj-tb-pieces",
                Some("tournament-unit-pieces"),
                TField::TbPieces,
            ));
        }
    }
    page.into()
}

fn failures_section<'a>(draft: &'a TournamentDraft, i18n: &I18n) -> Element<'a, Message> {
    let current = draft.config().failures.on_time_loss;
    column![
        labelled(
            i18n.tr("tournament-time-loss"),
            group(vec![
                choice(
                    i18n.tr("tournament-time-loss-loss"),
                    OnTimeLoss::Loss,
                    current,
                    true,
                    |p| edit(DraftEdit::OnTimeLoss(p)),
                ),
                choice(
                    i18n.tr("tournament-time-loss-replay"),
                    OnTimeLoss::Replay,
                    current,
                    true,
                    |p| edit(DraftEdit::OnTimeLoss(p)),
                ),
            ]),
        ),
        number_row(i18n, draft, "tournament-retries", None, TField::Retries),
        number_row(
            i18n,
            draft,
            "tournament-max-failures",
            None,
            TField::MaxFailures,
        ),
    ]
    .spacing(6)
    .into()
}

fn recording_section<'a>(draft: &'a TournamentDraft, i18n: &I18n) -> Element<'a, Message> {
    let comments = draft.comments();
    let pick = |s: ScoreComment| edit(DraftEdit::Score(s));
    column![
        labelled(
            i18n.tr("tournament-comment-score"),
            group(vec![
                choice(
                    i18n.tr("tournament-score-eval"),
                    ScoreComment::Eval,
                    comments.score,
                    true,
                    pick,
                ),
                choice(
                    i18n.tr("tournament-score-pct"),
                    ScoreComment::Pct,
                    comments.score,
                    true,
                    pick,
                ),
                choice(
                    i18n.tr("tournament-score-text"),
                    ScoreComment::Text,
                    comments.score,
                    true,
                    pick,
                ),
                choice(
                    i18n.tr("tournament-score-none"),
                    ScoreComment::None,
                    comments.score,
                    true,
                    pick,
                ),
            ]),
        ),
        check(
            i18n.tr("tournament-comment-clock"),
            comments.clock,
            true,
            |on| edit(DraftEdit::CommentClock(on)),
        ),
        check(
            i18n.tr("tournament-comment-time"),
            comments.move_time,
            true,
            |on| edit(DraftEdit::CommentTime(on)),
        ),
        check(
            i18n.tr("tournament-claim-draws"),
            draft.config().claim_draws,
            true,
            |on| edit(DraftEdit::ClaimDraws(on)),
        ),
        labelled(
            i18n.tr("tournament-destination"),
            group(vec![
                choice(
                    i18n.tr("tournament-dest-none"),
                    ExtraDestination::None,
                    draft.config().destination,
                    true,
                    |d| edit(DraftEdit::Destination(d)),
                ),
                choice(
                    i18n.tr("tournament-dest-slot"),
                    ExtraDestination::CurrentSlot,
                    draft.config().destination,
                    true,
                    |d| edit(DraftEdit::Destination(d)),
                ),
                choice(
                    i18n.tr("tournament-dest-clipbase"),
                    ExtraDestination::Clipbase,
                    draft.config().destination,
                    true,
                    |d| edit(DraftEdit::Destination(d)),
                ),
            ]),
        ),
    ]
    .spacing(6)
    .into()
}

/// The line under the form: why it cannot start, or the games and the time
/// it will take (D1289).
fn estimate_line<'a>(draft: &TournamentDraft, i18n: &I18n) -> Element<'a, Message> {
    if let Some(error) = draft.error() {
        return text(config_error_text(i18n, error))
            .size(13)
            .style(text::danger)
            .into();
    }
    let estimate = draft.estimate();
    let mut args = fluent_bundle::FluentArgs::new();
    args.set("games", i64::try_from(estimate.games).unwrap_or(i64::MAX));
    args.set("time", duration_text(estimate.max_duration));
    args.set("lanes", i64::from(estimate.lanes));
    let key = if estimate.lanes > 1 {
        "tournament-estimate-parallel"
    } else {
        "tournament-estimate"
    };
    let mut line = text(i18n.tr_args(key, &args)).size(13);
    if estimate.warn || estimate.large_state {
        line = line.style(text::danger);
    } else {
        line = line.style(theme::muted_text);
    }
    line.into()
}

/// The form, when open.
pub fn form<'a>(app: &AppState, ui: &'a TournamentUi, i18n: &I18n) -> Option<Element<'a, Message>> {
    let draft = ui.draft.as_deref()?;
    let body = column![
        section_title(i18n, "tournament-section-participants"),
        participants_section(app, draft, i18n),
        section_title(i18n, "tournament-section-tournament"),
        tournament_section(draft, i18n),
        section_title(i18n, "tournament-section-time"),
        time_section(draft, i18n),
        section_title(i18n, "tournament-section-opening"),
        opening_section(draft, i18n),
        section_title(i18n, "tournament-section-engines"),
        engines_section(app, draft, i18n),
        section_title(i18n, "tournament-section-adjudication"),
        adjudication_section(draft, i18n, app),
        section_title(i18n, "tournament-section-failures"),
        failures_section(draft, i18n),
        section_title(i18n, "tournament-section-recording"),
        recording_section(draft, i18n),
    ]
    .spacing(10);
    let can_start = draft.error().is_none();
    let start = button(text(i18n.tr("tournament-start")))
        .style(button::primary)
        .on_press_maybe(can_start.then(|| send(TournamentUiMessage::Submit)));
    let resume = button(text(i18n.tr("tournament-resume")))
        .style(button::secondary)
        .on_press(send(TournamentUiMessage::OpenResume));
    let content = column![
        scrollable(container(body).padding([0, 12])).height(Length::Fixed(BODY_HEIGHT)),
        container(estimate_line(draft, i18n)).padding([4, 12]),
    ]
    .spacing(4);
    Some(modal(Card {
        id: TOURNAMENT_BOX_ID,
        title: i18n.tr("tournament-title"),
        body: content.into(),
        buttons: vec![
            resume.into(),
            space::horizontal().into(),
            button(text(i18n.tr("dialog-cancel")))
                .style(button::secondary)
                .on_press(send(TournamentUiMessage::Cancel))
                .into(),
            start.into(),
        ],
        width: 760.0,
    }))
}

/// The tournaments that can be resumed (D1285).
pub fn resume_list<'a>(app: &AppState, i18n: &I18n) -> Element<'a, Message> {
    let list = app.tournament_resumable();
    let body: Element<'a, Message> = if list.is_empty() {
        text(i18n.tr("tournament-resume-none")).into()
    } else {
        let mut rows = column![].spacing(4);
        for item in list {
            let mut args = fluent_bundle::FluentArgs::new();
            args.set("played", i64::try_from(item.played).unwrap_or(i64::MAX));
            args.set("total", i64::try_from(item.total).unwrap_or(i64::MAX));
            let label = format!(
                "{}  -  {}",
                item.name,
                i18n.tr_args("tournament-resume-progress", &args)
            );
            rows = rows.push(
                button(text(label))
                    .width(Length::Fill)
                    .style(button::secondary)
                    .on_press(send(TournamentUiMessage::ResumePick(item.path.clone()))),
            );
        }
        scrollable(rows).height(Length::Fixed(240.0)).into()
    };
    modal(Card {
        id: TOURNAMENT_RESUME_BOX_ID,
        title: i18n.tr("tournament-resume-title"),
        body,
        buttons: vec![
            button(text(i18n.tr("dialog-cancel")))
                .style(button::secondary)
                .on_press(send(TournamentUiMessage::ResumeClose))
                .into(),
        ],
        width: 520.0,
    })
}

/// The confirmation of something that loses work.
pub fn confirm_box<'a>(confirm: TournamentConfirm, i18n: &I18n) -> Element<'a, Message> {
    let mut args = fluent_bundle::FluentArgs::new();
    let (title, body) = match confirm {
        TournamentConfirm::Abort => ("tournament-confirm-abort-title", "tournament-confirm-abort"),
        TournamentConfirm::Adjudicate(result) => {
            let key = match result {
                GameResult::White => "tournament-adj-white",
                GameResult::Black => "tournament-adj-black",
                _ => "tournament-adj-draw-result",
            };
            args.set("result", i18n.tr(key));
            (
                "tournament-confirm-adjudicate-title",
                "tournament-confirm-adjudicate",
            )
        }
        TournamentConfirm::SkipCurrent => {
            ("tournament-confirm-skip-title", "tournament-confirm-skip")
        }
        TournamentConfirm::CloseTournament => {
            ("tournament-confirm-close-title", "tournament-confirm-close")
        }
        TournamentConfirm::Quit => ("tournament-confirm-quit-title", "tournament-confirm-quit"),
    };
    modal(Card {
        id: TOURNAMENT_CONFIRM_BOX_ID,
        title: i18n.tr(title),
        body: text(i18n.tr_args(body, &args)).into(),
        buttons: vec![
            button(text(i18n.tr("tournament-no")))
                .style(button::secondary)
                .on_press(send(TournamentUiMessage::Confirm(false)))
                .into(),
            button(text(i18n.tr("tournament-yes")))
                .style(button::primary)
                .on_press(send(TournamentUiMessage::Confirm(true)))
                .into(),
        ],
        width: 460.0,
    })
}

/// The question about a participant that failed too often (D1269): try
/// again, let it leave, or put another engine in its place. There is no
/// "skip": the tournament waits for an answer.
pub fn decision_box<'a>(
    app: &AppState,
    ui: &'a TournamentUi,
    i18n: &I18n,
) -> Option<Element<'a, Message>> {
    let tournament = app.tournament()?;
    let Phase::Decision(participant) = tournament.phase() else {
        return None;
    };
    let names = &ui.snapshot.names;
    let mut args = fluent_bundle::FluentArgs::new();
    args.set("name", name_of(names, participant));
    let engines: Vec<Element<'a, Message>> = playable_engines(app)
        .map(|e| {
            choice(
                e.name.clone(),
                Some(e.id),
                ui.replace_with,
                true,
                |id: Option<mychess_app::engines::EngineId>| {
                    send(TournamentUiMessage::ReplaceWith(
                        id.unwrap_or(mychess_app::engines::EngineId(0)),
                    ))
                },
            )
        })
        .collect();
    let body = column![
        text(i18n.tr_args("tournament-decision-body", &args)),
        labelled(i18n.tr("tournament-decision-replace-with"), group(engines)),
    ]
    .spacing(10);
    let decide = |choice: DecisionChoice| send(TournamentUiMessage::Decide(choice));
    Some(modal(Card {
        id: TOURNAMENT_DECISION_BOX_ID,
        title: i18n.tr("tournament-decision-title"),
        body: body.into(),
        buttons: vec![
            button(text(i18n.tr("tournament-decision-later")))
                .style(button::text)
                .on_press(send(TournamentUiMessage::DecisionLater))
                .into(),
            button(text(i18n.tr("tournament-decision-withdraw")))
                .style(button::danger)
                .on_press(decide(DecisionChoice::Withdraw))
                .into(),
            button(text(i18n.tr("tournament-decision-replace")))
                .style(button::secondary)
                .on_press_maybe(ui.replace_with.map(|_| decide(DecisionChoice::Replace)))
                .into(),
            button(text(i18n.tr("tournament-decision-retry")))
                .style(button::primary)
                .on_press(decide(DecisionChoice::Retry))
                .into(),
        ],
        width: 560.0,
    }))
}
