//! The dialogs of the masks (spec M5-4c, D1817, D1826, D1832, D1833): the
//! question about saving, "fill with the line", the comment editor, the
//! search and the outline.
//!
//! The search result (up to 10 000 rows) and the outline (up to 20 000
//! nodes) are lists of rows of one height inside a `scrollable`; only the
//! rows in view (plus a margin) become widgets, the rest is a spacer
//! (`window` in `state.rs`).

use fluent_bundle::FluentArgs;
use iced::keyboard::{self, key::Named};
use iced::widget::text::Wrapping;
use iced::widget::text_editor::{Binding, KeyPress, Status};
use iced::widget::{
    button, checkbox, column, container, mouse_area, pick_list, row, scrollable, space, text,
    text_editor, text_input,
};
use iced::{Alignment, Element, Length, Padding, Size};
use mychess_app::{
    AppState, DiscardChoice, FillSide, Marker, MarkerSlot, MaskColor, MaskKey, MaskNag, MaskPrompt,
    Outline, OutlineNode,
};

use super::state::{
    CommentDraft, CommentFor, FlatRow, LIST_HEIGHT, LIST_ROW_HEIGHT, MaskUiMessage, OutlineUi,
    SearchEdit, SearchForm, SearchUi, node_at, window,
};
use super::{cells, color_label, marker_label, nag_label};
use crate::i18n::I18n;
use crate::keymap::{Platform, binding_label};
use crate::shell::Message;
use crate::theme::{self, RowLook};
use crate::widgets::icon::{Icon, IconTone, icon};
use crate::widgets::modal::{Card, modal};
use crate::windows::header_dialog::Labeled;

/// Id of the question about saving.
pub const MASK_PROMPT_BOX_ID: &str = "mask-prompt-box";
/// Id of the question of "fill with the line".
pub const MASK_FILL_BOX_ID: &str = "mask-fill-box";
/// Id of the comment editor.
pub const MASK_COMMENT_BOX_ID: &str = "mask-comment-box";
/// Id of the text area of the comment editor.
pub const MASK_COMMENT_EDITOR_ID: &str = "mask-comment-editor";
/// Id of the search dialog.
pub const MASK_SEARCH_BOX_ID: &str = "mask-search-box";
/// Id of the outline dialog.
pub const MASK_OUTLINE_BOX_ID: &str = "mask-outline-box";
/// Id of the list of the search result.
pub const MASK_SEARCH_LIST_ID: &str = "mask-search-list";
/// Id of the list of the outline.
pub const MASK_OUTLINE_LIST_ID: &str = "mask-outline-list";

fn ui(message: MaskUiMessage) -> Message {
    Message::MaskUi(message)
}

fn app_button<'a>(label: String, message: Message, primary: bool) -> Element<'a, Message> {
    let button = button(text(label)).on_press(message);
    if primary {
        button.style(button::primary).into()
    } else {
        button.style(button::secondary).into()
    }
}

// ---- the question about saving ----

/// "The mask was changed: save it first?" with Save, Don't save and Cancel.
pub fn prompt<'a>(prompt: &MaskPrompt, i18n: &I18n) -> Element<'a, Message> {
    let answer = |choice| Message::App(super::state::answer(choice));
    let body = match &prompt.name {
        Some(name) => {
            let mut args = FluentArgs::new();
            args.set("name", name.clone());
            i18n.tr_args("tree-mask-unsaved-body", &args)
        }
        None => i18n.tr("tree-mask-unsaved-body-unnamed"),
    };
    modal(Card {
        id: MASK_PROMPT_BOX_ID,
        title: i18n.tr("tree-mask-unsaved-title"),
        body: text(body).into(),
        buttons: vec![
            app_button(
                i18n.tr("dialog-cancel"),
                answer(DiscardChoice::Cancel),
                false,
            ),
            app_button(
                i18n.tr("dialog-discard-dont-save"),
                answer(DiscardChoice::DontSave),
                false,
            ),
            app_button(
                i18n.tr("dialog-discard-save"),
                answer(DiscardChoice::Save),
                true,
            ),
        ],
        width: 460.0,
    })
}

