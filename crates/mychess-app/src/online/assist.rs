//! The lock of the assistance during an online game (spec M8-3, D1330,
//! D1332): which messages are dropped, and the marker file that keeps the
//! lock on across a restart of the program.

use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::message::MarkerInfo;
use crate::analysis::AnalysisMessage;
use crate::engines::EngineMessage;
use crate::message::Message;
use crate::session::GameMessage;
use crate::shortcuts::Action;

/// The name of the marker file, in the directory of the preferences.
pub const MARKER_FILE: &str = "online-active.toml";

/// Longest marker file read, in bytes (it holds two short values).
const MAX_MARKER_BYTES: u64 = 4096;

/// What kind of help a message asks for (the table of spec 4.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Family {
    /// Engines: analysis, threat, shoot-out, automove, annotation, games and
    /// tournaments between engines.
    Engines,
    /// The evaluation bar, the arrows, the traffic light and the graphs.
    Evaluation,
    /// The opening tree and the training of openings.
    Openings,
    /// Inserting moves, variations, comments or NAGs in the game, and the
    /// trial mode.
    Edit,
    /// The tablebases: the panel, the training and the results of the
    /// moves (spec M8-8, D2570).
    Tablebases,
}

/// The family of help `message` asks for, or `None` when it is allowed
/// during an online game.
///
/// Every message that starts, feeds or shows engine output, opening
/// statistics, evaluations or that writes moves into the game is listed
/// here. A message of an assistive family that is not listed is a defect
/// (the sweep test of this module lists one example of each family).
#[allow(clippy::too_many_lines)] // one flat match over every message
pub fn family_of(message: &Message) -> Option<Family> {
    match message {
        Message::Engine(
            EngineMessage::Start(..)
            | EngineMessage::Restart(_)
            | EngineMessage::ToggleHotkey(_)
            | EngineMessage::ToggleMainAnalysis,
        )
        | Message::Analysis(
            AnalysisMessage::Toggle(_)
            | AnalysisMessage::AddMove(_)
            | AnalysisMessage::AddScore(_)
            | AnalysisMessage::AddLine(_)
            | AnalysisMessage::AddLineNumber(..)
            | AnalysisMessage::AddAllLines(..)
            | AnalysisMessage::SetMultiPv(..)
            | AnalysisMessage::ToggleLock(_)
            | AnalysisMessage::SetExclusion(..)
            | AnalysisMessage::RequestStats(_),
        )
        | Message::Annotate(
            crate::annotate::AnnotateMessage::Open | crate::annotate::AnnotateMessage::Start,
        )
        | Message::Mode(
            crate::training::ModeMessage::ToggleThreat(_)
            | crate::training::ModeMessage::ToggleFinishGame(_)
            | crate::training::ModeMessage::ToggleAutomove(_),
        )
        | Message::Report(
            crate::report::ReportMessage::Generate
            | crate::report::ReportMessage::Continue
            | crate::report::ReportMessage::OpenConfig
            | crate::report::ReportMessage::ConfigStart,
        )
        | Message::Play(crate::play::PlayMessage::Start(_))
        | Message::Tournament(
            crate::tournament::TournamentMessage::Start { .. }
            | crate::tournament::TournamentMessage::Resume { .. },
        ) => Some(Family::Engines),
        // Writing the report edits the game (D1447); listed here, before the
        // catch-all of the other report messages below.
        #[allow(clippy::match_same_arms)]
        Message::Report(crate::report::ReportMessage::WriteToGame) => Some(Family::Edit),
        // The training shows the right move and judges the user's: help of
        // the family of the engines (spec M8-4, D1932).
        Message::Tactics(m) => tactics_family(m),
        // Opening, editing and searching an EPD file is not help, but the
        // analysis of its positions and "Paste analysis" are engine help
        // (spec M7-5, D3821).
        Message::Epd(m) => epd_family(m),
        // Searching the databases is not help (D3239), but "test on this
        // game" runs the detectors, the mate solver included, on the
        // position being played: help of the family of the engines.
        Message::PatternSearch(crate::pattern_search::PatternSearchMessage::TestGame) => {
            Some(Family::Engines)
        }
        // The exercise shows the best lines of an engine: help of the family
        // of the engines (spec M8-6, D2309).
        Message::Calvar(m) => calvar_family(m),
        // The tablebase tells the result of every move of the endgame:
        // knowledge, help of its own family (spec M8-8, D2570).
        Message::Tablebase(m) => tablebase_family(m),
        Message::OpeningTrainer(m) => opening_trainer_family(m),
        Message::EvalBar(_)
        | Message::Graph(_)
        | Message::Report(
            crate::report::ReportMessage::FromGame
            | crate::report::ReportMessage::StartReview { .. }
            | crate::report::ReportMessage::ReviewTry
            | crate::report::ReportMessage::ReviewShowBest,
        ) => Some(Family::Evaluation),
        Message::Book(m) => match m {
            // Closing and the answers of the effects are never help.
            crate::book::BookMessage::SetOpen(false)
            | crate::book::BookMessage::SlotLoaded { .. }
            | crate::book::BookMessage::OpeningLoaded { .. }
            | crate::book::BookMessage::Saved { .. }
            | crate::book::BookMessage::Created { .. }
            | crate::book::BookMessage::Merged { .. }
            | crate::book::BookMessage::Exported { .. } => None,
            _ => Some(Family::Openings),
        },
        Message::Tree(m) => match m {
            crate::tree::TreeMessage::Finished(_) | crate::tree::TreeMessage::Close => None,
            crate::tree::TreeMessage::Mask(m) => mask_family(m),
            _ => Some(Family::Openings),
        },
        // The ECO browser, "identify opening" and "find novelty" show
        // opening knowledge; classifying a database is maintenance.
        Message::Eco(m) => eco_family(m),
        Message::Action(action) => action_family(*action),
        Message::Game(m) => game_family(m),
        // Everything else is listed so that a new variant of `Message` does
        // not compile until somebody decides (D1370). The inner enums of the
        // first group are covered above for what they do to the game.
        Message::Engine(_)
        | Message::Analysis(_)
        | Message::Annotate(_)
        | Message::Report(_)
        | Message::Mode(_)
        | Message::Play(_)
        | Message::Tournament(_)
        | Message::Command(_)
        | Message::Setup(_)
        | Message::Import(_)
        | Message::FileImport(_)
        | Message::GamesExport(_)
        | Message::ExportDocs(_)
        // Searching local databases is not help (spec M6-1, D3239).
        | Message::AdvancedSearch(_)
        // The other messages of the search for patterns read databases and files.
        | Message::PatternSearch(_)
        // The crosstable, the statistics, the rating graph and the piece
        // tracker read stored databases and say nothing about the position
        // or the game being played: not help (spec M6-4, D3394).
        | Message::Crosstable(_)
        // The spelling file, the finders and the player information read
        // stored databases and files, not the game being played (spec M6-3,
        // D3463, like D3239).
        | Message::Spelling(_)
        | Message::Finder(_)
        | Message::PlayerInfo(_)
        // Merging a game is a local edit of the game, not help; a live game
        // refuses it through `refuse_edit` (spec M7-6, rule 12).
        | Message::Merge(_)
        | Message::OpeningReport(_)
        | Message::Manual(_)
        | Message::Start(_)
        | Message::PlayerReport(_)
        // The maintenance of a database is a file action, not help (spec
        // M7-1, section "Adaptações").
        | Message::Maintenance(_)
        | Message::Charts(_)
        | Message::PieceTracker(_)
        | Message::FilesPicked(..)
        | Message::Info(_)
        | Message::Chooser(_)
        | Message::Bases(_)
        | Message::Bookmarks(_)
        | Message::Online(_)
        | Message::Fics(_)
        // The sounds are not help: they say what the board shows (spec M8-11, D2800).
        | Message::Sound(_)
        // The electronic board is an input device, like the mouse (spec M8-10, D2735).
        | Message::Eboard(_)
        | Message::Prefs(_)
        | Message::AnswerDiscard(_)
        | Message::AnswerOverwrite(_)
        | Message::Startup
        | Message::Quit
        | Message::FlushPreferences
        | Message::WindowGeometryChanged(_)
        | Message::FilePicked(..)
        | Message::ClipboardRead { .. }
        | Message::ClipboardWriteFailed
        | Message::HostEffectNotHandled
        | Message::PgnScanned { .. }
        | Message::CollectionGameParsed { .. }
        | Message::PgnSaved { .. }
        | Message::MovetextParsed { .. }
        | Message::PreferencesLoaded(_)
        | Message::PreferencesSaved(_)
        | Message::EcoLoaded { .. }
        | Message::Today(..) => None,
        // The keys typed at the board are replayed as move entry and NAG
        // sequences: they write into the game (D1370).
        Message::NagKey { .. } | Message::NagTick(_) | Message::NagInterrupt => Some(Family::Edit),
    }
}

