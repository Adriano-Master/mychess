//! The texts of the tournament between engines: notices, errors, the log,
//! the clocks and the points (spec M8-2b, D1360-D1379). The application
//! sends variants; the words come from Fluent here.

use std::time::Duration;

use fluent_bundle::FluentArgs;
use mychess_app::tournament::{
    ConfigError, ExportLabels, FailKind, GameScore, GameStatus, LogEvent, Phase, Reliability,
    Termination, TimeVerdict, TournamentNotice,
};
use mychess_core::GameResult;

use crate::i18n::I18n;

/// The score of a move as `+0.31`; a mate score (the engine layer maps it
/// far from any evaluation) is `+M` or `-M`.
pub fn score_text(centipawns: i32) -> String {
    if centipawns.abs() >= 30_000 {
        return if centipawns > 0 { "+M" } else { "-M" }.to_owned();
    }
    let sign = if centipawns < 0 { '-' } else { '+' };
    let abs = centipawns.unsigned_abs();
    format!("{sign}{}.{:02}", abs / 100, abs % 100)
}

/// A clock as `1:05:09`, `4:32` or, under twenty seconds, `0:09.4`.
pub fn clock_text(time: Duration) -> String {
    let total = time.as_secs();
    let (hours, minutes, seconds) = (total / 3600, total / 60 % 60, total % 60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else if total < 20 {
        format!("{minutes}:{seconds:02}.{}", time.subsec_millis() / 100)
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

/// The name of participant `index`, or `#n` when the list does not have it.
pub fn name_of(names: &[String], index: usize) -> String {
    names
        .get(index)
        .cloned()
        .unwrap_or_else(|| format!("#{}", index + 1))
}

/// The error of the form or of a refused start.
pub fn config_error_text(i18n: &I18n, error: &ConfigError) -> String {
    let mut args = FluentArgs::new();
    match error {
        ConfigError::EmptyName(index) => {
            args.set("number", i64::try_from(index + 1).unwrap_or(i64::MAX));
        }
        ConfigError::DuplicateName(name) => args.set("name", name.as_str()),
        ConfigError::BadTime(what)
        | ConfigError::BadWatchdog(what)
        | ConfigError::MissingOpeningData(what)
        | ConfigError::BadAdjudication(what) => args.set("what", *what),
        ConfigError::BookNotFound(path) | ConfigError::SuiteUnreadable(path) => {
            args.set("path", path.display().to_string());
        }
        _ => {}
    }
    i18n.tr_args(error.message_key(), &args)
}

/// The names of the participants as one text.
fn join_names(names: &[String], indexes: &[usize]) -> String {
    indexes
        .iter()
        .map(|i| name_of(names, *i))
        .collect::<Vec<_>>()
        .join(", ")
}

fn fail_key(kind: FailKind) -> &'static str {
    match kind {
        FailKind::StartFailed => "tournament-fail-start",
        FailKind::Died => "tournament-fail-died",
        FailKind::Hung => "tournament-fail-hung",
        FailKind::TimeLoss => "tournament-fail-time",
    }
}

/// A remark of the tournament for the status bar. `names` are the
/// participants (empty once the tournament is closed: they become `#n`).
pub fn notice_text(i18n: &I18n, notice: &TournamentNotice, names: &[String]) -> String {
    let mut args = FluentArgs::new();
    match notice {
        TournamentNotice::InvalidConfig(error) => return config_error_text(i18n, error),
        TournamentNotice::Finished { winners } => {
            args.set("winners", join_names(names, winners));
        }
        TournamentNotice::EngineFailed { participant, kind } => {
            args.set("name", name_of(names, *participant));
            args.set("reason", i18n.tr(fail_key(*kind)));
        }
        TournamentNotice::TooManyFailures { participant } => {
            args.set("name", name_of(names, *participant));
        }
        TournamentNotice::MissingEngines(missing) => args.set("names", missing.join(", ")),
        TournamentNotice::UnsupportedTime(name) => args.set("name", name.as_str()),
        TournamentNotice::ConcurrencyReduced { from, to } => {
            args.set("from", i64::from(*from));
            args.set("to", i64::from(*to));
        }
        _ => {}
    }
    i18n.tr_args(notice.message_key(), &args)
}

/// One line of the log.
pub fn log_text(i18n: &I18n, event: &LogEvent, names: &[String]) -> String {
    let mut args = FluentArgs::new();
    let number = |args: &mut FluentArgs<'_>, n: u32| args.set("number", i64::from(n));
    let key = match event {
        LogEvent::Started => "tournament-log-started",
        LogEvent::GameStarted { number: n, attempt } => {
            number(&mut args, *n);
            args.set("attempt", i64::from(*attempt) + 1);
            "tournament-log-game-started"
        }
        LogEvent::GameFinished {
            number: n,
            result,
            termination,
        } => {
            number(&mut args, *n);
            args.set("result", result.as_str());
            args.set("how", i18n.tr(termination_key(*termination)));
            "tournament-log-game-finished"
        }
        LogEvent::EngineFailed { participant, kind } => {
            args.set("name", name_of(names, *participant));
            args.set("reason", i18n.tr(fail_key(*kind)));
            "tournament-log-engine-failed"
        }
        LogEvent::Discarded { number: n, .. } => {
            number(&mut args, *n);
            "tournament-log-discarded"
        }
        LogEvent::Withdrawn(p) => {
            args.set("name", name_of(names, *p));
            "tournament-log-withdrawn"
        }
        LogEvent::Replaced(p) => {
            args.set("name", name_of(names, *p));
            "tournament-log-replaced"
        }
        LogEvent::SuiteWrapped { number: n } => {
            number(&mut args, *n);
            "tournament-log-suite-wrapped"
        }
        LogEvent::Skipped(n) => {
            number(&mut args, *n);
            "tournament-log-skipped"
        }
        LogEvent::Redo(n) => {
            number(&mut args, *n);
            "tournament-log-redo"
        }
        LogEvent::Paused => "tournament-log-paused",
        LogEvent::Resumed => "tournament-log-resumed",
        LogEvent::Stopped => "tournament-log-stopped",
        LogEvent::Aborted => "tournament-log-aborted",
        LogEvent::Finished => "tournament-log-finished",
    };
    i18n.tr_args(key, &args)
}

/// The Fluent key of how a game ended.
pub fn termination_key(termination: Termination) -> &'static str {
    match termination {
        Termination::Normal => "tournament-term-normal",
        Termination::TimeForfeit => "tournament-term-time",
        Termination::RulesInfraction => "tournament-term-rules",
        Termination::Abandoned => "tournament-term-abandoned",
        Termination::Adjudication => "tournament-term-adjudication",
        Termination::Unterminated => "tournament-term-unterminated",
    }
}

/// The state of a game of the list, and its result when it has one.
pub fn status_text(i18n: &I18n, status: &GameStatus) -> String {
    match status {
        GameStatus::Pending => i18n.tr("tournament-status-pending"),
        GameStatus::Playing => i18n.tr("tournament-status-playing"),
        GameStatus::Played(outcome) | GameStatus::Replaced(outcome) => {
            let key = if matches!(status, GameStatus::Replaced(_)) {
                "tournament-status-replaced"
            } else {
                "tournament-status-played"
            };
            let mut args = FluentArgs::new();
            args.set("result", outcome.result.as_str());
            args.set("how", i18n.tr(termination_key(outcome.termination)));
            i18n.tr_args(key, &args)
        }
        GameStatus::Skipped => i18n.tr("tournament-status-skipped"),
        GameStatus::Withdrawn => i18n.tr("tournament-status-withdrawn"),
    }
}

/// The Fluent key of the phase of the tournament.
pub fn phase_key(phase: Phase) -> &'static str {
    match phase {
        Phase::Running => "tournament-phase-running",
        Phase::Pausing => "tournament-phase-pausing",
        Phase::Paused => "tournament-phase-paused",
        Phase::Decision(_) => "tournament-phase-decision",
        Phase::StoppedAfterGame => "tournament-phase-stopped",
        Phase::Finished => "tournament-phase-finished",
        Phase::Aborted => "tournament-phase-aborted",
    }
}

