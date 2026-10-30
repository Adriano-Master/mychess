//! The window of the advanced searches: by position, by material and
//! patterns, and by a sequence of moves (spec M6-1, slice c, D3236).
//!
//! The application owns every option ([`AdvancedSearchView`]); this module
//! only draws them and turns the clicks into [`AdvancedSearchMessage`]. What
//! it keeps is purely visual: the name typed for a new saved search and the
//! text of the numeric fields while they are being edited (a field may be
//! empty, or hold a lone `-`, for a moment without the search changing).

use std::collections::HashMap;

use iced::widget::{
    button, column, container, pick_list, progress_bar, row, scrollable, space, text, text_input,
    tooltip,
};
use iced::{Alignment, Element, Length};
use mychess_app::{
    AdvancedKind, AdvancedSearchMessage, AdvancedSearchView, AppState, BishopsKind, BoardKind,
    CheckKind, DatabaseChoice, MaterialPiece, MaterialView, PATTERN_ROWS, PatternView, PresetView,
    RangeView, SearchOp, SideKind,
};
use mychess_core::{Color, File, Piece, Rank, Role};

use crate::i18n::I18n;
use crate::shell::Message;
use crate::theme::{self, ButtonLook};
use crate::widgets::icon::{Icon, IconTone, icon};
use crate::widgets::modal::{Card, modal};
use crate::windows::advanced_search_text::{check_key, notice_text, preset_key, summary_text};
use crate::windows::annotate_dialog::{check, choice, group, labelled};
use crate::windows::header_dialog::Labeled;

/// Id of the box.
pub const ADVANCED_BOX_ID: &str = "advanced-search-box";
/// Id of the field of the FEN.
pub const ADVANCED_FEN_ID: &str = "advanced-search-fen";
/// Id of the field of the moves.
pub const ADVANCED_MOVES_ID: &str = "advanced-search-moves";
/// Id of the "Search" button.
pub const ADVANCED_START_ID: &str = "advanced-search-start";
/// Id of the "Stop" button.
pub const ADVANCED_STOP_ID: &str = "advanced-search-stop";
/// Id of the "Close" button.
pub const ADVANCED_CLOSE_ID: &str = "advanced-search-close";
/// Id of the "Save" button of the saved searches.
pub const ADVANCED_SAVE_ID: &str = "advanced-search-save";
/// Id of the "Import .sso" button.
pub const ADVANCED_IMPORT_ID: &str = "advanced-search-import";
/// Id of the field with the name of a new saved search.
pub const ADVANCED_NAME_ID: &str = "advanced-search-name";

/// Id of the button of a tab.
pub const fn tab_id(kind: AdvancedKind) -> &'static str {
    match kind {
        AdvancedKind::Position => "advanced-search-tab-position",
        AdvancedKind::Material => "advanced-search-tab-material",
        AdvancedKind::Moves => "advanced-search-tab-moves",
    }
}

/// The tallest the scrolling form is; a low window makes it shorter.
const BODY_HEIGHT: f32 = 430.0;
/// The shortest the form is made, however low the window.
const BODY_MIN_HEIGHT: f32 = 160.0;
/// What the card spends besides the form: title, tabs, status lines, buttons,
/// padding and gaps.
const CHROME_HEIGHT: f32 = 270.0;

/// The height of the scrolling form for a window `window_height` tall.
fn body_height(window_height: f32) -> f32 {
    (window_height - CHROME_HEIGHT).clamp(BODY_MIN_HEIGHT, BODY_HEIGHT)
}
const CARD_WIDTH: f32 = 780.0;
const SIDE_WIDTH: f32 = 250.0;
const PIECE_LABEL_WIDTH: f32 = 80.0;
const FIELD_WIDTH: f32 = 36.0;
const WIDE_FIELD_WIDTH: f32 = 52.0;
const SAVED_HEIGHT: f32 = 84.0;

/// The numeric fields that can be edited as text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NumField {
    /// Fewest pieces of a colour.
    Min(Color, MaterialPiece),
    /// Most pieces of a colour.
    Max(Color, MaterialPiece),
    /// Lowest material difference.
    DiffMin,
    /// Highest material difference.
    DiffMax,
    /// First move number.
    MoveMin,
    /// Last move number.
    MoveMax,
    /// "Match for at least" this many half moves.
    MinPlies,
}

impl NumField {
    fn signed(self) -> bool {
        matches!(self, NumField::DiffMin | NumField::DiffMax)
    }

    fn single_digit(self) -> bool {
        matches!(self, NumField::Min(..) | NumField::Max(..))
    }
}

/// An event of the window that is not just a message for the application.
#[derive(Debug, Clone, PartialEq)]
pub enum AdvancedUiMessage {
    /// A message for the application (the visual drafts are dropped).
    Send(AdvancedSearchMessage),
    /// The text of a numeric field changed.
    Number(NumField, String),
    /// The name of a new saved search changed.
    Name(String),
    /// "Save": the search of the tab under the name typed.
    SaveNamed,
    /// "Import .sso": asks for the file.
    PickSso,
    /// The file dialog answered (`None`: cancelled).
    SsoPicked(Option<std::path::PathBuf>),
}

