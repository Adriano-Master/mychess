//! The central keyboard shortcut map (spec M2-2, D148-D154), pure data with
//! no GUI toolkit.
//!
//! The front-end turns a key press into a [`KeyInput`] (letters in lower
//! case, `shift` apart, `command` meaning Ctrl on Linux/Windows and Cmd on
//! macOS) and asks [`Shortcuts::lookup_in`] for an [`Action`], which it
//! sends to `update` as `Message::Action`.
//!
//! Where the Scid's help and code differ, the code wins (D152): the up and
//! down arrows move 10 plies, and only `Delete` truncates the game.

use std::collections::BTreeMap;
use std::fmt;

/// Something the user can trigger from the keyboard or a menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[non_exhaustive]
pub enum Action {
    /// One ply back.
    Back,
    /// One ply forward.
    Forward,
    /// Leaves the variation, or goes 10 plies back.
    BackFast,
    /// 10 plies forward.
    ForwardFast,
    /// To the start of the game.
    GoStart,
    /// To the end of the game.
    GoEnd,
    /// Asks for a move number and goes there (the front-end's dialog).
    GotoMove,
    /// Turns the board.
    FlipBoard,
    /// Clears the keyboard move entry.
    ClearMoveEntry,
    /// Deletes everything after the cursor.
    TruncateGame,
    /// Arms a new variation.
    AddVariation,
    /// Enters the variation (or opens the chooser).
    EnterVariation,
    /// Leaves the variation.
    ExitVariation,
    /// Toggles trial mode.
    ToggleTrialMode,
    /// Undo.
    Undo,
    /// Redo.
    Redo,
    /// New game.
    NewGame,
    /// Open a PGN file.
    OpenFile,
    /// Save.
    Save,
    /// Save as a PGN file.
    SaveAs,
    /// Append the game to a PGN file.
    AppendToPgn,
    /// Set up a position.
    SetupBoard,
    /// Copy the position as FEN.
    CopyFen,
    /// Paste a FEN.
    PasteFen,
    /// Copy the game as PGN.
    CopyPgn,
    /// Import PGN text.
    ImportPgn,
    /// Import PGN files into a database (spec M3-8, D1970); it has no
    /// default key.
    ImportPgnFile,
    /// Export games to a PGN file (spec M3-9, D2000); it has no default key.
    ExportGames,
    /// Opens the search by position (spec M6-1, D3249 item 7); no default
    /// key.
    SearchBoard,
    /// Opens the search by material and patterns (spec M6-1); no default
    /// key.
    SearchMaterial,
    /// Opens the search by a sequence of moves (spec M6-1); no default key.
    SearchMoves,
    /// Opens the search for tactics and patterns (spec M6-2); no default key
    /// (D3289).
    SearchPattern,
    /// Opens the player finder (spec M6-3, D3349): `Command+Shift+F5`.
    PlayerFinder,
    /// Opens the tournament finder (spec M6-3, D3349): `Command+Shift+F4`.
    TournamentFinder,
    /// Opens the player information window on the last player looked up (or
    /// the White player of the game); no default key (spec M6-3, D3349).
    PlayerInfo,
    /// Opens the merge dialog on the game selected in the list of the current
    /// database; no default key (spec M7-6, D3852).
    MergeGame,
    /// Next game of the open file.
    NextGame,
    /// Previous game of the open file.
    PrevGame,
    /// First game of the open file.
    FirstGame,
    /// Last game of the open file.
    LastGame,
    /// Shows or hides the game information panel.
    ToggleGameInfo,
    /// Opens the game information dialog (the tags of the game).
    EditGameInfo,
    /// Shows the notation panel.
    ShowNotation,
    /// Shows the comment editor.
    ShowCommentEditor,
    /// Smaller board.
    BoardSmaller,
    /// Larger board.
    BoardLarger,
    /// Full screen.
    FullScreen,
    /// Help.
    Help,
    /// Starts or stops autoplay.
    ToggleAutoplay,
    /// Quit.
    Quit,
    /// Opens the nth recent file (1-based); no default key.
    OpenRecent(u8),
    /// Makes database slot `n` (1 to 8; 9 is the Clipbase) the current one
    /// (spec M3-4, D504). Not configurable, like `OpenRecent`.
    SelectBase(u8),
    /// Bookmarks the game being edited (spec M3-6, D679).
    AddBookmark,
    /// Opens the bookmarks editor.
    ManageBookmarks,
    /// Starts (or ends) the engine of key F2, F3 or F4 (`1` to `3`; spec
    /// M4-3). Not configurable, like `OpenRecent`: the keys are the ones of
    /// the engine list.
    StartEngine(u8),
    /// Opens the "Engines" dialog.
    ManageEngines,
    /// Turns the analysis of the first running engine on or off.
    ToggleEngineAnalysis,
    /// Shows or hides the evaluation bar (spec M4-5, D455).
    ToggleEvalBar,
    /// Shows or hides the MultiPV traffic lights.
    ToggleTrafficLights,
    /// Numbers the secondary arrows of the engine, or not.
    ToggleArrowNumbers,
    /// The next open database, in order, wrapping around.
    NextBase,
    /// The previous open database, in order, wrapping around.
    PreviousBase,
    /// Shows or hides the game list panel (the front-end's business).
    ToggleGameList,
    /// Shows or hides the analysis panel (the front-end's business; spec
    /// M4-4).
    ToggleAnalysisPane,
    /// Shows or hides the score and time graph (the front-end's business;
    /// spec M4-7, D870).
    ToggleGraphPane,
    /// Shows or hides the opening tree (the front-end's business; spec
    /// M5-2, D971).
    ToggleTreePane,
    /// Shows or hides the book panel (the front-end's business; spec
    /// M5-6, D1790).
    ToggleBookPane,
    /// Shows or hides the tablebase panel (the front-end's business; spec
    /// M8-8, D2579).
    ToggleTablebasePane,
    /// Shows or hides the crosstable panel (the front-end's business; spec
    /// M6-4, D3395: no default key).
    ToggleCrosstablePane,
    /// Shows or hides the statistics panel (the front-end's business; spec
    /// M6-4, D3395: no default key).
    ToggleStatsPane,
    /// Shows or hides the rating graph (the front-end's business; spec
    /// M6-4, D3395: no default key).
    ToggleRatingPane,
    /// Shows or hides the piece tracker (the front-end's business; spec
    /// M6-4, D3395: no default key).
    TogglePieceTrackerPane,
    /// Shows or hides the EPD panel (the front-end's business; spec M7-5c,
    /// D3850: no default key).
    ToggleEpdPane,
    /// Loads the first record of the active EPD file (spec M7-5c, D3851).
    EpdFirst,
    /// Loads the previous record of the active EPD file.
    EpdPrev,
    /// Loads the next record of the active EPD file.
    EpdNext,
    /// Loads the last record of the active EPD file.
    EpdLast,
    /// Selects the games visible in the game list (slice 4c; the
    /// front-end's business: it knows which rows are visible).
    ListSelectAll,
    /// Removes the selected games of the list from the filter.
    ListRemoveFromFilter,
    /// Toggles the "deleted" mark of the selected games of the list.
    ListToggleDeleted,
    /// Moves the focus to the quick-search field of the game list.
    ListFocusSearch,
    /// Opens the "Play against the engine" dialog (spec M8-1b; the
    /// front-end's business).
    PlayEngine,
    /// Opens the dialog of the tactical game, a game against an engine of
    /// limited strength (spec M8-7, D2469; the front-end's business).
    TacticalGame,
    /// Opens the dialog of the tactics training (spec M8-4, D1937; the
    /// front-end's business).
    TacticsTrainer,
    /// Opens the dialog of the opening trainer (spec M8-5, D2238; the
    /// front-end's business).
    OpeningTrainer,
    /// Opens the dialog of the calculation exercise (spec M8-6, D2325; the
    /// front-end's business).
    CalvarTrainer,
    /// Opens the dialog of a tournament between engines (spec M8-2b,
    /// D1361; the front-end's business).
    TournamentEngines,
    /// Opens the "Connect to Lichess" dialog (spec M8-3c, D1470; the
    /// front-end's business).
    OnlineConnect,
    /// Opens the "Find a game" dialog of Lichess (spec M8-3c, D1470; the
    /// front-end's business).
    OnlineSeek,
    /// Opens the window of the electronic board (spec M8-10, D2742; the
    /// front-end's business).
    EboardDialog,
    /// Resigns the game of the user, in the mode that is on (spec M8-12,
    /// D3100; the front-end's business, it asks first like the buttons do).
    ResignGame,
    /// Plays the finished game again with the colours swapped (spec M8-12,
    /// D3101; the front-end's business).
    Rematch,
}