// ---- fill with the line ----

/// The question of which moves of the line go into the mask (D1826).
pub fn fill_line<'a>(i18n: &I18n) -> Element<'a, Message> {
    let side = |key: &str, side: FillSide, primary: bool| {
        app_button(i18n.tr(key), ui(MaskUiMessage::FillLine(side)), primary)
    };
    modal(Card {
        id: MASK_FILL_BOX_ID,
        title: i18n.tr("tree-mask-fill-line-title"),
        body: text(i18n.tr("tree-mask-fill-line-body")).into(),
        buttons: vec![
            app_button(
                i18n.tr("dialog-cancel"),
                ui(MaskUiMessage::CloseFillLine),
                false,
            ),
            side("tree-mask-side-black", FillSide::Black, false),
            side("tree-mask-side-white", FillSide::White, false),
            side("tree-mask-side-all", FillSide::All, true),
        ],
        width: 560.0,
    })
}

// ---- the comment editor ----

/// The keys of the text area: Ctrl/Cmd+Enter saves; Esc is the shell's.
pub fn comment_key_binding(press: KeyPress) -> Option<Binding<Message>> {
    if !matches!(press.status, Status::Focused { .. }) {
        return None;
    }
    match press.key.as_ref() {
        keyboard::Key::Named(Named::Enter) if press.modifiers.command() => {
            Some(Binding::Custom(ui(MaskUiMessage::CommentCommit)))
        }
        _ => Binding::from_key_press(press),
    }
}

/// `Ctrl+Enter` (`⌘Enter` on macOS) as the platform writes it.
fn save_shortcut_label() -> String {
    let binding = mychess_app::Binding::new(
        mychess_app::Key::Named(mychess_app::NamedKey::Enter),
        mychess_app::Modifiers::COMMAND,
    );
    binding_label(&binding, Platform::current())
}

/// The editor of the comment of a move or of the position (D1830).
pub fn comment<'a>(draft: &'a CommentDraft, i18n: &I18n) -> Element<'a, Message> {
    let title = match &draft.target {
        CommentFor::Move { san, .. } => {
            let mut args = FluentArgs::new();
            args.set("move", san.clone());
            i18n.tr_args("tree-mask-comment-move-title", &args)
        }
        CommentFor::Position => i18n.tr("tree-mask-comment-position-title"),
    };
    let mut args = FluentArgs::new();
    args.set("shortcut", save_shortcut_label());
    let area = text_editor(&draft.content)
        .id(MASK_COMMENT_EDITOR_ID)
        .placeholder(i18n.tr("tree-mask-comment-placeholder"))
        .height(Length::Fixed(180.0))
        .on_action(|action| ui(MaskUiMessage::CommentAction(action)))
        .key_binding(comment_key_binding);
    let body = column![
        area,
        text(i18n.tr_args("tree-mask-comment-hint", &args))
            .size(12)
            .style(theme::muted_text),
    ]
    .spacing(8);
    modal(Card {
        id: MASK_COMMENT_BOX_ID,
        title,
        body: body.into(),
        buttons: vec![
            app_button(
                i18n.tr("dialog-cancel"),
                ui(MaskUiMessage::CommentCancel),
                false,
            ),
            app_button(
                i18n.tr("tree-mask-comment-ok"),
                ui(MaskUiMessage::CommentCommit),
                true,
            ),
        ],
        width: 560.0,
    })
}

// ---- room in the window ----

/// What the card of a dialog takes around its body (title, buttons,
/// padding, spacing).
const CARD_CHROME: f32 = 190.0;
/// What the search dialog has besides the result list (form, summary,
/// header, hint).
const SEARCH_CHROME: f32 = 300.0;
/// The same for the outline (tools, hint).
const OUTLINE_CHROME: f32 = 110.0;
/// Space kept at each side of a card.
const SIDE_MARGIN: f32 = 24.0;

