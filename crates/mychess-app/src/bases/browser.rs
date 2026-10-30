//! The game browser (spec M3-4, slice 4e; D595 onwards; Scid
//! `tcl/game/browser.tcl` and `browseGames` in `tcl/windows/gamelist.tcl`).
//!
//! The browser shows one game (board, move text, buttons) or a grid of
//! several (one small board each). It is state of the application
//! ([`BrowserState`]): the games are read by an effect, kept as [`Game`]s
//! with the node of every ply of the main line, and the position of a
//! ply comes from the game itself, so `view` never reads a record.
//!
//! The behaviour of the messages is in `state/browser_flow.rs`.

use mychess_core::{Game, GameResult, Move, NodeId, Position};
use mychess_db::si4::GameFlags;
use mychess_db::{FILTER_LAST_MOVE, GameSource};

use super::error::BaseError;
use super::open::OpenControl;
use super::source::SharedSource;
use super::{BaseSlot, SlotId};
use crate::error::RequestId;
use crate::notation::{
    HeaderMode, Inline, NotationDoc, NotationLayout, NotationOptions, build_notation,
};
use crate::shared::SharedGame;

/// The most games the grid shows (the rest of a larger selection is left
/// out, with a notice; D596).
pub const MAX_BROWSER_GAMES: usize = 64;

/// The number of columns of the grid for `count` games (the Scid's
/// "quick hack to proportion the grid", `browseGames`).
pub fn grid_columns(count: usize) -> usize {
    if count > 36 {
        7
    } else if count > 20 {
        6
    } else if count > 12 {
        5
    } else if count > 6 || count == 4 {
        4
    } else {
        3
    }
}

/// One game or a grid of games.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrowseMode {
    /// Board, move text and buttons of a single game.
    Single,
    /// A grid of small boards.
    Grid,
}

/// Which game of the filter a single-game browser goes to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameStep {
    /// The first game of the filter.
    First,
    /// The one before the current game.
    Previous,
    /// The one after the current game.
    Next,
    /// The last game of the filter.
    Last,
}

/// An event of the browser.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum BrowseMessage {
    /// Open the browser on these games of a slot (one number: a single
    /// game; two or more: the grid). Numbers above the total become the
    /// last game; zero and repeated ones are dropped.
    Open {
        /// The slot.
        slot: SlotId,
        /// The game numbers, in the order they are shown.
        numbers: Vec<u32>,
    },
    /// The games asked for were read (answer to
    /// [`Effect::ReadBrowserGames`](crate::Effect)).
    Loaded {
        /// The request.
        request: RequestId,
        /// What was read, in the order asked.
        games: Vec<BrowseRead>,
    },
    /// Move `delta` plies; `None` moves every board.
    Step {
        /// The board, or all.
        game: Option<usize>,
        /// Plies, negative to go back.
        delta: i32,
    },
    /// Go to the start; `None` moves every board.
    GoToStart {
        /// The board, or all.
        game: Option<usize>,
    },
    /// Go to the end; `None` moves every board.
    GoToEnd {
        /// The board, or all.
        game: Option<usize>,
    },
    /// Go to a ply of one board (a click on a move of the text).
    GoToPly {
        /// The board.
        game: usize,
        /// The ply, clamped to the game.
        ply: u32,
    },
    /// Turn a board; `None` turns every board.
    Flip {
        /// The board, or all.
        game: Option<usize>,
    },
    /// Start or stop the autoplay of a single game.
    ToggleAutoplay,
    /// The autoplay timer fired.
    AutoplayTick {
        /// Identifies the run the tick belongs to.
        generation: u64,
    },
    /// Show another game of the filter (single game only).
    GoToGame(GameStep),
    /// The pointer is over this board of the grid.
    Focus(usize),
    /// Load a game into the board of the main window, at the ply and
    /// orientation shown.
    LoadIntoBoard {
        /// The board.
        game: usize,
        /// Keep the browser open (Ctrl/Cmd+click on Load).
        keep_open: bool,
    },
    /// Toggle the "deleted" mark of a game of an editable database.
    ToggleDeleted {
        /// The board.
        game: usize,
    },
    /// Close the browser.
    Close,
}