/// The mark of a game in the crosstable.
pub fn cell_text(score: GameScore) -> &'static str {
    match score {
        GameScore::Win => "1",
        GameScore::Draw => "\u{bd}",
        GameScore::Loss => "0",
    }
}

/// The result of a game as the list shows it.
pub fn result_text(result: GameResult) -> &'static str {
    result.as_str()
}

/// The Fluent key of how much an estimate can be trusted (D2511).
pub fn reliability_key(reliability: Reliability) -> &'static str {
    match reliability {
        Reliability::Empty => "tournament-sample-empty",
        Reliability::VeryUncertain => "tournament-sample-very-uncertain",
        Reliability::Low => "tournament-sample-low",
        Reliability::Fair => "tournament-sample-fair",
    }
}

/// The headers of the exported tables, in the language of the interface.
pub fn export_labels(i18n: &I18n) -> ExportLabels {
    ExportLabels {
        rank: i18n.tr("tournament-col-rank"),
        name: i18n.tr("tournament-col-name"),
        played: i18n.tr("tournament-col-played"),
        wins: i18n.tr("tournament-col-wins"),
        draws: i18n.tr("tournament-col-draws"),
        losses: i18n.tr("tournament-col-losses"),
        points: i18n.tr("tournament-col-points"),
        percent: i18n.tr("tournament-col-percent"),
        sonneborn_berger: i18n.tr("tournament-col-sb"),
        withdrawn: i18n.tr("tournament-col-withdrawn"),
        elo: i18n.tr("tournament-col-elo"),
        margin: i18n.tr("tournament-col-margin"),
        los: i18n.tr("tournament-col-los"),
        performance: i18n.tr("tournament-col-performance"),
        sample: i18n.tr("tournament-col-sample"),
        reliability: [
            Reliability::Empty,
            Reliability::VeryUncertain,
            Reliability::Low,
            Reliability::Fair,
        ]
        .map(|r| i18n.tr(reliability_key(r))),
        moves: i18n.tr("tournament-col-moves"),
        mean: i18n.tr("tournament-col-mean"),
        slowest: i18n.tr("tournament-col-slowest"),
        overhead: i18n.tr("tournament-col-overhead"),
        overruns: i18n.tr("tournament-col-overruns"),
        forfeits: i18n.tr("tournament-col-forfeits"),
        clock_used: i18n.tr("tournament-col-clock-used"),
        verdict: i18n.tr("tournament-col-verdict"),
        verdicts: [
            TimeVerdict::NoData,
            TimeVerdict::Respects,
            TimeVerdict::Overruns,
            TimeVerdict::Forfeits,
        ]
        .map(|v| i18n.tr(v.label_key())),
    }
}

