//! The book window (spec M5-6, D1650-D1669; redesign M5-6c, D2040-D2069).
//!
//! By default the panel shows **one** Polyglot book: a card with its
//! information (file, state, the moves of the position with weight and
//! percentage bars) under a row of icon-only file actions. A discreet
//! "Compare" button opens the second book beside the first. Weight tuning,
//! adding and removing lines, importing a branch, making and merging books
//! and the choice of the opening book live in a collapsible "Tools" card
//! that starts closed; while it is open the rows of the table become
//! editable.
//!
//! The application prepares a [`BookView`]; this module lays it out and
//! formats it with Fluent. The file dialogs are the front-end's business
//! (like the engine dialogs): the shell opens them and sends the chosen path
//! to the application. Whether the second book is shown is visual state of
//! the panel ([`BookPanelUi`]), never saved (D2042).

use std::borrow::Cow;
use std::collections::HashMap;
use std::path::PathBuf;

use fluent_bundle::FluentArgs;
use iced::advanced::widget::Id;
use iced::widget::{
    button, checkbox, column, container, progress_bar, row, scrollable, space, text, text_input,
    tooltip,
};
use iced::{Alignment, Element, Length};
use mychess_app::{
    BookCreateOptions, BookExportOptions, BookMergeMode, BookMessage, BookNotice, BookRowView,
    BookSide, BookSideView, BookView, Message as AppMessage, WeightScale,
};
use mychess_core::{Move, NodeId};

use super::panes;
use crate::i18n::I18n;
use crate::shell::{Message, Shell};
use crate::theme::{self, ButtonLook, book_styles};
use crate::widgets::icon::{Icon, IconTone, icon};
use crate::widgets::modal::{Card, modal};

/// Width of the move column.
const MOVE_WIDTH: f32 = 48.0;
/// Width of the weight column and field.
const WEIGHT_WIDTH: f32 = 52.0;
/// Width of the percentage column.
const PERCENT_WIDTH: f32 = 40.0;
/// Text size of the panel.
const TEXT: f32 = 12.0;
/// Text size of the moves (the information stands out, D2043).
const MOVE_TEXT: f32 = 14.0;
/// Text size of the title of a card.
const TITLE: f32 = 15.0;
/// Size of the icons of the buttons.
const ICON: f32 = 14.0;
/// Widest a lone book card grows (logical pixels).
const SINGLE_MAX_WIDTH: f32 = 520.0;
/// Widest a card grows while two books are shown: two of them fit in a
/// panel of about 750 and wrap under each other below that (D2046).
const PAIR_MAX_WIDTH: f32 = 360.0;

/// Id of the "Compare" button (and of "Close the comparison" of book 2).
pub const COMPARE_ID: &str = "book-compare";
/// Id of the button that shows and hides the tools.
pub const TOOLS_ID: &str = "book-tools";
/// Id of the button that dismisses the notice.
pub const DISMISS_ID: &str = "book-dismiss";
/// Id of the merge button.
pub const MERGE_ID: &str = "book-merge";
/// Id of the stop button of "make a book".
pub const STOP_ID: &str = "book-create-stop";
/// Id of the button that stops the export of a branch.
pub const EXPORT_STOP_ID: &str = "book-export-stop";
/// Id of the button that clears the opening book.
pub const CLEAR_OPENING_ID: &str = "book-opening-clear";

/// The id of the button `name` of the book of `side` (`book-1-save`, ...).
pub fn side_id(side: BookSide, name: &str) -> String {
    format!("book-{}-{name}", side.index() + 1)
}

/// An event of the panel that only the front-end can handle: the file
/// dialogs, the text of the weight fields and the visual toggles.
#[derive(Debug, Clone)]
pub enum BookUiMessage {
    /// Ask for a book file to open into a side.
    Open(BookSide),
    /// The file dialog of [`BookUiMessage::Open`] answered.
    Opened(BookSide, Option<PathBuf>),
    /// "Save": the book of the side, or the dialog when it has no file.
    Save(BookSide),
    /// Ask where to save the book of a side.
    SaveAs(BookSide),
    /// The file dialog of [`BookUiMessage::SaveAs`] answered.
    SavedAs(BookSide, Option<PathBuf>),
    /// Ask where to write the branch of a book as PGN (D2160).
    ExportBranch(BookSide),
    /// The file dialog of [`BookUiMessage::ExportBranch`] answered.
    ExportChosen(BookSide, Option<PathBuf>),
    /// The answer to "save the books before quitting?" (D1792).
    QuitAnswer(QuitChoice),
    /// The text of a weight field changed.
    WeightInput {
        /// The side.
        side: BookSide,
        /// The row (the move identifies it in the position).
        mv: Move,
        /// What was typed.
        text: String,
    },
    /// Show the second book, or ask to close it (D2041).
    ToggleCompare,
    /// Open or close the tools (D2044).
    ToggleTools,
}

/// The answers of the question before the program quits with unsaved books.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuitChoice {
    /// Save every book that has changes, then quit.
    Save,
    /// Quit and lose the changes.
    Discard,
    /// Do not quit.
    Cancel,
}

/// Where the quit with unsaved books stands (D1792).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BookQuit {
    /// The question is on screen.
    Asking,
    /// Saving the books one by one; `pending` is the side being saved and
    /// whether its file dialog is still open.
    Saving {
        /// The side whose save was asked for.
        pending: Option<(BookSide, bool)>,
        /// Seconds spent waiting for a job of the save (D1796).
        ticks: u8,
    },
    /// Quitting while an export runs: it was asked to stop and the program
    /// waits for its answer, up to a limit, so no temporary file is left
    /// behind (D2175).
    Draining {
        /// Seconds spent waiting.
        ticks: u8,
    },
}

/// Id of the box that asks about the unsaved books.
pub const BOOK_QUIT_BOX_ID: &str = "book-quit-box";

/// What the panel remembers between frames: the text being typed in the
/// weight fields, valid for one position of one game, and the two visual
/// switches (D2042, D2044).
#[derive(Debug, Default)]
pub struct BookPanelUi {
    /// The program is quitting and the books have unsaved changes.
    pub quit: Option<BookQuit>,
    /// The user asked to compare: the second book is shown even without a
    /// book in it. The second book is also shown whenever it holds a book
    /// (so a book with changes is never hidden).
    pub compare: bool,
    /// The tools are open (and the table is editable).
    pub tools_open: bool,
    at: Option<(NodeId, u64)>,
    drafts: [HashMap<Move, String>; 2],
}

impl BookPanelUi {
    /// The text typed in the field of `mv` in `side`, if it is still for
    /// the position `at` shows (no allocation: called once per row).
    fn draft(&self, at: (NodeId, u64), side: BookSide, mv: Move) -> Option<&str> {
        if self.at != Some(at) {
            return None;
        }
        self.drafts[side.index()].get(&mv).map(String::as_str)
    }

    /// Keeps what was typed.
    pub fn input(&mut self, at: (NodeId, u64), side: BookSide, mv: Move, text: String) {
        if self.at != Some(at) {
            self.clear();
            self.at = Some(at);
        }
        self.drafts[side.index()].insert(mv, text);
    }