/// A game read for the browser.
#[derive(Debug, Clone, PartialEq)]
pub struct BrowseRead {
    /// The game number.
    pub number: u32,
    /// The game, or why it could not be read.
    pub result: Result<SharedGame, BaseError>,
    /// Whether the index entry has the "deleted" mark.
    pub deleted: bool,
}

/// What a board of the browser holds.
#[derive(Debug, Clone)]
pub enum BrowserContent {
    /// The game is being read.
    Loading,
    /// The game could not be read.
    Unreadable(BaseError),
    /// The game, ready to show.
    Ready(Box<ReadyGame>),
}

/// The names and result shown with a board.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserHeader {
    /// White player.
    pub white: String,
    /// Black player.
    pub black: String,
    /// The result.
    pub result: GameResult,
    /// The event.
    pub event: String,
    /// The date as written in PGN (`1992.??.??`).
    pub date: String,
    /// The ECO code, empty when unknown.
    pub eco: String,
}

/// A game read, with the main line unrolled.
#[derive(Debug, Clone)]
pub struct ReadyGame {
    game: SharedGame,
    /// The root and then every node of the main line: `nodes[ply]`.
    nodes: Vec<NodeId>,
    ply: usize,
    /// The header.
    pub header: BrowserHeader,
    /// The move text (main line only), for the single-game browser.
    pub doc: Option<NotationDoc>,
    /// The move text as flat items with the ply of each move, built once
    /// (empty unless `doc` is).
    pub text_items: Vec<TextItem>,
}

/// What a [`TextItem`] is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextKind {
    /// A move number (`12.`).
    Number,
    /// A move of the main line.
    Move,
    /// A blank.
    Space,
    /// The result at the end.
    Result,
}

/// One piece of the move text of the single-game browser.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextItem {
    /// What it is.
    pub kind: TextKind,
    /// The text.
    pub text: Box<str>,
    /// The ply a move leads to.
    pub ply: Option<usize>,
}

fn text_items(doc: &NotationDoc, nodes: &[NodeId]) -> Vec<TextItem> {
    let plies: std::collections::HashMap<NodeId, usize> =
        nodes.iter().enumerate().map(|(i, n)| (*n, i)).collect();
    let item = |kind, text: &str, ply| TextItem {
        kind,
        text: text.into(),
        ply,
    };
    let mut items = Vec::new();
    for block in &doc.blocks {
        for inline in &block.inlines {
            match inline {
                Inline::Number(number) => items.push(item(TextKind::Number, number, None)),
                Inline::Move { node, san, .. } => {
                    items.push(item(TextKind::Move, san, plies.get(node).copied()));
                }
                Inline::Space => items.push(item(TextKind::Space, " ", None)),
                _ => {}
            }
        }
    }
    if let Some(result) = doc.result {
        items.push(item(TextKind::Space, "  ", None));
        items.push(item(TextKind::Result, result.as_str(), None));
    }
    items
}

impl ReadyGame {
    fn new(game: SharedGame, want_doc: bool) -> ReadyGame {
        let nodes: Vec<NodeId> = std::iter::once(game.root())
            .chain(game.mainline())
            .collect();
        let tags = game.tags();
        let header = BrowserHeader {
            white: tags.white.clone(),
            black: tags.black.clone(),
            result: tags.result,
            event: tags.event.clone(),
            date: tags.date.to_string(),
            eco: tags.eco.map(|eco| eco.to_string()).unwrap_or_default(),
        };
        let doc = want_doc.then(|| build_notation(&game, &browser_notation_options(), 0));
        let text_items = doc
            .as_ref()
            .map(|doc| text_items(doc, &nodes))
            .unwrap_or_default();
        ReadyGame {
            game,
            nodes,
            ply: 0,
            header,
            doc,
            text_items,
        }
    }