/// The visual state of the window.
#[derive(Debug, Default)]
pub struct AdvancedSearchUi {
    name: String,
    drafts: HashMap<NumField, String>,
}

impl AdvancedSearchUi {
    /// Forgets the drafts when the window is gone.
    pub fn sync(&mut self, app: &AppState) {
        if !app.advanced_search().is_open() {
            self.drafts.clear();
        }
    }

    /// The name typed for a new saved search.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Replaces the name of the new saved search.
    pub fn set_name(&mut self, name: String) {
        self.name = name;
    }

    /// Forgets the text of every numeric field.
    pub fn clear_drafts(&mut self) {
        self.drafts.clear();
    }

    /// What a numeric field shows: the draft while it still stands for the
    /// value (or is half typed), else the value.
    pub fn shown(&self, field: NumField, current: i64) -> String {
        match self.drafts.get(&field) {
            Some(draft)
                if draft.is_empty()
                    || (draft == "-" && field.signed())
                    || draft.trim().parse::<i64>().ok() == Some(current) =>
            {
                draft.clone()
            }
            _ => current.to_string(),
        }
    }

    /// Takes the text typed in a field. The result is the number to apply,
    /// or `None` when the text is half typed or is not a number (the field
    /// goes back to the value).
    pub fn type_into(&mut self, field: NumField, typed: &str) -> Option<i64> {
        let typed = typed.trim();
        // The fields of a count hold one digit: the last one typed wins.
        let typed = if field.single_digit() {
            typed
                .chars()
                .rev()
                .find(char::is_ascii_digit)
                .map(String::from)
                .unwrap_or_default()
        } else {
            typed.to_owned()
        };
        if typed.is_empty() || (typed == "-" && field.signed()) {
            self.drafts.insert(field, typed);
            return None;
        }
        let number: i64 = typed.parse().ok()?;
        if number < 0 && !field.signed() {
            return None;
        }
        self.drafts.insert(field, typed);
        Some(number)
    }
}

/// The message for the application that a number typed stands for.
pub fn number_message(
    field: NumField,
    number: i64,
    view: &AdvancedSearchView,
) -> AdvancedSearchMessage {
    let material = &view.material;
    let clamp_u8 = |n: i64| u8::try_from(n.clamp(0, 255)).unwrap_or(u8::MAX);
    let clamp_u16 = |n: i64| u16::try_from(n.clamp(0, 9999)).unwrap_or(u16::MAX);
    match field {
        NumField::Min(color, piece) => {
            let now = side_range(material, color, piece);
            let min = clamp_u8(number);
            AdvancedSearchMessage::SetRange {
                color,
                piece,
                min,
                max: now.max.max(min),
            }
        }
        NumField::Max(color, piece) => {
            let now = side_range(material, color, piece);
            let max = clamp_u8(number);
            AdvancedSearchMessage::SetRange {
                color,
                piece,
                min: now.min.min(max),
                max,
            }
        }
        NumField::DiffMin | NumField::DiffMax => {
            let (low, high) = material.diff.unwrap_or(DEFAULT_DIFF);
            let value = i16::try_from(number.clamp(-99, 99)).unwrap_or(0);
            AdvancedSearchMessage::SetDiff(Some(if field == NumField::DiffMin {
                (value, high)
            } else {
                (low, value)
            }))
        }
        NumField::MoveMin => {
            AdvancedSearchMessage::SetMoveRange(clamp_u16(number), material.move_max)
        }
        NumField::MoveMax => {
            AdvancedSearchMessage::SetMoveRange(material.move_min, clamp_u16(number))
        }
        NumField::MinPlies => AdvancedSearchMessage::SetMinPlies(clamp_u16(number)),
    }
}

/// The difference shown when none is set (the Scid's "Any", -40..+40).
const DEFAULT_DIFF: (i16, i16) = (-40, 40);

fn side_range(material: &MaterialView, color: Color, piece: MaterialPiece) -> RangeView {
    match color {
        Color::White => material.white.get(piece),
        Color::Black => material.black.get(piece),
    }
}

fn send(message: AdvancedSearchMessage) -> Message {
    Message::AdvancedUi(AdvancedUiMessage::Send(message))
}

fn ui(message: AdvancedUiMessage) -> Message {
    Message::AdvancedUi(message)
}

fn start() -> Message {
    send(AdvancedSearchMessage::Start)
}

/// The message of the application for "Close" and Esc outside a search.
pub fn close_message() -> mychess_app::Message {
    mychess_app::Message::AdvancedSearch(AdvancedSearchMessage::Close)
}

/// The message of the application for "Stop" and Esc during a search.
pub fn stop_message() -> mychess_app::Message {
    mychess_app::Message::AdvancedSearch(AdvancedSearchMessage::Stop)
}

/// The message of the application for "Search" and Enter.
pub fn start_message() -> mychess_app::Message {
    mychess_app::Message::AdvancedSearch(AdvancedSearchMessage::Start)
}

// ---- small pieces ----