    /// Forgets everything that was typed (a message went to the book).
    pub fn clear(&mut self) {
        for drafts in &mut self.drafts {
            drafts.clear();
        }
        self.at = None;
    }

    /// Whether the second book is shown for `view`.
    pub fn comparing(&self, view: &BookView) -> bool {
        self.compare || view.sides[BookSide::Second.index()].is_some()
    }

    /// Whether the tools are shown for `view`: open on request, and always
    /// while a book is being made (its stop button must be in reach).
    pub fn tools_shown(&self, view: &BookView) -> bool {
        self.tools_open || view.creating || view.export_progress.is_some()
    }
}

fn to_app(message: BookMessage) -> Message {
    Message::App(AppMessage::Book(message))
}

fn to_ui(message: BookUiMessage) -> Message {
    Message::Book(message)
}

/// The key of the title of a side.
fn title_key(side: BookSide) -> &'static str {
    match side {
        BookSide::First => "book-title-first",
        BookSide::Second => "book-title-second",
    }
}

// ---- building blocks ----

fn tip_box<'a>(content: String) -> Element<'a, Message> {
    container(text(content).size(TEXT))
        .padding([4, 8])
        .style(theme::tooltip_box)
        .into()
}

/// `body` with a tooltip below it.
fn with_tip<'a>(body: impl Into<Element<'a, Message>>, content: String) -> Element<'a, Message> {
    tooltip(body, tip_box(content), tooltip::Position::Bottom).into()
}

/// `body` under the id `id` (the way the tests reach a button).
fn identified<'a>(body: impl Into<Element<'a, Message>>, id: String) -> Element<'a, Message> {
    container(body).id(Id::from(id)).into()
}

/// A button of icon only. The tooltip carries the text (D2045).
fn icon_button<'a>(
    i18n: &I18n,
    id: String,
    glyph: Icon,
    tip_key: &str,
    on: Option<Message>,
) -> Element<'a, Message> {
    icon_button_on(false, i18n, id, glyph, tip_key, on)
}

/// [`icon_button`] on a surface of its own color (`inset`: inside the
/// information card, D2050).
fn icon_button_on<'a>(
    inset: bool,
    i18n: &I18n,
    id: String,
    glyph: Icon,
    tip_key: &str,
    on: Option<Message>,
) -> Element<'a, Message> {
    let tone = if on.is_some() {
        IconTone::Text
    } else {
        IconTone::Disabled
    };
    let body = button(icon(glyph, ICON, tone))
        .padding([5, 8])
        .style(book_styles::button_style(inset))
        .on_press_maybe(on);
    with_tip(identified(body, id), i18n.tr(tip_key))
}

/// A button with an icon and a short label; the tooltip (when there is one)
/// explains it.
fn label_button<'a>(
    i18n: &I18n,
    id: String,
    glyph: Icon,
    label_key: &str,
    tip_key: Option<&str>,
    on: Option<Message>,
) -> Element<'a, Message> {
    label_button_on(false, i18n, id, glyph, label_key, tip_key, on)
}

/// [`label_button`] with the surface of [`icon_button_on`].
fn label_button_on<'a>(
    inset: bool,
    i18n: &I18n,
    id: String,
    glyph: Icon,
    label_key: &str,
    tip_key: Option<&str>,
    on: Option<Message>,
) -> Element<'a, Message> {
    let enabled = on.is_some();
    let tone = if enabled {
        IconTone::Text
    } else {
        IconTone::Disabled
    };
    let mut label = text(i18n.tr(label_key)).size(TEXT);
    if !enabled {
        label = label.style(theme::muted_text);
    }
    let body = button(
        row![icon(glyph, ICON, tone), label]
            .spacing(6)
            .align_y(Alignment::Center),
    )
    .padding([4, 8])
    .style(book_styles::button_style(inset))
    .on_press_maybe(on);
    let body = identified(body, id);
    match tip_key {
        Some(key) => with_tip(body, i18n.tr(key)),
        None => body,
    }
}

fn muted<'a>(content: String) -> Element<'a, Message> {
    text(content).size(TEXT).style(theme::muted_text).into()
}

/// A small pill: an optional icon and a text.
fn chip<'a>(glyph: Option<Icon>, label: String, danger: bool) -> Element<'a, Message> {
    let tone = if danger {
        IconTone::Danger
    } else {
        IconTone::Muted
    };
    let mut body = row![].spacing(4).align_y(Alignment::Center);
    if let Some(glyph) = glyph {
        body = body.push(icon(glyph, 11.0, tone));
    }
    body = body.push(text(label).size(11));
    container(body)
        .padding([1, 6])
        .style(book_styles::chip(danger))
        .into()
}

/// The text of a notice (`None` for the failures: those are danger text).
fn notice_text(i18n: &I18n, notice: BookNotice) -> String {
    let mut args = FluentArgs::new();
    match notice {
        BookNotice::Created {
            used,
            skipped,
            entries,
        } => {
            args.set("used", i64::try_from(used).unwrap_or(i64::MAX));
            args.set("skipped", i64::try_from(skipped).unwrap_or(i64::MAX));
            args.set("entries", i64::try_from(entries).unwrap_or(i64::MAX));
        }
        BookNotice::Merged { written, skipped } => {
            args.set("written", i64::try_from(written).unwrap_or(i64::MAX));
            args.set("skipped", i64::try_from(skipped).unwrap_or(i64::MAX));
        }
        BookNotice::Exported { lines, .. } | BookNotice::ExportFinishedAnyway { lines } => {
            args.set("count", i64::try_from(lines).unwrap_or(i64::MAX));
        }
        BookNotice::Added(count) | BookNotice::Removed(count) => {
            args.set("count", i64::try_from(count).unwrap_or(i64::MAX));
        }
        _ => {}
    }
    i18n.tr_args(notice.key(), &args)
}

// ---- one book: actions, information ----

/// The row of icon-only actions of a book (D2045).
fn action_row<'a>(
    i18n: &I18n,
    book: Option<&BookSideView>,
    side: BookSide,
    busy: bool,
) -> Element<'a, Message> {
    let present = book.is_some();
    let idle = |message: Message| (!busy).then_some(message);
    let saveable = book.is_some_and(|b| b.dirty || b.path.is_none());
    let items: Vec<Element<'a, Message>> = vec![
        icon_button(
            i18n,
            side_id(side, "open"),
            Icon::FolderOpen,
            "book-open",
            idle(to_ui(BookUiMessage::Open(side))),
        ),
        icon_button(
            i18n,
            side_id(side, "new"),
            Icon::FileNew,
            "book-new",
            idle(to_app(BookMessage::New { side })),
        ),
        icon_button(
            i18n,
            side_id(side, "save"),
            Icon::Save,
            "book-save",
            (present && saveable)
                .then(|| to_ui(BookUiMessage::Save(side)))
                .and_then(idle),
        ),
        icon_button(
            i18n,
            side_id(side, "save-as"),
            Icon::AppendFile,
            "book-save-as",
            present
                .then(|| to_ui(BookUiMessage::SaveAs(side)))
                .and_then(idle),
        ),
        icon_button(
            i18n,
            side_id(side, "close"),
            Icon::Close,
            "book-close-book",
            present
                .then(|| to_app(BookMessage::Close { side }))
                .and_then(idle),
        ),
    ];
    row::Row::with_children(items)
        .spacing(4)
        .align_y(Alignment::Center)
        .wrap()
        .into()
}

