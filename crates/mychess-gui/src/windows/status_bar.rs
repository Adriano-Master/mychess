//! The status bar: the application's notice, the position summary and the
//! modified marker (spec rule 7, D169).

use fluent_bundle::FluentArgs;
use iced::widget::{button, container, progress_bar, row, space, text};
use iced::{Element, Length};
use std::path::Path;

use mychess_app::{
    AppNotice, BaseError, BasesNotice, BookmarkNotice, ClipboardError, FileError, FileErrorKind,
    MoveEntry, Notice, PasteFenError, PrefsIssue, ShortcutIssue,
};
use mychess_core::{Color, Position};

use crate::i18n::I18n;
use crate::shell::Message;
use crate::theme;

/// The translated text of a notice.
pub fn notice_text(i18n: &I18n, notice: &Notice) -> String {
    let key = match notice {
        Notice::IllegalMove => "status-notice-illegal-move",
        Notice::NullMoveInCheck => "status-notice-null-move-in-check",
        Notice::PromptPending => "status-notice-prompt-pending",
        Notice::StaleNode => "status-notice-stale-node",
        Notice::NoMoveHere => "status-notice-no-move-here",
        Notice::NotAVariation => "status-notice-not-a-variation",
        Notice::NotInVariation => "status-notice-not-in-variation",
        Notice::UndoDisabledInTrial | Notice::RedoDisabledInTrial => "status-notice-undo-disabled",
        Notice::GameTooLarge => "status-notice-game-too-large",
        Notice::NothingToStrip => "status-nothing-to-strip",
        Notice::UnreadableMove(text) => {
            let mut args = FluentArgs::new();
            args.set("text", text.as_str());
            return i18n.tr_args("status-notice-unreadable-move", &args);
        }
        Notice::MoveEntryNoMatch(text) => {
            let mut args = FluentArgs::new();
            args.set("text", text.as_str());
            return i18n.tr_args("status-move-entry-no-match", &args);
        }
    };
    i18n.tr(key)
}

/// Bytes in a mebibyte, for the limits shown to the user.
const MIB: u64 = 1 << 20;

/// The file name of `path` (the whole path when it has none).
fn file_label(path: &Path) -> String {
    path.file_name()
        .unwrap_or(path.as_os_str())
        .to_string_lossy()
        .into_owned()
}

/// The translated text of a failed file operation: the reason, with the
/// file name when there is one.
pub fn file_error_text(i18n: &I18n, error: &FileError) -> String {
    let mut args = FluentArgs::new();
    match &error.kind {
        FileErrorKind::TooLarge { limit } => args.set("limit", *limit / MIB),
        FileErrorKind::UnsupportedEncoding(encoding) => args.set("encoding", *encoding),
        _ => {}
    }
    let reason = i18n.tr_args(error.message_key(), &args);
    let Some(path) = &error.path else {
        return reason;
    };
    let mut args = FluentArgs::new();
    args.set("reason", reason);
    args.set("path", file_label(path));
    i18n.tr_args("status-file-error", &args)
}

fn shortcut_issue_text(i18n: &I18n, issue: &ShortcutIssue) -> String {
    let mut args = FluentArgs::new();
    match issue {
        ShortcutIssue::UnknownAction { name } => args.set("name", name.as_str()),
        ShortcutIssue::InvalidBinding { action, text } => {
            args.set("action", action.name());
            args.set("text", text.as_str());
        }
        ShortcutIssue::Conflict { action, other } => {
            args.set("action", action.name());
            args.set("other", other.name());
        }
        _ => {}
    }
    i18n.tr_args(issue.message_key(), &args)
}

/// The translated text of a remark about the preferences.
pub fn prefs_issue_text(i18n: &I18n, issue: &PrefsIssue) -> String {
    let mut args = FluentArgs::new();
    match issue {
        PrefsIssue::InvalidValue { key } | PrefsIssue::Clamped { key } => {
            args.set("key", key.as_str());
        }
        PrefsIssue::NewerVersion { found } => args.set("found", *found),
        PrefsIssue::Shortcut(issue) => return shortcut_issue_text(i18n, issue),
        _ => {}
    }
    i18n.tr_args(issue.message_key(), &args)
}