fn tip(body: Element<'_, Message>, tip: String) -> Element<'_, Message> {
    tooltip(body, text(tip).size(12), tooltip::Position::Top)
        .style(theme::tooltip_box)
        .into()
}

/// A button with an icon and a label.
fn command<'a>(
    glyph: Icon,
    label: String,
    primary: bool,
    message: Option<Message>,
) -> iced::widget::Button<'a, Message> {
    let tone = if message.is_none() {
        IconTone::Disabled
    } else if primary {
        IconTone::OnAccent
    } else {
        IconTone::Text
    };
    let body = row![icon(glyph, 14.0, tone), text(label).size(13)]
        .spacing(6)
        .align_y(Alignment::Center);
    button(body)
        .padding([4, 10])
        .style(if primary {
            button::primary
        } else {
            button::secondary
        })
        .on_press_maybe(message)
}

fn with_id<'a>(id: &'static str, body: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    container(body).id(id).into()
}

fn small<'a>(label: String, on: bool, message: Message) -> Element<'a, Message> {
    button(text(label).size(12))
        .padding([2, 6])
        .style(theme::command_button(ButtonLook::Secondary, on))
        .on_press(message)
        .into()
}

fn number_field<'a>(
    ui_state: &AdvancedSearchUi,
    field: NumField,
    current: i64,
    enabled: bool,
    width: f32,
) -> Element<'a, Message> {
    let shown = ui_state.shown(field, current);
    let mut input = text_input("", &shown)
        .size(13)
        .padding([2, 4])
        .width(Length::Fixed(width));
    if enabled {
        input = input
            .on_input(move |t| ui(AdvancedUiMessage::Number(field, t)))
            .on_submit(start());
    }
    input.into()
}

fn dash<'a>() -> Element<'a, Message> {
    text("-").size(13).into()
}

fn section(title: String, body: Element<'_, Message>) -> Element<'_, Message> {
    column![text(title).size(13).style(theme::accent_text), body]
        .spacing(6)
        .into()
}

fn pick<'a, T: Clone + PartialEq + 'a>(
    options: Vec<Labeled<T>>,
    current: Option<Labeled<T>>,
    placeholder: String,
    width: f32,
    on: impl Fn(T) -> Message + 'a,
) -> Element<'a, Message> {
    pick_list(options, current, move |l: Labeled<T>| on(l.value))
        .placeholder(placeholder)
        .text_size(12)
        .padding(3)
        .width(Length::Fixed(width))
        .into()
}

fn labeled<T>(value: T, label: String) -> Labeled<T> {
    Labeled { value, label }
}

// ---- common parts ----

fn tabs<'a>(view: &AdvancedSearchView, i18n: &I18n) -> Element<'a, Message> {
    let tab = |kind: AdvancedKind, key: &str| {
        with_id(
            tab_id(kind),
            choice(i18n.tr(key), kind, view.tab, true, |k| {
                send(AdvancedSearchMessage::SelectTab(k))
            }),
        )
    };
    row![
        tab(AdvancedKind::Position, "advsearch-tab-position"),
        tab(AdvancedKind::Material, "advsearch-tab-material"),
        tab(AdvancedKind::Moves, "advsearch-tab-moves"),
    ]
    .spacing(4)
    .into()
}

fn op_row<'a>(kind: AdvancedKind, current: SearchOp, i18n: &I18n) -> Element<'a, Message> {
    let op = |value: SearchOp, key: &str| {
        choice(i18n.tr(key), value, current, true, move |v| {
            send(AdvancedSearchMessage::SetOp(kind, v))
        })
    };
    labelled(
        i18n.tr("advsearch-op"),
        group(vec![
            op(SearchOp::Reset, "advsearch-op-reset"),
            op(SearchOp::And, "advsearch-op-and"),
            op(SearchOp::Or, "advsearch-op-or"),
        ]),
    )
}

fn database_label(d: &DatabaseChoice, i18n: &I18n) -> String {
    if d.clipbase {
        i18n.tr("games-clipbase")
    } else if d.name.is_empty() {
        let mut args = fluent_bundle::FluentArgs::new();
        args.set("number", i64::try_from(d.slot.number()).unwrap_or(0));
        i18n.tr_args("games-slot-number", &args)
    } else {
        d.name.clone()
    }
}

fn reference_row<'a>(
    kind: AdvancedKind,
    reference: Option<mychess_app::SlotId>,
    view: &AdvancedSearchView,
    i18n: &I18n,
) -> Element<'a, Message> {
    let first = view.databases.first().map(|d| d.slot);
    let options: Vec<Labeled<mychess_app::SlotId>> = view
        .databases
        .iter()
        .map(|d| labeled(d.slot, database_label(d, i18n)))
        .collect();
    let current = reference.and_then(|slot| {
        view.databases
            .iter()
            .find(|d| d.slot == slot)
            .map(|d| labeled(slot, database_label(d, i18n)))
    });
    let toggle = check(
        i18n.tr("advsearch-reference"),
        reference.is_some(),
        first.is_some(),
        move |on| {
            send(AdvancedSearchMessage::SetReference(
                kind,
                if on { first } else { None },
            ))
        },
    );
    let list = pick(
        options,
        current,
        i18n.tr("advsearch-reference-none"),
        200.0,
        move |slot| send(AdvancedSearchMessage::SetReference(kind, Some(slot))),
    );
    row![toggle, list]
        .spacing(10)
        .align_y(Alignment::Center)
        .into()
}

