//! The modal dialogs of the window: which one is on top and how it looks
//! (spec M2-4, D186).
//!
//! Every dialog is a [`modal`] layer pushed on the window's `stack`; this
//! module draws the two questions of the file flow ("save the changes?" and
//! "replace the file?") and picks, with [`ActiveDialog`], the one to show
//! from the others (`prompts`, `setup_board`, `header_dialog`,
//! `paste_pgn`, `goto_move`, `game_chooser`, `variation_picker`).

use fluent_bundle::FluentArgs;
use iced::Element;
use iced::widget::{button, text};
use mychess_app::{CopyPrompt, CopyScope, DiscardChoice, OverwriteReason};

use crate::app_api;
use crate::i18n::I18n;
use crate::shell::{Message, Shell};
use crate::widgets::modal::{Card, modal};
use crate::windows::dialog_ui::ActiveDialog;
use crate::windows::mask::dialogs as mask_dialogs;
use crate::windows::{
    annotate_dialog, appearance_view, book_panel, bookmark_editor, eco_browser, eco_dialogs,
    engine_editor, engine_options, engines_dialog, export_games_dialog, game_chooser, goto_move,
    header_dialog, import_file_dialog, merge_dialog, online_dialog, paste_pgn, play_dialog,
    play_panel, prompts, report_dialog, setup_board, status_bar, tournament_dialog,
    variation_picker,
};

/// Id of the "save changes?" box.
pub const DISCARD_BOX_ID: &str = "discard-prompt";
/// Id of the "replace file?" box.
pub const OVERWRITE_BOX_ID: &str = "overwrite-prompt";
/// Id of the "copy games?" box.
pub const COPY_BOX_ID: &str = "copy-games-prompt";

fn answer_button<'a>(label: String, message: Message, is_default: bool) -> Element<'a, Message> {
    let button = button(text(label)).on_press(message);
    if is_default {
        button.style(button::primary).into()
    } else {
        button.style(button::secondary).into()
    }
}

fn discard_choice(choice: DiscardChoice) -> Message {
    Message::App(app_api::answer_discard(choice))
}

/// The "save the changes?" box: Save, Don't save, Cancel.
fn discard_box<'a>(i18n: &I18n) -> Element<'a, Message> {
    modal(Card {
        id: DISCARD_BOX_ID,
        title: i18n.tr("dialog-discard-title"),
        body: text(i18n.tr("dialog-discard-message")).into(),
        buttons: vec![
            answer_button(
                i18n.tr("dialog-discard-save"),
                discard_choice(DiscardChoice::Save),
                false,
            ),
            answer_button(
                i18n.tr("dialog-discard-dont-save"),
                discard_choice(DiscardChoice::DontSave),
                true,
            ),
            answer_button(
                i18n.tr("dialog-cancel"),
                discard_choice(DiscardChoice::Cancel),
                false,
            ),
        ],
        width: 480.0,
    })
}

/// The Fluent key of the question for each reason a file needs consent.
fn overwrite_key(reason: OverwriteReason) -> &'static str {
    match reason {
        OverwriteReason::MultiGame => "dialog-overwrite-message",
        OverwriteReason::PartiallyRead => "dialog-overwrite-partial",
        // `ExistingFile` and any reason a newer application adds.
        _ => "dialog-overwrite-existing",
    }
}

/// The "replace the file?" box: Replace or Cancel, with a text for each
/// reason (several games, partly read, already there).
fn overwrite_box<'a>(
    i18n: &I18n,
    name: &str,
    games: usize,
    reason: OverwriteReason,
) -> Element<'a, Message> {
    let mut args = FluentArgs::new();
    args.set("name", name);
    args.set("games", i64::try_from(games).unwrap_or(i64::MAX));
    modal(Card {
        id: OVERWRITE_BOX_ID,
        title: i18n.tr("dialog-overwrite-title"),
        body: text(i18n.tr_args(overwrite_key(reason), &args)).into(),
        buttons: vec![
            answer_button(
                i18n.tr("dialog-overwrite-yes"),
                Message::App(app_api::answer_overwrite(true)),
                false,
            ),
            answer_button(
                i18n.tr("dialog-cancel"),
                Message::App(app_api::answer_overwrite(false)),
                true,
            ),
        ],
        width: 480.0,
    })
}