/// The family of a message of the tree masks (spec M5-4b, D1860): what
/// shows lines of the repertoire for the position of the game (the
/// search, the outline) or opens the position of a line is help of the
/// family of the openings. Files, preferences, the answers of the effects
/// and the edits (which have no table to read the rows from during the game,
/// since the tree is locked) are not.
fn mask_family(message: &crate::tree::MaskMessage) -> Option<Family> {
    use crate::tree::MaskMessage as M;
    match message {
        M::Search(_) | M::BuildOutline | M::GoTo(_) | M::PlayOnlyRow(_) => Some(Family::Openings),
        _ => None,
    }
}

/// The family of a message of the tactics training: everything that starts
/// it, shows a hint or the solution or moves on is help; closing it and the
/// timer and the answers of the effects are not.
fn tactics_family(message: &crate::tactics::TacticsMessage) -> Option<Family> {
    use crate::tactics::TacticsMessage as T;
    match message {
        T::Open | T::Start { .. } | T::Hint | T::ShowSolution | T::Next | T::Retry => {
            Some(Family::Engines)
        }
        _ => None,
    }
}

/// The family of a message of the opening trainer (D2234): it is opening
/// knowledge, like the tree and the book. Starting, hints and moving on are
/// help; closing, the timer and the answers of the effects are not.
fn opening_trainer_family(message: &crate::opening_trainer::OpeningMessage) -> Option<Family> {
    use crate::opening_trainer::OpeningMessage as O;
    match message {
        O::Open | O::Start { .. } | O::Hint | O::Next | O::Retry | O::Report => {
            Some(Family::Openings)
        }
        _ => None,
    }
}