// ---- the position tab ----

fn position_tab<'a>(view: &AdvancedSearchView, i18n: &I18n) -> Element<'a, Message> {
    let p = &view.position;
    let kind = |value: BoardKind, key: &str| {
        tip(
            choice(i18n.tr(key), value, p.kind, true, |v| {
                send(AdvancedSearchMessage::SetBoardKind(v))
            }),
            i18n.tr(&format!("{key}-tip")),
        )
    };
    let mut page = column![
        labelled(
            i18n.tr("advsearch-board-type"),
            group(vec![
                kind(BoardKind::Exact, "advsearch-board-exact"),
                kind(BoardKind::Pawns, "advsearch-board-pawns"),
                kind(BoardKind::Files, "advsearch-board-files"),
                kind(BoardKind::Material, "advsearch-board-material"),
            ]),
        ),
        row![
            check(
                i18n.tr("advsearch-board-variations"),
                p.variations,
                true,
                |v| send(AdvancedSearchMessage::SetBoardVariations(v)),
            ),
            check(
                i18n.tr("advsearch-ignore-colors"),
                p.ignore_colors,
                true,
                |v| send(AdvancedSearchMessage::SetBoardIgnoreColors(v)),
            ),
        ]
        .spacing(16),
    ]
    .spacing(10);
    let board_fen = p.board_fen.clone();
    let typed = p.fen.clone();
    let source = labelled(
        i18n.tr("advsearch-board-position"),
        group(vec![
            choice(
                i18n.tr("advsearch-board-from-board"),
                typed.is_some(),
                false,
                true,
                |_| send(AdvancedSearchMessage::UseCurrentPosition),
            ),
            choice(
                i18n.tr("advsearch-board-from-fen"),
                typed.is_some(),
                true,
                true,
                move |_| send(AdvancedSearchMessage::SetFen(board_fen.clone())),
            ),
        ]),
    );
    page = page.push(source);
    match typed {
        Some(fen) => {
            page = page.push(
                text_input(&i18n.tr("advsearch-fen-placeholder"), &fen)
                    .id(ADVANCED_FEN_ID)
                    .on_input(|t| send(AdvancedSearchMessage::SetFen(t)))
                    .on_submit(start())
                    .size(13)
                    .padding([3, 6]),
            );
        }
        None => {
            page = page.push(text(p.board_fen.clone()).size(12).style(theme::muted_text));
        }
    }
    page = page.push(
        text(i18n.tr("advsearch-board-hint"))
            .size(12)
            .style(theme::muted_text),
    );
    page.into()
}

// ---- the material tab ----

fn piece_key(piece: MaterialPiece) -> &'static str {
    match piece {
        MaterialPiece::Queen => "advsearch-material-queen",
        MaterialPiece::Rook => "advsearch-material-rook",
        MaterialPiece::Bishop => "advsearch-material-bishop",
        MaterialPiece::Knight => "advsearch-material-knight",
        MaterialPiece::Minor => "advsearch-material-minor",
        MaterialPiece::Pawn => "advsearch-material-pawn",
    }
}

/// The largest count each row accepts (Scid: the `0+` and `1+` buttons).
fn row_limit(piece: MaterialPiece) -> u8 {
    match piece {
        MaterialPiece::Minor => 4,
        MaterialPiece::Pawn => 8,
        _ => 2,
    }
}

fn range_side<'a>(
    ui_state: &AdvancedSearchUi,
    color: Color,
    piece: MaterialPiece,
    range: RangeView,
) -> Element<'a, Message> {
    let limit = row_limit(piece);
    let quick = |label: &str, min: u8, max: u8| {
        small(
            label.to_owned(),
            (range.min, range.max) == (min, max),
            send(AdvancedSearchMessage::SetRange {
                color,
                piece,
                min,
                max,
            }),
        )
    };
    row![
        quick("0", 0, 0),
        quick("1", 1, 1),
        quick("2", 2, 2),
        quick("0+", 0, limit),
        quick("1+", 1, limit),
        number_field(
            ui_state,
            NumField::Min(color, piece),
            i64::from(range.min),
            true,
            FIELD_WIDTH
        ),
        dash(),
        number_field(
            ui_state,
            NumField::Max(color, piece),
            i64::from(range.max),
            true,
            FIELD_WIDTH
        ),
    ]
    .spacing(3)
    .align_y(Alignment::Center)
    .width(Length::Fixed(SIDE_WIDTH))
    .into()
}

fn material_grid<'a>(
    ui_state: &AdvancedSearchUi,
    m: &MaterialView,
    i18n: &I18n,
) -> Element<'a, Message> {
    let mut grid = column![
        row![
            space::horizontal().width(Length::Fixed(PIECE_LABEL_WIDTH)),
            text(i18n.tr("advsearch-white"))
                .size(13)
                .width(Length::Fixed(SIDE_WIDTH)),
            text(i18n.tr("advsearch-black"))
                .size(13)
                .width(Length::Fixed(SIDE_WIDTH)),
        ]
        .spacing(10),
    ]
    .spacing(4);
    for piece in MaterialPiece::ALL {
        grid = grid.push(
            row![
                text(i18n.tr(piece_key(piece)))
                    .size(13)
                    .width(Length::Fixed(PIECE_LABEL_WIDTH)),
                range_side(ui_state, Color::White, piece, m.white.get(piece)),
                range_side(ui_state, Color::Black, piece, m.black.get(piece)),
            ]
            .spacing(10)
            .align_y(Alignment::Center),
        );
    }
    grid.into()
}