/// Name (the TOML key), Fluent key of every action except `OpenRecent`.
const NAMES: &[(Action, &str, &str)] = &[
    (Action::Back, "Back", "action-back"),
    (Action::Forward, "Forward", "action-forward"),
    (Action::BackFast, "BackFast", "action-back-fast"),
    (Action::ForwardFast, "ForwardFast", "action-forward-fast"),
    (Action::GoStart, "GoStart", "action-go-start"),
    (Action::GoEnd, "GoEnd", "action-go-end"),
    (Action::GotoMove, "GotoMove", "action-goto-move"),
    (Action::FlipBoard, "FlipBoard", "action-flip-board"),
    (
        Action::ClearMoveEntry,
        "ClearMoveEntry",
        "action-clear-move-entry",
    ),
    (Action::TruncateGame, "TruncateGame", "action-truncate-game"),
    (Action::AddVariation, "AddVariation", "action-add-variation"),
    (
        Action::EnterVariation,
        "EnterVariation",
        "action-enter-variation",
    ),
    (
        Action::ExitVariation,
        "ExitVariation",
        "action-exit-variation",
    ),
    (
        Action::ToggleTrialMode,
        "ToggleTrialMode",
        "action-toggle-trial-mode",
    ),
    (Action::Undo, "Undo", "action-undo"),
    (Action::Redo, "Redo", "action-redo"),
    (Action::NewGame, "NewGame", "action-new-game"),
    (Action::OpenFile, "OpenFile", "action-open-file"),
    (Action::Save, "Save", "action-save"),
    (Action::SaveAs, "SaveAs", "action-save-as"),
    (Action::AppendToPgn, "AppendToPgn", "action-append-to-pgn"),
    (Action::SetupBoard, "SetupBoard", "action-setup-board"),
    (Action::CopyFen, "CopyFen", "action-copy-fen"),
    (Action::PasteFen, "PasteFen", "action-paste-fen"),
    (Action::CopyPgn, "CopyPgn", "action-copy-pgn"),
    (Action::ImportPgn, "ImportPgn", "action-import-pgn"),
    (Action::NextGame, "NextGame", "action-next-game"),
    (Action::PrevGame, "PrevGame", "action-prev-game"),
    (Action::FirstGame, "FirstGame", "action-first-game"),
    (Action::LastGame, "LastGame", "action-last-game"),
    (
        Action::ToggleGameInfo,
        "ToggleGameInfo",
        "action-toggle-game-info",
    ),
    (
        Action::EditGameInfo,
        "EditGameInfo",
        "action-edit-game-info",
    ),
    (Action::ShowNotation, "ShowNotation", "action-show-notation"),
    (
        Action::ShowCommentEditor,
        "ShowCommentEditor",
        "action-show-comment-editor",
    ),
    (Action::BoardSmaller, "BoardSmaller", "action-board-smaller"),
    (Action::BoardLarger, "BoardLarger", "action-board-larger"),
    (Action::FullScreen, "FullScreen", "action-full-screen"),
    (Action::Help, "Help", "action-help"),
    (
        Action::ToggleAutoplay,
        "ToggleAutoplay",
        "action-toggle-autoplay",
    ),
    (Action::Quit, "Quit", "action-quit"),
];