/// The button that closes the comparison: the "Compare" icon, lit, as the
/// toggle it undoes (distinct from "Close book").
fn compare_close_button<'a>(i18n: &I18n) -> Element<'a, Message> {
    let body = button(icon(Icon::Columns, ICON, IconTone::OnAccent))
        .padding([5, 8])
        .style(theme::command_button(ButtonLook::Primary, true))
        .on_press(to_ui(BookUiMessage::ToggleCompare));
    with_tip(
        identified(body, COMPARE_ID.to_owned()),
        i18n.tr("book-compare-close-tip"),
    )
}

fn confirm_close<'a>(i18n: &I18n) -> Element<'a, Message> {
    let answer = |key: &str, id: &str, confirmed: bool| {
        label_button_on(
            true,
            i18n,
            id.to_owned(),
            if confirmed {
                Icon::Eraser
            } else {
                Icon::ArrowLeft
            },
            key,
            None,
            Some(to_app(BookMessage::ConfirmClose(confirmed))),
        )
    };
    container(
        column![
            text(i18n.tr("book-confirm-close"))
                .size(TEXT)
                .style(book_styles::danger_text),
            row![
                answer("book-confirm-yes", "book-confirm-yes", true),
                answer("book-confirm-no", "book-confirm-no", false),
            ]
            .spacing(4)
            .wrap(),
        ]
        .spacing(6),
    )
    .padding(8)
    .width(Length::Fill)
    .style(book_styles::notice(true))
    .into()
}

/// The header of the card: the name, the state and the size of the book.
fn card_header<'a>(
    i18n: &I18n,
    book: &BookSideView,
    side: BookSide,
    comparing: bool,
) -> Element<'a, Message> {
    let name = book
        .name
        .clone()
        .unwrap_or_else(|| i18n.tr("book-untitled"));
    let mut title = row![].spacing(8).align_y(Alignment::Center);
    if comparing {
        title = title.push(chip(None, i18n.tr(title_key(side)), false));
    }
    title = title.push(text(name).size(TITLE).style(theme::accent_text));
    let mut entries_args = FluentArgs::new();
    entries_args.set("count", i64::try_from(book.entries).unwrap_or(i64::MAX));
    let mut facts = row![muted(i18n.tr_args("book-entries", &entries_args))]
        .spacing(6)
        .align_y(Alignment::Center);
    if book.dirty {
        facts = facts.push(chip(None, i18n.tr("book-changed"), true));
    }
    if book.read_only {
        facts = facts.push(chip(Some(Icon::Lock), i18n.tr("book-read-only"), false));
    }
    if book.is_opening {
        facts = facts.push(chip(
            Some(Icon::Bookmark),
            i18n.tr("book-is-opening"),
            false,
        ));
    }
    column![title.wrap(), facts.wrap()].spacing(4).into()
}

/// One row of the table. With `editing` the weight is a field and a second
/// line holds the double, halve and remove buttons: the row never needs more
/// than the width of its first line, so no button is cut in a narrow panel
/// (D2049).
fn move_row<'a>(
    i18n: &I18n,
    ui: &BookPanelUi,
    at: (NodeId, u64),
    side: BookSide,
    book_row: &BookRowView,
    editable: bool,
    editing: bool,
) -> Element<'a, Message> {
    let mv = book_row.mv;
    let mut name = text(book_row.san.clone()).size(MOVE_TEXT);
    if book_row.in_game {
        name = name.style(theme::accent_text);
    }
    let weight: Element<'a, Message> = if editing {
        let typed = ui.draft(at, side, mv);
        let shown = typed.map_or_else(|| Cow::Owned(book_row.weight.to_string()), Cow::Borrowed);
        // Enter sends what was typed; with nothing typed there is nothing
        // to send.
        let submit = typed.map(|typed| {
            to_app(BookMessage::SetWeight {
                side,
                mv,
                text: typed.to_owned(),
            })
        });
        let input = text_input("", &shown)
            .size(TEXT)
            .padding(3)
            .width(Length::Fixed(WEIGHT_WIDTH))
            .on_input_maybe(
                editable
                    .then_some(move |text| to_ui(BookUiMessage::WeightInput { side, mv, text })),
            )
            .on_submit_maybe(submit.filter(|_| editable));
        with_tip(input, i18n.tr("book-weight-tip"))
    } else {
        container(
            text(book_row.weight.to_string())
                .size(TEXT)
                .style(theme::muted_text),
        )
        .width(Length::Fixed(WEIGHT_WIDTH))
        .into()
    };
    let line = row![
        container(name).width(Length::Fixed(MOVE_WIDTH)),
        weight,
        progress_bar(0.0..=100.0, f32::from(book_row.percent))
            .length(Length::Fill)
            .girth(8)
            .style(book_styles::weight_bar),
        container(text(format!("{}%", book_row.percent)).size(MOVE_TEXT))
            .width(Length::Fixed(PERCENT_WIDTH))
            .align_x(Alignment::End),
    ]
    .spacing(6)
    .align_y(Alignment::Center);
    if !editing {
        return line.into();
    }
    let scale = |name: &str, label_key: &str, tip: &str, scale| {
        let on = editable.then(|| to_app(BookMessage::ScaleWeight { side, mv, scale }));
        let body = button(text(i18n.tr(label_key)).size(TEXT))
            .padding([2, 8])
            .style(book_styles::button_style(true))
            .on_press_maybe(on);
        with_tip(
            identified(body, side_id(side, &format!("{name}-{}", book_row.san))),
            i18n.tr(tip),
        )
    };
    let tools = row![
        scale(
            "double",
            "book-double",
            "book-double-tip",
            WeightScale::Double
        ),
        scale("half", "book-half", "book-half-tip", WeightScale::Half),
        icon_button_on(
            true,
            i18n,
            side_id(side, &format!("remove-{}", book_row.san)),
            Icon::Trash,
            "book-remove-move-tip",
            editable.then(|| to_app(BookMessage::RemoveMove { side, mv })),
        ),
    ]
    .spacing(4)
    .align_y(Alignment::Center);
    column![
        line,
        row![space::horizontal().width(Length::Fixed(MOVE_WIDTH)), tools].spacing(6)
    ]
    .spacing(4)
    .into()
}