/// `width` or, in a narrower window, the window less its margins (D1901).
pub fn fit_width(width: f32, window: Size) -> f32 {
    if window.width.is_finite() && window.width > 0.0 {
        width.min((window.width - 2.0 * SIDE_MARGIN).max(280.0))
    } else {
        width
    }
}

/// The height the body of a card may take in `window`.
fn body_room(window: Size) -> f32 {
    if window.height.is_finite() && window.height > 0.0 {
        (window.height - CARD_CHROME).max(160.0)
    } else {
        f32::MAX
    }
}

/// The height of a list that shares the body with `chrome` pixels of other
/// things: the usual one, less in a low window.
fn list_height(window: Size, chrome: f32) -> f32 {
    (body_room(window) - chrome).clamp(96.0, LIST_HEIGHT)
}

/// The body in an area that scrolls when the window is too low for it, so
/// the buttons of the card never leave the screen.
fn scrolled(body: Element<'_, Message>, window: Size) -> Element<'_, Message> {
    container(scrollable(container(body).padding(Padding {
        right: 12.0,
        ..Padding::ZERO
    })))
    .max_height(body_room(window))
    .into()
}

// ---- the search ----

fn nag_options(i18n: &I18n) -> Vec<Labeled<Option<MaskNag>>> {
    let mut options: Vec<Labeled<Option<MaskNag>>> = MaskNag::STANDARD
        .iter()
        .map(|nag| Labeled {
            value: Some(nag.clone()),
            label: nag_label(i18n, nag),
        })
        .collect();
    options.push(Labeled {
        value: None,
        label: i18n.tr("tree-mask-nag-none"),
    });
    options
}

fn marker_options(i18n: &I18n) -> Vec<Labeled<Option<Marker>>> {
    let mut options: Vec<Labeled<Option<Marker>>> = Marker::ALL
        .iter()
        .map(|marker| Labeled {
            value: Some(marker.clone()),
            label: marker_label(i18n, marker),
        })
        .collect();
    options.push(Labeled {
        value: None,
        label: i18n.tr("tree-mask-marker-none"),
    });
    options
}

fn color_options(i18n: &I18n) -> Vec<Labeled<MaskColor>> {
    MaskColor::STANDARD
        .iter()
        .map(|color| Labeled {
            value: color.clone(),
            label: color_label(i18n, color),
        })
        .collect()
}

fn filter_row<'a>(
    label: String,
    on: bool,
    toggle: impl Fn(bool) -> SearchEdit + 'a,
    control: Element<'a, Message>,
) -> Element<'a, Message> {
    row![
        checkbox(on)
            .label(label)
            .size(14)
            .text_size(13)
            .width(Length::Fixed(240.0))
            .on_toggle(move |value| ui(MaskUiMessage::Search(toggle(value)))),
        control,
    ]
    .spacing(8)
    .align_y(Alignment::Center)
    .into()
}

fn marker_row<'a>(form: &SearchForm, slot: MarkerSlot, i18n: &I18n) -> Element<'a, Message> {
    let (index, key) = match slot {
        MarkerSlot::First => (0, "tree-mask-search-marker-1"),
        MarkerSlot::Second => (1, "tree-mask-search-marker-2"),
    };
    let current = form.markers[index].clone();
    let selected = Labeled {
        label: current.as_ref().map_or_else(
            || i18n.tr("tree-mask-marker-none"),
            |m| marker_label(i18n, m),
        ),
        value: current,
    };
    let control = pick_list(marker_options(i18n), Some(selected), move |choice| {
        ui(MaskUiMessage::Search(SearchEdit::Marker(
            slot,
            choice.value,
        )))
    })
    .text_size(13)
    .padding(3);
    filter_row(
        i18n.tr(key),
        form.marker_on[index],
        move |on| SearchEdit::MarkerOn(slot, on),
        control.into(),
    )
}

