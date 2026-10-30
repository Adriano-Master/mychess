//! The toolbar of the game list: search, filter buttons and job progress
//! (spec M3-4c).

use fluent_bundle::FluentArgs;
use iced::widget::{
    button, checkbox, container, progress_bar, row, space, text, text_input, tooltip,
};
use iced::{Alignment, Element, Length, Point};
use mychess_app::{BaseKind, BaseSlot, ListJobKind, SlotId};

use super::keys::ListAction;
use super::update::GameListMessage;
use super::{DEFAULT_WIDTH, MENU_WIDTH, SEARCH_ID};
use crate::i18n::I18n;
use crate::shell::{Message, Shell};
use crate::theme::{self, ButtonLook};
use crate::widgets::icon::{Icon, IconTone, icon};

// ---- toolbar ----

/// An icon button with a tooltip; without a message it is shown disabled.
pub(super) fn tool_button<'a>(
    i18n: &I18n,
    glyph: Icon,
    tip_key: &str,
    message: Option<Message>,
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
    tooltip(
        body,
        container(text(i18n.tr(tip_key)).size(12))
            .padding([4, 8])
            .style(theme::tooltip_box),
        tooltip::Position::Bottom,
    )
    .into()
}

pub(super) fn act(action: ListAction) -> Message {
    Message::Games(GameListMessage::Act(action))
}

/// What the selection allows, for the buttons and the menus.
pub(super) struct Enabled {
    pub(super) selection: bool,
    pub(super) copy: bool,
    pub(super) flag: bool,
}

pub(super) fn enabled(id: SlotId, slot: &BaseSlot) -> Enabled {
    let selection = !slot.list.selection.is_empty();
    Enabled {
        selection,
        copy: selection && !id.is_clipbase(),
        flag: selection && slot.kind == BaseKind::Si4 && !slot.read_only,
    }
}

/// The job in flight as text and progress: `Sorting: 40%` and a bar.
pub(super) fn job_view(shell: &Shell) -> Option<Element<'_, Message>> {
    let (kind, control) = shell.app.list_job()?;
    let i18n = &shell.i18n;
    let key = match kind {
        ListJobKind::Find => "games-job-find",
        ListJobKind::Filter => "games-job-filter",
        ListJobKind::Sort(_) => "games-job-sort",
        ListJobKind::Copy => "games-job-copy",
        ListJobKind::CopyGames => "games-job-copy-games",
        ListJobKind::Flags => "games-job-flags",
        ListJobKind::EcoClassify => "games-job-eco-classify",
        ListJobKind::EcoNovelty => "games-job-eco-novelty",
        ListJobKind::ImportPgn => "games-job-import-pgn",
        ListJobKind::ExportGames => "games-job-export",
        ListJobKind::SpellApply => "games-job-spell-apply",
        ListJobKind::EloAdd => "games-job-elo-add",
    };
    let (done, total) = control.progress();
    let fraction = if total > 0 {
        // A ratio to draw: the precision of an f32 is plenty.
        #[allow(clippy::cast_precision_loss)]
        let ratio = (done as f32 / total as f32).clamp(0.0, 1.0);
        ratio
    } else {
        0.0
    };
    let mut args = FluentArgs::new();
    args.set("job", i18n.tr(key));
    // Between 0 and 100 after the clamp.
    #[allow(clippy::cast_possible_truncation)]
    args.set("percent", (fraction * 100.0).round() as i64);
    Some(
        row![
            text(i18n.tr_args("games-job-progress", &args)).size(12),
            progress_bar(0.0..=1.0, fraction)
                .length(Length::Fixed(90.0))
                .girth(Length::Fixed(6.0)),
            tool_button(
                i18n,
                Icon::Close,
                "games-tip-cancel",
                Some(act(ListAction::Cancel))
            ),
        ]
        .spacing(6)
        .align_y(Alignment::Center)
        .into(),
    )
}

