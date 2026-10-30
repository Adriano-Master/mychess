//! The tabs, the buttons and the filter row of the EPD panel (spec M7-5c).

use iced::widget::{button, checkbox, container, row, space, text, text_input, tooltip};
use iced::{Alignment, Element, Length};
use mychess_app::epd_window::{EpdMessage, EpdSort, EpdTabView, EpdView, SortKey};

use super::{EPD_FILTER_ID, EpdUiMessage, active_tab, epd, text as words};
use crate::i18n::I18n;
use crate::shell::{Message, Shell};
use crate::theme::{self, ButtonLook};
use crate::widgets::icon::{Icon, IconTone, icon};
use crate::windows::panel_kit::{BODY_TEXT, choice};

fn ui(message: EpdUiMessage) -> Message {
    Message::Epd(message)
}

/// An icon button with a tooltip; without a message it is shown disabled.
pub(super) fn tool<'a>(
    i18n: &I18n,
    glyph: Icon,
    tip_key: &str,
    message: Option<Message>,
    id: &'static str,
) -> Element<'a, Message> {
    let tone = if message.is_some() {
        IconTone::Text
    } else {
        IconTone::Disabled
    };
    let body = button(icon(glyph, 16.0, tone))
        .padding(4)
        .style(theme::command_button(ButtonLook::Secondary, false))
        .on_press_maybe(message);
    let tip = container(text(i18n.tr(tip_key)).size(12))
        .padding([4, 8])
        .style(theme::tooltip_box);
    container(tooltip(body, tip, tooltip::Position::Bottom))
        .id(id)
        .into()
}

/// One tab per open file, and the buttons that open or create one.
pub(super) fn tabs<'a>(shell: &'a Shell, view: &EpdView) -> Element<'a, Message> {
    let i18n = &shell.i18n;
    let active = active_tab(view).map(|t| t.id);
    let mut strip = row![].spacing(4).align_y(Alignment::Center);
    for tab in &view.tabs {
        let mut label = String::new();
        if tab.dirty {
            // ASCII: the embedded fonts, not the system's, draw the tab.
            label.push_str("* ");
        }
        label.push_str(&tab.name);
        if tab.read_only {
            label.push_str(" (");
            label.push_str(&i18n.tr("epd-read-only"));
            label.push(')');
        }
        let close = button(icon(Icon::Close, 12.0, IconTone::Muted))
            .padding(3)
            .style(theme::command_button(ButtonLook::Secondary, false))
            .on_press(epd(EpdMessage::Close(tab.id)));
        let cell = row![
            choice(
                label,
                Some(tab.id) == active,
                Some(epd(EpdMessage::Activate(tab.id)))
            ),
            tooltip(
                container(close).id(format!("epd-tab-close-{}", tab.id.0)),
                container(text(i18n.tr("epd-close-tab")).size(12))
                    .padding([4, 8])
                    .style(theme::tooltip_box),
                tooltip::Position::Bottom,
            ),
        ]
        .align_y(Alignment::Center);
        strip = strip.push(container(cell).id(format!("epd-tab-{}", tab.id.0)));
    }
    let can_open = view.tabs.len() < view.max_files && !view.opening;
    row![
        strip,
        space::horizontal(),
        tool(
            i18n,
            Icon::FolderOpen,
            "epd-open",
            can_open.then(|| epd(EpdMessage::Open)),
            "epd-btn-open"
        ),
        tool(
            i18n,
            Icon::FileNew,
            "epd-new",
            can_open.then(|| epd(EpdMessage::New)),
            "epd-btn-new"
        ),
    ]
    .spacing(4)
    .align_y(Alignment::Center)
    .into()
}

