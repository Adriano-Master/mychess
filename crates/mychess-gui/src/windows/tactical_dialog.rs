//! The tactical game on screen (spec M8-7c, D2469-D2472): the sections of
//! the form of the game against an engine that change for it, and the line
//! of the panel.
//!
//! The form is the one of [`play_dialog`](super::play_dialog) with
//! `tactical = true`; the game is the one of the game against an engine.
//! What is here is only the strength (an Elo and its adjustment), the
//! places the game can start from and the texts.

use fluent_bundle::FluentArgs;
use iced::widget::{button, column, row, text, text_input};
use iced::{Alignment, Element, Length};
use mychess_app::AppState;
use mychess_app::tactical_game::{
    ADJUST_STEPS, EngineCaps, StartProblem, StrengthPlan, TacticalStartPref, TacticalStats,
    TacticalView, plan_strength,
};

use crate::i18n::I18n;
use crate::shell::Message;
use crate::theme;
use crate::windows::annotate_dialog::{check, choice, group, labelled};
use crate::windows::play_dialog::{
    PlayDraft, PlayNumber, PlayUiMessage, declared_strength, number_row, opening_editor, send,
};

/// What keeps the form from starting, if anything: the start position
/// chosen cannot be used (D2457-D2459).
pub fn start_problem(app: &AppState, draft: &PlayDraft) -> Option<StartProblem> {
    if !draft.tactical {
        return None;
    }
    let prefs = draft.to_prefs();
    prefs
        .tactical
        .start_from(&prefs, app.tactical_problem_position())
        .err()
}

/// The plan the engine of the form would be given for the Elo typed
/// (D2452), when the engine has already told its options.
pub fn plan_of_form(app: &AppState, draft: &PlayDraft) -> Option<StrengthPlan> {
    let strength = declared_strength(app, draft.engine);
    if !strength.known {
        return None;
    }
    let caps = EngineCaps {
        limit_elo: strength.elo,
        skill: strength.skill,
    };
    Some(plan_strength(caps, draft.to_prefs().tactical.elo))
}

/// The sentence about the plan, for the form (D2470).
pub fn plan_text(app: &AppState, draft: &PlayDraft, i18n: &I18n) -> String {
    let Some(plan) = plan_of_form(app, draft) else {
        return i18n.tr("play-tactical-plan-unknown");
    };
    let strength = declared_strength(app, draft.engine);
    let mut args = FluentArgs::new();
    match plan {
        StrengthPlan::Elo { elo, clamped } => {
            args.set("elo", elo);
            if clamped {
                let (min, max) = strength.elo.unwrap_or((elo, elo));
                args.set("min", min);
                args.set("max", max);
                i18n.tr_args("play-tactical-plan-elo-clamped", &args)
            } else {
                i18n.tr_args("play-tactical-plan-elo", &args)
            }
        }
        StrengthPlan::Skill { level } => {
            args.set("level", level);
            i18n.tr_args("play-tactical-plan-skill", &args)
        }
        StrengthPlan::Depth { depth } => {
            args.set("depth", i64::from(depth));
            i18n.tr_args("play-tactical-plan-depth", &args)
        }
        // `StrengthPlan` is non-exhaustive.
        _ => i18n.tr("play-tactical-plan-unknown"),
    }
}

/// The score in words (D2463).
pub fn stats_text(stats: &TacticalStats, i18n: &I18n) -> String {
    if stats.games == 0 {
        return i18n.tr("play-tactical-stats-none");
    }
    let mut args = FluentArgs::new();
    args.set("count", i64::from(stats.games));
    args.set("wins", i64::from(stats.wins));
    args.set("draws", i64::from(stats.draws));
    args.set("losses", i64::from(stats.losses));
    args.set("percent", i64::from(stats.score_percent()));
    args.set("streak", i64::from(stats.best_streak));
    args.set("elo", i64::from(stats.best_win_elo));
    i18n.tr_args("play-tactical-stats", &args)
}

/// The line of the panel under the board (D2471): the strength of the
/// engine and the score.
pub fn line_text(view: &TacticalView, i18n: &I18n) -> String {
    let mut plan = FluentArgs::new();
    let how = match view.plan {
        Some(StrengthPlan::Elo { elo, .. }) => {
            plan.set("elo", elo);
            i18n.tr_args("play-tactical-line-elo", &plan)
        }
        Some(StrengthPlan::Skill { level }) => {
            plan.set("level", level);
            plan.set("elo", i64::from(view.elo));
            i18n.tr_args("play-tactical-line-skill", &plan)
        }
        Some(StrengthPlan::Depth { depth }) => {
            plan.set("depth", i64::from(depth));
            plan.set("elo", i64::from(view.elo));
            i18n.tr_args("play-tactical-line-depth", &plan)
        }
        _ => String::new(),
    };
    let mut args = FluentArgs::new();
    args.set("plan", how);
    args.set("wins", i64::from(view.stats.wins));
    args.set("draws", i64::from(view.stats.draws));
    args.set("losses", i64::from(view.stats.losses));
    i18n.tr_args("play-tactical-line", &args)
}