#[allow(clippy::too_many_lines)] // one row of buttons
pub(super) fn toolbar<'a>(
    shell: &'a Shell,
    id: SlotId,
    slot: &'a BaseSlot,
) -> Element<'a, Message> {
    let i18n = &shell.i18n;
    let on = enabled(id, slot);
    let busy = shell.app.list_job().is_some();
    let search = text_input(&i18n.tr("games-search-placeholder"), &slot.list.search)
        .id(SEARCH_ID)
        .on_input(|text| Message::Games(GameListMessage::SearchInput(text)))
        .on_submit(act(ListAction::Find))
        .padding(4)
        .size(13)
        .width(Length::FillPortion(3));
    let ignore_case = checkbox(slot.list.ignore_case)
        .label(i18n.tr("games-ignore-case"))
        .size(14)
        .text_size(12)
        .on_toggle(|on| Message::Games(GameListMessage::IgnoreCase(on)));
    let unless_busy = |m: Message| (!busy).then_some(m);
    let selection = |ok: bool, action: ListAction| (ok && !busy).then(|| act(action));
    let mut tools = row![
        tool_button(
            i18n,
            Icon::Search,
            "games-tip-find",
            unless_busy(act(ListAction::Find))
        ),
        tool_button(
            i18n,
            Icon::FilterList,
            "games-tip-filter",
            unless_busy(act(ListAction::FilterText))
        ),
        tool_button(
            i18n,
            Icon::ResetFilter,
            "games-tip-reset",
            unless_busy(act(ListAction::Reset))
        ),
        tool_button(
            i18n,
            Icon::Negate,
            "games-tip-negate",
            unless_busy(act(ListAction::Negate))
        ),
        tool_button(
            i18n,
            Icon::RemoveFromFilter,
            "games-tip-remove",
            selection(on.selection, ListAction::RemoveSelected)
        ),
        tool_button(
            i18n,
            Icon::KeepSelected,
            "games-tip-keep",
            selection(on.selection, ListAction::Keep)
        ),
        tool_button(
            i18n,
            Icon::Copy,
            "games-tip-copy",
            selection(on.copy, ListAction::CopySelected)
        ),
        tool_button(
            i18n,
            Icon::Trash,
            "games-tip-delete",
            selection(on.flag, ListAction::ToggleDeleted)
        ),
    ]
    .spacing(2)
    .align_y(Alignment::Center);
    // The columns button opens its list under the button; the window point
    // is not known here, so it opens at the top-right of the panel.
    let columns_at = Point::new(
        (shell.ui.window_size.map_or(DEFAULT_WIDTH, |s| s.width) - MENU_WIDTH - 24.0).max(8.0),
        shell.ui.window_size.map_or(300.0, |s| s.height * 0.4),
    );
    tools = tools.push(tool_button(
        i18n,
        Icon::Columns,
        "games-tip-columns",
        Some(Message::Games(GameListMessage::ColumnsButton(columns_at))),
    ));
    tools = tools.push(tool_button(
        i18n,
        Icon::Browse,
        "games-tip-browse",
        // Without a selection it browses the game being edited, if the
        // slot has one (D615); never an empty filter.
        (slot.filtered() > 0
            && (on.selection
                || slot.list.selected.is_some()
                || slot.current_game.is_some_and(|n| n > 0)))
        .then(|| act(ListAction::Browse))
        .filter(|_| !busy),
    ));
    let mut counter_args = FluentArgs::new();
    counter_args.set("filtered", i64::from(slot.filtered()));
    counter_args.set("total", i64::from(slot.total()));
    let mut info = row![
        text(i18n.tr_args("games-counter", &counter_args))
            .size(12)
            .style(theme::muted_text)
    ]
    .spacing(8)
    .align_y(Alignment::Center);
    if !slot.list.selection.is_empty() {
        let mut args = FluentArgs::new();
        args.set(
            "count",
            i64::try_from(slot.list.selection.len()).unwrap_or(i64::MAX),
        );
        info = info.push(text(i18n.tr_args("games-selected", &args)).size(12));
    }
    let tail: Element<'a, Message> = job_view(shell).unwrap_or_else(|| info.into());
    container(
        row![search, ignore_case, tools, space::horizontal(), tail]
            .spacing(8)
            .align_y(Alignment::Center),
    )
    .id("glist-toolbar")
    .padding([4, 8])
    .into()
}