/// The buttons of the file and of the record.
#[allow(clippy::too_many_lines)] // one row of buttons
pub(super) fn buttons<'a>(shell: &'a Shell, tab: &EpdTabView) -> Element<'a, Message> {
    let i18n = &shell.i18n;
    let busy = tab.busy;
    let running = tab.run.is_some();
    let editable = !tab.read_only && !busy && !running;
    let record = tab.selected.is_some();
    let has_rows = tab.rows > 0;
    let on = |ok: bool, m: EpdMessage| ok.then(|| epd(m));
    let prefs = &shell.app.prefs.epd;
    let analysis = if running {
        tool(
            i18n,
            Icon::Pause,
            "epd-stop",
            Some(epd(EpdMessage::StopAnnotate)),
            "epd-btn-stop",
        )
    } else {
        tool(
            i18n,
            Icon::Engine,
            "epd-analyze",
            (!busy && tab.records > 0).then(|| ui(EpdUiMessage::OpenAnalyze)),
            "epd-btn-analyze",
        )
    };
    let files = row![
        tool(
            i18n,
            Icon::Save,
            "epd-save",
            on(!tab.read_only && !busy && !running, EpdMessage::Save),
            "epd-btn-save"
        ),
        tool(
            i18n,
            Icon::SaveAs,
            "epd-save-as",
            on(!busy && !running, EpdMessage::SaveAs),
            "epd-btn-save-as"
        ),
    ]
    .spacing(2);
    let navigation = row![
        tool(
            i18n,
            Icon::First,
            "epd-first",
            on(has_rows, EpdMessage::First),
            "epd-btn-first"
        ),
        tool(
            i18n,
            Icon::Back,
            "epd-prev",
            on(has_rows, EpdMessage::Prev),
            "epd-btn-prev"
        ),
        tool(
            i18n,
            Icon::Forward,
            "epd-next",
            on(has_rows, EpdMessage::Next),
            "epd-btn-next"
        ),
        tool(
            i18n,
            Icon::Last,
            "epd-last",
            on(has_rows, EpdMessage::Last),
            "epd-btn-last"
        ),
        tool(
            i18n,
            Icon::Hash,
            "epd-random",
            has_rows.then(|| ui(EpdUiMessage::Random)),
            "epd-btn-random"
        ),
    ]
    .spacing(2);
    let records = row![
        tool(
            i18n,
            Icon::AddVariation,
            "epd-add-position",
            on(editable, EpdMessage::AddPosition),
            "epd-btn-add"
        ),
        tool(
            i18n,
            Icon::Trash,
            "epd-remove-position",
            on(editable && record, EpdMessage::RemovePosition),
            "epd-btn-remove"
        ),
        tool(
            i18n,
            Icon::SortUp,
            "epd-sort-opcodes",
            on(editable && record, EpdMessage::SortRecord),
            "epd-btn-sort"
        ),
        tool(
            i18n,
            Icon::ListOrdered,
            "epd-sort-all",
            on(editable, EpdMessage::SortAll),
            "epd-btn-sort-all"
        ),
        tool(
            i18n,
            Icon::Scissors,
            "epd-strip",
            (editable && tab.records > 0).then(|| ui(EpdUiMessage::OpenStrip)),
            "epd-btn-strip"
        ),
    ]
    .spacing(2);
    let tools = row![
        tool(
            i18n,
            Icon::ClipboardPaste,
            "epd-paste-analysis",
            on(editable && record, EpdMessage::PasteAnalysis(None)),
            "epd-btn-paste"
        ),
        tool(
            i18n,
            Icon::Target,
            "epd-find-deepest",
            on(has_rows, EpdMessage::Deepest),
            "epd-btn-deepest"
        ),
        analysis,
        tool(
            i18n,
            Icon::Undo,
            "epd-undo-run",
            on(tab.can_undo_run && !busy && !running, EpdMessage::UndoRun),
            "epd-btn-undo-run"
        ),
    ]
    .spacing(2);
    let flip = checkbox(prefs.flip_to_side_to_move)
        .label(i18n.tr("epd-flip"))
        .size(14)
        .text_size(12)
        .on_toggle(|on| ui(EpdUiMessage::SetFlip(on)));
    let dialect = checkbox(prefs.dialect == mychess_app::epd_window::Dialect::Scid)
        .label(i18n.tr("epd-dialect-scid"))
        .size(14)
        .text_size(12)
        .on_toggle(|on| ui(EpdUiMessage::SetDialectScid(on)));
    row![files, navigation, records, tools, flip, dialect]
        .spacing(10)
        .align_y(Alignment::Center)
        .wrap()
        .into()
}

/// The filter field and the choices of the order of the list.
pub(super) fn filter_row<'a>(shell: &'a Shell, tab: &EpdTabView) -> Element<'a, Message> {
    let i18n = &shell.i18n;
    let filter = text_input(&i18n.tr("epd-filter-placeholder"), &tab.filter)
        .id(EPD_FILTER_ID)
        .on_input(|text| epd(EpdMessage::SetFilter(text)))
        .padding(4)
        .size(BODY_TEXT)
        .width(Length::FillPortion(3));
    let mut order = row![text(i18n.tr("epd-sort-by")).size(BODY_TEXT)]
        .spacing(4)
        .align_y(Alignment::Center);
    for key in [SortKey::File, SortKey::Fen, SortKey::Id, SortKey::Eval] {
        let active = tab.sort.key == key;
        // A second click on the active order reverses it.
        let next = EpdSort {
            key,
            descending: active && !tab.sort.descending,
        };
        let mark = if active {
            if tab.sort.descending { " ▼" } else { " ▲" }
        } else {
            ""
        };
        order = order.push(choice(
            format!("{}{mark}", i18n.tr(words::sort_key(key))),
            active,
            Some(epd(EpdMessage::SetSort(next))),
        ));
    }
    row![filter, order]
        .spacing(10)
        .align_y(Alignment::Center)
        .into()
}
