//! The game browser (spec M3-4e; Scid `::gbrowser` and `browseGames`): one
//! game with its board, move text and buttons, or a grid of small boards.
//!
//! It is a modal layer over the window (D597). What it shows is state of
//! the application ([`BrowserState`]: the games, the ply and the
//! orientation of each board); this module keeps only what the screen
//! needs ([`BrowserUi`]: one board cache per game and the size of the grid
//! cells) and maps keys to messages ([`key_message`], in `keys`).
//!
//! Layout: `keys` (keys and wheel), `grid` (the grid of boards); the
//! single-game layer stays here with the shared widgets (a contract test
//! reads the Fluent keys from this file).
//!
//! Keyboard note: Enter and Space act on the browser, not on a focused
//! button (a button of the layer reached with Tab in the single-game mode
//! is pressed with the mouse).

use fluent_bundle::FluentArgs;
use iced::mouse::ScrollDelta;
use iced::widget::text::Span;
use iced::widget::{
    button, column, container, mouse_area, progress_bar, rich_text, row, scrollable, space, span,
    text, tooltip,
};
use iced::{Alignment, Border, Element, Length};
use mychess_app::{
    AppState, BaseSlot, BrowseMessage, BrowseMode, BrowserContent, BrowserGame, BrowserState,
    GameStep, ReadyGame, SlotId, TextKind,
};

use crate::i18n::I18n;
use crate::shell::{Message, Shell};
use crate::theme::{self, ButtonLook, Tokens};
use crate::widgets::board::geometry::{CoordsMode, Orientation};
use crate::widgets::board::marks::MarkBrush;
use crate::widgets::board::pieces::{PieceSet, PieceSetId};
use crate::widgets::board::{BoardCaches, BoardData, BoardOptions, BoardWidget};
use crate::widgets::icon::{Icon, IconTone, icon};
use crate::widgets::modal::{Card, modal};

/// Id of the browser box (the focus scope of the modal layer).
pub const BROWSER_BOX_ID: &str = "game-browser";
/// Id of the scrollable with the move text.
pub const BROWSER_MOVES_ID: &str = "game-browser-moves";
/// Smallest side of a board of the grid, in logical pixels.
pub const CELL_MIN: f32 = 96.0;
/// Largest side of a board of the grid.
pub const CELL_MAX: f32 = 320.0;
/// What a step of the size buttons adds or takes.
pub const CELL_STEP: f32 = 24.0;
/// Side of a board of the grid until the user changes it.
pub const CELL_DEFAULT: f32 = 152.0;
/// Plies a press of Up or Down moves in the single-game browser.
const BIG_STEP: i32 = 10;
/// Width of the text column of the single-game browser.
const TEXT_WIDTH: f32 = 340.0;

/// A message of the browser's own widgets.
#[derive(Debug, Clone)]
pub enum BrowserMessage {
    /// Pass this to the application.
    Act(BrowseMessage),
    /// Load a board's game into the main board. `keep_open` `None` means
    /// "Ctrl/Cmd keeps the browser open" (buttons, double click).
    Load {
        /// The board.
        game: usize,
        /// Keep the browser open, when the caller knows.
        keep_open: Option<bool>,
    },
    /// The wheel turned over a board: a ply, or the size with Ctrl/Cmd.
    Wheel {
        /// The board.
        game: usize,
        /// How much.
        delta: ScrollDelta,
    },
    /// Make the grid's boards larger (positive) or smaller (negative).
    Resize(i32),
    /// Nothing to do (events of the board canvases).
    Noop,
}

/// The visual state of the browser.
#[derive(Debug)]
pub struct BrowserUi {
    caches: Vec<BoardCaches>,
    pieces: PieceSet,
    brush: MarkBrush,
    options: BoardOptions,
    /// Side of a board of the grid.
    pub cell: f32,
    /// Scroll not yet worth a ply (trackpads send many small deltas).
    wheel: f32,
}

/// Pixels of a smooth scroll that make one ply.
const WHEEL_PIXELS_PER_PLY: f32 = 40.0;