/// The "copy N games from X to Y?" box (Scid `copyFilter`): Copy or Cancel.
fn copy_box<'a>(i18n: &I18n, prompt: &CopyPrompt) -> Element<'a, Message> {
    let mut args = FluentArgs::new();
    args.set("count", prompt.count);
    args.set("from", status_bar::copy_end_label(i18n, &prompt.from));
    args.set("to", status_bar::copy_end_label(i18n, &prompt.to));
    let key = match prompt.scope {
        CopyScope::Filter => "dialog-copy-message",
        CopyScope::Selection => "dialog-copy-selected-message",
    };
    modal(Card {
        id: COPY_BOX_ID,
        title: i18n.tr("dialog-copy-title"),
        body: text(i18n.tr_args(key, &args)).into(),
        buttons: vec![
            answer_button(
                i18n.tr("dialog-copy-yes"),
                Message::App(app_api::answer_copy(true)),
                true,
            ),
            answer_button(
                i18n.tr("dialog-cancel"),
                Message::App(app_api::answer_copy(false)),
                false,
            ),
        ],
        width: 480.0,
    })
}

/// The modal box to show over the window, if a dialog is open.
#[allow(clippy::too_many_lines)] // one flat match over the dialogs
pub fn view(shell: &Shell) -> Option<Element<'_, Message>> {
    let app = &shell.app;
    let ui = &shell.ui.dialogs;
    let i18n = &shell.i18n;
    let window = shell
        .ui
        .window_size
        .unwrap_or(iced::Size::new(1280.0, 800.0));
    match ActiveDialog::of(app, ui)? {
        ActiveDialog::Discard => Some(discard_box(i18n)),
        ActiveDialog::Overwrite => {
            let prompt = app_api::overwrite_prompt(app)?;
            let name = prompt
                .path
                .file_name()
                .unwrap_or(prompt.path.as_os_str())
                .to_string_lossy();
            Some(overwrite_box(i18n, &name, prompt.games, prompt.reason))
        }
        ActiveDialog::CopyGames => app_api::copy_prompt(app).map(|p| copy_box(i18n, p)),
        ActiveDialog::Prompt => {
            app_api::session_prompt(app).map(|p| prompts::view(p, i18n, ui.dont_ask))
        }
        ActiveDialog::Setup => {
            let theme = app_api::appearance(app, shell.options).board.theme;
            setup_board::view(app, ui, i18n, theme)
        }
        ActiveDialog::Info => header_dialog::view(app, ui, i18n),
        ActiveDialog::Import => paste_pgn::view(app, ui, i18n),
        ActiveDialog::ImportFile => import_file_dialog::view(app, i18n),
        ActiveDialog::ExportGames => export_games_dialog::view(app, i18n),
        ActiveDialog::Merge => merge_dialog::view(shell),
        ActiveDialog::Goto => ui.goto.as_ref().map(|draft| goto_move::view(draft, i18n)),
        ActiveDialog::Chess960 => ui
            .chess960
            .as_ref()
            .map(|draft| crate::windows::chess960_dialog::view(draft, i18n)),
        ActiveDialog::Chooser => game_chooser::view(app, ui, i18n),
        ActiveDialog::Variations => variation_picker::view(app, i18n),
        ActiveDialog::Bookmarks => bookmark_editor::view(app, i18n),
        ActiveDialog::Annotate => annotate_dialog::view(app, ui.annotate_informant_open, i18n),
        ActiveDialog::ReportConfig => report_dialog::view(app, i18n),
        ActiveDialog::EcoBrowser => eco_browser::view(app, i18n),
        ActiveDialog::EcoClassify => eco_dialogs::classify(app, i18n),
        ActiveDialog::EcoNovelty => eco_dialogs::novelty(app, i18n),
        ActiveDialog::PlayConfig => play_dialog::view(app, &ui.play, i18n),
        ActiveDialog::PlayConfirm => ui
            .play
            .confirm
            .map(|kind| play_panel::confirm_box(kind, i18n)),
        ActiveDialog::PlayAsk => ui
            .play_view(app)
            .and_then(|v| v.question)
            .map(|q| play_panel::ask_box(q, i18n)),
        ActiveDialog::PlayEnd => ui.play_view(app).and_then(|v| match v.phase {
            mychess_app::PlayPhase::Ended(end) => Some(play_panel::end_box_with(
                end,
                &v,
                app.play_tactical().as_ref(),
                play_panel::current_base_read_only(app),
                i18n,
            )),
            _ => None,
        }),
        ActiveDialog::PlayLost => Some(play_panel::lost_box(app, i18n)),
        ActiveDialog::TournamentForm => tournament_dialog::form(app, &ui.tournament, i18n),
        ActiveDialog::TournamentResume => Some(tournament_dialog::resume_list(app, i18n)),
        ActiveDialog::TournamentConfirm => ui
            .tournament
            .confirm
            .map(|kind| tournament_dialog::confirm_box(kind, i18n)),
        ActiveDialog::TournamentDecision => {
            tournament_dialog::decision_box(app, &ui.tournament, i18n)
        }
        ActiveDialog::OnlineConnect => Some(online_dialog::connect(app, &ui.online, i18n)),
        ActiveDialog::OnlineSeek => Some(online_dialog::seek(app, &ui.online, i18n)),
        ActiveDialog::OnlineChallenge => Some(online_dialog::challenge(app, &ui.online, i18n)),
        ActiveDialog::OnlineIncoming => Some(online_dialog::incoming(app, i18n)),
        ActiveDialog::OnlineWatch => Some(online_dialog::watch(app, &ui.online, i18n)),
        ActiveDialog::OnlinePromotion => Some(online_dialog::promotion(i18n)),
        ActiveDialog::OnlineConfirm => ui
            .online
            .confirm
            .map(|kind| online_dialog::confirm(kind, i18n)),
        ActiveDialog::FicsConfirm => Some(crate::windows::fics_dialog::confirm_resign(i18n)),
        ActiveDialog::Engines => engines_dialog::view_with(
            app,
            ui.engines_prefs_open.then(|| engines_dialog::PrefsPanel {
                log_text: ui.engines_prefs_text[0].as_deref(),
                ply_text: ui.engines_prefs_text[1].as_deref(),
            }),
            i18n,
        ),
        ActiveDialog::EngineEditor => engine_editor::view(app, i18n),
        ActiveDialog::EngineOptions => engine_options::view(app, i18n),
        ActiveDialog::EngineDelete => engines_dialog::delete_view(app, i18n),
        ActiveDialog::BookQuit => Some(book_panel::quit_box(i18n)),
        ActiveDialog::TacticsSetup => Some(crate::windows::tactics_dialog::view(
            &ui.tactics,
            &app.tactics_view(),
            i18n,
        )),
        ActiveDialog::OpeningSetup => {
            let opening = app.opening_view();
            Some(crate::windows::opening_trainer_dialog::view(
                &ui.opening,
                &opening,
                ui.opening.names(),
                i18n,
            ))
        }
        ActiveDialog::Eboard => ui
            .eboard
            .as_ref()
            .map(|eboard| crate::windows::eboard_dialog::view(app, eboard, i18n)),
        ActiveDialog::Sound => Some(crate::windows::sound_dialog::view(app, i18n)),
        ActiveDialog::Fics => Some(crate::windows::fics_dialog::view(app, &ui.fics, i18n)),
        ActiveDialog::PlayerFinder => app.player_finder_view().map(|v| {
            crate::windows::player_tools::finder_view::players(&ui.player_tools, &v, i18n, window)
        }),
        ActiveDialog::TournamentFinder => app.tournament_finder_view().map(|v| {
            crate::windows::player_tools::finder_view::tournaments(
                &ui.player_tools,
                &v,
                i18n,
                window,
            )
        }),
        ActiveDialog::PlayerInfo => app
            .player_info_view()
            .map(|v| crate::windows::player_tools::info_view::view(&v, i18n, window)),
        ActiveDialog::SpellCheck => app.spell_view().map(|v| {
            crate::windows::player_tools::spell_view::check_view(&ui.player_tools, &v, i18n, window)
        }),
        ActiveDialog::AddElo => app
            .add_elo_view()
            .map(|v| crate::windows::player_tools::spell_view::add_elo_view(&v, i18n, window)),
        ActiveDialog::AdvancedSearch => app.advanced_search_view().map(|view| {
            crate::windows::advanced_search_dialog::view(&ui.advanced, &view, i18n, window.height)
        }),
        ActiveDialog::PatternSearch => app.pattern_search_view().map(|view| {
            crate::windows::pattern_search_dialog::view(&ui.pattern, &view, i18n, window.height)
        }),
        ActiveDialog::PatternCost => app
            .pattern_search_confirm()
            .map(|cost| crate::windows::pattern_search_dialog::cost_view(cost, i18n)),
        ActiveDialog::CalvarSetup => Some(crate::windows::calvar_dialog::view(
            &ui.calvar,
            &app.calvar_view(),
            &app.calvar_engine_choices(),
            i18n,
        )),
        ActiveDialog::MaskPrompt => app.mask_prompt().map(|p| mask_dialogs::prompt(p, i18n)),
        ActiveDialog::MaskFillLine => Some(mask_dialogs::fill_line(i18n)),
        ActiveDialog::MaskComment => ui
            .mask
            .comment
            .as_ref()
            .map(|draft| mask_dialogs::comment(draft, i18n)),
        ActiveDialog::MaskSearch => ui
            .mask
            .search
            .as_ref()
            .map(|search| mask_dialogs::search(search, app, i18n, window)),
        ActiveDialog::MaskOutline => {
            Some(mask_dialogs::outline(app, &ui.mask.outline, i18n, window))
        }
        ActiveDialog::Appearance => ui
            .appearance
            .as_ref()
            .map(|window| appearance_view::view(window, shell.ui.system_mode, i18n)),
    }
}