fn moves_table<'a>(
    i18n: &I18n,
    ui: &BookPanelUi,
    at: (NodeId, u64),
    view: &BookView,
    book: &BookSideView,
    editing: bool,
) -> Element<'a, Message> {
    let side = book.side;
    if !view.standard {
        return muted(i18n.tr("book-not-standard"));
    }
    let editable = !book.read_only && !view.busy;
    let head = |key: &str, width: f32| {
        container(text(i18n.tr(key)).size(TEXT).style(theme::muted_text))
            .width(Length::Fixed(width))
    };
    let mut table = column![
        row![
            head("book-col-move", MOVE_WIDTH),
            head("book-col-weight", WEIGHT_WIDTH),
            space::horizontal(),
            head("book-col-percent", PERCENT_WIDTH).align_x(Alignment::End),
        ]
        .spacing(6)
    ]
    .spacing(4);
    if book.rows.is_empty() {
        table = table.push(muted(i18n.tr("book-no-moves")));
    }
    for book_row in &book.rows {
        table = table.push(move_row(i18n, ui, at, side, book_row, editable, editing));
    }
    if editing && !book.leading.is_empty() {
        table = table.push(with_tip(
            text(i18n.tr("book-others"))
                .size(TEXT)
                .style(theme::muted_text),
            i18n.tr("book-others-tip"),
        ));
        let leads: Vec<Element<'a, Message>> = book
            .leading
            .iter()
            .map(|lead| {
                let on = editable.then(|| to_app(BookMessage::AddMove { side, mv: lead.mv }));
                button(text(lead.san.clone()).size(TEXT))
                    .padding([2, 6])
                    .style(book_styles::button_style(true))
                    .on_press_maybe(on)
                    .into()
            })
            .collect();
        table = table.push(row::Row::with_children(leads).spacing(4).wrap());
    }
    table.into()
}

/// One book: the row of actions over the card with its information.
fn side_column<'a>(
    i18n: &I18n,
    ui: &BookPanelUi,
    at: (NodeId, u64),
    view: &BookView,
    side: BookSide,
    comparing: bool,
) -> Element<'a, Message> {
    let book = view.sides[side.index()].as_ref();
    let editing = ui.tools_shown(view);
    let actions = action_row(i18n, book, side, view.busy);
    // The book that is not the first has the button that closes the
    // comparison at the top right of its card.
    let closer = |content: Element<'a, Message>| -> Element<'a, Message> {
        if side == BookSide::Second && comparing {
            row![
                container(content).width(Length::Fill),
                compare_close_button(i18n)
            ]
            .spacing(8)
            .into()
        } else {
            content
        }
    };
    let mut card = column![].spacing(10);
    match book {
        None => {
            let mut head = row![].spacing(8).align_y(Alignment::Center);
            if comparing {
                head = head.push(chip(None, i18n.tr(title_key(side)), false));
            }
            card = card.push(closer(head.into()));
            card = card.push(muted(i18n.tr("book-empty-slot")));
        }
        Some(book) => {
            card = card.push(closer(card_header(i18n, book, side, comparing)));
            if view.confirm_close == Some(side) {
                card = card.push(confirm_close(i18n));
            }
            card = card.push(moves_table(i18n, ui, at, view, book, editing));
        }
    }
    let max = if comparing {
        PAIR_MAX_WIDTH
    } else {
        SINGLE_MAX_WIDTH
    };
    column![
        actions,
        container(card)
            .padding(10)
            .width(Length::Fill)
            .style(book_styles::info_card)
    ]
    .spacing(6)
    .width(Length::Fill)
    .max_width(max)
    .into()
}

// ---- the tools ----

fn stepper<'a>(
    i18n: &I18n,
    name: &str,
    label_key: &str,
    value: u32,
    down: BookCreateOptions,
    up: BookCreateOptions,
) -> Element<'a, Message> {
    let step = |label: &'static str, which: &str, options: BookCreateOptions| {
        identified(
            button(text(label).size(TEXT))
                .padding([1, 8])
                .style(theme::command_button(ButtonLook::Primary, false))
                .on_press(to_app(BookMessage::SetCreateOptions(options))),
            format!("book-step-{name}-{which}"),
        )
    };
    row![
        container(text(i18n.tr(label_key)).size(TEXT)).width(Length::Fill),
        step("-", "down", down),
        text(value.to_string()).size(TEXT),
        step("+", "up", up),
    ]
    .spacing(6)
    .align_y(Alignment::Center)
    .into()
}

/// A group of the tools: a small icon and title over its content.
fn group<'a>(
    i18n: &I18n,
    glyph: Icon,
    title_key: &str,
    content: Element<'a, Message>,
) -> Element<'a, Message> {
    container(
        column![
            row![
                icon(glyph, 13.0, IconTone::Muted),
                text(i18n.tr(title_key)).size(TEXT).style(theme::muted_text)
            ]
            .spacing(6)
            .align_y(Alignment::Center),
            content
        ]
        .spacing(8),
    )
    .padding(8)
    .width(Length::Fill)
    .style(book_styles::tools_group)
    .into()
}

/// Edit the position: one set of buttons per shown book.
fn edit_group<'a>(i18n: &I18n, view: &BookView, comparing: bool) -> Option<Element<'a, Message>> {
    let mut sets = column![].spacing(8);
    let mut any = false;
    for side in BookSide::ALL {
        let Some(book) = view.sides[side.index()].as_ref() else {
            continue;
        };
        if side == BookSide::Second && !comparing {
            continue;
        }
        any = true;
        let ok = view.standard && !book.read_only && !view.busy;
        let go = |on: bool, message: BookMessage| (ok && on).then(|| to_app(message));
        let other_present = view.sides[side.other().index()].is_some();
        let items: Vec<Element<'a, Message>> = vec![
            label_button(
                i18n,
                side_id(side, "add-move"),
                Icon::AddVariation,
                "book-add-game-move",
                Some("book-add-game-move-tip"),
                go(view.game_move.is_some(), BookMessage::AddGameMove { side }),
            ),
            label_button(
                i18n,
                side_id(side, "add-line"),
                Icon::ListOrdered,
                "book-add-line",
                Some("book-add-line-tip"),
                go(view.has_continuation, BookMessage::AddLine { side }),
            ),
            label_button(
                i18n,
                side_id(side, "remove-line"),
                Icon::RemoveFromFilter,
                "book-remove-line",
                Some("book-remove-line-tip"),
                go(view.has_continuation, BookMessage::RemoveLine { side }),
            ),
            label_button(
                i18n,
                side_id(side, "import-branch"),
                Icon::Import,
                "book-import-branch",
                Some("book-import-branch-tip"),
                go(view.has_continuation, BookMessage::ImportBranch { side }),
            ),
            label_button(
                i18n,
                side_id(side, "copy"),
                Icon::Copy,
                "book-copy-from-other",
                Some("book-copy-tip"),
                go(other_present, BookMessage::CopyPosition { to: side }),
            ),
        ];
        let mut set = column![].spacing(4);
        if comparing {
            set = set.push(muted(i18n.tr(title_key(side))));
        }
        sets = sets.push(set.push(row::Row::with_children(items).spacing(4).wrap()));
    }
    any.then(|| {
        group(
            i18n,
            Icon::Branch,
            "book-group-edit",
            column![muted(i18n.tr("book-tools-hint")), sets]
                .spacing(8)
                .into(),
        )
    })
}