/// `1h 30min` / `12min` / `45s`, for the estimate of the form.
pub fn duration_text(duration: Duration) -> String {
    let total = duration.as_secs();
    let (days, hours, minutes) = (total / 86_400, total / 3600 % 24, total / 60 % 60);
    if days > 0 {
        format!("{days}d {hours}h")
    } else if hours > 0 {
        format!("{hours}h {minutes:02}min")
    } else if minutes > 0 {
        format!("{minutes}min")
    } else {
        format!("{total}s")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::Language;

    #[test]
    fn scores_have_a_sign_and_two_decimals() {
        assert_eq!(score_text(31), "+0.31");
        assert_eq!(score_text(-120), "-1.20");
        assert_eq!(score_text(0), "+0.00");
        assert_eq!(score_text(32_000), "+M");
        assert_eq!(score_text(-32_000), "-M");
    }

    #[test]
    fn clocks_show_tenths_only_when_short() {
        assert_eq!(clock_text(Duration::from_millis(9_400)), "0:09.4");
        assert_eq!(clock_text(Duration::from_secs(272)), "4:32");
        assert_eq!(clock_text(Duration::from_secs(3_909)), "1:05:09");
    }

    #[test]
    fn unknown_participants_are_numbered() {
        let names = vec!["A".to_owned()];
        assert_eq!(name_of(&names, 0), "A");
        assert_eq!(name_of(&names, 2), "#3");
    }

    #[test]
    fn durations_are_short() {
        assert_eq!(duration_text(Duration::from_secs(45)), "45s");
        assert_eq!(duration_text(Duration::from_mins(90)), "1h 30min");
        assert_eq!(duration_text(Duration::from_hours(25)), "1d 1h");
    }

    #[test]
    fn notices_and_log_lines_exist_in_both_languages() {
        let names = vec!["Alpha".to_owned(), "Beta".to_owned()];
        for language in [Language::EnUs, Language::PtBr] {
            let i18n = I18n::new(language);
            let notices = [
                TournamentNotice::Started,
                TournamentNotice::Finished {
                    winners: vec![0, 1],
                },
                TournamentNotice::EngineFailed {
                    participant: 1,
                    kind: FailKind::Hung,
                },
                TournamentNotice::TooManyFailures { participant: 0 },
                TournamentNotice::MissingEngines(vec!["X".to_owned()]),
                TournamentNotice::InvalidConfig(ConfigError::TooFewParticipants),
            ];
            for notice in &notices {
                let text = notice_text(&i18n, notice, &names);
                assert!(
                    !text.is_empty() && !text.starts_with("tournament-"),
                    "{text}"
                );
            }
            let events = [
                LogEvent::Started,
                LogEvent::GameStarted {
                    number: 3,
                    attempt: 0,
                },
                LogEvent::GameFinished {
                    number: 3,
                    result: GameResult::Draw,
                    termination: Termination::Adjudication,
                },
                LogEvent::Withdrawn(1),
                LogEvent::Finished,
            ];
            for event in &events {
                let text = log_text(&i18n, event, &names);
                assert!(!text.starts_with("tournament-"), "{text}");
            }
        }
    }
}