/// The family of a message of the calculation exercise: opening the
/// dialog, starting and adding the lines to the game is help; closing it,
/// the entry of the lines, the timer and the answers of the effects are not
/// (the exercise cannot be on during an online game).
fn calvar_family(message: &crate::calvar::CalvarMessage) -> Option<Family> {
    use crate::calvar::CalvarMessage as C;
    match message {
        C::Open
        | C::Start { .. }
        | C::StartFromGame { .. }
        | C::StartFromTactics { .. }
        | C::Review(_)
        | C::AddToGame => Some(Family::Engines),
        _ => None,
    }
}

/// The family of a message of the tablebase panel: opening it, playing a move
/// of its list, the training and the new positions are help; closing it, the
/// configuration and the answers of the effects are not (D2570).
fn tablebase_family(message: &crate::tablebase::TablebaseMessage) -> Option<Family> {
    use crate::tablebase::TablebaseMessage as T;
    match message {
        T::Toggle | T::SetOpen(true) | T::PlayRow(_) | T::TrainStart | T::NewPosition { .. } => {
            Some(Family::Tablebases)
        }
        _ => None,
    }
}

/// The family of an ECO message: the browser, "identify opening" and "find
/// novelty" show opening knowledge; classifying a database is maintenance.
fn eco_family(message: &crate::eco_ui::EcoMessage) -> Option<Family> {
    use crate::eco_ui::EcoMessage as E;
    match message {
        E::CloseBrowser
        | E::CloseClassify
        | E::CloseNovelty
        | E::StatsBuilt { .. }
        | E::OpenClassify
        | E::SetClassifyChoice(_)
        | E::SetClassifySince(_)
        | E::SetClassifyExtended(_)
        | E::ClassifyPreview
        | E::ClassifyRun => None,
        _ => Some(Family::Openings),
    }
}