impl Default for BrowserUi {
    fn default() -> BrowserUi {
        BrowserUi {
            caches: Vec::new(),
            pieces: PieceSet::default(),
            brush: MarkBrush::default(),
            options: BoardOptions::default(),
            cell: CELL_DEFAULT,
            wheel: 0.0,
        }
    }
}

impl BrowserUi {
    /// Brings the board caches up to date with the application (after every
    /// message). `board` is the look of the main board.
    pub fn sync(&mut self, app: &AppState, piece_set: PieceSetId, board: BoardOptions) {
        let Some(browser) = &app.browser else {
            self.caches.clear();
            return;
        };
        if self.pieces.id() != piece_set {
            self.pieces = PieceSet::load(piece_set);
        }
        // The same board colors; no hints or marks on a read-only board,
        // and coordinates only on the big board.
        self.options = BoardOptions {
            coords: if browser.mode == BrowseMode::Single {
                board.coords
            } else {
                CoordsMode::None
            },
            show_legal_hints: false,
            show_marks: false,
            ..board
        };
        self.caches
            .resize_with(browser.games.len(), BoardCaches::default);
        for (cache, game) in self.caches.iter_mut().zip(&browser.games) {
            if let Some(data) = board_data(game, self.options, &self.brush, &self.pieces) {
                cache.sync_data(&data);
            }
        }
    }

    /// The plies a turn of the wheel makes (positive: forward). A wheel
    /// with notches gives one per notch; smooth scrolling (pixels) adds up
    /// until `WHEEL_PIXELS_PER_PLY` and keeps the remainder.
    pub fn wheel_steps(&mut self, delta: ScrollDelta) -> i32 {
        match delta {
            ScrollDelta::Lines { .. } => {
                self.wheel = 0.0;
                wheel_ply(delta)
            }
            ScrollDelta::Pixels { y, .. } => {
                if !y.is_finite() {
                    return 0;
                }
                // Towards the end of the game when scrolled down (y < 0).
                self.wheel -= y;
                let whole = (self.wheel / WHEEL_PIXELS_PER_PLY).trunc();
                self.wheel -= whole * WHEEL_PIXELS_PER_PLY;
                // A few plies at most: the cast is exact.
                #[allow(clippy::cast_possible_truncation)]
                let steps = whole.clamp(-100.0, 100.0) as i32;
                steps
            }
        }
    }

    /// Changes the size of the grid's boards by `steps` steps.
    pub fn resize(&mut self, steps: i32) {
        let delta = f32::from(i16::try_from(steps.clamp(-20, 20)).unwrap_or(0)) * CELL_STEP;
        self.cell = (self.cell + delta).clamp(CELL_MIN, CELL_MAX);
    }
}

fn board_data<'a>(
    game: &'a BrowserGame,
    options: BoardOptions,
    brush: &'a MarkBrush,
    pieces: &'a PieceSet,
) -> Option<BoardData<'a>> {
    let ready = game.ready()?;
    Some(BoardData {
        position: ready.position()?,
        last_move: ready.last_move(),
        marks: &[],
        orientation: if game.flipped {
            Orientation::Black
        } else {
            Orientation::White
        },
        options,
        accepts_moves: false,
        brush,
        pieces,
        animation: None,
    })
}

mod grid;
mod keys;

pub use keys::{key_message, wheel_ply};

// ---- view ----

pub(super) fn base_label(i18n: &I18n, id: SlotId, slot: &BaseSlot) -> String {
    if id.is_clipbase() {
        return i18n.tr("games-clipbase");
    }
    let name = slot
        .path
        .as_deref()
        .and_then(std::path::Path::file_name)
        .map(|n| n.to_string_lossy().into_owned());
    name.unwrap_or_else(|| {
        let mut args = FluentArgs::new();
        args.set("number", i64::try_from(id.number()).unwrap_or(0));
        i18n.tr_args("games-slot-number", &args)
    })
}

pub(super) fn act(message: BrowseMessage) -> Message {
    Message::Browser(BrowserMessage::Act(message))
}