/// How a copy names a database: the file name, "Clipbase", or the number
/// of the slot when it has no name.
pub(crate) fn copy_end_label(i18n: &I18n, end: &mychess_app::CopyEnd) -> String {
    if end.slot.is_clipbase() {
        i18n.tr("games-clipbase")
    } else if end.name.is_empty() {
        let mut args = FluentArgs::new();
        args.set("number", i64::try_from(end.slot.number()).unwrap_or(0));
        i18n.tr_args("games-slot-number", &args)
    } else {
        end.name.clone()
    }
}

/// The notices whose text a window module builds.
fn delegated_notice_text(i18n: &I18n, notice: &AppNotice) -> Option<String> {
    Some(match notice {
        AppNotice::FileImport(notice) => {
            crate::windows::import_file_dialog::notice_text(i18n, notice)
        }
        AppNotice::GamesExport(notice) => {
            crate::windows::export_games_dialog::notice_text(i18n, notice)
        }
        AppNotice::Merge(notice) => crate::windows::merge_dialog::notice_text(i18n, notice),
        AppNotice::Maintenance(notice) => {
            crate::windows::maintenance::text::notice_text(i18n, notice)
        }
        AppNotice::Prefs(issue) => prefs_issue_text(i18n, issue),
        AppNotice::Engine(notice) => crate::windows::engine_text::engine_notice_text(i18n, notice),
        AppNotice::Online(notice) => crate::windows::online_text::notice_text(i18n, notice),
        AppNotice::Tournament(notice) => {
            crate::windows::tournament_text::notice_text(i18n, notice, &[])
        }
        AppNotice::Eco(notice) => crate::windows::eco_text::notice_text(i18n, notice),
        AppNotice::Annotate(notice) => crate::windows::annotate_dialog::notice_text(i18n, notice),
        AppNotice::Analysis(notice) => crate::windows::analysis_text::notice_text(i18n, notice),
        AppNotice::Mask(notice) => crate::windows::mask::notice_text(i18n, notice),
        AppNotice::AdvancedSearch(notice) => {
            crate::windows::advanced_search_text::notice_text(i18n, notice)
        }
        AppNotice::PatternSearch(notice) => {
            crate::windows::pattern_search_text::notice_text(i18n, notice)
        }
        AppNotice::Epd(notice) => crate::windows::epd::text::notice_text(i18n, notice),
        AppNotice::Start(notice) => crate::windows::start_dialogs::notice_text(i18n, notice),
        AppNotice::OpeningReport(notice) => {
            crate::windows::opening_report::text::notice_text(i18n, notice)
        }
        AppNotice::Eboard(notice) => crate::windows::eboard_dialog::notice_text(i18n, notice),
        AppNotice::Spelling(notice) => {
            crate::windows::player_tools::text::spelling_notice_text(i18n, notice)
        }
        AppNotice::Finder(notice) => {
            crate::windows::player_tools::text::finder_notice_text(i18n, *notice)
        }
        AppNotice::PlayerInfo(notice) => {
            crate::windows::player_tools::text::info_notice_text(i18n, *notice)
        }
        _ => return None,
    })
}