/// The family of an [`Action`]; the list is exhaustive on purpose.
fn action_family(action: Action) -> Option<Family> {
    match action {
        Action::StartEngine(_)
        | Action::ToggleEngineAnalysis
        | Action::PlayEngine
        | Action::TacticalGame
        | Action::TacticsTrainer
        | Action::CalvarTrainer => Some(Family::Engines),
        Action::ToggleTablebasePane => Some(Family::Tablebases),
        // Statistics over the stored databases, not tied to the position
        // being played: not help (spec M6-4, D3394).
        Action::ToggleEvalBar
        | Action::ToggleTrafficLights
        | Action::ToggleArrowNumbers
        | Action::ToggleGraphPane => Some(Family::Evaluation),
        Action::ToggleTreePane | Action::ToggleBookPane | Action::OpeningTrainer => {
            Some(Family::Openings)
        }
        Action::AddVariation
        | Action::TruncateGame
        | Action::ToggleTrialMode
        | Action::ClearMoveEntry
        | Action::Undo
        | Action::Redo => Some(Family::Edit),
        Action::Back
        | Action::Forward
        | Action::BackFast
        | Action::ForwardFast
        | Action::GoStart
        | Action::GoEnd
        | Action::GotoMove
        | Action::FlipBoard
        | Action::EnterVariation
        | Action::ExitVariation
        | Action::NewGame
        | Action::OpenFile
        | Action::Save
        | Action::SaveAs
        | Action::AppendToPgn
        | Action::SetupBoard
        | Action::CopyFen
        | Action::PasteFen
        | Action::CopyPgn
        | Action::ImportPgn
        | Action::ImportPgnFile
        | Action::ExportGames
        // Searching local databases is not help (spec M6-1, D3239).
        | Action::SearchBoard
        | Action::SearchMaterial
        | Action::SearchMoves
        // Searching local databases is not help (spec M6-2).
        | Action::SearchPattern
        // The finders and the player information read stored databases
        // (spec M6-3, D3463).
        | Action::PlayerFinder
        | Action::TournamentFinder
        | Action::PlayerInfo
        | Action::MergeGame
        // Statistics over the stored databases, not tied to the position
        // being played: not help (spec M6-4, D3394).
        | Action::ToggleCrosstablePane
        | Action::ToggleStatsPane
        | Action::ToggleRatingPane
        | Action::TogglePieceTrackerPane
        // The EPD files are the user's own data (spec M7-5, D3821): only
        // pasting an analysis and analysing are help.
        | Action::ToggleEpdPane
        | Action::EpdFirst
        | Action::EpdPrev
        | Action::EpdNext
        | Action::EpdLast
        | Action::NextGame
        | Action::PrevGame
        | Action::FirstGame
        | Action::LastGame
        | Action::ToggleGameInfo
        | Action::EditGameInfo
        | Action::ShowNotation
        | Action::ShowCommentEditor
        | Action::BoardSmaller
        | Action::BoardLarger
        | Action::FullScreen
        | Action::Help
        | Action::ToggleAutoplay
        | Action::Quit
        | Action::OpenRecent(_)
        | Action::SelectBase(_)
        | Action::AddBookmark
        | Action::ManageBookmarks
        | Action::ManageEngines
        | Action::NextBase
        | Action::PreviousBase
        // Not assistance: they are the game itself (M8-12).
        | Action::ResignGame
        | Action::Rematch
        | Action::ToggleGameList
        | Action::ToggleAnalysisPane
        | Action::ListSelectAll
        | Action::ListRemoveFromFilter
        | Action::ListToggleDeleted
        | Action::ListFocusSearch
        | Action::TournamentEngines
        | Action::OnlineConnect
        | Action::OnlineSeek
        | Action::EboardDialog => None,
    }
}