    /// The ply shown (0 is the start position).
    pub fn ply(&self) -> usize {
        self.ply
    }

    /// The last ply of the main line.
    pub fn max_ply(&self) -> usize {
        self.nodes.len().saturating_sub(1)
    }

    /// The node of the ply shown.
    pub fn node(&self) -> NodeId {
        self.nodes
            .get(self.ply)
            .copied()
            .unwrap_or_else(|| self.game.root())
    }

    /// The position at the ply shown, computed by the game.
    pub fn position(&self) -> Option<&Position> {
        self.game.position_at(self.node()).ok()
    }

    /// The move that led to the position shown (none at the start).
    pub fn last_move(&self) -> Option<Move> {
        if self.ply == 0 {
            return None;
        }
        self.game.mv(self.node()).ok().filter(|m| !m.is_null())
    }

    /// The ply of a node of the main line.
    pub fn ply_of(&self, node: NodeId) -> Option<usize> {
        self.nodes.iter().position(|n| *n == node)
    }

    /// The game.
    pub fn game(&self) -> &Game {
        &self.game
    }

    fn set_ply(&mut self, ply: usize) {
        self.ply = ply.min(self.max_ply());
    }

    fn step(&mut self, delta: i32) {
        let target = i64::try_from(self.ply)
            .unwrap_or(i64::MAX)
            .saturating_add(i64::from(delta))
            .max(0);
        self.set_ply(usize::try_from(target).unwrap_or(usize::MAX));
    }
}

/// How the move text of the browser is built: the main line only, with
/// the result (the Scid shows the main line without comments).
fn browser_notation_options() -> NotationOptions {
    NotationOptions {
        layout: NotationLayout::Inline,
        header: HeaderMode::Hidden,
        show_comments: false,
        show_variations: false,
        show_result: true,
        symbolic_nags: false,
        move_number_space: false,
        ..NotationOptions::default()
    }
}

/// One board of the browser.
#[derive(Debug, Clone)]
pub struct BrowserGame {
    /// The game number in the database.
    pub number: u32,
    /// Whether the board is turned.
    pub flipped: bool,
    /// Whether the game has the "deleted" mark.
    pub deleted: bool,
    /// The game, or why there is none yet.
    pub content: BrowserContent,
}

impl BrowserGame {
    pub(crate) fn loading(number: u32, flipped: bool) -> BrowserGame {
        BrowserGame {
            number,
            flipped,
            deleted: false,
            content: BrowserContent::Loading,
        }
    }

    /// The game, once read.
    pub fn ready(&self) -> Option<&ReadyGame> {
        match &self.content {
            BrowserContent::Ready(ready) => Some(ready),
            _ => None,
        }
    }

    fn ready_mut(&mut self) -> Option<&mut ReadyGame> {
        match &mut self.content {
            BrowserContent::Ready(ready) => Some(ready),
            _ => None,
        }
    }
}

/// The state of the game browser.
#[derive(Debug, Clone)]
pub struct BrowserState {
    /// The slot the games belong to.
    pub slot: SlotId,
    /// One game or a grid.
    pub mode: BrowseMode,
    /// The boards: one in [`BrowseMode::Single`].
    pub games: Vec<BrowserGame>,
    /// The board the pointer is over (grid): the one Ctrl+Delete and
    /// Ctrl+Enter act on.
    pub focus: usize,
    /// How many games were asked for beyond [`MAX_BROWSER_GAMES`] (they
    /// were left out), zero when all fit.
    pub left_out: usize,
    /// The autoplay of the single game is running.
    pub autoplay: bool,
    pub(crate) generation: u64,
    pub(crate) request: RequestId,
    pub(crate) control: OpenControl,
}