/// A small icon button with a tooltip; without a message it is disabled.
pub(super) fn tool<'a>(
    i18n: &I18n,
    glyph: Icon,
    tip_key: &str,
    message: Option<Message>,
    toggled: bool,
) -> Element<'a, Message> {
    let tone = match (&message, toggled) {
        (None, _) => IconTone::Disabled,
        (Some(_), true) => IconTone::OnAccent,
        (Some(_), false) => IconTone::Text,
    };
    let body = button(icon(glyph, 16.0, tone))
        .padding(8)
        .style(theme::command_button(ButtonLook::Secondary, toggled))
        .on_press_maybe(message);
    tooltip(
        body,
        container(text(i18n.tr(tip_key)).size(12))
            .padding([4, 8])
            .style(theme::tooltip_box),
        tooltip::Position::Bottom,
    )
    .into()
}

pub(super) fn text_button<'a>(
    label: String,
    message: Option<Message>,
    primary: bool,
) -> Element<'a, Message> {
    let body = button(text(label).size(13)).on_press_maybe(message);
    if primary {
        body.style(button::primary).into()
    } else {
        body.style(button::secondary).into()
    }
}

/// The browser's board, `side` pixels wide, or a placeholder.
pub(super) fn board<'a>(
    shell: &'a Shell,
    index: usize,
    game: &'a BrowserGame,
    side: f32,
) -> Element<'a, Message> {
    let ui = &shell.ui.browser;
    let drawn = board_data(game, ui.options, &ui.brush, &ui.pieces)
        .zip(ui.caches.get(index))
        .map(|(data, cache)| {
            BoardWidget::new(data, cache)
                .into_element()
                .map(|_| Message::Browser(BrowserMessage::Noop))
        });
    let inner: Element<'a, Message> = match (drawn, &game.content) {
        (Some(board), _) => board,
        (None, BrowserContent::Unreadable(_)) => container(
            text(shell.i18n.tr("browser-unreadable"))
                .size(13)
                .style(theme::muted_text),
        )
        .center(Length::Fill)
        .into(),
        (None, _) => container(
            text(shell.i18n.tr("browser-loading"))
                .size(13)
                .style(theme::muted_text),
        )
        .center(Length::Fill)
        .into(),
    };
    container(inner)
        .width(Length::Fixed(side))
        .height(Length::Fixed(side))
        .into()
}

/// The layer to push on the window `stack`: the browser, if it is open.
pub fn view(shell: &Shell) -> Option<Element<'_, Message>> {
    let browser = shell.app.browser.as_ref()?;
    let slot = shell.app.bases.slot(browser.slot)?;
    let i18n = &shell.i18n;
    let mut args = FluentArgs::new();
    args.set("base", base_label(i18n, browser.slot, slot));
    let (title_key, body_and_buttons) = match browser.mode {
        BrowseMode::Single => {
            args.set(
                "number",
                i64::from(browser.games.first().map_or(0, |g| g.number)),
            );
            ("browser-title-single", single(shell, browser))
        }
        BrowseMode::Grid => {
            args.set(
                "count",
                i64::try_from(browser.games.len()).unwrap_or(i64::MAX),
            );
            ("browser-title-grid", grid::grid(shell, browser))
        }
    };
    let (body, buttons, width) = body_and_buttons;
    Some(modal(Card {
        id: BROWSER_BOX_ID,
        title: i18n.tr_args(title_key, &args),
        body,
        buttons,
        width,
    }))
}

pub(super) type Layer<'a> = (Element<'a, Message>, Vec<Element<'a, Message>>, f32);