/// The family of a message of the EPD window; the list is exhaustive on
/// purpose.
fn epd_family(message: &crate::epd_window::EpdMessage) -> Option<Family> {
    use crate::epd_window::EpdMessage as M;
    match message {
        M::Open
        | M::New
        | M::OpenPath(_)
        | M::CancelOpen
        | M::Save
        | M::SaveAs
        | M::Close(_)
        | M::AnswerClose(_)
        | M::Activate(_)
        | M::ClearRecent
        | M::SetSort(_)
        | M::SetFilter(_)
        | M::Select(_)
        | M::GoTo(_)
        | M::First
        | M::Last
        | M::Next
        | M::Prev
        | M::Random(_)
        | M::Edit(_)
        | M::Commit
        | M::AddPosition
        | M::RemovePosition
        | M::ConfirmRemove(_)
        | M::SortRecord
        | M::SortAll
        | M::StripOpcode(_)
        | M::Deepest
        | M::Find(..)
        | M::StopAnnotate
        | M::UndoRun
        | M::Tick { .. }
        | M::Opened { .. }
        | M::Saved { .. }
        | M::Rows { .. }
        | M::Found { .. }
        | M::Batch { .. } => None,
        // The analysis of the positions and "Paste analysis" are engine
        // help; so is any new message until somebody decides (`EpdMessage`
        // is non-exhaustive).
        _ => Some(Family::Engines),
    }
}

/// The family of a [`GameMessage`]; the list is exhaustive on purpose.
fn game_family(message: &GameMessage) -> Option<Family> {
    match message {
        GameMessage::PlayMove { .. }
        | GameMessage::PlaySquares { .. }
        | GameMessage::PlayNullMove
        | GameMessage::StartVariation
        | GameMessage::AnswerReplace(_)
        | GameMessage::AnswerPromotion(_)
        | GameMessage::ChooserConfirm(_)
        | GameMessage::MoveEntryChar(_)
        | GameMessage::MoveEntryTyped(_)
        | GameMessage::MoveEntryBackspace
        | GameMessage::MoveEntryCommit
        | GameMessage::MoveEntryClear
        | GameMessage::EnterMoveText(_)
        | GameMessage::PromoteVariation(_)
        | GameMessage::MakeFirstVariation(_)
        | GameMessage::DeleteVariation(_)
        | GameMessage::TruncateEnd
        | GameMessage::TruncateFrom
        | GameMessage::TruncateStart
        | GameMessage::StripComments
        | GameMessage::StripVariations
        | GameMessage::StripNags
        | GameMessage::RequestStrip { .. }
        | GameMessage::AnswerStrip(_)
        | GameMessage::SetComment { .. }
        | GameMessage::SetCommentBefore { .. }
        | GameMessage::AddNag(_)
        | GameMessage::ToggleNag(_)
        | GameMessage::ToggleNagAt { .. }
        | GameMessage::RemoveNag(_)
        | GameMessage::ClearNags
        | GameMessage::SetNags(_)
        | GameMessage::Undo
        | GameMessage::Redo
        | GameMessage::EnterTrial { .. }
        | GameMessage::ToggleTrial { .. }
        | GameMessage::AddEngineLine { .. }
        | GameMessage::AddEngineLines { .. }
        | GameMessage::PrependComment { .. } => Some(Family::Edit),
        GameMessage::Navigate(_)
        | GameMessage::OpenVariationChooser
        | GameMessage::ChooserMove(_)
        | GameMessage::CloseVariationChooser
        | GameMessage::ToggleAutoplay
        | GameMessage::StartAutoplay
        | GameMessage::StopAutoplay
        | GameMessage::SetAutoplayDelay(_)
        | GameMessage::AutoplayTick { .. }
        | GameMessage::DismissPrompt
        | GameMessage::ExitTrial
        | GameMessage::ApplySettings(_) => None,
    }
}