fn create_group<'a>(i18n: &I18n, view: &BookView, comparing: bool) -> Element<'a, Message> {
    let o = view.create;
    let with = |f: &dyn Fn(&mut BookCreateOptions)| {
        let mut next = o;
        f(&mut next);
        next
    };
    let flag = |label_key: &str, on: bool, set: fn(&mut BookCreateOptions, bool)| {
        checkbox(on)
            .label(i18n.tr(label_key))
            .size(14)
            .text_size(TEXT)
            .on_toggle(move |value| {
                let mut next = o;
                set(&mut next, value);
                to_app(BookMessage::SetCreateOptions(next))
            })
    };
    let go = |side: BookSide, key: &str| {
        label_button(
            i18n,
            format!("book-create-{}", side.index() + 1),
            Icon::Database,
            key,
            None,
            (!view.busy && view.filter_games > 0)
                .then(|| to_app(BookMessage::CreateFromFilter { side })),
        )
    };
    let mut games_args = FluentArgs::new();
    games_args.set("count", i64::from(view.filter_games));
    let mut targets = row![].spacing(4);
    if comparing {
        targets = targets
            .push(go(BookSide::First, "book-create-into-first"))
            .push(go(BookSide::Second, "book-create-into-second"));
    } else {
        targets = targets.push(go(BookSide::First, "book-create-make"));
    }
    targets = targets.push(label_button(
        i18n,
        STOP_ID.to_owned(),
        Icon::Close,
        "book-create-stop",
        Some("book-create-stop-tip"),
        view.creating.then(|| to_app(BookMessage::CancelCreate)),
    ));
    let content = column![
        muted(i18n.tr_args("book-create-games", &games_args)),
        stepper(
            i18n,
            "plies",
            "book-create-max-ply",
            o.max_ply,
            with(&|n| n.max_ply = n.max_ply.saturating_sub(50)),
            with(&|n| n.max_ply = n.max_ply.saturating_add(50)),
        ),
        stepper(
            i18n,
            "games",
            "book-create-min-games",
            o.min_games,
            with(&|n| n.min_games = n.min_games.saturating_sub(1)),
            with(&|n| n.min_games = n.min_games.saturating_add(1)),
        ),
        stepper(
            i18n,
            "score",
            "book-create-min-score",
            u32::from(o.min_score),
            with(&|n| n.min_score = n.min_score.saturating_sub(5)),
            with(&|n| n.min_score = n.min_score.saturating_add(5)),
        ),
        flag("book-create-only-white", o.only_white, |n, v| {
            n.only_white = v;
            if v {
                n.only_black = false;
            }
        }),
        flag("book-create-only-black", o.only_black, |n, v| {
            n.only_black = v;
            if v {
                n.only_white = false;
            }
        }),
        flag("book-create-uniform", o.uniform, |n, v| n.uniform = v),
        targets.wrap(),
    ]
    .spacing(6);
    group(i18n, Icon::Database, "book-create-title", content.into())
}

/// A stepper of the export options, like [`stepper`].
fn export_stepper<'a>(
    i18n: &I18n,
    name: &str,
    label_key: &str,
    value: u32,
    down: BookExportOptions,
    up: BookExportOptions,
) -> Element<'a, Message> {
    let step = |label: &'static str, which: &str, options: BookExportOptions| {
        identified(
            button(text(label).size(TEXT))
                .padding([1, 8])
                .style(theme::command_button(ButtonLook::Primary, false))
                .on_press(to_app(BookMessage::SetExportOptions(options))),
            format!("book-export-{name}-{which}"),
        )
    };
    row![
        container(text(i18n.tr(label_key)).size(TEXT)).width(Length::Fill),
        step("-", "down", down),
        text(value.to_string()).size(TEXT),
        step("+", "up", up),
    ]
    .spacing(6)
    .align_y(Alignment::Center)
    .into()
}

/// Export the branch of a book as PGN (D2160-D2166).
fn export_group<'a>(i18n: &I18n, view: &BookView, comparing: bool) -> Element<'a, Message> {
    let o = view.export;
    let with = |f: &dyn Fn(&mut BookExportOptions)| {
        let mut next = o;
        f(&mut next);
        next.normalized()
    };
    let exporting = view.export_progress.is_some();
    let mut buttons = row![].spacing(4);
    for side in BookSide::ALL {
        let Some(book) = view.sides[side.index()].as_ref() else {
            continue;
        };
        if side == BookSide::Second && !comparing {
            continue;
        }
        let key = match (comparing, side) {
            (false, _) => "book-export-make",
            (true, BookSide::First) => "book-export-first",
            (true, BookSide::Second) => "book-export-second",
        };
        let ok = view.standard && !exporting && !book.rows.is_empty();
        buttons = buttons.push(label_button(
            i18n,
            side_id(side, "export"),
            Icon::Upload,
            key,
            Some("book-export-tip"),
            ok.then(|| to_ui(BookUiMessage::ExportBranch(side))),
        ));
    }
    buttons = buttons.push(label_button(
        i18n,
        EXPORT_STOP_ID.to_owned(),
        Icon::Close,
        "book-export-stop",
        Some("book-export-stop-tip"),
        (exporting && !view.export_stopping).then(|| to_app(BookMessage::CancelExport)),
    ));
    let mut content = column![
        muted(i18n.tr("book-export-hint")),
        export_stepper(
            i18n,
            "plies",
            "book-export-max-plies",
            o.max_plies,
            with(&|n| n.max_plies =
                n.max_plies
                    .saturating_sub(if n.max_plies > 4 { 2 } else { 1 })),
            with(&|n| n.max_plies =
                n.max_plies
                    .saturating_add(if n.max_plies < 4 { 1 } else { 2 })),
        ),
        export_stepper(
            i18n,
            "lines",
            "book-export-max-lines",
            o.max_lines,
            with(&|n| n.max_lines = n.max_lines.saturating_sub(50)),
            with(&|n| n.max_lines = n.max_lines.saturating_add(50)),
        ),
        export_stepper(
            i18n,
            "weight",
            "book-export-min-weight",
            u32::from(o.min_weight),
            with(&|n| n.min_weight = n.min_weight.saturating_sub(10)),
            with(&|n| n.min_weight = n.min_weight.saturating_add(10)),
        ),
        buttons.wrap(),
    ]
    .spacing(6);
    if let Some((done, total)) = view.export_progress {
        // The progress is only meaningful once the lines are counted.
        let percent = done
            .saturating_mul(100)
            .checked_div(total)
            .map_or(0, |p| u16::try_from(p).unwrap_or(100));
        content = content.push(
            progress_bar(0.0..=100.0, f32::from(percent.min(100)))
                .girth(8)
                .style(book_styles::weight_bar),
        );
    }
    group(i18n, Icon::Upload, "book-export-title", content.into())
}

/// The question before the program quits with unsaved books: Save,
/// Discard or Cancel (D1792).
pub fn quit_box<'a>(i18n: &I18n) -> Element<'a, Message> {
    let answer = |key: &str,
                  choice: QuitChoice,
                  style: fn(&iced::Theme, button::Status) -> button::Style| {
        button(text(i18n.tr(key)))
            .style(style)
            .on_press(to_ui(BookUiMessage::QuitAnswer(choice)))
            .into()
    };
    modal(Card {
        id: BOOK_QUIT_BOX_ID,
        title: i18n.tr("book-quit-title"),
        body: text(i18n.tr("book-quit-message")).size(13).into(),
        buttons: vec![
            answer("book-quit-save", QuitChoice::Save, button::primary),
            answer("book-quit-discard", QuitChoice::Discard, button::danger),
            answer("book-quit-cancel", QuitChoice::Cancel, button::secondary),
        ],
        width: 480.0,
    })
}

