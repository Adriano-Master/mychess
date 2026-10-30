//! The texts of the engine management that need arguments: failures,
//! remarks of the status bar, form errors, UCI option refusals and the
//! date of the last use (spec M4-3). Pure functions over the data the
//! application keeps; the Fluent keys come from the application's
//! `message_key()` wherever it has one.

use fluent_bundle::FluentArgs;
use mychess_app::engines::{
    CommandWarning, EditorError, EngineFailure, EngineNotice, ImportWarning, LoadIssue,
    OptionError, Protocol, ScidWarning,
};

use crate::i18n::I18n;
use crate::windows::status_bar;

/// How many characters of the engine's standard error a one-line message
/// keeps.
pub const STDERR_CHARS: usize = 160;

/// `text` cut to `limit` characters, with an ellipsis when it was longer.
pub(crate) fn truncate(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_owned();
    }
    let mut out: String = text.chars().take(limit.saturating_sub(1)).collect();
    out.push('\u{2026}');
    out
}

/// The failure as one line: the message (with the exit code, when there is
/// one) and the end of what the engine wrote to its standard error.
pub fn failure_text(i18n: &I18n, failure: &EngineFailure) -> String {
    let detail = match failure.exit_code {
        Some(code) => {
            let mut args = FluentArgs::new();
            args.set("code", i64::from(code));
            format!(" ({})", i18n.tr_args("engines-failure-exit-code", &args))
        }
        None => String::new(),
    };
    let mut args = FluentArgs::new();
    args.set("detail", detail);
    let mut text = i18n.tr_args(failure.message_key(), &args);
    // Control characters (`\r`, tabs, ESC of colour codes) become spaces and
    // runs of blanks collapse, so the text stays on one line (D751).
    let tail: Vec<String> = failure
        .stderr_tail
        .iter()
        .map(|l| {
            let spaced: String = l
                .chars()
                .map(|c| if c.is_control() { ' ' } else { c })
                .collect();
            spaced.split_whitespace().collect::<Vec<_>>().join(" ")
        })
        .filter(|l| !l.is_empty())
        .collect();
    if !tail.is_empty() {
        let mut args = FluentArgs::new();
        args.set("text", truncate(&tail.join(" | "), STDERR_CHARS));
        text.push(' ');
        text.push_str(&i18n.tr_args("engines-failure-stderr", &args));
    }
    text
}

fn named(i18n: &I18n, name: &str, text: &str) -> String {
    let mut args = FluentArgs::new();
    args.set("name", name);
    args.set("text", text);
    i18n.tr_args("engines-notice-with-name", &args)
}

fn count(value: usize) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

/// The text of a remark of the engine management for the status bar.
pub fn engine_notice_text(i18n: &I18n, notice: &EngineNotice) -> String {
    let mut args = FluentArgs::new();
    match notice {
        EngineNotice::StartFailed { name, failure } | EngineNotice::Crashed { name, failure } => {
            return named(i18n, name, &failure_text(i18n, failure));
        }
        EngineNotice::FoundRejected { path, failure } => {
            let file = path
                .file_name()
                .unwrap_or(path.as_os_str())
                .to_string_lossy();
            return named(i18n, &file, &failure_text(i18n, failure));
        }
        EngineNotice::ImportFailed(error) => return status_bar::file_error_text(i18n, error),
        EngineNotice::FileFixed { count: n } | EngineNotice::ManyRunning { count: n } => {
            args.set("count", count(*n));
        }
        EngineNotice::NoEngineOnKey(key) => args.set("key", key.name()),
        EngineNotice::Imported {
            added,
            duplicates,
            invalid,
        } => {
            args.set("added", count(*added));
            args.set("duplicates", count(*duplicates));
            args.set("invalid", count(*invalid));
        }
        _ => {}
    }
    i18n.tr_args(notice.message_key(), &args)
}

/// The text of an error of the editor form.
pub fn editor_error_text(i18n: &I18n, error: &EditorError) -> String {
    i18n.tr(error.message_key())
}