// ---- the marker file (D1332) ----

/// What the marker file holds.
#[derive(Debug, Serialize, Deserialize)]
struct MarkerFile {
    game: String,
    date: String,
}

/// The path of the marker inside `dir`.
pub fn marker_path(dir: &Path) -> PathBuf {
    dir.join(MARKER_FILE)
}

/// Reads the marker of `dir`. An absent, unreadable or malformed file gives
/// `None`: the lock is a courtesy, never a reason to fail.
pub fn read_marker(dir: &Path) -> Option<MarkerInfo> {
    use std::io::Read;
    let file = std::fs::File::open(marker_path(dir)).ok()?;
    let mut text = String::new();
    file.take(MAX_MARKER_BYTES).read_to_string(&mut text).ok()?;
    let parsed: MarkerFile = toml::from_str(&text).ok()?;
    let valid = (8..=12).contains(&parsed.game.len())
        && parsed.game.bytes().all(|b| b.is_ascii_alphanumeric());
    valid.then(|| MarkerInfo {
        game_id: parsed.game,
        date: parsed.date.chars().take(32).collect(),
    })
}

/// Writes the marker (a temporary file in `dir`, then a rename).
///
/// # Errors
///
/// The I/O error; the caller only logs it.
pub(crate) fn write_marker(dir: &Path, game_id: &str, date: &str) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let text = toml::to_string(&MarkerFile {
        game: game_id.to_owned(),
        date: date.to_owned(),
    })
    .map_err(std::io::Error::other)?;
    let mut temp = tempfile::NamedTempFile::new_in(dir)?;
    temp.write_all(text.as_bytes())?;
    temp.persist(marker_path(dir)).map_err(|e| e.error)?;
    Ok(())
}