impl BrowserState {
    pub(crate) fn new(
        slot: SlotId,
        mode: BrowseMode,
        numbers: &[u32],
        left_out: usize,
        request: RequestId,
        control: OpenControl,
    ) -> BrowserState {
        BrowserState {
            slot,
            mode,
            games: numbers
                .iter()
                .map(|n| BrowserGame::loading(*n, false))
                .collect(),
            focus: 0,
            left_out,
            autoplay: false,
            generation: 0,
            request,
            control,
        }
    }

    /// The number of columns of the grid.
    pub fn columns(&self) -> usize {
        grid_columns(self.games.len())
    }

    /// Whether some game is still being read.
    pub fn is_loading(&self) -> bool {
        self.games
            .iter()
            .any(|g| matches!(g.content, BrowserContent::Loading))
    }

    /// Games read and the total being read (for the progress bar).
    pub fn progress(&self) -> (u64, u64) {
        self.control.progress()
    }

    /// The games the buttons and keys act on: `game`, or every board.
    pub(crate) fn targets(&self, game: Option<usize>) -> Vec<usize> {
        match game {
            Some(i) if i < self.games.len() => vec![i],
            Some(_) => Vec::new(),
            None => (0..self.games.len()).collect(),
        }
    }

    /// Moves the boards named by `game` `delta` plies.
    pub(crate) fn step(&mut self, game: Option<usize>, delta: i32) {
        for i in self.targets(game) {
            if let Some(ready) = self.games.get_mut(i).and_then(BrowserGame::ready_mut) {
                ready.step(delta);
            }
        }
    }

    /// Moves the boards named by `game` to `ply`.
    pub(crate) fn go_to(&mut self, game: Option<usize>, ply: usize) {
        for i in self.targets(game) {
            if let Some(ready) = self.games.get_mut(i).and_then(BrowserGame::ready_mut) {
                ready.set_ply(ply);
            }
        }
    }

    /// Whether the single game is at its last ply (or has none).
    pub(crate) fn at_end(&self) -> bool {
        self.games
            .first()
            .and_then(BrowserGame::ready)
            .is_none_or(|r| r.ply >= r.max_ply())
    }

    /// Puts a read game on its board with the starting ply of the Scid.
    pub(crate) fn install(&mut self, index: usize, read: BrowseRead, raw_filter: u8) {
        let single = self.mode == BrowseMode::Single;
        let Some(board) = self.games.get_mut(index) else {
            return;
        };
        board.deleted = read.deleted;
        board.content = match read.result {
            Ok(game) => {
                let mut ready = ReadyGame::new(game, single);
                ready.set_ply(initial_ply(raw_filter, single, ready.max_ply()));
                BrowserContent::Ready(Box::new(ready))
            }
            Err(error) => BrowserContent::Unreadable(error),
        };
    }
}

/// The ply a game opens at, from its value in the filter (`raw`: 0 = out,
/// otherwise ply + 1, 255 = the last move), as the Scid does: a single
/// game opens at the ply of the filter (the start when there is none), a
/// grid shows the end of games that have no ply of their own
/// (`browseGames`: "if ply > max or ply == 0, ply = max").
pub(crate) fn initial_ply(raw: u8, single: bool, max: usize) -> usize {
    if raw == FILTER_LAST_MOVE {
        return max;
    }
    let ply = usize::from(raw.saturating_sub(1));
    if single {
        ply.min(max)
    } else if ply == 0 || ply > max {
        max
    } else {
        ply
    }
}

/// Reads the games for the browser, one at a time (the write lock is
/// held for one record only), reporting progress and stopping when
/// cancelled. Run by `perform` on a blocking thread.
pub(crate) fn read_browser_games(
    source: &SharedSource,
    numbers: &[u32],
    control: &OpenControl,
) -> Vec<BrowseRead> {
    let mut report = control.reporter();
    let total = numbers.len() as u64;
    let mut out = Vec::with_capacity(numbers.len());
    for (done, &number) in numbers.iter().enumerate() {
        if control.is_cancelled() {
            break;
        }
        report(done as u64, total);
        let mut guard = source.lock();
        let deleted = guard
            .entry(number)
            .is_ok_and(|entry| entry.flags.contains(GameFlags::DELETE));
        let result = GameSource::game(&mut *guard, number)
            .map(SharedGame::new)
            .map_err(|error| {
                tracing::warn!(game = number, %error, "cannot read a game for the browser");
                BaseError::GameUnreadable { number }
            });
        drop(guard);
        out.push(BrowseRead {
            number,
            result,
            deleted,
        });
    }
    report(total, total);
    out
}