/// The text of a remark about the command of the editor.
pub fn command_warning_text(i18n: &I18n, warning: CommandWarning) -> String {
    i18n.tr(match warning {
        CommandWarning::CommandNotFound => "engines-warning-command-not-found",
        CommandWarning::NotExecutable => "engines-warning-not-executable",
        _ => "engines-warning-working-dir-missing",
    })
}

/// The name of a protocol as the list shows it (`uci`, `xboard`).
pub fn protocol_label(protocol: Protocol) -> &'static str {
    match protocol {
        Protocol::Uci => "uci",
        _ => "xboard",
    }
}

/// The text of a remark of the reading of `engines.toml`.
pub fn load_issue_text(i18n: &I18n, issue: &LoadIssue) -> String {
    let mut args = FluentArgs::new();
    let key = match issue {
        LoadIssue::FieldReset { entry, field } => {
            args.set("entry", count(entry + 1));
            args.set("field", *field);
            "engines-issue-field-reset"
        }
        LoadIssue::EntryDropped { entry, .. } => {
            args.set("entry", count(entry + 1));
            "engines-issue-entry-dropped"
        }
        LoadIssue::EntryNotATable { entry } => {
            args.set("entry", count(entry + 1));
            "engines-issue-not-a-table"
        }
        LoadIssue::DuplicateHotkey { entry } => {
            args.set("entry", count(entry + 1));
            "engines-issue-duplicate-hotkey"
        }
        LoadIssue::TooManyEngines { limit } => {
            args.set("limit", count(*limit));
            "engines-issue-too-many"
        }
        _ => "engines-issue-entry-dropped",
    };
    i18n.tr_args(key, &args)
}

/// The text of a remark about an engine imported from the Scid.
pub fn import_warning_text(i18n: &I18n, warning: &ImportWarning) -> String {
    let mut args = FluentArgs::new();
    args.set("engine", warning.engine.as_str());
    i18n.tr_args(
        match warning.kind {
            ScidWarning::RelativePath => "engines-import-relative",
            _ => "engines-import-args",
        },
        &args,
    )
}

/// The label of a UCI option: translated for the few the Scid translates
/// (`uci.tcl:761-765`), the engine's own name for the rest.
pub fn option_label(i18n: &I18n, name: &str) -> String {
    let key = match name {
        "MultiPV" => "engines-option-multipv",
        "Hash" => "engines-option-hash",
        "OwnBook" => "engines-option-ownbook",
        "BookFile" => "engines-option-bookfile",
        "UCI_LimitStrength" => "engines-option-limitstrength",
        _ => return name.to_owned(),
    };
    i18n.tr(key)
}

/// The text of a refused value of a UCI option.
pub fn option_error_text(i18n: &I18n, error: &OptionError) -> String {
    let mut args = FluentArgs::new();
    let key = match error {
        OptionError::OutOfRange { min, max, .. } => {
            args.set("min", *min);
            args.set("max", *max);
            "engines-option-error-range"
        }
        OptionError::NotInList { .. } => "engines-option-error-not-in-list",
        OptionError::WrongType { .. } => "engines-option-error-wrong-type",
        OptionError::ControlCharacter { .. } => "engines-option-error-control",
        _ => "engines-option-error-other",
    };
    i18n.tr_args(key, &args)
}