/// The change of Elo after the game, for the end box (D2462).
pub fn next_elo_text(view: &TacticalView, i18n: &I18n) -> Option<String> {
    let (from, to) = view.adjusted?;
    let mut args = FluentArgs::new();
    args.set("from", i64::from(from));
    args.set("to", i64::from(to));
    Some(i18n.tr_args("play-tactical-next-elo", &args))
}

/// The place of the Elo, its adjustment, the plan and the score (D2470).
pub fn strength_section<'a>(
    app: &AppState,
    draft: &PlayDraft,
    i18n: &I18n,
) -> Element<'a, Message> {
    let tactical = &draft.prefs.tactical;
    let steps: Vec<Element<'a, Message>> = ADJUST_STEPS
        .iter()
        .map(|step| {
            choice(
                step.to_string(),
                *step,
                tactical.adjust_step,
                tactical.adjust,
                |s: u32| send(PlayUiMessage::TacticalAdjustStep(s)),
            )
        })
        .collect();
    let reset_label = if draft.reset_armed {
        i18n.tr("play-tactical-reset-confirm")
    } else {
        i18n.tr("play-tactical-reset")
    };
    let has_games = app.prefs.play.tactical.stats.games > 0;
    column![
        number_row(
            i18n,
            draft,
            i18n.tr("play-tactical-elo"),
            "Elo",
            PlayNumber::TacticalElo,
        ),
        check(
            i18n.tr("play-tactical-adjust"),
            tactical.adjust,
            true,
            |v| send(PlayUiMessage::TacticalAdjust(v)),
        ),
        labelled(i18n.tr("play-tactical-adjust-step"), group(steps)),
        text(plan_text(app, draft, i18n))
            .size(12)
            .style(theme::muted_text),
        row![
            text(stats_text(&app.prefs.play.tactical.stats, i18n))
                .size(12)
                .style(theme::muted_text),
            button(text(reset_label).size(13))
                .padding([3, 8])
                .style(button::secondary)
                .on_press_maybe(has_games.then(|| send(PlayUiMessage::TacticalResetStats))),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    ]
    .spacing(6)
    .into()
}

/// The places the game can start from (D2457).
pub fn start_section<'a>(
    app: &AppState,
    draft: &'a PlayDraft,
    i18n: &I18n,
) -> Element<'a, Message> {
    let current = draft.prefs.tactical.start;
    let pick = |s: TacticalStartPref| send(PlayUiMessage::TacticalStart(s));
    let has_problem = app.tactical_problem_position().is_some();
    let item = |key: &str, value: TacticalStartPref, enabled: bool| {
        choice(i18n.tr(key), value, current, enabled, pick)
    };
    let mut page = column![labelled(
        i18n.tr("play-start"),
        group(vec![
            item("play-tactical-start-new", TacticalStartPref::New, true),
            item(
                "play-tactical-start-current",
                TacticalStartPref::Current,
                true
            ),
            item(
                "play-tactical-start-opening",
                TacticalStartPref::Opening,
                true
            ),
            item(
                "play-tactical-start-problem",
                TacticalStartPref::Problem,
                has_problem
            ),
            item("play-tactical-start-fen", TacticalStartPref::Fen, true),
            item(
                "play-tactical-start-file",
                TacticalStartPref::RandomFile,
                true
            ),
            item(
                "play-tactical-start-base",
                TacticalStartPref::RandomBase,
                true
            ),
        ]),
    )]
    .spacing(6);
    match current {
        TacticalStartPref::Opening => page = page.push(opening_editor(draft, i18n)),
        TacticalStartPref::Problem if !has_problem => {
            page = page.push(
                text(i18n.tr("play-tactical-start-problem-none"))
                    .size(12)
                    .style(theme::muted_text),
            );
        }
        TacticalStartPref::Fen => {
            let field = text_input(
                &i18n.tr("play-tactical-start-fen-hint"),
                &draft.prefs.tactical.fen,
            )
            .on_input(|t| send(PlayUiMessage::TacticalFen(t)))
            .on_submit(send(PlayUiMessage::Submit))
            .padding(4)
            .width(Length::Fill);
            page = page.push(field);
            match start_problem(app, draft) {
                Some(StartProblem::BadFen) if !draft.prefs.tactical.fen.trim().is_empty() => {
                    page = page.push(
                        text(i18n.tr("play-tactical-fen-bad"))
                            .size(12)
                            .style(text::danger),
                    );
                }
                Some(StartProblem::Ended) => {
                    page = page.push(
                        text(i18n.tr("play-tactical-fen-ended"))
                            .size(12)
                            .style(text::danger),
                    );
                }
                _ => {}
            }
        }
        TacticalStartPref::RandomFile => {
            let file = draft.prefs.tactical.random_file.as_ref().map_or_else(
                || i18n.tr("play-tactical-file-none"),
                |p| p.display().to_string(),
            );
            page = page.push(
                row![
                    button(text(i18n.tr("play-tactical-file-choose")).size(13))
                        .padding([3, 8])
                        .style(button::secondary)
                        .on_press(send(PlayUiMessage::TacticalPickFile)),
                    text(file).size(12).style(theme::muted_text),
                ]
                .spacing(8)
                .align_y(Alignment::Center),
            );
        }
        _ => {}
    }
    page.into()
}