fn preset_pick<'a>(
    presets: &[PresetView],
    placeholder: String,
    i18n: &I18n,
    on: impl Fn(usize) -> Message + 'a,
) -> Element<'a, Message> {
    let options: Vec<Labeled<usize>> = presets
        .iter()
        .map(|p| labeled(p.index, i18n.tr(&preset_key(p.key))))
        .collect();
    pick(options, None, placeholder, 220.0, on)
}

fn diff_row<'a>(
    ui_state: &AdvancedSearchUi,
    m: &MaterialView,
    i18n: &I18n,
) -> Element<'a, Message> {
    let (low, high) = m.diff.unwrap_or(DEFAULT_DIFF);
    let set = |label: &str, value: Option<(i16, i16)>| {
        small(
            label.to_owned(),
            m.diff == value,
            send(AdvancedSearchMessage::SetDiff(value)),
        )
    };
    column![
        row![
            number_field(
                ui_state,
                NumField::DiffMin,
                i64::from(low),
                true,
                WIDE_FIELD_WIDTH
            ),
            dash(),
            number_field(
                ui_state,
                NumField::DiffMax,
                i64::from(high),
                true,
                WIDE_FIELD_WIDTH
            ),
            set(&i18n.tr("advsearch-diff-any"), None),
            set("+", Some((1, 40))),
            set("=", Some((0, 0))),
            set("-", Some((-40, -1))),
        ]
        .spacing(4)
        .align_y(Alignment::Center),
        text(i18n.tr("advsearch-diff-hint"))
            .size(12)
            .style(theme::muted_text),
    ]
    .spacing(4)
    .into()
}

fn pattern_piece_options(i18n: &I18n) -> Vec<Labeled<Option<Piece>>> {
    let mut options = vec![labeled(None, i18n.tr("advsearch-pattern-any"))];
    for color in [Color::White, Color::Black] {
        for role in [
            Role::King,
            Role::Queen,
            Role::Rook,
            Role::Bishop,
            Role::Knight,
            Role::Pawn,
        ] {
            let piece = Piece { color, role };
            options.push(labeled(Some(piece), i18n.tr(pattern_piece_key(piece))));
        }
    }
    options
}

fn pattern_piece_key(piece: Piece) -> &'static str {
    match (piece.color, piece.role) {
        (Color::White, Role::King) => "advsearch-piece-wk",
        (Color::White, Role::Queen) => "advsearch-piece-wq",
        (Color::White, Role::Rook) => "advsearch-piece-wr",
        (Color::White, Role::Bishop) => "advsearch-piece-wb",
        (Color::White, Role::Knight) => "advsearch-piece-wn",
        (Color::White, Role::Pawn) => "advsearch-piece-wp",
        (Color::Black, Role::King) => "advsearch-piece-bk",
        (Color::Black, Role::Queen) => "advsearch-piece-bq",
        (Color::Black, Role::Rook) => "advsearch-piece-br",
        (Color::Black, Role::Bishop) => "advsearch-piece-bb",
        (Color::Black, Role::Knight) => "advsearch-piece-bn",
        (Color::Black, Role::Pawn) => "advsearch-piece-bp",
    }
}

fn pattern_row<'a>(
    index: usize,
    p: PatternView,
    options: &[Labeled<Option<Piece>>],
    i18n: &I18n,
) -> Element<'a, Message> {
    let current = options.iter().find(|o| o.value == p.piece).cloned();
    let files: Vec<Labeled<Option<File>>> = std::iter::once(labeled(None, "?".to_owned()))
        .chain(
            File::ALL
                .into_iter()
                .map(|f| labeled(Some(f), f.char().to_string())),
        )
        .collect();
    let ranks: Vec<Labeled<Option<Rank>>> = std::iter::once(labeled(None, "?".to_owned()))
        .chain(
            Rank::ALL
                .into_iter()
                .map(|r| labeled(Some(r), r.char().to_string())),
        )
        .collect();
    let file_now = files.iter().find(|o| o.value == p.file).cloned();
    let rank_now = ranks.iter().find(|o| o.value == p.rank).cloned();
    let sense = if p.present {
        i18n.tr("advsearch-pattern-yes")
    } else {
        i18n.tr("advsearch-pattern-no")
    };
    row![
        button(text(sense).size(12))
            .padding([2, 6])
            .width(Length::Fixed(40.0))
            .style(theme::command_button(ButtonLook::Secondary, p.present))
            .on_press(send(AdvancedSearchMessage::SetPatternPresent {
                index,
                present: !p.present,
            })),
        pick(
            options.to_vec(),
            current,
            String::new(),
            108.0,
            move |piece| { send(AdvancedSearchMessage::SetPatternPiece { index, piece }) }
        ),
        pick(files, file_now, String::new(), 54.0, move |file| {
            send(AdvancedSearchMessage::SetPatternFile { index, file })
        }),
        pick(ranks, rank_now, String::new(), 54.0, move |rank| {
            send(AdvancedSearchMessage::SetPatternRank { index, rank })
        }),
    ]
    .spacing(4)
    .align_y(Alignment::Center)
    .into()
}