fn form<'a>(form: &SearchForm, i18n: &I18n) -> Element<'a, Message> {
    let nag = Labeled {
        label: form
            .nag
            .as_ref()
            .map_or_else(|| i18n.tr("tree-mask-nag-none"), |n| nag_label(i18n, n)),
        value: form.nag.clone(),
    };
    let nag_control = pick_list(nag_options(i18n), Some(nag), |choice| {
        ui(MaskUiMessage::Search(SearchEdit::Nag(choice.value)))
    })
    .text_size(13)
    .padding(3);
    let color = Labeled {
        label: color_label(i18n, &form.color),
        value: form.color.clone(),
    };
    let color_control = pick_list(color_options(i18n), Some(color), |choice| {
        ui(MaskUiMessage::Search(SearchEdit::Color(choice.value)))
    })
    .text_size(13)
    .padding(3);
    let move_text = text_input("", &form.move_text)
        .size(13)
        .padding(4)
        .on_input(|text| ui(MaskUiMessage::Search(SearchEdit::MoveText(text))))
        .on_submit(ui(MaskUiMessage::SearchRun));
    let position_text = text_input("", &form.position_text)
        .size(13)
        .padding(4)
        .on_input(|text| ui(MaskUiMessage::Search(SearchEdit::PositionText(text))))
        .on_submit(ui(MaskUiMessage::SearchRun));
    column![
        filter_row(
            i18n.tr("tree-mask-search-nag"),
            form.nag_on,
            SearchEdit::NagOn,
            nag_control.into()
        ),
        marker_row(form, MarkerSlot::First, i18n),
        marker_row(form, MarkerSlot::Second, i18n),
        filter_row(
            i18n.tr("tree-mask-search-color"),
            form.color_on,
            SearchEdit::ColorOn,
            color_control.into()
        ),
        filter_row(
            i18n.tr("tree-mask-search-move-text"),
            form.move_on,
            SearchEdit::MoveTextOn,
            move_text.into()
        ),
        filter_row(
            i18n.tr("tree-mask-search-position-text"),
            form.position_on,
            SearchEdit::PositionTextOn,
            position_text.into()
        ),
    ]
    .spacing(6)
    .into()
}

/// A space as high as `rows` rows of the list.
fn spacer<'a>(rows: usize) -> Element<'a, Message> {
    // Row counts are at most a few tens of thousands.
    #[allow(clippy::cast_precision_loss)]
    let height = rows as f32 * LIST_ROW_HEIGHT;
    space().height(Length::Fixed(height)).into()
}

fn clipped<'a>(content: impl Into<String>, width: Length, muted: bool) -> Element<'a, Message> {
    let label = text(content.into()).size(13).wrapping(Wrapping::None);
    let label = if muted {
        label.style(theme::muted_text)
    } else {
        label
    };
    container(label).width(width).clip(true).into()
}

fn hit_row<'a>(
    index: usize,
    hit: &mychess_app::MaskHit,
    selected: bool,
    i18n: &I18n,
) -> Element<'a, Message> {
    let move_label = hit
        .san
        .clone()
        .unwrap_or_else(|| i18n.tr("tree-mask-search-position-row"));
    let fen = match &hit.key {
        MaskKey::Fen(text) | MaskKey::Opaque(text) => text.clone(),
    };
    let cells = row![
        clipped(move_label, Length::Fixed(90.0), false),
        clipped(hit.comment.clone(), Length::FillPortion(2), false),
        clipped(fen, Length::FillPortion(3), true),
    ]
    .spacing(8)
    .align_y(Alignment::Center);
    let body = container(cells)
        .height(Length::Fixed(LIST_ROW_HEIGHT))
        .padding([0, 6])
        .center_y(Length::Fixed(LIST_ROW_HEIGHT))
        .style(theme::list_row(RowLook {
            zebra: index % 2 == 1,
            selected,
            ..RowLook::default()
        }));
    mouse_area(body)
        .on_press(ui(MaskUiMessage::SearchSelect(index)))
        .on_double_click(ui(MaskUiMessage::SearchGo(index)))
        .into()
}