/// Name and Fluent key of the actions of the databases. They are outside
/// [`NAMES`] (and so outside [`Action::all`]): their keys are added to the
/// table by [`Shortcuts::defaults`] and the contract of the M2 table is
/// untouched.
const BASE_NAMES: &[(Action, &str, &str)] = &[
    (
        Action::ImportPgnFile,
        "ImportPgnFile",
        "action-import-pgn-file",
    ),
    (Action::ExportGames, "ExportGames", "action-export-games"),
    (Action::SearchBoard, "SearchBoard", "action-search-board"),
    (
        Action::SearchMaterial,
        "SearchMaterial",
        "action-search-material",
    ),
    (Action::SearchMoves, "SearchMoves", "action-search-moves"),
    (
        Action::SearchPattern,
        "SearchPattern",
        "action-pattern-search",
    ),
    (Action::PlayerFinder, "PlayerFinder", "action-player-finder"),
    (
        Action::TournamentFinder,
        "TournamentFinder",
        "action-tournament-finder",
    ),
    (Action::PlayerInfo, "PlayerInfo", "action-player-info"),
    (Action::MergeGame, "MergeGame", "action-merge-game"),
    (Action::PlayEngine, "PlayEngine", "action-play-engine"),
    (Action::TacticalGame, "TacticalGame", "action-tactical-game"),
    (
        Action::TacticsTrainer,
        "TacticsTrainer",
        "action-tactics-trainer",
    ),
    (
        Action::OpeningTrainer,
        "OpeningTrainer",
        "action-opening-trainer",
    ),
    (
        Action::CalvarTrainer,
        "CalvarTrainer",
        "action-calvar-trainer",
    ),
    (
        Action::ToggleTablebasePane,
        "ToggleTablebasePane",
        "action-toggle-tablebase-pane",
    ),
    (
        Action::ToggleCrosstablePane,
        "ToggleCrosstablePane",
        "action-toggle-crosstable-pane",
    ),
    (
        Action::ToggleStatsPane,
        "ToggleStatsPane",
        "action-toggle-stats-pane",
    ),
    (
        Action::ToggleRatingPane,
        "ToggleRatingPane",
        "action-toggle-rating-pane",
    ),
    (
        Action::TogglePieceTrackerPane,
        "TogglePieceTrackerPane",
        "action-toggle-piece-tracker-pane",
    ),
    (
        Action::ToggleEpdPane,
        "ToggleEpdPane",
        "action-toggle-epd-pane",
    ),
    (Action::EpdFirst, "EpdFirst", "action-epd-first"),
    (Action::EpdPrev, "EpdPrev", "action-epd-prev"),
    (Action::EpdNext, "EpdNext", "action-epd-next"),
    (Action::EpdLast, "EpdLast", "action-epd-last"),
    (
        Action::TournamentEngines,
        "TournamentEngines",
        "action-tournament-engines",
    ),
    (
        Action::OnlineConnect,
        "OnlineConnect",
        "action-online-connect",
    ),
    (Action::OnlineSeek, "OnlineSeek", "action-online-seek"),
    (Action::EboardDialog, "EboardDialog", "action-eboard-dialog"),
    (Action::ResignGame, "ResignGame", "action-resign-game"),
    (Action::Rematch, "Rematch", "action-rematch"),
    (Action::AddBookmark, "AddBookmark", "action-add-bookmark"),
    (
        Action::ManageBookmarks,
        "ManageBookmarks",
        "action-manage-bookmarks",
    ),
    (
        Action::ManageEngines,
        "ManageEngines",
        "action-manage-engines",
    ),
    (
        Action::ToggleEngineAnalysis,
        "ToggleEngineAnalysis",
        "action-toggle-engine-analysis",
    ),
    (
        Action::ToggleEvalBar,
        "ToggleEvalBar",
        "action-toggle-eval-bar",
    ),
    (
        Action::ToggleTrafficLights,
        "ToggleTrafficLights",
        "action-toggle-traffic-lights",
    ),
    (
        Action::ToggleArrowNumbers,
        "ToggleArrowNumbers",
        "action-toggle-arrow-numbers",
    ),
    (Action::NextBase, "NextBase", "action-next-base"),
    (Action::PreviousBase, "PreviousBase", "action-previous-base"),
    (
        Action::ToggleGameList,
        "ToggleGameList",
        "action-toggle-game-list",
    ),
    (
        Action::ToggleAnalysisPane,
        "ToggleAnalysisPane",
        "action-toggle-analysis-pane",
    ),
    (
        Action::ToggleGraphPane,
        "ToggleGraphPane",
        "action-toggle-graph-pane",
    ),
    (
        Action::ToggleTreePane,
        "ToggleTreePane",
        "action-toggle-tree-pane",
    ),
    (
        Action::ToggleBookPane,
        "ToggleBookPane",
        "action-toggle-book-pane",
    ),
    (
        Action::ListSelectAll,
        "ListSelectAll",
        "action-list-select-all",
    ),
    (
        Action::ListRemoveFromFilter,
        "ListRemoveFromFilter",
        "action-list-remove-from-filter",
    ),
    (
        Action::ListToggleDeleted,
        "ListToggleDeleted",
        "action-list-toggle-deleted",
    ),
    (
        Action::ListFocusSearch,
        "ListFocusSearch",
        "action-list-focus-search",
    ),
];

impl Action {
    /// Every action that can be configured (all but `OpenRecent`).
    pub fn all() -> impl Iterator<Item = Action> {
        NAMES.iter().map(|(action, _, _)| *action)
    }