#[cfg(test)]
mod tests {
    use iced::{Settings, Size};
    use iced_test::Simulator;

    use super::*;
    use crate::i18n::Language;

    fn messages(element: Element<'_, Message>, click: &str) -> Vec<Message> {
        let mut sim = Simulator::with_size(Settings::default(), Size::new(900.0, 600.0), element);
        sim.click(click).expect("button exists");
        sim.into_messages().collect()
    }

    #[test]
    fn the_discard_box_offers_save_dont_save_and_cancel() {
        let i18n = I18n::new(Language::EnUs);
        for (label, expected) in [
            ("Save", DiscardChoice::Save),
            ("Don't save", DiscardChoice::DontSave),
            ("Cancel", DiscardChoice::Cancel),
        ] {
            let sent = messages(discard_box(&i18n), label);
            assert!(
                matches!(
                    sent.as_slice(),
                    [Message::App(mychess_app::Message::AnswerDiscard(choice))] if *choice == expected
                ),
                "{label}: {sent:?}"
            );
        }
    }

    #[test]
    fn the_discard_box_is_translated() {
        let pt = I18n::new(Language::PtBr);
        let mut sim = Simulator::with_size(
            Settings::default(),
            Size::new(900.0, 600.0),
            discard_box(&pt),
        );
        for text in ["Salvar", "Não salvar", "Cancelar", "Alterações não salvas"] {
            assert!(sim.find(text).is_ok(), "{text}");
        }
        assert!(sim.find(iced::widget::Id::new(DISCARD_BOX_ID)).is_ok());
    }