/// Merging needs both books, so it only shows while they are both there.
fn merge_group<'a>(i18n: &I18n, view: &BookView) -> Option<Element<'a, Message>> {
    if !view.sides.iter().all(Option::is_some) {
        return None;
    }
    let mode = |label_key: &str, value: BookMergeMode| {
        checkbox(view.merge_mode == value)
            .label(i18n.tr(label_key))
            .size(14)
            .text_size(TEXT)
            .on_toggle(move |_| to_app(BookMessage::SetMergeMode(value)))
    };
    let content = column![
        mode("book-merge-keep-first", BookMergeMode::KeepFirst),
        mode("book-merge-sum", BookMergeMode::SumWeights),
        label_button(
            i18n,
            MERGE_ID.to_owned(),
            Icon::Negate,
            "book-merge-button",
            None,
            (!view.busy).then(|| to_app(BookMessage::Merge)),
        ),
    ]
    .spacing(6);
    Some(group(
        i18n,
        Icon::Negate,
        "book-merge-title",
        content.into(),
    ))
}

fn opening_group<'a>(i18n: &I18n, view: &BookView, comparing: bool) -> Element<'a, Message> {
    let mut col = column![].spacing(6);
    match &view.opening {
        None => col = col.push(muted(i18n.tr("book-opening-none"))),
        Some(opening) => {
            let file = opening
                .path
                .file_name()
                .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
            col = col.push(text(file).size(TEXT));
            if let Some(name) = &opening.loaded_name {
                let mut args = FluentArgs::new();
                args.set("name", name.clone());
                col = col.push(muted(i18n.tr_args("book-opening-loaded", &args)));
            }
            if let Some(issue) = opening.issue {
                col = col.push(
                    text(i18n.tr(issue.key()))
                        .size(TEXT)
                        .style(book_styles::danger_text),
                );
            }
        }
    }
    let mut buttons = row![].spacing(4);
    for side in BookSide::ALL {
        let has_file = view.sides[side.index()]
            .as_ref()
            .is_some_and(|book| book.path.is_some());
        if !has_file || (side == BookSide::Second && !comparing) {
            continue;
        }
        let key = match (comparing, side) {
            (false, _) => "book-use-this",
            (true, BookSide::First) => "book-use-first",
            (true, BookSide::Second) => "book-use-second",
        };
        buttons = buttons.push(label_button(
            i18n,
            side_id(side, "use-opening"),
            Icon::Bookmark,
            key,
            Some("book-use-as-opening"),
            (!view.busy).then(|| to_app(BookMessage::UseAsOpening(side))),
        ));
    }
    if view.opening.is_some() {
        buttons = buttons.push(label_button(
            i18n,
            CLEAR_OPENING_ID.to_owned(),
            Icon::Eraser,
            "book-opening-clear",
            None,
            Some(to_app(BookMessage::ClearOpening)),
        ));
    }
    col = col.push(buttons.wrap());
    group(i18n, Icon::Bookmark, "book-opening-title", col.into())
}

/// The collapsible card of the tools and settings (D2044): dimmer than the
/// information, with small muted labels.
fn tools_card<'a>(
    i18n: &I18n,
    view: &BookView,
    open: bool,
    comparing: bool,
) -> Element<'a, Message> {
    let tip = if open {
        "book-tools-hide-tip"
    } else {
        "book-tools-show-tip"
    };
    let mut title = row![
        icon(
            if open { Icon::SortDown } else { Icon::Forward },
            ICON,
            IconTone::Muted
        ),
        icon(Icon::Options, ICON, IconTone::Muted),
        text(i18n.tr("book-tools-title"))
            .size(13)
            .style(theme::muted_text),
    ]
    .spacing(8)
    .align_y(Alignment::Center);
    // A problem with the opening book must not hide with the tools closed.
    if !open && view.opening.as_ref().is_some_and(|o| o.issue.is_some()) {
        title = title.push(chip(
            Some(Icon::Info),
            i18n.tr("book-opening-issue-badge"),
            true,
        ));
    }
    let header = with_tip(
        identified(
            button(title)
            .width(Length::Fill)
            .padding([5, 8])
            .style(theme::command_button(ButtonLook::Secondary, false))
            // The tools stay open while a book is being made: the header
            // does nothing then (D2053).
            .on_press_maybe(
                (!view.creating && view.export_progress.is_none())
                    .then(|| to_ui(BookUiMessage::ToggleTools)),
            ),
            TOOLS_ID.to_owned(),
        ),
        i18n.tr(tip),
    );
    let mut col = column![header].spacing(8);
    if open {
        if let Some(edit) = edit_group(i18n, view, comparing) {
            col = col.push(edit);
        }
        col = col.push(create_group(i18n, view, comparing));
        col = col.push(export_group(i18n, view, comparing));
        if let Some(merge) = merge_group(i18n, view) {
            col = col.push(merge);
        }
        col = col.push(opening_group(i18n, view, comparing));
    }
    container(col)
        .padding(6)
        .width(Length::Fill)
        .style(book_styles::tools_card)
        .into()
}

fn notice_row<'a>(i18n: &I18n, notice: BookNotice) -> Element<'a, Message> {
    let failure = matches!(notice, BookNotice::Issue(_));
    let mut body = text(notice_text(i18n, notice)).size(TEXT);
    if failure {
        body = body.style(book_styles::danger_text);
    }
    container(
        row![
            container(body).width(Length::Fill),
            icon_button(
                i18n,
                DISMISS_ID.to_owned(),
                Icon::Close,
                "book-dismiss",
                Some(to_app(BookMessage::DismissNotice)),
            ),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    )
    .padding([4, 8])
    .width(Length::Fill)
    .style(book_styles::notice(failure))
    .into()
}

/// The content of the panel for `view` (a pure function of its inputs).
pub(crate) fn content<'a>(
    view: &BookView,
    ui: &BookPanelUi,
    at: (NodeId, u64),
    i18n: &I18n,
) -> Element<'a, Message> {
    let comparing = ui.comparing(view);
    // The title gives way (wraps) before the Compare button is cut.
    let mut top = row![container(text(i18n.tr("pane-book")).size(TITLE)).width(Length::Fill)]
        .spacing(12)
        .align_y(Alignment::Center);
    if !comparing {
        top = top.push(label_button(
            i18n,
            COMPARE_ID.to_owned(),
            Icon::Columns,
            "book-compare",
            Some("book-compare-tip"),
            Some(to_ui(BookUiMessage::ToggleCompare)),
        ));
    }
    let mut sides = row![side_column(i18n, ui, at, view, BookSide::First, comparing)];
    if comparing {
        sides = sides.push(side_column(i18n, ui, at, view, BookSide::Second, comparing));
    }
    let mut page = column![top].spacing(12).padding(8);
    if let Some(notice) = view.notice {
        page = page.push(notice_row(i18n, notice));
    }
    if let Some(san) = &view.game_move {
        let mut args = FluentArgs::new();
        args.set("san", san.clone());
        page = page.push(row![chip(None, i18n.tr_args("book-game-move", &args), false)].wrap());
    }
    page = page.push(sides.spacing(12).wrap());
    page = page.push(tools_card(i18n, view, ui.tools_shown(view), comparing));
    scrollable(page)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