    /// The name used in the preferences (`[shortcuts]` keys).
    pub fn name(self) -> &'static str {
        match self {
            Action::OpenRecent(_) => "OpenRecent",
            Action::SelectBase(_) => "SelectBase",
            Action::StartEngine(_) => "StartEngine",
            _ => NAMES
                .iter()
                .chain(BASE_NAMES)
                .find(|(action, _, _)| *action == self)
                .map_or("OpenRecent", |(_, name, _)| name),
        }
    }

    /// Finds an action by its [`name`](Action::name). `OpenRecent` and
    /// `SelectBase` have a name but no value without their number.
    pub fn from_name(name: &str) -> Option<Action> {
        NAMES
            .iter()
            .chain(BASE_NAMES)
            .find(|(_, candidate, _)| *candidate == name)
            .map(|(action, _, _)| *action)
    }

    /// The Fluent key of the menu label.
    pub fn message_key(self) -> &'static str {
        match self {
            Action::OpenRecent(_) => "action-open-recent",
            Action::SelectBase(_) => "action-select-base",
            Action::StartEngine(_) => "action-start-engine",
            _ => NAMES
                .iter()
                .chain(BASE_NAMES)
                .find(|(action, _, _)| *action == self)
                .map_or("action-open-recent", |(_, _, key)| key),
        }
    }
}

/// A key that is not a character.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[non_exhaustive]
pub enum NamedKey {
    /// Left arrow.
    Left,
    /// Right arrow.
    Right,
    /// Up arrow.
    Up,
    /// Down arrow.
    Down,
    /// Home.
    Home,
    /// End.
    End,
    /// Delete.
    Delete,
    /// Backspace.
    Backspace,
    /// Enter.
    Enter,
    /// Space bar.
    Space,
    /// Escape.
    Escape,
    /// Tab.
    Tab,
    /// Page Up.
    PageUp,
    /// Page Down.
    PageDown,
    /// A function key, `F(1)` to `F(12)`.
    F(u8),
}

const NAMED_KEYS: [(&str, NamedKey); 14] = [
    ("Left", NamedKey::Left),
    ("Right", NamedKey::Right),
    ("Up", NamedKey::Up),
    ("Down", NamedKey::Down),
    ("Home", NamedKey::Home),
    ("End", NamedKey::End),
    ("Delete", NamedKey::Delete),
    ("Backspace", NamedKey::Backspace),
    ("Enter", NamedKey::Enter),
    ("Space", NamedKey::Space),
    ("Escape", NamedKey::Escape),
    ("Tab", NamedKey::Tab),
    ("PageUp", NamedKey::PageUp),
    ("PageDown", NamedKey::PageDown),
];

impl fmt::Display for NamedKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let NamedKey::F(n) = self {
            return write!(f, "F{n}");
        }
        let name = NAMED_KEYS
            .iter()
            .find(|(_, key)| key == self)
            .map_or("?", |(name, _)| name);
        f.write_str(name)
    }
}

impl NamedKey {
    fn parse(text: &str) -> Option<NamedKey> {
        if let Some((_, key)) = NAMED_KEYS
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(text))
        {
            return Some(*key);
        }
        if let (Some(head), Some(tail)) = (text.get(..1), text.get(1..))
            && !tail.is_empty()
            && head.eq_ignore_ascii_case("f")
        {
            let n: u8 = tail.parse().ok()?;
            return (1..=12).contains(&n).then_some(NamedKey::F(n));
        }
        None
    }
}

/// A key: a character (letters in lower case) or a named key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Key {
    /// A character key, lower case.
    Char(char),
    /// A named key.
    Named(NamedKey),
}

/// Modifier keys. `command` is Ctrl on Linux and Windows and Cmd on macOS.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct Modifiers {
    /// Ctrl (Linux, Windows) or Cmd (macOS).
    pub command: bool,
    /// Shift.
    pub shift: bool,
    /// Alt (Option on macOS). Reserved for menu mnemonics: no default
    /// binding uses it.
    pub alt: bool,
}

impl Modifiers {
    /// No modifier.
    pub const NONE: Modifiers = Modifiers {
        command: false,
        shift: false,
        alt: false,
    };
    /// `Command` only.
    pub const COMMAND: Modifiers = Modifiers {
        command: true,
        shift: false,
        alt: false,
    };
    /// `Command+Shift`.
    pub const COMMAND_SHIFT: Modifiers = Modifiers {
        command: true,
        shift: true,
        alt: false,
    };
}

/// A key press as the front-end reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyInput {
    /// The key (letters in lower case).
    pub key: Key,
    /// The modifiers held.
    pub modifiers: Modifiers,
}

/// Where a binding applies (D150).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Scope {
    /// Everywhere.
    Always,
    /// Not while a text field has the focus (keys that editing text uses).
    UnlessTextFocus,
}

/// Keys that a text field also uses with `Command` (select all, copy, ...).
const TEXT_EDIT_CHARS: [char; 6] = ['a', 'c', 'v', 'x', 'z', 'y'];

/// Drops `shift` for symbols that need it to be typed (D149) and lowers
/// letters.
fn normalize(key: Key, mut modifiers: Modifiers) -> (Key, Modifiers) {
    match key {
        Key::Char(c) if !c.is_alphabetic() => {
            modifiers.shift = false;
            (key, modifiers)
        }
        Key::Char(c) => {
            let mut lower = c.to_lowercase();
            let one = match (lower.next(), lower.next()) {
                (Some(l), None) => l,
                _ => c,
            };
            (Key::Char(one), modifiers)
        }
        Key::Named(_) => (key, modifiers),
    }
}

/// A key with its modifiers and scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Binding {
    /// The key.
    pub key: Key,
    /// The modifiers.
    pub modifiers: Modifiers,
    /// Where it applies.
    pub scope: Scope,
}

/// Why a binding text could not be read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum BindingParseError {
    /// The text is empty or has no key after the modifiers.
    #[error("no key in {0:?}")]
    MissingKey(String),
    /// A part before the key is not a modifier.
    #[error("unknown modifier {0:?}")]
    UnknownModifier(String),
    /// The key is neither a character nor a known key name.
    #[error("unknown key {0:?}")]
    UnknownKey(String),
}