/// The answer when the reading thread panicked: every game unreadable.
pub(crate) fn unreadable_games(numbers: &[u32]) -> Vec<BrowseRead> {
    numbers
        .iter()
        .map(|&number| BrowseRead {
            number,
            result: Err(BaseError::GameUnreadable { number }),
            deleted: false,
        })
        .collect()
}

/// How many positions of the view [`BaseSlot::browse_targets`] looks at
/// per step.
const SCAN_CHUNK: u32 = 4096;

impl BaseSlot {
    /// The games the browser opens for the list: the selection when it has
    /// two or more games (in the order of the view), else the one
    /// `clicked` (a row of the menu), else the selected one, else the game
    /// being edited from this slot.
    ///
    /// Only the first [`MAX_BROWSER_GAMES`] + 1 games of the selection are
    /// returned (the extra one tells the browser that some were left out):
    /// the view is walked in order and the walk stops there, so a selection
    /// of hundreds of thousands of games costs the same as a small one.
    pub fn browse_targets(&self, clicked: Option<u32>) -> Vec<u32> {
        let selection = &self.list.selection;
        if selection.len() >= 2 && clicked.is_none_or(|n| selection.contains(&n)) {
            // A small selection is sorted by position; a large one walks
            // the view and stops early.
            if selection.len() <= MAX_BROWSER_GAMES {
                let mut numbers: Vec<u32> = selection.iter().copied().collect();
                numbers.sort_by_key(|n| self.position_of(*n).unwrap_or(u32::MAX));
                return numbers;
            }
            let limit = MAX_BROWSER_GAMES + 1;
            let mut out = Vec::with_capacity(limit);
            let total = self.filtered();
            let mut from = 1u32;
            while from <= total && out.len() < limit {
                let to = from.saturating_add(SCAN_CHUNK - 1);
                for number in self.visible_range(from, to) {
                    if selection.contains(&number) {
                        out.push(number);
                        if out.len() == limit {
                            break;
                        }
                    }
                }
                if to == u32::MAX {
                    break;
                }
                from = to + 1;
            }
            return out;
        }
        clicked
            .or(self.list.selected)
            .or(self.current_game.filter(|n| *n > 0))
            .into_iter()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_grid_has_the_columns_of_the_scid() {
        let expected = [
            (1, 3),
            (4, 4),
            (5, 3),
            (6, 3),
            (7, 4),
            (12, 4),
            (13, 5),
            (20, 5),
            (21, 6),
            (36, 6),
            (37, 7),
            (64, 7),
        ];
        for (count, columns) in expected {
            assert_eq!(grid_columns(count), columns, "{count} games");
        }
    }

    #[test]
    fn the_starting_ply_follows_the_filter() {
        // A single game: the ply of the filter, the start when none.
        assert_eq!(initial_ply(0, true, 40), 0);
        assert_eq!(initial_ply(1, true, 40), 0);
        assert_eq!(initial_ply(11, true, 40), 10);
        assert_eq!(initial_ply(200, true, 40), 40);
        assert_eq!(initial_ply(FILTER_LAST_MOVE, true, 300), 300);
        // A grid: the end unless the filter has a ply inside the game.
        assert_eq!(initial_ply(0, false, 40), 40);
        assert_eq!(initial_ply(1, false, 40), 40);
        assert_eq!(initial_ply(11, false, 40), 10);
        assert_eq!(initial_ply(200, false, 40), 40);
    }
}