/// The book panel.
pub fn view(shell: &Shell) -> Element<'_, Message> {
    let book = shell.app.book_view();
    let at = (shell.app.session.cursor(), shell.app.session.generation());
    panes::body(
        content(&book, &shell.ui.dialogs.book, at, &shell.i18n),
        "book-pane",
    )
}

#[cfg(test)]
mod tests {
    use iced::{Settings, Size};
    use iced_test::Simulator;
    use mychess_app::{AppState, BookIssue, GameMessage, Nav, update};

    use super::*;
    use crate::i18n::Language;
    use crate::shell::UiMessage;
    use crate::windows::layout::PaneKind;

    fn en() -> I18n {
        I18n::new(Language::EnUs)
    }

    fn pt() -> I18n {
        I18n::new(Language::PtBr)
    }

    /// A state with the game 1.e4 e5 and a book on the left that knows e4.
    fn state_with_book() -> AppState {
        let mut app = AppState::default();
        let mut game = mychess_core::Game::new();
        let mut node = game.root();
        for uci in ["e2e4", "e7e5"] {
            let mv = game
                .position_at(node)
                .expect("position")
                .parse_uci(uci)
                .expect("uci");
            node = game.add_move(node, mv).expect("move");
        }
        app.session.replace_game(game);
        drop(app.session.update(GameMessage::Navigate(Nav::Start)));
        for message in [
            BookMessage::New {
                side: BookSide::First,
            },
            BookMessage::AddGameMove {
                side: BookSide::First,
            },
        ] {
            drop(update(&mut app, AppMessage::Book(message)));
        }
        app
    }

    fn at(app: &AppState) -> (NodeId, u64) {
        (app.session.cursor(), app.session.generation())
    }