fn list_header<'a>(i18n: &I18n) -> Element<'a, Message> {
    container(
        row![
            clipped(
                i18n.tr("tree-mask-search-col-move"),
                Length::Fixed(90.0),
                true
            ),
            clipped(
                i18n.tr("tree-mask-search-col-comment"),
                Length::FillPortion(2),
                true
            ),
            clipped(
                i18n.tr("tree-mask-search-col-fen"),
                Length::FillPortion(3),
                true
            ),
        ]
        .spacing(8),
    )
    .padding([4, 6])
    .width(Length::Fill)
    .style(theme::list_header)
    .into()
}

fn results<'a>(
    search: &SearchUi,
    app: &AppState,
    i18n: &I18n,
    list_height: f32,
) -> Element<'a, Message> {
    let Some(state) = app.mask_search() else {
        return space().height(Length::Fixed(list_height)).into();
    };
    let Some(hits) = state.hits.as_ref() else {
        return container(text(i18n.tr("tree-mask-progress-search")).size(13))
            .height(Length::Fixed(list_height))
            .into();
    };
    let count = |value: usize| i64::try_from(value).unwrap_or(i64::MAX);
    let mut args = FluentArgs::new();
    args.set("positions_found", count(hits.positions_found));
    args.set("positions_total", count(hits.positions_total));
    args.set("moves_found", count(hits.moves_found));
    args.set("moves_total", count(hits.moves_total));
    let mut page = column![
        text(i18n.tr_args("tree-mask-search-summary", &args))
            .size(13)
            .style(theme::muted_text),
    ]
    .spacing(4);
    if hits.truncated {
        let mut limit = FluentArgs::new();
        // The rows kept are the limit that was hit.
        limit.set("limit", i64::try_from(hits.rows.len()).unwrap_or(i64::MAX));
        page = page.push(
            text(i18n.tr_args("tree-mask-search-truncated", &limit))
                .size(13)
                .style(text::danger),
        );
    }
    if hits.rows.is_empty() {
        return page
            .push(text(i18n.tr("tree-mask-search-empty")).size(13))
            .into();
    }
    let total = hits.rows.len();
    let (first, count) = window(search.scroll, total, LIST_ROW_HEIGHT, LIST_HEIGHT);
    let mut rows = column![spacer(first)].width(Length::Fill);
    for (offset, hit) in hits.rows.iter().skip(first).take(count).enumerate() {
        let index = first + offset;
        rows = rows.push(hit_row(index, hit, search.selected == Some(index), i18n));
    }
    rows = rows.push(spacer(total.saturating_sub(first + count)));
    page.push(list_header(i18n))
        .push(
            scrollable(rows)
                .id(MASK_SEARCH_LIST_ID)
                .height(Length::Fixed(list_height))
                .on_scroll(|viewport| {
                    ui(MaskUiMessage::SearchScrolled(viewport.absolute_offset().y))
                }),
        )
        .into()
}

/// The search dialog (D1832): the filters, the button and the result.
pub fn search<'a>(
    search: &'a SearchUi,
    app: &AppState,
    i18n: &I18n,
    window: Size,
) -> Element<'a, Message> {
    let list_height = list_height(window, SEARCH_CHROME);
    let body = column![
        form(&search.form, i18n),
        results(search, app, i18n, list_height),
        text(i18n.tr("tree-mask-search-hint"))
            .size(12)
            .style(theme::muted_text),
    ]
    .spacing(10);
    let body = scrolled(body.into(), window);
    let can_go = search.selected.is_some();
    modal(Card {
        id: MASK_SEARCH_BOX_ID,
        title: i18n.tr("tree-mask-search-title"),
        body,
        buttons: vec![
            app_button(
                i18n.tr("tree-mask-search-close"),
                ui(MaskUiMessage::SearchClose),
                false,
            ),
            button(text(i18n.tr("tree-mask-search-go")))
                .style(button::secondary)
                .on_press_maybe(
                    search
                        .selected
                        .filter(|_| can_go)
                        .map(|row| ui(MaskUiMessage::SearchGo(row))),
                )
                .into(),
            app_button(
                i18n.tr("tree-mask-search-run"),
                ui(MaskUiMessage::SearchRun),
                true,
            ),
        ],
        width: fit_width(880.0, window),
    })
}