    #[test]
    fn the_overwrite_box_names_the_file_and_the_number_of_games() {
        let en = I18n::new(Language::EnUs);
        let mut sim = Simulator::with_size(
            Settings::default(),
            Size::new(900.0, 600.0),
            overwrite_box(&en, "big.pgn", 3, OverwriteReason::MultiGame),
        );
        assert!(
            sim.find("big.pgn holds 3 games. Replace all of them with this game?")
                .is_ok()
        );
        let one = overwrite_box(&en, "one.pgn", 1, OverwriteReason::MultiGame);
        let mut sim = Simulator::with_size(Settings::default(), Size::new(900.0, 600.0), one);
        assert!(
            sim.find("one.pgn holds one game. Replace all of them with this game?")
                .is_ok()
        );
    }

    #[test]
    fn each_reason_has_its_own_text_and_only_the_multi_game_one_counts() {
        let en = I18n::new(Language::EnUs);
        for (reason, expected) in [
            (
                OverwriteReason::ExistingFile,
                "x.pgn already exists. Replace it?",
            ),
            (
                OverwriteReason::PartiallyRead,
                "x.pgn was only partly read; replacing it erases the rest. Replace it?",
            ),
        ] {
            let mut sim = Simulator::with_size(
                Settings::default(),
                Size::new(900.0, 600.0),
                overwrite_box(&en, "x.pgn", 0, reason),
            );
            assert!(sim.find(expected).is_ok(), "{expected}");
        }
        let pt = I18n::new(Language::PtBr);
        let mut sim = Simulator::with_size(
            Settings::default(),
            Size::new(900.0, 600.0),
            overwrite_box(&pt, "x.pgn", 0, OverwriteReason::ExistingFile),
        );
        assert!(sim.find("x.pgn já existe. Substituir?").is_ok());
    }

    #[test]
    fn the_overwrite_box_answers_yes_or_no() {
        let en = I18n::new(Language::EnUs);
        let yes = messages(
            overwrite_box(&en, "a.pgn", 2, OverwriteReason::MultiGame),
            "Replace",
        );
        assert!(matches!(
            yes.as_slice(),
            [Message::App(mychess_app::Message::AnswerOverwrite(true))]
        ));
        let no = messages(
            overwrite_box(&en, "a.pgn", 2, OverwriteReason::MultiGame),
            "Cancel",
        );
        assert!(matches!(
            no.as_slice(),
            [Message::App(mychess_app::Message::AnswerOverwrite(false))]
        ));
    }