fn patterns_block<'a>(
    view: &AdvancedSearchView,
    m: &MaterialView,
    i18n: &I18n,
) -> Element<'a, Message> {
    let options = pattern_piece_options(i18n);
    let rows: Vec<PatternView> = m.patterns.iter().copied().take(PATTERN_ROWS).collect();
    let half = rows.len().div_ceil(2);
    let mut left = column![].spacing(4);
    let mut right = column![].spacing(4);
    for (index, p) in rows.into_iter().enumerate() {
        let line = pattern_row(index, p, &options, i18n);
        if index < half {
            left = left.push(line);
        } else {
            right = right.push(line);
        }
    }
    column![
        row![
            preset_pick(
                &view.patterns,
                i18n.tr("advsearch-patterns-common"),
                i18n,
                |i| send(AdvancedSearchMessage::ApplyPatternPreset(i)),
            ),
            command(
                Icon::Eraser,
                i18n.tr("advsearch-patterns-clear"),
                false,
                Some(send(AdvancedSearchMessage::ClearPatterns)),
            ),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
        row![left, right].spacing(20),
    ]
    .spacing(8)
    .into()
}

#[allow(clippy::too_many_lines)] // one flat column of the sections of the tab
fn material_tab<'a>(
    ui_state: &AdvancedSearchUi,
    view: &AdvancedSearchView,
    i18n: &I18n,
) -> Element<'a, Message> {
    let m = &view.material;
    let tools = row![
        command(
            Icon::ResetFilter,
            i18n.tr("advsearch-material-zero"),
            false,
            Some(send(AdvancedSearchMessage::MaterialZero)),
        ),
        command(
            Icon::Material,
            i18n.tr("advsearch-material-any"),
            false,
            Some(send(AdvancedSearchMessage::MaterialAny)),
        ),
        command(
            Icon::SetupBoard,
            i18n.tr("advsearch-material-current"),
            false,
            Some(send(AdvancedSearchMessage::MaterialCurrentBoard)),
        ),
        preset_pick(
            &view.endings,
            i18n.tr("advsearch-material-endings"),
            i18n,
            |i| send(AdvancedSearchMessage::ApplyEnding(i)),
        ),
    ]
    .spacing(8)
    .align_y(Alignment::Center);
    let bishop = |value: BishopsKind, key: &str| {
        choice(i18n.tr(key), value, m.bishops, true, |v| {
            send(AdvancedSearchMessage::SetBishops(v))
        })
    };
    let ranges = !m.end_only;
    let moves = row![
        number_field(
            ui_state,
            NumField::MoveMin,
            i64::from(m.move_min),
            ranges,
            WIDE_FIELD_WIDTH
        ),
        dash(),
        number_field(
            ui_state,
            NumField::MoveMax,
            i64::from(m.move_max),
            ranges,
            WIDE_FIELD_WIDTH
        ),
    ]
    .spacing(4)
    .align_y(Alignment::Center);
    let plies = row![
        number_field(
            ui_state,
            NumField::MinPlies,
            i64::from(m.min_plies),
            ranges,
            FIELD_WIDTH
        ),
        text(i18n.tr("advsearch-material-halfmoves")).size(13),
    ]
    .spacing(6)
    .align_y(Alignment::Center);
    column![
        section(
            i18n.tr("advsearch-material-group"),
            column![material_grid(ui_state, m, i18n), tools]
                .spacing(8)
                .into(),
        ),
        section(
            i18n.tr("advsearch-material-diff"),
            diff_row(ui_state, m, i18n)
        ),
        labelled(
            i18n.tr("advsearch-bishops"),
            group(vec![
                bishop(BishopsKind::Either, "advsearch-bishops-either"),
                bishop(BishopsKind::Same, "advsearch-bishops-same"),
                bishop(BishopsKind::Opposite, "advsearch-bishops-opposite"),
            ]),
        ),
        labelled(i18n.tr("advsearch-material-range"), moves.into()),
        labelled(i18n.tr("advsearch-material-length"), plies.into()),
        row![
            check(
                i18n.tr("advsearch-ignore-colors"),
                m.ignore_colors,
                true,
                |v| send(AdvancedSearchMessage::SetMaterialIgnoreColors(v)),
            ),
            check(
                i18n.tr("advsearch-material-end-only"),
                m.end_only,
                true,
                |v| send(AdvancedSearchMessage::SetEndOnly(v)),
            ),
        ]
        .spacing(16),
        section(
            i18n.tr("advsearch-patterns-group"),
            patterns_block(view, m, i18n)
        ),
    ]
    .spacing(10)
    .into()
}

// ---- the moves tab ----