/// Removes the marker; an absent file is fine.
///
/// # Errors
///
/// The I/O error other than "not found".
pub(crate) fn remove_marker(dir: &Path) -> std::io::Result<()> {
    match std::fs::remove_file(marker_path(dir)) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::Message;
    use mychess_core::Square;

    #[test]
    fn marker_round_trips_and_removal_is_idempotent() {
        let dir = tempfile::tempdir().expect("dir");
        assert_eq!(read_marker(dir.path()), None);
        write_marker(dir.path(), "abcd1234", "2026.10.24").expect("write");
        let info = read_marker(dir.path()).expect("marker");
        assert_eq!(info.game_id, "abcd1234");
        assert_eq!(info.date, "2026.10.24");
        remove_marker(dir.path()).expect("remove");
        remove_marker(dir.path()).expect("remove again");
        assert_eq!(read_marker(dir.path()), None);
    }

    #[test]
    fn a_malformed_marker_is_ignored() {
        let dir = tempfile::tempdir().expect("dir");
        for text in [
            "",
            "game = 3",
            "game = \"../etc\"\ndate = \"x\"",
            "\u{0}\u{1}",
        ] {
            std::fs::write(marker_path(dir.path()), text).expect("write");
            assert_eq!(read_marker(dir.path()), None, "{text:?}");
        }
    }

    /// The ways to put a move into the game from the keyboard or a chooser
    /// are edits too (D1370); the compiler forces every other variant to be
    /// classified, this lists the ones a review found missing.
    #[test]
    fn the_keyboard_entry_choosers_and_nag_keys_are_edits() {
        use crate::nag_shortcuts::NagKey;
        let messages = [
            Message::Game(GameMessage::MoveEntryChar('e')),
            Message::Game(GameMessage::MoveEntryTyped('e')),
            Message::Game(GameMessage::MoveEntryBackspace),
            Message::Game(GameMessage::MoveEntryClear),
            Message::Game(GameMessage::ChooserConfirm(Some(0))),
            Message::Game(GameMessage::AnswerPromotion(None)),
            Message::Action(Action::ClearMoveEntry),
            Message::NagKey {
                key: NagKey::Enter,
                at: std::time::Instant::now(),
            },
            Message::NagTick(std::time::Instant::now()),
            Message::NagInterrupt,
        ];
        for message in messages {
            assert_eq!(family_of(&message), Some(Family::Edit), "{message:?}");
        }
    }

    /// One example of each family of the table of spec 4.2.
    #[test]
    fn every_family_has_a_dropped_example_and_navigation_stays_allowed() {
        use crate::session::Nav;
        let sq = |s: &str| -> Square { s.parse().expect("square") };
        let engine = crate::engines::EngineId(1);
        let dropped = [
            (
                Message::Engine(EngineMessage::Start(
                    engine,
                    crate::engines::StartMode::Analyze,
                )),
                Family::Engines,
            ),
            (
                Message::Action(Action::ToggleEngineAnalysis),
                Family::Engines,
            ),
            (Message::Action(Action::StartEngine(1)), Family::Engines),
            (Message::Action(Action::PlayEngine), Family::Engines),
            (Message::Action(Action::TacticalGame), Family::Engines),
            (Message::Action(Action::TacticsTrainer), Family::Engines),
            (Message::Action(Action::OpeningTrainer), Family::Openings),
            (
                Message::OpeningTrainer(crate::opening_trainer::OpeningMessage::Hint),
                Family::Openings,
            ),
            (
                Message::Tactics(crate::tactics::TacticsMessage::Hint),
                Family::Engines,
            ),
            (
                Message::Annotate(crate::annotate::AnnotateMessage::Start),
                Family::Engines,
            ),
            (
                Message::EvalBar(crate::evalbar::EvalBarMessage::ToggleBar),
                Family::Evaluation,
            ),
            (
                Message::Graph(crate::graphs::GraphMessage::Toggle),
                Family::Evaluation,
            ),
            (
                Message::Tree(crate::tree::TreeMessage::Open),
                Family::Openings,
            ),
            (Message::Action(Action::ToggleTreePane), Family::Openings),
            (Message::Action(Action::ToggleBookPane), Family::Openings),
            (
                Message::Action(Action::ToggleTablebasePane),
                Family::Tablebases,
            ),
            (
                Message::Tablebase(crate::tablebase::TablebaseMessage::TrainStart),
                Family::Tablebases,
            ),
            (
                Message::Game(GameMessage::PlaySquares {
                    from: sq("e2"),
                    to: sq("e4"),
                    promotion: None,
                    options: crate::session::PlayOptions::default(),
                }),
                Family::Edit,
            ),
            (Message::Game(GameMessage::PlayNullMove), Family::Edit),
            (Message::Game(GameMessage::Undo), Family::Edit),
            (Message::Action(Action::ToggleTrialMode), Family::Edit),
        ];
        for (message, family) in dropped {
            assert_eq!(family_of(&message), Some(family), "{message:?}");
        }
        for message in [
            Message::Game(GameMessage::Navigate(Nav::Back(1))),
            Message::Action(Action::GoStart),
            Message::Action(Action::FlipBoard),
            Message::Tree(crate::tree::TreeMessage::Close),
            Message::Quit,
        ] {
            assert_eq!(family_of(&message), None, "{message:?}");
        }
    }
}