impl Binding {
    /// A binding for `key` + `modifiers`; the scope follows D150.
    pub fn new(key: Key, modifiers: Modifiers) -> Binding {
        let (key, modifiers) = normalize(key, modifiers);
        let text_editing = modifiers == Modifiers::COMMAND
            && matches!(key, Key::Char(c) if TEXT_EDIT_CHARS.contains(&c));
        // Function keys never type text, so they work while a field has focus.
        let function_key = matches!(key, Key::Named(NamedKey::F(_)));
        let scope = if (modifiers == Modifiers::NONE && !function_key) || text_editing {
            Scope::UnlessTextFocus
        } else {
            Scope::Always
        };
        Binding {
            key,
            modifiers,
            scope,
        }
    }

    /// Reads `Command+Shift+B`. `Ctrl`, `Control` and `Cmd` mean `Command`;
    /// names are case-insensitive.
    ///
    /// # Errors
    ///
    /// See [`BindingParseError`].
    pub fn parse(text: &str) -> Result<Binding, BindingParseError> {
        let text = text.trim();
        let (mods_part, key_part) = match text.strip_suffix('+') {
            // "+" alone or "Command++": the key is the plus sign.
            Some("") => ("", "+"),
            Some(rest) if rest.ends_with('+') => (&rest[..rest.len() - 1], "+"),
            Some(_) => return Err(BindingParseError::MissingKey(text.to_owned())),
            None => match text.rsplit_once('+') {
                Some((mods, key)) => (mods, key),
                None => ("", text),
            },
        };
        if key_part.is_empty() {
            return Err(BindingParseError::MissingKey(text.to_owned()));
        }
        let mut modifiers = Modifiers::NONE;
        for part in mods_part.split('+').filter(|p| !p.is_empty()) {
            match part.to_ascii_lowercase().as_str() {
                "command" | "cmd" | "ctrl" | "control" => modifiers.command = true,
                "shift" => modifiers.shift = true,
                "alt" | "option" => modifiers.alt = true,
                _ => return Err(BindingParseError::UnknownModifier(part.to_owned())),
            }
        }
        if !mods_part.is_empty() && mods_part.split('+').any(str::is_empty) {
            return Err(BindingParseError::MissingKey(text.to_owned()));
        }
        let mut chars = key_part.chars();
        let key = match (chars.next(), chars.next()) {
            (Some(c), None) => Key::Char(c),
            _ => Key::Named(
                NamedKey::parse(key_part)
                    .ok_or_else(|| BindingParseError::UnknownKey(key_part.to_owned()))?,
            ),
        };
        Ok(Binding::new(key, modifiers))
    }
}

impl fmt::Display for Binding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.modifiers.command {
            f.write_str("Command+")?;
        }
        if self.modifiers.alt {
            f.write_str("Alt+")?;
        }
        if self.modifiers.shift {
            f.write_str("Shift+")?;
        }
        match self.key {
            Key::Char(c) => write!(f, "{}", c.to_uppercase()),
            Key::Named(n) => write!(f, "{n}"),
        }
    }
}

/// Why a customisation of the preferences was not applied (D151).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ShortcutIssue {
    /// The name is not an action.
    UnknownAction {
        /// The name found in the preferences.
        name: String,
    },
    /// The key text cannot be read.
    InvalidBinding {
        /// The action.
        action: Action,
        /// The text found in the preferences.
        text: String,
    },
    /// The key is already used by another active action; nothing changed.
    Conflict {
        /// The action being customised.
        action: Action,
        /// The action that owns the key.
        other: Action,
    },
}

impl ShortcutIssue {
    /// The Fluent key of the message.
    pub fn message_key(&self) -> &'static str {
        match self {
            ShortcutIssue::UnknownAction { .. } => "shortcut-issue-unknown-action",
            ShortcutIssue::InvalidBinding { .. } => "shortcut-issue-invalid-binding",
            ShortcutIssue::Conflict { .. } => "shortcut-issue-conflict",
        }
    }
}

/// The default keys: `(action, binding text)`. `Command` is Ctrl or Cmd.
/// The Scid's `Ctrl+Space` for trial mode is `Command+T` here (it clashes
/// with Spotlight on macOS), see the spec's resolved question 7.
const DEFAULTS: &[(Action, &str)] = &[
    (Action::Back, "Left"),
    (Action::Forward, "Right"),
    (Action::BackFast, "Up"),
    (Action::ForwardFast, "Down"),
    (Action::GoStart, "Home"),
    (Action::GoEnd, "End"),
    (Action::GotoMove, "Command+G"),
    (Action::FlipBoard, "Command+F"),
    (Action::ClearMoveEntry, "Escape"),
    (Action::TruncateGame, "Delete"),
    // The Delete key of the Mac is Backspace (resolved question 7).
    (Action::TruncateGame, "Command+Backspace"),
    (Action::AddVariation, "Command+A"),
    (Action::EnterVariation, "V"),
    (Action::ExitVariation, "Z"),
    (Action::ToggleTrialMode, "Command+T"),
    (Action::Undo, "Command+Z"),
    (Action::Redo, "Command+Y"),
    (Action::Redo, "Command+Shift+Z"),
    (Action::EditGameInfo, "Command+Shift+H"),
    (Action::NewGame, "Command+Shift+N"),
    (Action::OpenFile, "Command+O"),
    (Action::Save, "Command+S"),
    (Action::SetupBoard, "Command+Shift+B"),
    (Action::CopyFen, "Command+Shift+C"),
    (Action::PasteFen, "Command+Shift+V"),
    (Action::ImportPgn, "Command+Shift+I"),
    (Action::NextGame, "Command+Down"),
    (Action::PrevGame, "Command+Up"),
    (Action::FirstGame, "Command+Home"),
    (Action::LastGame, "Command+End"),
    (Action::ToggleGameInfo, "Command+I"),
    (Action::ShowNotation, "Command+P"),
    (Action::ShowCommentEditor, "Command+E"),
    (Action::BoardSmaller, "Command+Shift+Left"),
    (Action::BoardLarger, "Command+Shift+Right"),
    (Action::FullScreen, "F11"),
    (Action::Help, "F1"),
    (Action::Quit, "Command+Q"),
];