fn moves_tab<'a>(view: &AdvancedSearchView, i18n: &I18n) -> Element<'a, Message> {
    let m = &view.moves;
    let side = |value: SideKind, key: &str| {
        choice(i18n.tr(key), value, m.side, true, |v| {
            send(AdvancedSearchMessage::SetMovesSide(v))
        })
    };
    let test = |value: CheckKind| {
        choice(i18n.tr(check_key(value)), value, m.check, true, |v| {
            send(AdvancedSearchMessage::SetMovesCheck(v))
        })
    };
    column![
        labelled(
            i18n.tr("advsearch-moves-label"),
            text_input(&i18n.tr("advsearch-moves-placeholder"), &m.text)
                .id(ADVANCED_MOVES_ID)
                .on_input(|t| send(AdvancedSearchMessage::SetMovesText(t)))
                .on_submit(start())
                .size(13)
                .padding([3, 6])
                .into(),
        ),
        text(i18n.tr("advsearch-moves-hint"))
            .size(12)
            .style(theme::muted_text),
        labelled(
            i18n.tr("advsearch-moves-side"),
            group(vec![
                side(SideKind::White, "advsearch-side-white"),
                side(SideKind::Black, "advsearch-side-black"),
                side(SideKind::Both, "advsearch-side-both"),
            ]),
        ),
        tip(
            labelled(
                i18n.tr("advsearch-moves-checktest"),
                group(vec![
                    test(CheckKind::No),
                    test(CheckKind::Check),
                    test(CheckKind::Mate)
                ]),
            ),
            i18n.tr("advsearch-moves-checktest-tip"),
        ),
        check(
            i18n.tr("advsearch-moves-variations"),
            m.variations,
            true,
            |v| send(AdvancedSearchMessage::SetMovesVariations(v)),
        ),
    ]
    .spacing(10)
    .into()
}

// ---- saved searches ----

fn saved_block<'a>(
    ui_state: &AdvancedSearchUi,
    view: &AdvancedSearchView,
    i18n: &I18n,
) -> Element<'a, Message> {
    let mut list = column![].spacing(2);
    if view.saved.is_empty() {
        list = list.push(
            text(i18n.tr("advsearch-saved-empty"))
                .size(12)
                .style(theme::muted_text),
        );
    }
    for entry in &view.saved {
        let load = button(text(entry.name.clone()).size(13))
            .padding([2, 8])
            .width(Length::Fill)
            .style(button::text)
            .on_press(send(AdvancedSearchMessage::LoadSaved(entry.name.clone())));
        let delete = button(icon(Icon::Trash, 14.0, IconTone::Text))
            .padding(3)
            .style(button::secondary)
            .on_press(send(AdvancedSearchMessage::DeleteSaved(entry.name.clone())));
        list = list.push(
            row![
                tip(load.into(), i18n.tr("advsearch-saved-load-tip")),
                tip(delete.into(), i18n.tr("advsearch-saved-delete-tip"))
            ]
            .spacing(4)
            .align_y(Alignment::Center),
        );
    }
    let can_save = view.can_save && !ui_state.name().trim().is_empty();
    let name = text_input(&i18n.tr("advsearch-saved-name"), ui_state.name())
        .id(ADVANCED_NAME_ID)
        .on_input(|t| ui(AdvancedUiMessage::Name(t)))
        .on_submit(ui(AdvancedUiMessage::SaveNamed))
        .size(13)
        .padding([3, 6]);
    let mut actions = row![
        name,
        with_id(
            ADVANCED_SAVE_ID,
            tip(
                command(
                    Icon::Save,
                    i18n.tr("advsearch-saved-save"),
                    false,
                    can_save.then(|| ui(AdvancedUiMessage::SaveNamed)),
                )
                .into(),
                i18n.tr("advsearch-saved-save-tip"),
            ),
        ),
    ]
    .spacing(8)
    .align_y(Alignment::Center);
    if view.tab == AdvancedKind::Material {
        actions = actions.push(with_id(
            ADVANCED_IMPORT_ID,
            tip(
                command(
                    Icon::Import,
                    i18n.tr("advsearch-saved-import"),
                    false,
                    Some(ui(AdvancedUiMessage::PickSso)),
                )
                .into(),
                i18n.tr("advsearch-saved-import-tip"),
            ),
        ));
    }
    section(
        i18n.tr("advsearch-saved-group"),
        column![
            container(scrollable(list).width(Length::Fill)).height(Length::Fixed(SAVED_HEIGHT)),
            actions,
            if view.can_save {
                Element::from(space::vertical().height(Length::Fixed(0.0)))
            } else {
                text(i18n.tr("advsearch-saved-no-folder"))
                    .size(12)
                    .style(text::danger)
                    .into()
            },
        ]
        .spacing(6)
        .into(),
    )
}

// ---- the window ----

fn ratio(done: u64, total: u64) -> f32 {
    if total == 0 {
        return 0.0;
    }
    // A fraction to draw: the precision of an f32 is plenty.
    #[allow(clippy::cast_precision_loss)]
    let ratio = (done as f32 / total as f32).clamp(0.0, 1.0);
    ratio
}

