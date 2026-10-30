//! What each [`Action`] does (spec M2-2, D148-D152).

use super::{AppState, update};
use crate::advanced_search::AdvancedSearchMessage;
use crate::bases::{BasesMessage, SlotId};
use crate::bookmarks::BookmarksMessage;
use crate::effect::Effect;
use crate::engines::EngineMessage;
use crate::evalbar::EvalBarMessage;
use crate::flow::Command;
use crate::game_info::{GameInfoMessage, in_variation};
use crate::games_export::GamesExportMessage;
use crate::import::ImportMessage;
use crate::message::Message;
use crate::session::{GameMessage, Nav};
use crate::shortcuts::Action;

/// How far the up and down arrows go (the Scid's code, not its help).
const FAST_STEP: u32 = 10;

impl AppState {
    /// Runs an action. Actions that only the front-end can carry out
    /// (go to move, panels, full screen, help, board size) do nothing here.
    #[allow(clippy::too_many_lines)] // one flat match over the actions reads best
    pub(crate) fn action(&mut self, action: Action, effects: &mut Vec<Effect>) {
        // On a moment of the guided review the arrows visit the moments
        // (D2118).
        if let Some(forward) = match action {
            Action::Forward => Some(true),
            Action::Back => Some(false),
            _ => None,
        } && self.review_owns_arrows()
        {
            self.report_message(
                if forward {
                    crate::report::ReportMessage::ReviewNext
                } else {
                    crate::report::ReportMessage::ReviewPrevious
                },
                effects,
            );
            return;
        }
        let message = match action {
            Action::Back => game(GameMessage::Navigate(Nav::Back(1))),
            Action::Forward => game(GameMessage::Navigate(Nav::Forward(1))),
            Action::BackFast => {
                let nav = if in_variation(self.session.game(), self.session.cursor()) {
                    Nav::ExitVariation
                } else {
                    Nav::Back(FAST_STEP)
                };
                game(GameMessage::Navigate(nav))
            }
            Action::ForwardFast => game(GameMessage::Navigate(Nav::Forward(FAST_STEP))),
            Action::GoStart => game(GameMessage::Navigate(Nav::Home)),
            Action::GoEnd => game(GameMessage::Navigate(Nav::End)),
            Action::FlipBoard => {
                self.board_flipped = !self.board_flipped;
                return;
            }
            Action::ClearMoveEntry => {
                // `Esc` also stops a shoot-out or an automove (D492, D493).
                self.training_stop_by_user(effects);
                game(GameMessage::MoveEntryClear)
            }
            Action::TruncateGame => game(GameMessage::TruncateEnd),
            Action::AddVariation => game(GameMessage::StartVariation),
            Action::EnterVariation => game(GameMessage::OpenVariationChooser),
            Action::ExitVariation => game(GameMessage::Navigate(Nav::ExitVariation)),
            Action::ToggleTrialMode => game(GameMessage::ToggleTrial { null_move: false }),
            Action::Undo => game(GameMessage::Undo),
            Action::Redo => game(GameMessage::Redo),
            Action::EditGameInfo => Message::Info(GameInfoMessage::Open),
            Action::ToggleAutoplay => game(GameMessage::ToggleAutoplay),
            Action::NewGame => Message::Command(Command::NewGame),
            Action::OpenFile => Message::Command(Command::Open),
            Action::Save => Message::Command(Command::Save),
            Action::SaveAs => Message::Command(Command::SaveAs),
            Action::AppendToPgn => Message::Command(Command::Append),
            Action::SetupBoard => Message::Command(Command::SetupBoard),
            Action::CopyFen => Message::Command(Command::CopyFen),
            Action::PasteFen => Message::Command(Command::PasteFen),
            Action::CopyPgn => Message::Command(Command::CopyPgn),
            Action::ImportPgn => Message::Import(ImportMessage::Open),
            Action::ImportPgnFile => {
                Message::FileImport(crate::file_import::FileImportMessage::Open)
            }
            Action::ExportGames => Message::GamesExport(GamesExportMessage::Open),
            Action::SearchBoard => Message::AdvancedSearch(AdvancedSearchMessage::Open(
                crate::advanced_search::AdvancedKind::Position,
            )),
            Action::SearchMaterial => Message::AdvancedSearch(AdvancedSearchMessage::Open(
                crate::advanced_search::AdvancedKind::Material,
            )),
            Action::SearchMoves => Message::AdvancedSearch(AdvancedSearchMessage::Open(
                crate::advanced_search::AdvancedKind::Moves,
            )),
            Action::SearchPattern => {
                Message::PatternSearch(crate::pattern_search::PatternSearchMessage::Open)
            }
            Action::PlayerFinder => Message::Finder(crate::finder_ui::FinderMessage::TogglePlayers),
            Action::TournamentFinder => {
                Message::Finder(crate::finder_ui::FinderMessage::ToggleTournaments)
            }
            Action::PlayerInfo => {
                Message::PlayerInfo(crate::player_info_ui::PlayerInfoMessage::Open {
                    name: String::new(),
                })
            }
            Action::MergeGame => {
                self.merge_selected(effects);
                return;
            }
            Action::NextGame => Message::Command(Command::NextGame),
            Action::PrevGame => Message::Command(Command::PrevGame),
            Action::FirstGame => Message::Command(Command::FirstGame),
            Action::LastGame => Message::Command(Command::LastGame),
            Action::OpenRecent(n) => {
                Message::Command(Command::OpenRecent(usize::from(n.saturating_sub(1))))
            }
            Action::SelectBase(n) => match SlotId::from_number(usize::from(n)) {
                Some(slot) => Message::Bases(BasesMessage::Select(slot)),
                None => return,
            },
            Action::AddBookmark => Message::Bookmarks(BookmarksMessage::AddCurrent),
            Action::ManageBookmarks => Message::Bookmarks(BookmarksMessage::OpenEditor),
            Action::StartEngine(n) => {
                self.engine_key_action(n, effects);
                return;
            }
            Action::ManageEngines => Message::Engine(EngineMessage::OpenDialog),
            Action::ToggleEngineAnalysis => Message::Engine(EngineMessage::ToggleMainAnalysis),
            Action::ToggleEvalBar => Message::EvalBar(EvalBarMessage::ToggleBar),
            Action::ToggleTrafficLights => Message::EvalBar(EvalBarMessage::ToggleTrafficLights),
            Action::ToggleArrowNumbers => Message::EvalBar(EvalBarMessage::ToggleArrowNumbers),
            Action::NextBase => Message::Bases(BasesMessage::Next),
            Action::PreviousBase => Message::Bases(BasesMessage::Previous),
            Action::Quit => Message::Quit,
            // The front-end's own business (dialogs, panels, window), the
            // panes included: `ToggleGraphPane` is like `ToggleAnalysisPane`,
            // since the layout decides what is open (D930).
            _ => return,
        };
        effects.extend(update(self, message));
    }
}

fn game(message: GameMessage) -> Message {
    Message::Game(message)
}