/// The default keys of the actions of the databases (spec M3-4, D504):
/// Ctrl/Cmd-1 to 8 are the slots, Ctrl/Cmd-9 and Ctrl/Cmd-backquote the Clipbase,
/// Ctrl/Cmd-Tab (and Shift-Tab, as in the Scid) walk the open databases.
/// Ctrl/Cmd with Page Down or Page Up is the alias that works on macOS, where
/// Cmd-Tab belongs to the system.
const BASE_DEFAULTS: &[(Action, &str)] = &[
    (Action::AddBookmark, "Command+D"),
    (Action::ManageBookmarks, "Command+Shift+D"),
    // Spec M4-3: the Scid's F2-F4 start the engines of the list. The
    // dialog is `Command+Shift+E` because `Command+Shift+A` is taken by
    // "select all games" of the list (D564); the space bar toggles the
    // analysis, as in the Scid, and does not work while a text field has
    // the focus.
    (Action::StartEngine(1), "F2"),
    (Action::StartEngine(2), "F3"),
    (Action::StartEngine(3), "F4"),
    (Action::ManageEngines, "Command+Shift+E"),
    // Spec M8-1b: the game against an engine. `Command+P` shows the
    // notation, so the dialog takes `Shift`.
    (Action::PlayEngine, "Command+Shift+P"),
    // Spec M8-7 (D2469): every `Command+Shift+letter` is taken or unsafe and
    // no default uses `Alt`; a function key is free and layout independent.
    (Action::TacticalGame, "Command+Shift+F7"),
    // Spec M8-4 (D1937): `X` of "exercise"; free in the map.
    (Action::TacticsTrainer, "Command+Shift+X"),
    // Spec M8-5 (D2238, D2253): every `Command+Shift+letter` is taken or
    // unsafe (`W` closes the window or tab, `Q` logs out on macOS, `J` is the
    // search of the list, `Y` the analysis pane) and the defaults never use
    // `Alt`; a function key is free and layout independent.
    (Action::OpeningTrainer, "Command+Shift+F6"),
    // Spec M8-6 (D2325): every `Command+Shift+letter` is taken or probed as
    // free by a test, and no default uses `Alt` (reserved for the menu
    // mnemonics), so the exercise takes the unshifted, unused `Command+U`.
    (Action::CalvarTrainer, "Command+U"),
    // Spec M8-2b (D1361): free key, `R` of "round robin".
    (Action::TournamentEngines, "Command+Shift+R"),
    // Spec M8-3c (D1470): free keys, `O` of "online"; `K` is left to the
    // users who remap `NewGame` to it (a test does), so the search takes `U`.
    (Action::OnlineConnect, "Command+Shift+O"),
    (Action::OnlineSeek, "Command+Shift+U"),
    // Spec M8-10 (D2742): `Command+Shift+B` is the set-up board, and the
    // letters are taken; a function key is free (F9, like F6-F8 above).
    (Action::EboardDialog, "Command+Shift+F9"),
    // Spec M6-3 (D3349): the letters with `Shift` are nearly all taken; the
    // function keys F4 and F5 with `Command+Shift` are free (`F4` alone is
    // the third engine, F6 to F9 are taken above).
    (Action::PlayerFinder, "Command+Shift+F5"),
    (Action::TournamentFinder, "Command+Shift+F4"),
    (Action::ToggleEngineAnalysis, "Space"),
    // Spec M4-5 (D823): the Scid's Ctrl+Shift+S/C/N; the last two already
    // mean "copy FEN" and "new game" here, so the lights and the numbers
    // take `L` and `M`.
    (Action::ToggleEvalBar, "Command+Shift+S"),
    (Action::ToggleTrafficLights, "Command+Shift+L"),
    (Action::ToggleArrowNumbers, "Command+Shift+M"),
    (Action::SelectBase(1), "Command+1"),
    (Action::SelectBase(2), "Command+2"),
    (Action::SelectBase(3), "Command+3"),
    (Action::SelectBase(4), "Command+4"),
    (Action::SelectBase(5), "Command+5"),
    (Action::SelectBase(6), "Command+6"),
    (Action::SelectBase(7), "Command+7"),
    (Action::SelectBase(8), "Command+8"),
    (Action::SelectBase(9), "Command+9"),
    (Action::SelectBase(9), "Command+`"),
    (Action::NextBase, "Command+Tab"),
    (Action::NextBase, "Command+PageDown"),
    (Action::PreviousBase, "Command+Shift+Tab"),
    (Action::PreviousBase, "Shift+Tab"),
    (Action::PreviousBase, "Command+PageUp"),
    (Action::ToggleGameList, "Command+L"),
    // Spec M4-4: the analysis panel. The Scid's `Ctrl+Shift+A` is "select
    // all games" here (D564), so the panel takes `Ctrl+Shift+Y`.
    (Action::ToggleAnalysisPane, "Command+Shift+Y"),
    // Spec M4-7 (D870): the Scid's `Ctrl+Shift+Z` is Redo here, so the
    // graph takes `G`.
    (Action::ToggleGraphPane, "Command+Shift+G"),
    // Spec M5-2 (D971): the Scid's `Ctrl+T` opens the tree, but `Command+T`
    // is trial mode here (see above), so the tree takes `Shift`.
    (Action::ToggleTreePane, "Command+Shift+T"),
    // Spec M5-6b (D1790): the book panel; `Command+K` is left alone and
    // `Command+Shift+K` was free in the map.
    (Action::ToggleBookPane, "Command+Shift+K"),
    // Spec M8-8 (D2579): every `Command+Shift+letter` is taken, so the
    // tablebase panel takes a function key (F6 and F7 belong to the opening
    // trainer and the tactical game); no default uses `Alt`.
    (Action::ToggleTablebasePane, "Command+Shift+F8"),
    // Slice 4c (D564). The keys of the Scid's game list (Ctrl+A, Delete,
    // Ctrl+F) mean other things in the M2 table; they work over the list
    // only, handled by the front-end, and these are the ones that work
    // everywhere.
    // Spec M7-5c (D3851): the Scid's `Ctrl+Down/Up/Home/End` of the EPD window
    // are the game navigation here, so the records take `Shift`.
    (Action::EpdFirst, "Command+Shift+Home"),
    (Action::EpdPrev, "Command+Shift+Up"),
    (Action::EpdNext, "Command+Shift+Down"),
    (Action::EpdLast, "Command+Shift+End"),
    (Action::ListSelectAll, "Command+Shift+A"),
    (Action::ListRemoveFromFilter, "Shift+Delete"),
    (Action::ListToggleDeleted, "Command+Delete"),
    (Action::ListFocusSearch, "Command+Shift+J"),
];