    #[test]
    fn the_window_shows_the_box_only_while_a_question_is_open() {
        let mut shell = Shell::new(None);
        assert!(view(&shell).is_none());
        let mv = shell
            .app
            .session
            .position()
            .find_move(
                "e2".parse().expect("square"),
                "e4".parse().expect("square"),
                None,
            )
            .expect("legal");
        drop(shell.update(Message::App(app_api::play_move(mv))));
        drop(shell.update(Message::App(app_api::command(
            mychess_app::Command::NewGame,
        ))));
        assert!(view(&shell).is_some());
        drop(shell.update(Message::App(app_api::answer_discard(DiscardChoice::Cancel))));
        assert!(view(&shell).is_none());
    }

    /// Opens each dialog through the shell and renders the whole window with
    /// it: the box must be in the tree and the page below must still build.
    #[test]
    fn every_dialog_is_drawn_over_the_window_when_it_opens() {
        use crate::windows::dialog_ui::DialogMessage;
        use crate::windows::main_window;

        let cases: Vec<(&str, Message)> = vec![
            (
                "setup-box",
                Message::App(app_api::command(mychess_app::Command::SetupBoard)),
            ),
            (
                "header-box",
                Message::Dialog(DialogMessage::OpenHeader(Some(
                    mychess_app::TagField::Black,
                ))),
            ),
            (
                "paste-pgn-box",
                Message::App(mychess_app::Message::Import(
                    mychess_app::ImportMessage::Open,
                )),
            ),
            ("goto-box", Message::Dialog(DialogMessage::OpenGoto)),
            ("chess960-box", Message::Dialog(DialogMessage::OpenChess960)),
            (
                "bookmarks-box",
                Message::App(mychess_app::Message::Bookmarks(
                    mychess_app::BookmarksMessage::OpenEditor,
                )),
            ),
        ];
        for (id, open) in cases {
            let mut shell = Shell::new(None);
            drop(shell.update(open));
            let element = main_window::view(&shell);
            let mut sim =
                Simulator::with_size(Settings::default(), Size::new(1300.0, 900.0), element);
            assert!(sim.find(iced::widget::Id::new(id)).is_ok(), "{id}");
            assert!(
                sim.find(iced::widget::Id::new("board-pane")).is_ok(),
                "{id}: page below"
            );
        }
        let mut shell = Shell::new(None);
        for (from, to) in [("e2", "e4"), ("e7", "e5")] {
            let mv = shell
                .app
                .session
                .position()
                .find_move(
                    from.parse().expect("square"),
                    to.parse().expect("square"),
                    None,
                )
                .expect("legal");
            drop(shell.update(Message::App(app_api::play_move(mv))));
        }
        drop(shell.update(Message::App(app_api::navigate(mychess_app::Nav::Start))));
        let mv = shell
            .app
            .session
            .position()
            .find_move(
                "d2".parse().expect("square"),
                "d4".parse().expect("square"),
                None,
            )
            .expect("legal");
        drop(shell.update(Message::App(app_api::play_move(mv))));
        let element = main_window::view(&shell);
        let mut sim = Simulator::with_size(Settings::default(), Size::new(1300.0, 900.0), element);
        assert!(sim.find(iced::widget::Id::new("replace-prompt")).is_ok());
    }

    #[test]
    fn escape_cancels_and_enter_confirms_the_bookmark_editor() {
        use crate::windows::dialog_ui::DialogMessage;
        use mychess_app::{BookmarksMessage, Key, KeyInput, Modifiers, NamedKey};
        let key = |named| {
            Message::Dialog(DialogMessage::Key {
                input: KeyInput {
                    key: Key::Named(named),
                    modifiers: Modifiers::NONE,
                },
                text_focused: false,
                typed: None,
            })
        };
        let open = || {
            let mut shell = Shell::new(None);
            drop(shell.update(Message::App(mychess_app::Message::Bookmarks(
                BookmarksMessage::OpenEditor,
            ))));
            shell
        };
        let mut shell = open();
        assert!(shell.app.bookmark_editor.is_some());
        drop(shell.update(key(NamedKey::Escape)));
        assert!(shell.app.bookmark_editor.is_none());
        let mut shell = open();
        drop(shell.update(key(NamedKey::Enter)));
        assert!(shell.app.bookmark_editor.is_none());
    }
}