// ---- the outline ----

/// The text of a node: the merged moves with their NAGs and comments
/// (D1833).
pub fn node_text(node: &OutlineNode, show_marks: bool, show_comments: bool) -> String {
    let mut out = String::new();
    for (index, step) in node.steps.iter().enumerate() {
        if !out.is_empty() {
            out.push(' ');
        }
        if show_comments && index == 0 && !step.position_comment.is_empty() {
            out.push('[');
            out.push_str(&step.position_comment);
            out.push_str("] ");
        }
        out.push_str(&step.san);
        if show_marks && let Some(nag) = &step.nag {
            out.push_str(nag.as_str());
        }
        if show_comments && !step.comment.is_empty() {
            out.push_str(" {");
            out.push_str(&step.comment);
            out.push('}');
        }
    }
    if node.truncated {
        out.push_str(" ...");
    }
    out
}

fn outline_row<'a>(
    index: usize,
    flat: &FlatRow,
    node: &OutlineNode,
    ui_state: &OutlineUi,
) -> Element<'a, Message> {
    let show_marks = !ui_state.hide_marks;
    let show_comments = !ui_state.hide_comments;
    // Depth is at most the budget (200 by default).
    #[allow(clippy::cast_precision_loss)]
    let indent = flat.depth.min(400) as f32 * 16.0;
    let toggle: Element<'a, Message> = if node.children.is_empty() {
        space().width(Length::Fixed(18.0)).into()
    } else {
        let folded = ui_state.folded.contains(&flat.path);
        button(icon(
            if folded {
                Icon::Forward
            } else {
                Icon::SortDown
            },
            12.0,
            IconTone::Text,
        ))
        .padding(2)
        .style(button::text)
        .on_press(ui(MaskUiMessage::OutlineToggle(index)))
        .into()
    };
    let mut cells = row![space().width(Length::Fixed(indent)), toggle]
        .spacing(4)
        .align_y(Alignment::Center);
    if show_marks && let Some(last) = node.steps.last() {
        if last.color != MaskColor::White {
            cells = cells.push(cells::swatch(last.color.clone(), 10.0));
        }
        for marker in last.markers.iter().flatten() {
            cells = cells.push(cells::marker_glyph(marker, 14.0));
        }
    }
    cells = cells.push(clipped(
        node_text(node, show_marks, show_comments),
        Length::Fill,
        false,
    ));
    let body = container(cells)
        .height(Length::Fixed(LIST_ROW_HEIGHT))
        .padding([0, 6])
        .center_y(Length::Fixed(LIST_ROW_HEIGHT))
        .style(theme::list_row(RowLook {
            zebra: index % 2 == 1,
            selected: ui_state.selected.as_deref() == Some(&*flat.path),
            ..RowLook::default()
        }));
    mouse_area(body)
        .on_press(ui(MaskUiMessage::OutlineSelect(index)))
        .on_double_click(ui(MaskUiMessage::OutlineGo(index)))
        .into()
}

fn outline_list<'a>(
    outline: &Outline,
    ui_state: &OutlineUi,
    list_height: f32,
) -> Element<'a, Message> {
    let total = ui_state.rows.len();
    let (first, count) = window(ui_state.scroll, total, LIST_ROW_HEIGHT, LIST_HEIGHT);
    let mut rows = column![spacer(first)].width(Length::Fill);
    for (offset, flat) in ui_state.rows.iter().skip(first).take(count).enumerate() {
        if let Some(node) = node_at(outline, &flat.path) {
            rows = rows.push(outline_row(first + offset, flat, node, ui_state));
        }
    }
    rows = rows.push(spacer(total.saturating_sub(first + count)));
    scrollable(rows)
        .id(MASK_OUTLINE_LIST_ID)
        .height(Length::Fixed(list_height))
        .on_scroll(|viewport| ui(MaskUiMessage::OutlineScrolled(viewport.absolute_offset().y)))
        .into()
}

