//! The translated words of the EPD window: the notices of the application,
//! the diagnostics of a read and the names of the choices (spec M7-5c).

use fluent_bundle::FluentArgs;
use mychess_app::epd_window::{AnnotateMode, EpdDiagnosticKind, EpdNotice, RunScope, SortKey};

use crate::i18n::I18n;
use crate::windows::status_bar::file_error_text;

/// The text of a remark about the EPD window.
pub fn notice_text(i18n: &I18n, notice: &EpdNotice) -> String {
    let mut args = FluentArgs::new();
    match notice {
        EpdNotice::TooMany { max } => args.set("max", i64::try_from(*max).unwrap_or(4)),
        EpdNotice::OpenFailed(error) | EpdNotice::SaveFailed(error) => {
            args.set("reason", file_error_text(i18n, error));
        }
        EpdNotice::Unwritable { opcode, .. } | EpdNotice::NonStandardOpcode { opcode } => {
            args.set("opcode", opcode.as_str());
        }
        EpdNotice::SkippedLines { count } => args.set("count", *count),
        EpdNotice::Stripped { opcode, count } => {
            args.set("opcode", opcode.as_str());
            args.set("count", i64::try_from(*count).unwrap_or(0));
        }
        EpdNotice::Sorted { count } => args.set("count", i64::try_from(*count).unwrap_or(0)),
        EpdNotice::RunDone {
            done,
            found,
            counted,
            illegal,
        } => {
            args.set("done", i64::from(*done));
            args.set("found", i64::from(*found));
            args.set("counted", i64::from(*counted));
            args.set("illegal", i64::from(*illegal));
        }
        EpdNotice::RunInterrupted { done, total } => {
            args.set("done", i64::from(*done));
            args.set("total", i64::from(*total));
        }
        _ => {}
    }
    i18n.tr_args(notice.message_key(), &args)
}

/// The text of what was found on a line of a file.
pub fn diagnostic_text(i18n: &I18n, kind: &EpdDiagnosticKind) -> String {
    i18n.tr(match kind {
        EpdDiagnosticKind::InvalidPosition { .. } => "epd-diag-invalid-position",
        EpdDiagnosticKind::DuplicatePosition { .. } => "epd-diag-duplicate-position",
        EpdDiagnosticKind::UnterminatedString => "epd-diag-unterminated-string",
        EpdDiagnosticKind::NonStandardOpcode { .. } => "epd-diag-non-standard-opcode",
        EpdDiagnosticKind::LineTooLong => "epd-diag-line-too-long",
        _ => "epd-diag-other",
    })
}

/// The Fluent key of a mode of the analysis.
pub fn mode_key(mode: AnnotateMode) -> &'static str {
    match mode {
        AnnotateMode::Count => "epd-mode-count",
        AnnotateMode::Annotate => "epd-mode-annotate",
        AnnotateMode::Both => "epd-mode-both",
    }
}

/// The Fluent key of a scope of the analysis.
pub fn scope_key(scope: RunScope) -> &'static str {
    match scope {
        RunScope::FromSelected => "epd-scope-selected",
        RunScope::WithoutCe => "epd-scope-without-ce",
        _ => "epd-scope-all",
    }
}

/// The Fluent key of an order of the list.
pub fn sort_key(key: SortKey) -> &'static str {
    match key {
        SortKey::Fen => "epd-sort-fen",
        SortKey::Id => "epd-sort-id",
        SortKey::Eval => "epd-sort-eval",
        _ => "epd-sort-file",
    }
}

#[cfg(test)]
mod tests {
    use mychess_app::epd_window::UnwritableKind;

    use super::*;
    use crate::i18n::Language;

    #[test]
    fn every_notice_has_a_text_in_both_languages() {
        let notices = [
            EpdNotice::TooMany { max: 4 },
            EpdNotice::AlreadyOpen,
            EpdNotice::Unwritable {
                opcode: "bm".into(),
                kind: UnwritableKind::Semicolon,
            },
            EpdNotice::ReadOnly,
            EpdNotice::Busy,
            EpdNotice::RunActive,
            EpdNotice::SkippedLines { count: 3 },
            EpdNotice::NonStandardOpcode {
                opcode: "1x".into(),
            },
            EpdNotice::Stripped {
                opcode: "ce".into(),
                count: 2,
            },
            EpdNotice::Sorted { count: 2 },
            EpdNotice::NotFound,
            EpdNotice::NoDeepest,
            EpdNotice::NoFile,
            EpdNotice::NoRecord,
            EpdNotice::NoAnalysis,
            EpdNotice::NoEngine,
            EpdNotice::UciOnly,
            EpdNotice::EngineBusy,
            EpdNotice::EngineFailed,
            EpdNotice::NothingToAnalyze,
            EpdNotice::RunDone {
                done: 5,
                found: 2,
                counted: 3,
                illegal: 0,
            },
            EpdNotice::RunInterrupted { done: 2, total: 5 },
            EpdNotice::JobFailed,
            EpdNotice::RunUndone,
            EpdNotice::NoRunToUndo,
        ];
        for language in Language::ALL {
            let i18n = I18n::new(language);
            for notice in &notices {
                let text = notice_text(&i18n, notice);
                assert!(
                    !text.starts_with("epd-"),
                    "{notice:?} in {language:?}: {text}"
                );
                assert_ne!(text, "");
            }
        }
    }
}
