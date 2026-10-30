//! The texts of the opening trainer: they are chosen from the state of the
//! application, never written in the code (spec M8-5, D2242). Pure
//! functions, so the tests need no window.

use fluent_bundle::FluentArgs;
use mychess_app::opening_trainer::{
    Candidate, LineEnd, OpeningNotice, OpeningPhase, OpeningView, ReportView,
};
use mychess_core::Color;

use crate::i18n::I18n;

/// The Fluent key of the line that says what the user has to do or what
/// happened (D2228).
pub fn status_key(view: &OpeningView) -> &'static str {
    match view.phase {
        OpeningPhase::Building => "opening-status-building",
        OpeningPhase::UserTurn => "opening-status-user-turn",
        OpeningPhase::LineDone(LineEnd::Depth) => "opening-status-line-depth",
        OpeningPhase::LineDone(_) => "opening-status-line-complete",
        OpeningPhase::Exhausted => "opening-status-exhausted",
        _ => "opening-status-closed",
    }
}

/// The line of the status, translated.
pub fn status_text(view: &OpeningView, i18n: &I18n) -> String {
    i18n.tr(status_key(view))
}

/// The title of the panel: the trainer and the colour of the line.
pub fn title_text(view: &OpeningView, i18n: &I18n) -> String {
    match view.color {
        Some(Color::White) => i18n.tr("opening-title-white"),
        Some(Color::Black) => i18n.tr("opening-title-black"),
        None => i18n.tr("opening-title"),
    }
}

/// The remark of the application, translated.
pub fn notice_text(notice: OpeningNotice, i18n: &I18n) -> String {
    i18n.tr(notice.message_key())
}

/// The counters of the session in one line.
pub fn session_text(view: &OpeningView, i18n: &I18n) -> String {
    let s = view.session;
    let mut args = FluentArgs::new();
    args.set("lines", i64::from(s.lines));
    args.set("perfect", i64::from(s.perfect_lines));
    args.set("good", i64::from(s.good));
    args.set("dubious", i64::from(s.dubious));
    args.set("out", i64::from(s.out));
    i18n.tr_args("opening-session", &args)
}

/// The moves of the repertoire in the position, with their share (D2217).
pub fn candidates_text(candidates: &[Candidate], i18n: &I18n) -> Option<String> {
    if candidates.is_empty() {
        return None;
    }
    let moves: Vec<String> = candidates
        .iter()
        .map(|c| {
            let mark = if c.dubious { "?!" } else { "" };
            format!("{}{mark} {}%", c.san, c.percent)
        })
        .collect();
    let mut args = FluentArgs::new();
    args.set("moves", moves.join(", "));
    Some(i18n.tr_args("opening-candidates", &args))
}

/// The report of the repertoire in one line (D2224).
pub fn report_text(report: &ReportView, i18n: &I18n) -> String {
    let mut args = FluentArgs::new();
    args.set("positions", i64::from(report.positions));
    args.set("never", i64::from(report.never));
    args.set("played", i64::from(report.played));
    args.set("due", i64::from(report.due));
    args.set("learned", i64::from(report.learned));
    args.set("good", i64::from(report.good));
    args.set("dubious", i64::from(report.dubious));
    args.set("out", i64::from(report.out));
    i18n.tr_args("opening-report", &args)
}

/// The line of the hint: the piece to move (level one).
pub fn hint_text(view: &OpeningView, i18n: &I18n) -> Option<String> {
    let hint = view.hint?;
    let mut args = FluentArgs::new();
    args.set("square", hint.from.to_string());
    Some(i18n.tr_args("opening-hint-piece", &args))
}