/// The map between actions and keys.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shortcuts {
    bindings: Vec<(Action, Vec<Binding>)>,
}

impl Default for Shortcuts {
    fn default() -> Shortcuts {
        Shortcuts::defaults()
    }
}

impl Shortcuts {
    /// The default table (spec "Ações do M2 e teclas padrão").
    pub fn defaults() -> Shortcuts {
        let bindings = Action::all()
            .map(|action| {
                let keys = DEFAULTS
                    .iter()
                    .filter(|(a, _)| *a == action)
                    .filter_map(|(_, text)| Binding::parse(text).ok())
                    .collect();
                (action, keys)
            })
            .collect();
        let mut bindings: Vec<(Action, Vec<Binding>)> = bindings;
        for action in BASE_DEFAULTS.iter().map(|(action, _)| *action) {
            if bindings.iter().any(|(a, _)| *a == action) {
                continue;
            }
            let keys = BASE_DEFAULTS
                .iter()
                .filter(|(a, _)| *a == action)
                .filter_map(|(_, text)| Binding::parse(text).ok())
                .collect();
            bindings.push((action, keys));
        }
        Shortcuts { bindings }
    }

    /// The action for a key press, ignoring where the focus is.
    pub fn lookup(&self, input: &KeyInput) -> Option<Action> {
        self.lookup_in(input, false)
    }

    /// The action for a key press; `text_focused` tells whether a text
    /// field has the focus (bindings of scope
    /// [`UnlessTextFocus`](Scope::UnlessTextFocus) are then skipped).
    pub fn lookup_in(&self, input: &KeyInput, text_focused: bool) -> Option<Action> {
        let (key, modifiers) = normalize(input.key, input.modifiers);
        self.bindings.iter().find_map(|(action, keys)| {
            keys.iter()
                .any(|b| {
                    b.key == key
                        && b.modifiers == modifiers
                        && !(text_focused && b.scope == Scope::UnlessTextFocus)
                })
                .then_some(*action)
        })
    }

    /// The bindings of `action` (empty when it has none).
    pub fn bindings(&self, action: Action) -> &[Binding] {
        self.bindings
            .iter()
            .find(|(a, _)| *a == action)
            .map_or(&[], |(_, keys)| keys.as_slice())
    }

    /// Every action with its bindings.
    pub fn iter(&self) -> impl Iterator<Item = (Action, &[Binding])> {
        self.bindings.iter().map(|(a, keys)| (*a, keys.as_slice()))
    }

    fn owner_of(&self, binding: &Binding, except: Action) -> Option<Action> {
        self.bindings.iter().find_map(|(action, keys)| {
            (*action != except
                && keys
                    .iter()
                    .any(|b| b.key == binding.key && b.modifiers == binding.modifiers))
            .then_some(*action)
        })
    }