/// The translated text of a remark of the application (files, clipboard,
/// preferences).
pub fn app_notice_text(i18n: &I18n, notice: &AppNotice) -> String {
    if let Some(text) = delegated_notice_text(i18n, notice) {
        return text;
    }
    let mut args = FluentArgs::new();
    match notice {
        AppNotice::File(error) => return file_error_text(i18n, error),
        AppNotice::BaseFile { path, error } => {
            args.set("reason", i18n.tr(error.message_key()));
            args.set("path", file_label(path));
            return i18n.tr_args("status-file-error", &args);
        }
        // A file that cannot be read says which one and why.
        AppNotice::Bases(BasesNotice::Failed(BaseError::File(error))) => {
            return file_error_text(i18n, error);
        }
        AppNotice::Bases(BasesNotice::Filtered { removed }) => args.set("removed", *removed),
        AppNotice::Bases(BasesNotice::Copied { count }) => args.set("count", *count),
        AppNotice::Bases(
            BasesNotice::CopyRefused { from, to, .. }
            | BasesNotice::FilterCopied { from, to, .. }
            | BasesNotice::FilterCopiedNotes { from, to, .. },
        ) => {
            args.set("from", copy_end_label(i18n, from));
            args.set("to", copy_end_label(i18n, to));
            match notice {
                AppNotice::Bases(BasesNotice::FilterCopied { count, .. }) => {
                    args.set("count", *count);
                }
                AppNotice::Bases(BasesNotice::FilterCopiedNotes {
                    count,
                    truncated,
                    reset,
                    ..
                }) => {
                    args.set("count", *count);
                    args.set("truncated", *truncated);
                    args.set("reset", i64::from(*reset));
                }
                _ => {}
            }
        }
        AppNotice::Bases(BasesNotice::CopyStopped {
            count,
            error,
            from,
            to,
        }) => {
            args.set("count", *count);
            args.set("from", copy_end_label(i18n, from));
            args.set("to", copy_end_label(i18n, to));
            args.set("reason", i18n.tr(error.message_key()));
        }
        AppNotice::Bases(BasesNotice::BrowserLimit { shown, asked }) => {
            args.set("shown", *shown);
            args.set("asked", *asked);
        }
        AppNotice::Bases(BasesNotice::UnreadableEntries { count }) => {
            args.set("count", i64::try_from(*count).unwrap_or(i64::MAX));
        }
        AppNotice::Bookmark(
            BookmarkNotice::BaseUnavailable { title } | BookmarkNotice::GameNotFound { title },
        ) => args.set("title", title.as_str()),
        AppNotice::Bookmark(BookmarkNotice::Dropped { count }) => {
            args.set("count", i64::try_from(*count).unwrap_or(i64::MAX));
        }
        AppNotice::Clipboard(ClipboardError::TooLarge { limit }) => {
            // The limit is a few dozen MiB: it fits an `i64` with room to spare.
            args.set(
                "limit",
                i64::try_from(*limit / (1 << 20)).unwrap_or(i64::MAX),
            );
        }
        AppNotice::PasteFen(PasteFenError::Invalid { excerpt, .. }) => {
            args.set("excerpt", excerpt.as_str());
        }
        _ => {}
    }
    i18n.tr_args(notice.message_key(), &args)
}

/// `White to move (12)` plus check, checkmate or stalemate.
pub fn position_summary(i18n: &I18n, position: &Position) -> String {
    let mut args = FluentArgs::new();
    args.set("move", i64::from(position.fullmove_number()));
    let key = match position.turn() {
        Color::White => "status-turn-white",
        Color::Black => "status-turn-black",
    };
    let mut out = i18n.tr_args(key, &args);
    let extra = if position.is_checkmate() {
        Some("status-checkmate")
    } else if position.is_stalemate() {
        Some("status-stalemate")
    } else if position.is_check() {
        Some("status-check")
    } else {
        None
    };
    if let Some(key) = extra {
        out.push_str("  -  ");
        out.push_str(&i18n.tr(key));
    }
    out
}

/// `Enter move: [Nf] Nf3 Nfd2` while a move is being typed (D204).
pub fn entry_text(i18n: &I18n, entry: &MoveEntry) -> String {
    let mut args = FluentArgs::new();
    args.set("text", entry.text());
    let candidates: Vec<&str> = entry.candidates().iter().map(|c| c.text.as_str()).collect();
    args.set("candidates", candidates.join(" "));
    i18n.tr_args("status-move-entry", &args)
}