/// `YYYY-MM-DD` (UTC) of a time in seconds since the Unix epoch. Time zones
/// are left out on purpose: the text must not depend on the machine.
pub fn format_date(seconds: i64) -> String {
    // Howard Hinnant's `civil_from_days`, on days since 1970-01-01.
    let days = seconds.div_euclid(86_400) + 719_468;
    let era = days.div_euclid(146_097);
    let day_of_era = days.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let mp = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

#[cfg(test)]
mod tests {
    use mychess_app::engines::{FailureKind, Hotkey, ValidationError};

    use super::*;
    use crate::i18n::Language;

    fn failure(kind: FailureKind, code: Option<i32>, tail: &[&str]) -> EngineFailure {
        EngineFailure {
            kind,
            exit_code: code,
            stderr_tail: tail.iter().map(|s| (*s).to_owned()).collect(),
        }
    }

    #[test]
    fn dates_are_utc_civil_dates() {
        assert_eq!(format_date(0), "1970-01-01");
        assert_eq!(format_date(86_399), "1970-01-01");
        assert_eq!(format_date(951_782_400), "2000-02-29");
        assert_eq!(format_date(1_700_000_000), "2023-11-14");
        assert_eq!(format_date(-1), "1969-12-31");
    }

    #[test]
    fn a_failure_names_the_exit_code_and_the_end_of_stderr() {
        let en = I18n::new(Language::EnUs);
        let text = failure_text(
            &en,
            &failure(FailureKind::ExitedEarly, Some(3), &["bad option", ""]),
        );
        assert_eq!(
            text,
            "The engine ended before it was ready (exit code 3). The engine said: bad option"
        );
        let plain = failure_text(&en, &failure(FailureKind::CannotStart, None, &[]));
        assert_eq!(plain, "The program could not be started.");
        let pt = I18n::new(Language::PtBr);
        assert!(failure_text(&pt, &failure(FailureKind::Crashed, Some(1), &[])).contains("código"));
    }

    #[test]
    fn control_characters_in_stderr_become_spaces() {
        let en = I18n::new(Language::EnUs);
        let text = failure_text(
            &en,
            &failure(FailureKind::ExitedEarly, None, &["a\rb\x1b[0m", "\tc\td"]),
        );
        assert!(text.ends_with("a b [0m | c d"), "{text}");
        assert!(!text.chars().any(char::is_control), "{text:?}");
    }

    #[test]
    fn stderr_is_cut_to_the_limit_and_by_characters() {
        let long = "é".repeat(500);
        let cut = truncate(&long, STDERR_CHARS);
        assert_eq!(cut.chars().count(), STDERR_CHARS);
        assert!(cut.ends_with('\u{2026}'));
        assert_eq!(truncate("short", STDERR_CHARS), "short");
    }

    #[test]
    fn notices_carry_their_arguments() {
        let en = I18n::new(Language::EnUs);
        let text = engine_notice_text(&en, &EngineNotice::NoEngineOnKey(Hotkey::F3));
        assert!(text.contains("F3"), "{text}");
        let text = engine_notice_text(
            &en,
            &EngineNotice::StartFailed {
                name: "Fish".to_owned(),
                failure: failure(
                    FailureKind::Timeout(mychess_app::engines::Phase::Uciok),
                    None,
                    &[],
                ),
            },
        );
        assert!(text.starts_with("Fish: "), "{text}");
        let text = engine_notice_text(
            &en,
            &EngineNotice::Imported {
                added: 2,
                duplicates: 1,
                invalid: 0,
            },
        );
        assert_eq!(
            text,
            "Imported 2 engines; one duplicate and 0 unusable skipped."
        );
    }

    #[test]
    fn option_errors_name_the_range() {
        let en = I18n::new(Language::EnUs);
        let error = OptionError::OutOfRange {
            name: "Hash".to_owned(),
            value: 0,
            min: 1,
            max: 1024,
        };
        assert_eq!(
            option_error_text(&en, &error),
            "The value must be between 1 and 1024."
        );
        assert_eq!(option_label(&en, "Hash"), "Hash memory (MB)");
        assert_eq!(option_label(&en, "Skill Level"), "Skill Level");
    }

    #[test]
    fn editor_errors_are_translated() {
        let pt = I18n::new(Language::PtBr);
        let text = editor_error_text(&pt, &EditorError::Invalid(ValidationError::EmptyName));
        assert_eq!(text, "O nome não pode ficar vazio.");
    }

    #[test]
    #[allow(clippy::too_many_lines)] // one flat table of every variant
    fn every_message_key_of_the_engine_management_is_translated_in_both_languages() {
        use std::path::PathBuf;

        use mychess_app::engines::{ArgsError, EngineNotice, Phase};
        use mychess_app::{FileError, FileErrorKind};

        let kinds = [
            FailureKind::CannotStart,
            FailureKind::ExitedEarly,
            FailureKind::Crashed,
            FailureKind::Timeout(Phase::Uciok),
            FailureKind::StopTimeout,
            FailureKind::NotAnEngine,
            FailureKind::Io,
            FailureKind::Closed,
            FailureKind::Other,
        ];
        let failures: Vec<EngineFailure> = kinds
            .into_iter()
            .map(|kind| failure(kind, Some(1), &["oops"]))
            .collect();
        let one = failures[0].clone();
        let mut notices = vec![
            EngineNotice::FileUnreadable,
            EngineNotice::NewerVersion,
            EngineNotice::FileFixed { count: 2 },
            EngineNotice::SaveFailed,
            EngineNotice::NoEngineOnKey(Hotkey::F4),
            EngineNotice::NoEngineRunning,
            EngineNotice::ListFull,
            EngineNotice::NotFound,
            EngineNotice::EntryInvalid,
            EngineNotice::RestartToApply,
            EngineNotice::ManyRunning { count: 4 },
            EngineNotice::CommandRefused,
            EngineNotice::Imported {
                added: 1,
                duplicates: 2,
                invalid: 3,
            },
            EngineNotice::NothingToImport,
            EngineNotice::ImportFailed(FileError::new(FileErrorKind::NotFound, None)),
        ];
        for failure in &failures {
            notices.push(EngineNotice::StartFailed {
                name: "E".to_owned(),
                failure: failure.clone(),
            });
            notices.push(EngineNotice::Crashed {
                name: "E".to_owned(),
                failure: failure.clone(),
            });
        }
        notices.push(EngineNotice::FoundRejected {
            path: PathBuf::from("bin").join("fish"),
            failure: one,
        });
        let errors = [
            EditorError::Invalid(ValidationError::EmptyName),
            EditorError::Invalid(ValidationError::EmptyCommand),
            EditorError::Invalid(ValidationError::NonUtf8Path),
            EditorError::Invalid(ValidationError::EloOutOfRange(5000)),
            EditorError::Invalid(ValidationError::TextTooLong {
                field: "name",
                limit: 4096,
            }),
            EditorError::Invalid(ValidationError::TooManyItems {
                field: "args",
                limit: 512,
            }),
            EditorError::Args(ArgsError::UnclosedQuote),
            EditorError::EloNotANumber,
        ];
        let issues = [
            LoadIssue::FieldReset {
                entry: 0,
                field: "elo",
            },
            LoadIssue::EntryDropped {
                entry: 1,
                reason: ValidationError::EmptyName,
            },
            LoadIssue::EntryNotATable { entry: 2 },
            LoadIssue::DuplicateHotkey { entry: 3 },
            LoadIssue::TooManyEngines { limit: 256 },
        ];
        for language in Language::ALL {
            let i18n = I18n::new(language);
            let mut texts = Vec::new();
            for notice in &notices {
                assert!(i18n.has(notice.message_key()), "{}", notice.message_key());
                texts.push(engine_notice_text(&i18n, notice));
            }
            for error in &errors {
                assert!(i18n.has(error.message_key()), "{}", error.message_key());
                texts.push(editor_error_text(&i18n, error));
            }
            for warning in [
                CommandWarning::CommandNotFound,
                CommandWarning::NotExecutable,
                CommandWarning::WorkingDirMissing,
            ] {
                texts.push(command_warning_text(&i18n, warning));
            }
            for issue in &issues {
                texts.push(load_issue_text(&i18n, issue));
            }
            for kind in [ScidWarning::RelativePath, ScidWarning::ArgsUnparsable] {
                texts.push(import_warning_text(
                    &i18n,
                    &ImportWarning {
                        engine: "E".to_owned(),
                        kind,
                    },
                ));
            }
            for text in texts {
                assert!(
                    !text.is_empty()
                        && !text.contains("engines-")
                        && !text.contains('$')
                        && !text.contains('{'),
                    "{language:?}: {text}"
                );
            }
        }
    }
}