    fn sim<'a>(app: &AppState, ui: &'a BookPanelUi, i18n: &I18n) -> Simulator<'a, Message> {
        Simulator::with_size(
            Settings::default(),
            Size::new(1200.0, 900.0),
            content(&app.book_view(), ui, at(app), i18n),
        )
    }

    fn tools_open() -> BookPanelUi {
        BookPanelUi {
            tools_open: true,
            ..BookPanelUi::default()
        }
    }

    /// What clicking the widget `id` sends, with the tools open.
    fn sent(app: &AppState, id: &str) -> Vec<Message> {
        let ui = tools_open();
        let mut s = sim(app, &ui, &en());
        s.click(Id::from(id.to_owned()))
            .unwrap_or_else(|e| panic!("{id}: {e:?}"));
        s.into_messages().collect()
    }

    fn is_book(message: &Message, wanted: &BookMessage) -> bool {
        matches!(message, Message::App(AppMessage::Book(got)) if got == wanted)
    }

    #[test]
    fn one_book_is_shown_by_default_with_its_information() {
        let app = state_with_book();
        let ui = BookPanelUi::default();
        let mut s = sim(&app, &ui, &en());
        for text in [
            "Opening books",
            "(no file)",
            "Changed",
            "e4",
            "100%",
            "Weight",
            "The game plays: e4",
            "Compare",
            "Tools",
        ] {
            assert!(s.find(text).is_ok(), "missing {text:?}");
        }
        for text in [
            "Book 1",
            "Book 2",
            "Make from the filter",
            "Add line",
            "Merge books",
        ] {
            assert!(s.find(text).is_err(), "{text:?} must stay hidden");
        }
    }

    #[test]
    fn the_empty_panel_says_what_to_do() {
        let app = AppState::default();
        let ui = BookPanelUi::default();
        let mut s = sim(&app, &ui, &en());
        assert!(
            s.find("No book yet. Open a file or start a new one.")
                .is_ok()
        );
    }

    #[test]
    fn the_panel_is_drawn_in_portuguese_with_variante_and_never_variacao() {
        let app = state_with_book();
        let ui = tools_open();
        let mut s = sim(&app, &ui, &pt());
        for text in [
            "Livros de abertura",
            "Comparar",
            "Ferramentas",
            "Adicionar linha",
            "Importar ramo",
        ] {
            assert!(s.find(text).is_ok(), "missing {text:?}");
        }
        let ftl = include_str!("../../../../assets/i18n/pt-BR/book.ftl").to_lowercase();
        assert!(!ftl.contains("variação") && !ftl.contains("variações"));
        assert!(ftl.contains("variantes"));
    }

    #[test]
    fn the_file_actions_are_icon_buttons_that_send_their_messages() {
        let app = state_with_book();
        let side = BookSide::First;
        let messages = sent(&app, &side_id(side, "close"));
        assert!(
            messages
                .iter()
                .any(|m| is_book(m, &BookMessage::Close { side }))
        );
        let messages = sent(&app, &side_id(side, "save-as"));
        assert!(
            messages
                .iter()
                .any(|m| matches!(m, Message::Book(BookUiMessage::SaveAs(BookSide::First)))),
            "{messages:?}"
        );
        let messages = sent(&app, &side_id(side, "open"));
        assert!(
            messages
                .iter()
                .any(|m| matches!(m, Message::Book(BookUiMessage::Open(BookSide::First)))),
            "{messages:?}"
        );
        let messages = sent(&app, &side_id(side, "save"));
        assert!(
            messages
                .iter()
                .any(|m| matches!(m, Message::Book(BookUiMessage::Save(BookSide::First)))),
            "{messages:?}"
        );
        let messages = sent(&app, &side_id(side, "new"));
        assert!(
            messages
                .iter()
                .any(|m| is_book(m, &BookMessage::New { side }))
        );
    }

    #[test]
    fn the_tools_edit_buttons_send_their_messages() {
        let app = state_with_book();
        let side = BookSide::First;
        for (name, wanted) in [
            ("add-line", BookMessage::AddLine { side }),
            ("remove-line", BookMessage::RemoveLine { side }),
            ("import-branch", BookMessage::ImportBranch { side }),
        ] {
            let messages = sent(&app, &side_id(side, name));
            assert!(
                messages.iter().any(|m| is_book(m, &wanted)),
                "{name}: {messages:?}"
            );
        }
        let messages = sent(&app, &side_id(side, "remove-e4"));
        assert!(
            messages.iter().any(|m| matches!(
                m,
                Message::App(AppMessage::Book(BookMessage::RemoveMove { .. }))
            )),
            "{messages:?}"
        );
        let messages = sent(&app, &side_id(side, "double-e4"));
        assert!(messages.iter().any(|m| matches!(
            m,
            Message::App(AppMessage::Book(BookMessage::ScaleWeight {
                scale: WeightScale::Double,
                ..
            }))
        )));
    }

    #[test]
    fn the_rows_are_read_only_until_the_tools_open() {
        let app = state_with_book();
        let closed = BookPanelUi::default();
        let mut s = sim(&app, &closed, &en());
        assert!(
            s.find(Id::from(side_id(BookSide::First, "double-e4")))
                .is_err()
        );
        assert!(
            s.find(Id::from(side_id(BookSide::First, "remove-e4")))
                .is_err()
        );
        drop(s);
        let open = tools_open();
        let mut s = sim(&app, &open, &en());
        assert!(
            s.find(Id::from(side_id(BookSide::First, "double-e4")))
                .is_ok()
        );
    }

    #[test]
    fn at_the_end_of_the_game_the_line_buttons_are_disabled() {
        let mut app = state_with_book();
        drop(app.session.update(GameMessage::Navigate(Nav::End)));
        assert!(sent(&app, &side_id(BookSide::First, "add-line")).is_empty());
        assert!(sent(&app, &side_id(BookSide::First, "add-move")).is_empty());
    }

    #[test]
    fn a_book_with_changes_asks_before_it_closes() {
        let mut app = state_with_book();
        drop(update(
            &mut app,
            AppMessage::Book(BookMessage::Close {
                side: BookSide::First,
            }),
        ));
        let ui = BookPanelUi::default();
        let mut s = sim(&app, &ui, &en());
        assert!(
            s.find("The book has changes that are not saved. Close it anyway?")
                .is_ok()
        );
        s.click("Close and discard").expect("button");
        assert!(
            s.into_messages()
                .any(|m| is_book(&m, &BookMessage::ConfirmClose(true)))
        );
    }

    #[test]
    fn a_failure_is_shown_with_its_text_and_can_be_dismissed() {
        let mut app = state_with_book();
        drop(update(
            &mut app,
            AppMessage::Book(BookMessage::SetWeight {
                side: BookSide::First,
                mv: mychess_core::Position::new()
                    .parse_uci("e2e4")
                    .expect("uci"),
                text: "0".to_owned(),
            }),
        ));
        assert_eq!(
            app.book_view().notice,
            Some(BookNotice::Issue(BookIssue::ZeroWeight))
        );
        let ui = BookPanelUi::default();
        let mut s = sim(&app, &ui, &en());
        assert!(
            s.find("The weight cannot be zero. Remove the move instead.")
                .is_ok()
        );
        s.click(Id::from(DISMISS_ID)).expect("button");
        assert!(
            s.into_messages()
                .any(|m| is_book(&m, &BookMessage::DismissNotice))
        );
    }

    #[test]
    fn an_opening_book_problem_shows_on_the_closed_tools() {
        let app = state_with_book();
        let mut view = app.book_view();
        view.opening = Some(mychess_app::OpeningBookView {
            path: PathBuf::from("missing.bin"),
            loaded_name: None,
            issue: Some(BookIssue::NotFound),
        });
        let i18n = en();
        let closed = BookPanelUi::default();
        let mut s = Simulator::with_size(
            Settings::default(),
            Size::new(800.0, 700.0),
            content(&view, &closed, at(&app), &i18n),
        );
        assert!(s.find("Opening book problem").is_ok(), "the badge");
        drop(s);
        let open = tools_open();
        let mut s = Simulator::with_size(
            Settings::default(),
            Size::new(800.0, 700.0),
            content(&view, &open, at(&app), &i18n),
        );
        assert!(s.find("The file does not exist.").is_ok(), "the reason");
        assert!(
            s.find("Opening book problem").is_err(),
            "no badge when open"
        );
    }

    #[test]
    fn a_set_up_position_says_the_book_does_not_apply() {
        let mut app = state_with_book();
        app.session.replace_game(mychess_core::Game::from_position(
            mychess_core::Position::chess960_start(100).expect("start"),
        ));
        let ui = BookPanelUi::default();
        let mut s = sim(&app, &ui, &en());
        assert!(
            s.find("A Polyglot book only holds standard chess positions.")
                .is_ok()
        );
    }

    #[test]
    fn what_is_typed_in_a_weight_field_belongs_to_one_position() {
        let app = state_with_book();
        let e4 = mychess_core::Position::new()
            .parse_uci("e2e4")
            .expect("uci");
        let here = at(&app);
        let mut ui = BookPanelUi::default();
        ui.input(here, BookSide::First, e4, "30".to_owned());
        assert_eq!(ui.draft(here, BookSide::First, e4), Some("30"));
        assert_eq!(ui.draft(here, BookSide::Second, e4), None);
        // Another position (or game) forgets it.
        let (_, generation) = here;
        let mut moved = state_with_book();
        drop(moved.session.update(GameMessage::Navigate(Nav::End)));
        let there = (moved.session.cursor(), generation);
        assert_eq!(ui.draft(there, BookSide::First, e4), None);
        ui.input(there, BookSide::First, e4, "7".to_owned());
        assert_eq!(ui.draft(here, BookSide::First, e4), None);
        ui.clear();
        assert_eq!(ui.draft(there, BookSide::First, e4), None);
    }

    #[test]
    fn the_pane_opens_from_the_toggle_and_tells_the_application() {
        let mut shell = Shell::new(None);
        shell.i18n = en();
        assert!(!shell.ui.is_open(PaneKind::Book) && !shell.app.book().is_open());
        drop(shell.update(Message::Ui(UiMessage::TogglePane(PaneKind::Book))));
        assert!(shell.ui.is_open(PaneKind::Book) && shell.app.book().is_open());
        assert!(shell.ui.layout().is_valid());
        let mut s =
            Simulator::with_size(Settings::default(), Size::new(1200.0, 700.0), view(&shell));
        assert!(s.find("Compare").is_ok());
        drop(s);
        drop(shell.update(Message::Ui(UiMessage::TogglePane(PaneKind::Book))));
        assert!(!shell.ui.is_open(PaneKind::Book) && !shell.app.book().is_open());
    }

    /// The ids of the messages of a Fluent file.
    fn ids(text: &str) -> Vec<String> {
        text.lines()
            .filter(|l| !l.starts_with([' ', '#', '[', '*', '}']) && l.contains(" ="))
            .filter_map(|l| l.split(" =").next())
            .map(str::to_owned)
            .collect()
    }

    #[test]
    fn both_languages_have_the_same_book_keys_and_every_issue_has_a_text() {
        let en_text = include_str!("../../../../assets/i18n/en-US/book.ftl");
        let pt_text = include_str!("../../../../assets/i18n/pt-BR/book.ftl");
        assert_eq!(ids(en_text), ids(pt_text));
        let known = ids(en_text);
        for issue in [
            BookIssue::NotFound,
            BookIssue::Unreadable,
            BookIssue::TooShort,
            BookIssue::TooLarge,
            BookIssue::ReadOnly,
            BookIssue::NoPath,
            BookIssue::WriteFailed,
            BookIssue::Unsupported,
            BookIssue::IllegalMove,
            BookIssue::ZeroWeight,
            BookIssue::WeightTooLarge,
            BookIssue::TooManyMoves,
            BookIssue::NotANumber,
            BookIssue::AlreadyThere,
            BookIssue::NothingToAdd,
            BookIssue::BranchTooBig,
            BookIssue::UnsavedChanges,
            BookIssue::NoBook,
            BookIssue::NoGames,
            BookIssue::Busy,
            BookIssue::Cancelled,
            BookIssue::Internal,
        ] {
            assert!(known.contains(&issue.key().to_owned()), "{}", issue.key());
        }
        for notice in [
            BookNotice::Saved,
            BookNotice::Created {
                used: 1,
                skipped: 0,
                entries: 2,
            },
            BookNotice::Merged {
                written: 1,
                skipped: 0,
            },
            BookNotice::Added(1),
            BookNotice::Removed(1),
            BookNotice::Copied,
            BookNotice::OpeningLoaded,
            BookNotice::CreateCancelled,
        ] {
            assert!(known.contains(&notice.key().to_owned()), "{}", notice.key());
        }
    }
}