/// `Chess960 no. 518` (or `Chess960` for a start that is not a numbered one)
/// for a Chess960 game, nothing for standard chess (M9-6).
pub fn variant_badge(i18n: &I18n, game: &mychess_core::Game) -> Option<String> {
    if game.variant() != mychess_core::Variant::Chess960 {
        return None;
    }
    Some(match game.start_position().chess960_number() {
        Some(n) => {
            let mut args = FluentArgs::new();
            args.set("n", i64::from(n));
            i18n.tr_args("status-chess960-number", &args)
        }
        None => i18n.tr("status-chess960"),
    })
}

/// A database being opened: shown in place of the left text, with a
/// progress bar and a button to stop (spec M3-4, D546).
#[derive(Debug, Clone)]
pub struct OpeningBar {
    /// `Opening games.pgn: 37 %`.
    pub text: String,
    /// The fraction read, once the size of the file is known.
    pub fraction: Option<f32>,
    /// The label of the stop button.
    pub cancel_label: String,
    /// What the button sends.
    pub cancel: Message,
}

/// The text of the progress of a database being opened.
pub fn opening_text(i18n: &I18n, name: &str, fraction: Option<f32>) -> String {
    let mut args = FluentArgs::new();
    args.set("name", name);
    match fraction {
        Some(fraction) => {
            // A percentage to show: rounded and within 0 to 100.
            #[allow(clippy::cast_possible_truncation)]
            let percent = (fraction.clamp(0.0, 1.0) * 100.0).round() as i64;
            args.set("percent", percent);
            i18n.tr_args("status-opening-base-percent", &args)
        }
        None => i18n.tr_args("status-opening-base", &args),
    }
}