fn outline_tools<'a>(ui_state: &OutlineUi, i18n: &I18n) -> Element<'a, Message> {
    let small = |key: &str, message: MaskUiMessage| {
        button(text(i18n.tr(key)).size(12))
            .padding([3, 8])
            .style(button::secondary)
            .on_press(ui(message))
    };
    row![
        small("tree-mask-outline-expand", MaskUiMessage::OutlineExpandAll),
        small(
            "tree-mask-outline-collapse",
            MaskUiMessage::OutlineCollapseAll
        ),
        checkbox(!ui_state.hide_marks)
            .label(i18n.tr("tree-mask-outline-nags"))
            .size(14)
            .text_size(12)
            .on_toggle(|on| ui(MaskUiMessage::OutlineMarks(on))),
        checkbox(!ui_state.hide_comments)
            .label(i18n.tr("tree-mask-outline-comments"))
            .size(14)
            .text_size(12)
            .on_toggle(|on| ui(MaskUiMessage::OutlineComments(on))),
        small("tree-mask-outline-refresh", MaskUiMessage::OutlineRefresh),
    ]
    .spacing(10)
    .align_y(Alignment::Center)
    .wrap()
    .into()
}

/// The outline of the mask (D1833): the lines from the position of the
/// cursor, folded or not.
pub fn outline<'a>(
    app: &AppState,
    ui_state: &OutlineUi,
    i18n: &I18n,
    window: Size,
) -> Element<'a, Message> {
    let built = app
        .mask_outline()
        .and_then(|state| state.outline.as_deref());
    let mut body = column![outline_tools(ui_state, i18n)].spacing(8);
    match built {
        None => {
            body = body.push(text(i18n.tr("tree-mask-outline-building")).size(13));
        }
        Some(outline) => {
            if outline.truncated {
                body = body.push(
                    text(i18n.tr("tree-mask-outline-truncated"))
                        .size(13)
                        .style(text::danger),
                );
            }
            if outline.roots.is_empty() {
                body = body.push(text(i18n.tr("tree-mask-outline-empty")).size(13));
            } else {
                if !outline.comment.is_empty() {
                    body = body.push(
                        text(format!("[{}]", outline.comment))
                            .size(13)
                            .style(theme::muted_text),
                    );
                }
                body = body.push(outline_list(
                    outline,
                    ui_state,
                    list_height(window, OUTLINE_CHROME),
                ));
            }
        }
    }
    body = body.push(
        text(i18n.tr("tree-mask-outline-hint"))
            .size(12)
            .style(theme::muted_text),
    );
    modal(Card {
        id: MASK_OUTLINE_BOX_ID,
        title: i18n.tr("tree-mask-outline-title"),
        body: scrolled(body.into(), window),
        buttons: vec![app_button(
            i18n.tr("tree-mask-search-close"),
            ui(MaskUiMessage::OutlineClose),
            true,
        )],
        width: fit_width(760.0, window),
    })
}

#[cfg(test)]
mod tests {
    use mychess_app::OutlineStep;

    use super::*;

    fn step(san: &str, nag: Option<MaskNag>, comment: &str, position: &str) -> OutlineStep {
        OutlineStep {
            position_comment: position.to_owned(),
            san: san.to_owned(),
            nag,
            color: MaskColor::White,
            markers: [None, None],
            comment: comment.to_owned(),
            after: None,
        }
    }

    #[test]
    fn a_merged_node_reads_as_a_line_of_moves() {
        let node = OutlineNode {
            steps: vec![
                step("e4", Some(MaskNag::Good), "", "start"),
                step("e5", None, "classical", ""),
            ],
            children: Vec::new(),
            truncated: true,
        };
        assert_eq!(
            node_text(&node, true, true),
            "[start] e4! e5 {classical} ..."
        );
        assert_eq!(node_text(&node, false, false), "e4 e5 ...");
    }
}