pub(super) fn window(shell: &Shell) -> iced::Size {
    shell
        .ui
        .window_size
        .unwrap_or(iced::Size::new(1280.0, 800.0))
}
#[allow(clippy::too_many_lines)] // the widgets of one dialog
pub(super) fn single<'a>(shell: &'a Shell, browser: &'a BrowserState) -> Layer<'a> {
    let i18n = &shell.i18n;
    let theme = shell.theme();
    let tokens = Tokens::of(&theme);
    let win = window(shell);
    let side = (win.height - 300.0).clamp(240.0, 500.0);
    let Some(game) = browser.games.first() else {
        return (space::vertical().into(), Vec::new(), 400.0);
    };
    let ready = game.ready();
    let loading = browser.is_loading();

    // Left: who plays, then the board.
    let mut head = column![].spacing(2);
    if let Some(ready) = ready {
        let mut players = FluentArgs::new();
        players.set("white", ready.header.white.as_str());
        players.set("black", ready.header.black.as_str());
        head = head.push(text(i18n.tr_args("browser-players", &players)).size(17));
        let mut event = FluentArgs::new();
        event.set("event", ready.header.event.as_str());
        event.set("date", ready.header.date.as_str());
        let mut line = i18n.tr_args("browser-event-line", &event);
        line.push_str("  ");
        line.push_str(ready.header.result.as_str());
        if !ready.header.eco.is_empty() {
            line.push_str("  ");
            line.push_str(&ready.header.eco);
        }
        head = head.push(text(line).size(12).style(theme::muted_text));
    } else if loading {
        head = head.push(loading_line(shell, browser));
    }
    let left = column![head, board(shell, 0, game, side)].spacing(8);

    // Right: the move text, the ply and the buttons.
    let moves: Element<'a, Message> = match ready {
        Some(ready) => scrollable(moves_text(ready, &tokens))
            .id(BROWSER_MOVES_ID)
            .width(Length::Fill)
            .height(Length::Fill)
            .into(),
        None => space::vertical().into(),
    };
    let position = ready.map_or_else(
        || text("").into(),
        |ready| {
            let mut args = FluentArgs::new();
            args.set("ply", i64::try_from(ready.ply()).unwrap_or(0));
            args.set("max", i64::try_from(ready.max_ply()).unwrap_or(0));
            let element: Element<'a, Message> = text(i18n.tr_args("browser-ply-line", &args))
                .size(12)
                .style(theme::muted_text)
                .into();
            element
        },
    );
    let ok = ready.is_some();
    let go = |m: BrowseMessage| ok.then(|| act(m));
    let nav = row![
        tool(
            i18n,
            Icon::First,
            "browser-tip-start",
            go(BrowseMessage::GoToStart { game: None }),
            false
        ),
        tool(
            i18n,
            Icon::Back,
            "browser-tip-back",
            go(BrowseMessage::Step {
                game: None,
                delta: -1
            }),
            false
        ),
        tool(
            i18n,
            Icon::Forward,
            "browser-tip-forward",
            go(BrowseMessage::Step {
                game: None,
                delta: 1
            }),
            false
        ),
        tool(
            i18n,
            Icon::Last,
            "browser-tip-end",
            go(BrowseMessage::GoToEnd { game: None }),
            false
        ),
        tool(
            i18n,
            if browser.autoplay {
                Icon::Pause
            } else {
                Icon::Play
            },
            if browser.autoplay {
                "browser-tip-pause"
            } else {
                "browser-tip-play"
            },
            go(BrowseMessage::ToggleAutoplay),
            browser.autoplay
        ),
        tool(
            i18n,
            Icon::Flip,
            "browser-tip-flip",
            go(BrowseMessage::Flip { game: None }),
            false
        ),
    ]
    .spacing(2);
    let games = row![
        text_button(
            i18n.tr("browser-first-game"),
            go(BrowseMessage::GoToGame(GameStep::First)),
            false
        ),
        text_button(
            i18n.tr("browser-previous-game"),
            go(BrowseMessage::GoToGame(GameStep::Previous)),
            false
        ),
        text_button(
            i18n.tr("browser-next-game"),
            go(BrowseMessage::GoToGame(GameStep::Next)),
            false
        ),
        text_button(
            i18n.tr("browser-last-game"),
            go(BrowseMessage::GoToGame(GameStep::Last)),
            false
        ),
    ]
    .spacing(4);
    let right = column![moves, position, nav, games]
        .spacing(8)
        .width(Length::Fixed(TEXT_WIDTH))
        .height(Length::Fixed(side + 34.0));
    let body = mouse_area(row![left, right].spacing(16).align_y(Alignment::Start))
        .on_scroll(|delta| Message::Browser(BrowserMessage::Wheel { game: 0, delta }));
    let buttons = vec![
        text_button(
            i18n.tr("browser-load"),
            ok.then(|| {
                Message::Browser(BrowserMessage::Load {
                    game: 0,
                    keep_open: None,
                })
            }),
            true,
        ),
        // Spec M7-6d: merge the browsed game into the game of the editor.
        text_button(
            i18n.tr("merge-button"),
            (ok && crate::windows::merge_dialog::can_merge(&shell.app)).then(|| {
                Message::App(crate::windows::merge_dialog::open_message_app(
                    browser.slot,
                    game.number,
                ))
            }),
            false,
        ),
        text_button(
            i18n.tr("browser-close"),
            Some(act(BrowseMessage::Close)),
            false,
        ),
    ];
    (body.into(), buttons, side + TEXT_WIDTH + 56.0)
}