fn status_lines<'a>(view: &AdvancedSearchView, i18n: &I18n) -> Element<'a, Message> {
    let mut lines = column![].spacing(2);
    if let Some(running) = &view.running {
        let (done, total) = running.progress;
        let ratio = ratio(done, total);
        let mut args = fluent_bundle::FluentArgs::new();
        args.set("done", i64::try_from(done).unwrap_or(i64::MAX));
        args.set("total", i64::try_from(total).unwrap_or(i64::MAX));
        lines = lines.push(
            row![
                progress_bar(0.0..=1.0, ratio).girth(8.0),
                text(i18n.tr_args("advsearch-progress", &args)).size(12),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        );
    }
    if let Some(issue) = &view.issue {
        let line = text(notice_text(i18n, issue)).size(13);
        lines = lines.push(if issue.is_form_error() {
            line.style(text::danger)
        } else {
            line
        });
    }
    if let Some(summary) = &view.summary {
        lines = lines.push(
            text(summary_text(i18n, summary))
                .size(13)
                .style(theme::muted_text),
        );
    }
    lines.into()
}

/// The window.
pub fn view<'a>(
    ui_state: &AdvancedSearchUi,
    view: &AdvancedSearchView,
    i18n: &I18n,
    window_height: f32,
) -> Element<'a, Message> {
    let kind = view.tab;
    let (op, reference) = match kind {
        AdvancedKind::Position => (view.position.op, view.position.reference),
        AdvancedKind::Material => (view.material.op, view.material.reference),
        AdvancedKind::Moves => (view.moves.op, view.moves.reference),
    };
    let tab_body = match kind {
        AdvancedKind::Position => position_tab(view, i18n),
        AdvancedKind::Material => material_tab(ui_state, view, i18n),
        AdvancedKind::Moves => moves_tab(view, i18n),
    };
    let form = column![
        tab_body,
        op_row(kind, op, i18n),
        reference_row(kind, reference, view, i18n),
        saved_block(ui_state, view, i18n),
    ]
    .spacing(14)
    .padding([0, 12]);
    let body = column![
        tabs(view, i18n),
        scrollable(form).height(Length::Fixed(body_height(window_height))),
        status_lines(view, i18n),
    ]
    .spacing(8);
    let running = view.running.is_some();
    modal(Card {
        id: ADVANCED_BOX_ID,
        title: i18n.tr("advsearch-title"),
        body: body.into(),
        buttons: vec![
            with_id(
                ADVANCED_CLOSE_ID,
                command(
                    Icon::Close,
                    i18n.tr("advsearch-close"),
                    false,
                    Some(send(AdvancedSearchMessage::Close)),
                ),
            ),
            with_id(
                ADVANCED_STOP_ID,
                tip(
                    command(
                        Icon::Pause,
                        i18n.tr("advsearch-stop"),
                        false,
                        running.then(|| send(AdvancedSearchMessage::Stop)),
                    )
                    .into(),
                    i18n.tr("advsearch-stop-tip"),
                ),
            ),
            with_id(
                ADVANCED_START_ID,
                command(
                    Icon::Search,
                    i18n.tr("advsearch-start"),
                    true,
                    Some(start()),
                ),
            ),
        ],
        width: CARD_WIDTH,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_form_shrinks_with_a_low_window_and_stays_readable() {
        assert!((body_height(900.0) - BODY_HEIGHT).abs() < f32::EPSILON);
        assert!((body_height(600.0) - 330.0).abs() < f32::EPSILON);
        assert!((body_height(100.0) - BODY_MIN_HEIGHT).abs() < f32::EPSILON);
    }

    #[test]
    fn a_count_field_keeps_the_last_digit_typed() {
        let mut ui = AdvancedSearchUi::default();
        let field = NumField::Min(Color::White, MaterialPiece::Rook);
        assert_eq!(ui.type_into(field, "27"), Some(7));
        assert_eq!(ui.shown(field, 7), "7");
        assert_eq!(ui.type_into(field, ""), None);
        assert_eq!(ui.shown(field, 7), "");
        assert_eq!(ui.type_into(field, "x"), None);
    }

    #[test]
    fn a_signed_field_allows_a_lone_minus_and_negative_numbers() {
        let mut ui = AdvancedSearchUi::default();
        assert_eq!(ui.type_into(NumField::DiffMin, "-"), None);
        assert_eq!(ui.shown(NumField::DiffMin, -40), "-");
        assert_eq!(ui.type_into(NumField::DiffMin, "-5"), Some(-5));
        assert_eq!(ui.shown(NumField::DiffMin, -5), "-5");
        // The value moved under the draft: the value wins.
        assert_eq!(ui.shown(NumField::DiffMin, 3), "3");
    }

    #[test]
    fn an_unsigned_field_refuses_a_minus() {
        let mut ui = AdvancedSearchUi::default();
        assert_eq!(ui.type_into(NumField::MoveMin, "-"), None);
        assert_eq!(ui.type_into(NumField::MoveMin, "-3"), None);
        assert_eq!(ui.type_into(NumField::MoveMin, "12"), Some(12));
    }

    #[test]
    fn clearing_the_drafts_shows_the_values_again() {
        let mut ui = AdvancedSearchUi::default();
        ui.type_into(NumField::MinPlies, "");
        assert_eq!(ui.shown(NumField::MinPlies, 4), "");
        ui.clear_drafts();
        assert_eq!(ui.shown(NumField::MinPlies, 4), "4");
    }
}