/// Renders the bar. `alert` draws the left text as a warning or error;
/// `opening` replaces it with the progress of a database being opened (an
/// `alert` remark is kept beside the progress).
pub fn view<'a>(
    left: String,
    alert: bool,
    opening: Option<OpeningBar>,
    position: &Position,
    modified: bool,
    badges: Vec<String>,
    i18n: &I18n,
) -> Element<'a, Message> {
    let mut right = row![text(position_summary(i18n, position))].spacing(12);
    for badge in badges {
        right = right.push(text(badge));
    }
    if modified {
        right = right.push(text(format!("● {}", i18n.tr("status-modified"))));
    }
    let left: Element<'a, Message> = if let Some(bar) = opening {
        let mut progress = row![
            text(bar.text),
            progress_bar(0.0..=1.0, bar.fraction.unwrap_or(0.0))
                .length(Length::Fixed(140.0))
                .girth(Length::Fixed(6.0)),
            button(text(bar.cancel_label).size(12))
                .padding([0, 8])
                .style(theme::menu_item)
                .on_press(bar.cancel),
        ]
        .spacing(10)
        .align_y(iced::Alignment::Center);
        // A live remark (the game cannot be edited during an annotation,
        // a second start...) stays visible beside the progress (D924).
        if alert {
            progress = progress.push(text(left).style(text::danger));
        }
        progress.into()
    } else if alert {
        text(left).style(text::danger).into()
    } else {
        text(left).into()
    };
    container(row![left, space::horizontal(), right].spacing(12))
        .id("status-bar")
        .padding([3, 10])
        .width(Length::Fill)
        .style(theme::status_bar)
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::Language;

    #[test]
    fn notices_are_translated_in_both_languages() {
        let en = I18n::new(Language::EnUs);
        let pt = I18n::new(Language::PtBr);
        assert_eq!(notice_text(&en, &Notice::IllegalMove), "Illegal move");
        assert_eq!(notice_text(&pt, &Notice::IllegalMove), "Lance ilegal");
        assert_eq!(
            notice_text(&en, &Notice::UnreadableMove("zz".into())),
            "Unreadable move: zz"
        );
    }

    #[test]
    fn a_chess960_game_shows_its_number_in_the_status_bar() {
        let en = I18n::new(Language::EnUs);
        let pt = I18n::new(Language::PtBr);
        let standard = mychess_core::Game::new();
        assert_eq!(variant_badge(&en, &standard), None);
        let start = Position::chess960_start(518).unwrap();
        let game = mychess_core::Game::from_position(start);
        assert_eq!(
            variant_badge(&en, &game).as_deref(),
            Some("Chess960 no. 518")
        );
        assert_eq!(
            variant_badge(&pt, &game).as_deref(),
            Some("Chess960 nº 518")
        );
        // A Chess960 game from a position that is not a numbered start.
        let odd = Position::from_fen_with(
            "4k3/8/8/8/8/8/8/RK6 w Q - 0 1",
            mychess_core::FenMode::Chess960,
        )
        .unwrap();
        let game = mychess_core::Game::from_position(odd);
        assert_eq!(variant_badge(&en, &game).as_deref(), Some("Chess960"));
    }

    #[test]
    fn bookmark_notices_carry_their_arguments() {
        let en = I18n::new(Language::EnUs);
        let pt = I18n::new(Language::PtBr);
        let missing = AppNotice::Bookmark(BookmarkNotice::BaseUnavailable {
            title: "Old game".into(),
        });
        assert!(app_notice_text(&en, &missing).contains("Old game"));
        assert!(app_notice_text(&pt, &missing).contains("Old game"));
        let dropped = AppNotice::Bookmark(BookmarkNotice::Dropped { count: 3 });
        assert!(app_notice_text(&en, &dropped).contains('3'));
        for notice in [
            BookmarkNotice::Added,
            BookmarkNotice::CannotAdd,
            BookmarkNotice::PathNotUtf8,
            BookmarkNotice::TreeFull,
            BookmarkNotice::FileUnreadable,
            BookmarkNotice::NewerVersion,
            BookmarkNotice::SaveFailed,
        ] {
            let key = notice.message_key();
            assert!(en.has(key) && pt.has(key), "{key}");
        }
    }

    #[test]
    fn summary_names_the_side_and_the_special_states() {
        let en = I18n::new(Language::EnUs);
        assert_eq!(position_summary(&en, &Position::new()), "White to move (1)");
        let mate =
            Position::from_fen("rnb1kbnr/pppp1ppp/8/4p3/6Pq/5P2/PPPPP2P/RNBQKBNR w KQkq - 1 3")
                .expect("fen");
        assert!(position_summary(&en, &mate).ends_with("Checkmate"));
        let check = Position::from_fen("4k3/8/8/8/8/8/4r3/4K3 w - - 0 1").expect("fen");
        assert!(position_summary(&en, &check).ends_with("Check"));
        let stale = Position::from_fen("7k/5Q2/6K1/8/8/8/8/8 b - - 0 1").expect("fen");
        assert!(position_summary(&en, &stale).ends_with("Stalemate"));
        let pt = I18n::new(Language::PtBr);
        assert!(position_summary(&pt, &stale).starts_with("Pretas jogam"));
    }

    fn every_app_notice() -> Vec<AppNotice> {
        use std::io::ErrorKind;
        let kinds = [
            FileErrorKind::NotFound,
            FileErrorKind::PermissionDenied,
            FileErrorKind::AlreadyExists,
            FileErrorKind::TooLarge { limit: 64 << 20 },
            FileErrorKind::UnsupportedEncoding("UTF-16LE"),
            FileErrorKind::CorruptGzip,
            FileErrorKind::NoGamesFound { skipped_bytes: 3 },
            FileErrorKind::EncodingLossy,
            FileErrorKind::Io(ErrorKind::TimedOut),
        ];
        let mut all: Vec<AppNotice> = kinds
            .iter()
            .flat_map(|kind| {
                [
                    AppNotice::File(FileError::new(kind.clone(), None)),
                    AppNotice::File(FileError::new(
                        kind.clone(),
                        Some(std::path::PathBuf::from("games").join("a.pgn")),
                    )),
                ]
            })
            .collect();
        all.extend([
            AppNotice::Clipboard(ClipboardError::Empty),
            AppNotice::Clipboard(ClipboardError::TooLarge { limit: 32 << 20 }),
            AppNotice::Clipboard(ClipboardError::Unavailable),
            AppNotice::PasteFen(PasteFenError::Empty),
            AppNotice::PasteFen(PasteFenError::Invalid {
                excerpt: "8/8".to_owned(),
                error: Position::from_fen("8/8").expect_err("invalid fen"),
            }),
            AppNotice::ClipboardWriteFailed,
            AppNotice::HostEffectNotHandled,
            AppNotice::SetupChess960Unsupported,
        ]);
        let action = mychess_app::Action::Save;
        let issues = [
            PrefsIssue::InvalidValue {
                key: "board.theme".to_owned(),
            },
            PrefsIssue::Clamped {
                key: "board.animation_ms".to_owned(),
            },
            PrefsIssue::NewerVersion { found: 9 },
            PrefsIssue::Unreadable,
            PrefsIssue::SaveFailed(FileErrorKind::NotFound),
            PrefsIssue::NoConfigDir,
            PrefsIssue::Shortcut(ShortcutIssue::UnknownAction {
                name: "Nope".to_owned(),
            }),
            PrefsIssue::Shortcut(ShortcutIssue::InvalidBinding {
                action,
                text: "??".to_owned(),
            }),
            PrefsIssue::Shortcut(ShortcutIssue::Conflict {
                action,
                other: mychess_app::Action::SaveAs,
            }),
        ];
        all.extend(issues.into_iter().map(AppNotice::Prefs));
        all
    }

    #[test]
    fn every_remark_of_the_application_is_translated_in_both_languages() {
        for language in Language::ALL {
            let i18n = I18n::new(language);
            for notice in every_app_notice() {
                let text = app_notice_text(&i18n, &notice);
                assert!(!text.is_empty(), "{notice:?}");
                assert!(
                    !text.contains("-error-") && !text.contains("-issue-") && !text.contains('$'),
                    "{language:?} {notice:?}: {text}"
                );
            }
        }
    }

    #[test]
    fn file_errors_name_the_file_and_the_reason() {
        let en = I18n::new(Language::EnUs);
        let path = std::path::PathBuf::from("dir").join("a.pgn");
        let error = FileError::new(FileErrorKind::NotFound, Some(path));
        assert_eq!(file_error_text(&en, &error), "File not found (a.pgn)");
        let error = FileError::new(FileErrorKind::TooLarge { limit: 64 << 20 }, None);
        assert_eq!(
            file_error_text(&en, &error),
            "The file is larger than the limit of 64 MiB"
        );
        let pt = I18n::new(Language::PtBr);
        let error = FileError::new(FileErrorKind::NotFound, None);
        assert_eq!(file_error_text(&pt, &error), "Arquivo não encontrado");
    }

    #[test]
    fn remarks_carry_their_details() {
        let en = I18n::new(Language::EnUs);
        let issue = PrefsIssue::Clamped {
            key: "board.animation_ms".to_owned(),
        };
        assert!(prefs_issue_text(&en, &issue).contains("board.animation_ms"));
        let paste = AppNotice::PasteFen(PasteFenError::Invalid {
            excerpt: "zzz".to_owned(),
            error: Position::from_fen("zzz").expect_err("invalid fen"),
        });
        assert!(app_notice_text(&en, &paste).ends_with("zzz"));
    }

    #[test]
    fn the_move_being_typed_shows_with_its_candidates() {
        let mut app = mychess_app::AppState::default();
        mychess_app::update(
            &mut app,
            mychess_app::Message::Game(mychess_app::GameMessage::MoveEntryChar('N')),
        );
        let en = I18n::new(Language::EnUs);
        let text = entry_text(&en, app.session.move_entry());
        assert!(text.starts_with("Enter move: [N] "), "{text}");
        assert!(text.contains("Nf3"));
        let pt = I18n::new(Language::PtBr);
        assert!(entry_text(&pt, app.session.move_entry()).starts_with("Digite o lance: [N]"));
    }
}