pub(super) fn loading_line<'a>(
    shell: &'a Shell,
    browser: &'a BrowserState,
) -> Element<'a, Message> {
    let (done, total) = browser.progress();
    let mut args = FluentArgs::new();
    args.set("done", i64::try_from(done).unwrap_or(i64::MAX));
    args.set("total", i64::try_from(total).unwrap_or(i64::MAX));
    column![
        text(shell.i18n.tr_args("browser-progress", &args))
            .size(12)
            .style(theme::muted_text),
        progress_bar(0.0..=1.0, ratio(done, total))
            .length(Length::Fixed(160.0))
            .girth(Length::Fixed(6.0)),
    ]
    .spacing(4)
    .into()
}

fn ratio(done: u64, total: u64) -> f32 {
    if total == 0 {
        return 0.0;
    }
    // A fraction to draw: the precision of an f32 is plenty.
    #[allow(clippy::cast_precision_loss)]
    let ratio = (done as f32 / total as f32).clamp(0.0, 1.0);
    ratio
}

/// The main line as a paragraph; the move shown is highlighted and every
/// move is a link to its ply.
fn moves_text<'a>(ready: &'a ReadyGame, tokens: &Tokens) -> Element<'a, Message> {
    let shown = Some(ready.ply());
    let spans: Vec<Span<'a, usize>> = ready
        .text_items
        .iter()
        .map(|item| {
            let text = span(item.text.as_ref());
            match item.kind {
                TextKind::Number | TextKind::Result => text.color(tokens.muted),
                TextKind::Space => text,
                TextKind::Move if item.ply == shown => text
                    .link_maybe(item.ply)
                    .color(tokens.on_accent)
                    .background(tokens.accent)
                    .border(Border {
                        radius: tokens.radius.control.into(),
                        ..Border::default()
                    })
                    .padding([0, 3]),
                TextKind::Move => text.link_maybe(item.ply).color(tokens.text),
            }
        })
        .collect();
    rich_text(spans)
        .on_link_click(|ply| {
            Message::Browser(BrowserMessage::Act(BrowseMessage::GoToPly {
                game: 0,
                ply: u32::try_from(ply).unwrap_or(u32::MAX),
            }))
        })
        .size(15)
        .width(Length::Fill)
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cell_size_stays_inside_its_limits() {
        let mut ui = BrowserUi::default();
        ui.resize(100);
        assert!((ui.cell - CELL_MAX).abs() < f32::EPSILON);
        ui.resize(-100);
        assert!((ui.cell - CELL_MIN).abs() < f32::EPSILON);
    }

    #[test]
    fn the_wheel_steps_one_ply() {
        assert_eq!(wheel_ply(ScrollDelta::Lines { x: 0.0, y: -1.0 }), 1);
        assert_eq!(wheel_ply(ScrollDelta::Lines { x: 0.0, y: 2.0 }), -1);
        assert_eq!(wheel_ply(ScrollDelta::Pixels { x: 0.0, y: 0.0 }), 0);
    }
}