    /// Applies the `[shortcuts]` table of the preferences (D151): an empty
    /// text turns the action's keys off; a key used by another active
    /// action is refused with [`ShortcutIssue::Conflict`].
    pub fn with_overrides(
        mut self,
        overrides: &BTreeMap<String, String>,
    ) -> (Shortcuts, Vec<ShortcutIssue>) {
        let mut issues = Vec::new();
        // Two passes: disabling an action frees its keys for the others, no
        // matter the (alphabetical) order of the overrides.
        for (name, text) in overrides {
            if !text.trim().is_empty() {
                continue;
            }
            if let Some(action) = Action::from_name(name)
                && let Some((_, keys)) = self.bindings.iter_mut().find(|(a, _)| *a == action)
            {
                keys.clear();
            }
        }
        for (name, text) in overrides {
            let Some(action) = Action::from_name(name) else {
                issues.push(ShortcutIssue::UnknownAction { name: name.clone() });
                continue;
            };
            let Some(entry) = self.bindings.iter().position(|(a, _)| *a == action) else {
                continue;
            };
            if text.trim().is_empty() {
                continue;
            }
            let Ok(binding) = Binding::parse(text) else {
                issues.push(ShortcutIssue::InvalidBinding {
                    action,
                    text: text.clone(),
                });
                continue;
            };
            if let Some(other) = self.owner_of(&binding, action) {
                issues.push(ShortcutIssue::Conflict { action, other });
                continue;
            }
            if let Some((_, keys)) = self.bindings.get_mut(entry) {
                *keys = vec![binding];
            }
        }
        (self, issues)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(text: &str) -> KeyInput {
        let b = Binding::parse(text).expect("binding");
        KeyInput {
            key: b.key,
            modifiers: b.modifiers,
        }
    }

    #[test]
    fn defaults_have_no_collisions_and_cover_the_table() {
        let table = Shortcuts::defaults();
        let mut seen = Vec::new();
        for (action, keys) in table.iter() {
            for b in keys {
                assert!(
                    !seen.contains(&(b.key, b.modifiers)),
                    "{action:?} collides on {b}"
                );
                seen.push((b.key, b.modifiers));
                assert!(!b.modifiers.alt, "{action:?} uses alt");
            }
        }
        assert_eq!(DEFAULTS.len(), 38);
        for (action, text) in DEFAULTS {
            assert!(Binding::parse(text).is_ok(), "{text}");
            assert!(!table.bindings(*action).is_empty());
        }
    }

    #[test]
    fn lookup_follows_the_scid_table() {
        let table = Shortcuts::defaults();
        assert_eq!(
            table.lookup(&input("Command+Shift+B")),
            Some(Action::SetupBoard)
        );
        assert_eq!(table.lookup(&input("Command+B")), None);
        assert_eq!(table.lookup(&input("Up")), Some(Action::BackFast));
        assert_eq!(table.lookup(&input("v")), Some(Action::EnterVariation));
        assert_eq!(table.lookup(&input("Command+v")), None);
        assert_eq!(
            table.lookup(&input("Command+T")),
            Some(Action::ToggleTrialMode)
        );
    }

    #[test]
    fn symbols_ignore_shift() {
        let plain = Binding::parse("?").expect("parse");
        let shifted = Binding::parse("Shift+?").expect("parse");
        assert_eq!(plain, shifted);
        let mut table = Shortcuts::defaults();
        table.bindings.push((Action::OpenRecent(1), vec![plain]));
        for shift in [false, true] {
            let key = KeyInput {
                key: Key::Char('?'),
                modifiers: Modifiers {
                    shift,
                    ..Modifiers::NONE
                },
            };
            assert_eq!(table.lookup(&key), Some(Action::OpenRecent(1)));
        }
    }

    #[test]
    fn scopes_follow_d150() {
        for (_, keys) in Shortcuts::defaults().iter() {
            for b in keys {
                let text_key = (b.modifiers == Modifiers::NONE
                    && !matches!(b.key, Key::Named(NamedKey::F(_))))
                    || (b.modifiers == Modifiers::COMMAND
                        && matches!(b.key, Key::Char(c) if TEXT_EDIT_CHARS.contains(&c)));
                let expected = if text_key {
                    Scope::UnlessTextFocus
                } else {
                    Scope::Always
                };
                assert_eq!(b.scope, expected, "{b}");
            }
        }
        let table = Shortcuts::defaults();
        assert_eq!(table.lookup_in(&input("Left"), true), None);
        assert_eq!(table.lookup_in(&input("Left"), false), Some(Action::Back));
        assert_eq!(
            table.lookup_in(&input("Command+S"), true),
            Some(Action::Save)
        );
    }

    #[test]
    fn parse_and_display_round_trip() {
        for text in [
            "Command+Shift+B",
            "F11",
            "Command+Down",
            "Left",
            "V",
            "Command+Alt+X",
        ] {
            let b = Binding::parse(text).expect(text);
            assert_eq!(b.to_string(), text);
        }
        assert_eq!(Binding::parse("Ctrl+b"), Binding::parse("Command+B"));
        assert_eq!(Binding::parse("cmd+B"), Binding::parse("Command+b"));
        assert_eq!(
            Binding::parse("Command++").map(|b| b.key),
            Ok(Key::Char('+'))
        );
        for bad in ["Command+", "Foo+B", "Command+Shift", "", "+x+", "F13"] {
            assert!(Binding::parse(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn overrides_replace_disable_and_report() {
        let mut o = BTreeMap::new();
        o.insert("FlipBoard".to_owned(), "Command+Shift+F".to_owned());
        o.insert("Help".to_owned(), String::new());
        o.insert("Save".to_owned(), "Command+O".to_owned());
        o.insert("Nope".to_owned(), "X".to_owned());
        o.insert("Quit".to_owned(), "Command+".to_owned());
        let (table, issues) = Shortcuts::defaults().with_overrides(&o);
        assert_eq!(
            table.lookup(&input("Command+Shift+F")),
            Some(Action::FlipBoard)
        );
        assert_eq!(table.lookup(&input("Command+F")), None);
        assert!(table.bindings(Action::Help).is_empty());
        assert_eq!(table.lookup(&input("Command+S")), Some(Action::Save));
        assert!(issues.contains(&ShortcutIssue::Conflict {
            action: Action::Save,
            other: Action::OpenFile
        }));
        assert!(issues.contains(&ShortcutIssue::UnknownAction {
            name: "Nope".to_owned()
        }));
        assert!(issues.iter().any(|i| matches!(
            i,
            ShortcutIssue::InvalidBinding {
                action: Action::Quit,
                ..
            }
        )));
    }

    #[test]
    fn action_names_round_trip() {
        for action in Action::all() {
            assert_eq!(Action::from_name(action.name()), Some(action));
            assert!(action.message_key().starts_with("action-"));
        }
        assert_eq!(Action::from_name("OpenRecent"), None);
    }

    #[test]
    fn the_keys_of_the_databases_are_in_the_table_but_not_in_the_m2_list() {
        let table = Shortcuts::defaults();
        let lookup = |text: &str| table.lookup(&input(text));
        for n in 1..=9u8 {
            assert_eq!(lookup(&format!("Command+{n}")), Some(Action::SelectBase(n)));
        }
        assert_eq!(lookup("Command+`"), Some(Action::SelectBase(9)));
        assert_eq!(lookup("Command+Tab"), Some(Action::NextBase));
        assert_eq!(lookup("Command+Shift+Tab"), Some(Action::PreviousBase));
        assert_eq!(lookup("Shift+Tab"), Some(Action::PreviousBase));
        assert_eq!(lookup("Command+PageDown"), Some(Action::NextBase));
        assert_eq!(lookup("Command+PageUp"), Some(Action::PreviousBase));
        assert_eq!(lookup("Command+L"), Some(Action::ToggleGameList));
        assert_eq!(
            Action::all()
                .filter(|a| matches!(a, Action::NextBase))
                .count(),
            0
        );
        assert_eq!(Action::from_name("NextBase"), Some(Action::NextBase));
        assert_eq!(Action::SelectBase(3).message_key(), "action-select-base");
        assert_eq!(Action::ToggleGameList.name(), "ToggleGameList");
    }

    #[test]
    fn page_keys_parse_and_print() {
        let binding = Binding::parse("Command+PageDown").expect("binding");
        assert_eq!(binding.key, Key::Named(NamedKey::PageDown));
        assert_eq!(binding.to_string(), "Command+PageDown");
    }
}
