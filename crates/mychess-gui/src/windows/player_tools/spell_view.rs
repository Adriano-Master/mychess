//! The name checker and "Add Elo ratings" (spec M6-3, sections 5 and 6,
//! D3325): a list of corrections with a tick, the editable correct name, the
//! number of games and the years of life; the confirmation is part of the
//! window.

use fluent_bundle::FluentArgs;
use iced::widget::{button, column, container, progress_bar, row, text, text_input, tooltip};
use iced::{Alignment, Element, Length, Size};
use mychess_app::{NameKind, SpellView, SpellingMessage};

use super::table;
use super::{
    ADD_ELO_BOX_ID, ListId, PlayerToolsUi, SPELL_BOX_ID, app, card_width, list_height,
    text as texts,
};
use crate::i18n::I18n;
use crate::shell::Message;
use crate::theme;
use crate::widgets::icon::{Icon, IconTone, icon};
use crate::widgets::modal::{Card, modal};
use crate::windows::annotate_dialog::check;

/// Id of the "Close" button of the name checker.
pub const SPELL_CLOSE_ID: &str = "spell-check-close";
/// Id of the "Stop" button of the name checker.
pub const SPELL_STOP_ID: &str = "spell-check-stop";
/// Id of the "Make corrections" button.
pub const SPELL_APPLY_ID: &str = "spell-check-apply";
/// Id of the "yes" of the confirmation.
pub const SPELL_CONFIRM_YES_ID: &str = "spell-check-confirm-yes";
/// Id of the "no" of the confirmation.
pub const SPELL_CONFIRM_NO_ID: &str = "spell-check-confirm-no";
/// Id of the search field of the list.
pub const SPELL_FIND_ID: &str = "spell-check-find";
/// Id of the box "Ambiguous".
pub const SPELL_AMBIGUOUS_ID: &str = "spell-check-ambiguous";
/// Id of the box "Surnames".
pub const SPELL_SURNAMES_ID: &str = "spell-check-surnames";
/// Id of the button "Tick all".
pub const SPELL_SELECT_ALL_ID: &str = "spell-check-select-all";
/// Id of the button "Untick all".
pub const SPELL_SELECT_NONE_ID: &str = "spell-check-select-none";
/// Id of the button "Scan again".
pub const SPELL_RESCAN_ID: &str = "spell-check-rescan";
/// Id of the button "Copy as text".
pub const SPELL_COPY_ID: &str = "spell-check-copy";
/// Id of the "Close" button of "Add Elo".
pub const ELO_CLOSE_ID: &str = "add-elo-close";
/// Id of the "Check" button of "Add Elo".
pub const ELO_PLAN_ID: &str = "add-elo-plan";
/// Id of the "Add ratings" button.
pub const ELO_ADD_ID: &str = "add-elo-add";
/// Id of the box "Overwrite existing ratings".
pub const ELO_OVERWRITE_ID: &str = "add-elo-overwrite";
/// Id of the choice "All games".
pub const ELO_ALL_ID: &str = "add-elo-all";
/// Id of the choice "Games of the filter".
pub const ELO_FILTER_ID: &str = "add-elo-filter";

/// What the name checker spends besides the rows.
pub const CHROME: f32 = 420.0;
const SPELL_ROW: f32 = 30.0;
const APPLY_W: f32 = 70.0;
const GAMES_W: f32 = 70.0;
const LIFE_W: f32 = 140.0;
const MARK_W: f32 = 80.0;

fn spell(message: SpellingMessage) -> Message {
    app(mychess_app::Message::Spelling(message))
}

fn with_id<'a>(id: &'static str, body: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    container(body).id(id).into()
}

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
    button(
        row![icon(glyph, 14.0, tone), text(label).size(13)]
            .spacing(6)
            .align_y(Alignment::Center),
    )
    .padding([4, 10])
    .style(if primary {
        button::primary
    } else {
        button::secondary
    })
    .on_press_maybe(message)
}

fn small<'a>(
    label: String,
    on: bool,
    message: Option<Message>,
) -> iced::widget::Button<'a, Message> {
    button(text(label).size(12))
        .padding([2, 8])
        .style(theme::command_button(theme::ButtonLook::Secondary, on))
        .on_press_maybe(message)
}

fn bar<'a>(i18n: &I18n, key: &str, (done, total): (u64, u64)) -> Element<'a, Message> {
    let ratio = if total == 0 {
        0.0
    } else {
        // A fraction to draw: the precision of an `f32` is plenty.
        #[allow(clippy::cast_precision_loss)]
        let ratio = (done as f32 / total as f32).clamp(0.0, 1.0);
        ratio
    };
    let mut args = FluentArgs::new();
    args.set("done", i64::try_from(done).unwrap_or(i64::MAX));
    args.set("total", i64::try_from(total).unwrap_or(i64::MAX));
    row![
        progress_bar(0.0..=1.0, ratio).girth(8.0),
        text(format!(
            "{}: {}",
            i18n.tr(key),
            i18n.tr_args("ptools-progress", &args)
        ))
        .size(12),
    ]
    .spacing(8)
    .align_y(Alignment::Center)
    .into()
}

/// The id of the field of the new name of a row.
pub fn new_name_id(index: usize) -> iced::widget::Id {
    iced::widget::Id::from(format!("spell-check-new-{index}"))
}

fn spell_row<'a>(
    r: &mychess_app::SpellRowView,
    cursor: Option<usize>,
    position: usize,
    enabled: bool,
    mark: &str,
) -> Element<'a, Message> {
    let index = r.index;
    let row_data = &r.row;
    let tick = check(String::new(), row_data.selected, enabled, move |_| {
        spell(SpellingMessage::ToggleRow(index))
    });
    let new_name = text_input("", &row_data.new)
        .id(new_name_id(index))
        .size(13)
        .padding([1, 4])
        .width(Length::FillPortion(3))
        .on_input_maybe(
            enabled.then_some(move |t: String| spell(SpellingMessage::EditNew(index, t))),
        );
    let mark = if row_data.ambiguous {
        mark.to_owned()
    } else {
        String::new()
    };
    let body = row![
        container(tick).width(Length::Fixed(APPLY_W)),
        text(row_data.old.clone())
            .size(13)
            .width(Length::FillPortion(3)),
        new_name,
        text(row_data.games.to_string())
            .size(13)
            .width(Length::Fixed(GAMES_W)),
        text(texts::life(row_data.born, row_data.died))
            .size(13)
            .width(Length::Fixed(LIFE_W)),
        text(mark).size(13).width(Length::Fixed(MARK_W)),
    ]
    .spacing(8)
    .align_y(Alignment::Center);
    container(body)
        .height(Length::Fixed(SPELL_ROW))
        .padding([0, 6])
        .style(theme::list_row(theme::RowLook {
            zebra: position % 2 == 1,
            selected: cursor == Some(position),
            ..theme::RowLook::default()
        }))
        .into()
}

fn spell_header<'a>(i18n: &I18n) -> Element<'a, Message> {
    let cell = |key: &str, width: Length| table::header(i18n.tr(key), width, None, None);
    container(
        row![
            cell("spell-col-apply", Length::Fixed(APPLY_W)),
            cell("spell-col-old", Length::FillPortion(3)),
            cell("spell-col-new", Length::FillPortion(3)),
            cell("spell-col-games", Length::Fixed(GAMES_W)),
            cell("spell-col-life", Length::Fixed(LIFE_W)),
            cell("spell-ambiguous", Length::Fixed(MARK_W)),
        ]
        .spacing(8),
    )
    .padding([0, 6])
    .style(theme::list_header)
    .into()
}

fn tip(body: Element<'_, Message>, tip: String) -> Element<'_, Message> {
    tooltip(body, text(tip).size(12), tooltip::Position::Top)
        .style(theme::tooltip_box)
        .into()
}

fn options<'a>(v: &SpellView, i18n: &I18n) -> Element<'a, Message> {
    let busy = v.scanning.is_some() || v.writing.is_some();
    let mut line = row![with_id(
        SPELL_AMBIGUOUS_ID,
        tip(
            check(i18n.tr("spell-ambiguous"), v.ambiguous, !busy, |on| {
                spell(SpellingMessage::SetAmbiguous(on))
            }),
            i18n.tr("spell-ambiguous-tip")
        )
    ),]
    .spacing(14)
    .align_y(Alignment::Center);
    if v.kind == NameKind::Player {
        line = line.push(with_id(
            SPELL_SURNAMES_ID,
            tip(
                check(i18n.tr("spell-surnames"), v.surnames, !busy, |on| {
                    spell(SpellingMessage::SetSurnames(on))
                }),
                i18n.tr("spell-surnames-tip"),
            ),
        ));
    }
    line = line
        .push(
            text_input(&i18n.tr("spell-find-placeholder"), &v.find)
                .id(SPELL_FIND_ID)
                .size(13)
                .padding([2, 4])
                .width(Length::Fixed(220.0))
                .on_input(|t| spell(SpellingMessage::SetFind(t))),
        )
        .push(with_id(
            SPELL_SELECT_ALL_ID,
            small(
                i18n.tr("spell-select-all"),
                false,
                Some(spell(SpellingMessage::SelectAll(true))),
            ),
        ))
        .push(with_id(
            SPELL_SELECT_NONE_ID,
            small(
                i18n.tr("spell-select-none"),
                false,
                Some(spell(SpellingMessage::SelectAll(false))),
            ),
        ))
        .push(with_id(
            SPELL_COPY_ID,
            small(
                i18n.tr("spell-copy-text"),
                false,
                Some(spell(SpellingMessage::CopyText)),
            ),
        ))
        .push(with_id(
            SPELL_RESCAN_ID,
            small(
                i18n.tr("spell-rescan"),
                false,
                (!busy).then(|| spell(SpellingMessage::Rescan)),
            ),
        ));
    line.into()
}

fn spell_status<'a>(v: &SpellView, i18n: &I18n) -> Element<'a, Message> {
    if let Some(progress) = v.writing {
        return bar(i18n, "spell-writing", progress);
    }
    if let Some(progress) = v.scanning {
        return bar(i18n, "spell-scanning", progress);
    }
    let mut lines = column![].spacing(2);
    if v.confirm {
        lines = lines.push(text(i18n.tr("spell-confirm")).size(13).style(text::danger));
    }
    if !v.writable {
        lines = lines.push(
            text(i18n.tr("spell-read-only"))
                .size(13)
                .style(text::danger),
        );
    }
    if v.interrupted {
        lines = lines.push(text(i18n.tr("spell-partial")).size(13));
    } else if v.scanned && v.total == 0 {
        lines = lines.push(text(i18n.tr("spell-empty")).size(13));
    }
    if let Some(result) = &v.result {
        lines = lines.push(text(texts::spelling_notice_text(i18n, result)).size(13));
    }
    let mut args = FluentArgs::new();
    args.set("ticked", i64::try_from(v.ticked).unwrap_or(i64::MAX));
    args.set("total", i64::try_from(v.total).unwrap_or(i64::MAX));
    lines = lines.push(
        text(i18n.tr_args("spell-ticked", &args))
            .size(12)
            .style(theme::muted_text),
    );
    lines.into()
}

/// The name checker.
pub fn check_view<'a>(
    ui: &PlayerToolsUi,
    v: &SpellView,
    i18n: &I18n,
    window: Size,
) -> Element<'a, Message> {
    let height = list_height(window, CHROME);
    let enabled = v.writable && v.writing.is_none() && !v.confirm;
    let rows = v.rows.clone();
    let cursor = ui.spell.cursor;
    let mark = i18n.tr("spell-ambiguous-mark");
    let list = table::rows(
        ListId::Spell,
        rows.len(),
        ui.spell.scroll,
        height,
        SPELL_ROW,
        move |position| match rows.get(position) {
            Some(r) => spell_row(r, cursor, position, enabled, &mark),
            None => text("").into(),
        },
    );
    let mut found = FluentArgs::new();
    found.set("count", i64::try_from(v.total).unwrap_or(i64::MAX));
    found.set("kind", i18n.tr(texts::kind_key(v.kind)));
    let mut title = FluentArgs::new();
    title.set("kind", i18n.tr(texts::kind_key(v.kind)));
    let body = column![
        text(i18n.tr_args("spell-found", &found)).size(13),
        options(v, i18n),
        spell_header(i18n),
        list,
        spell_status(v, i18n),
    ]
    .spacing(8);
    let buttons: Vec<Element<'a, Message>> = if v.confirm {
        vec![
            with_id(
                SPELL_CONFIRM_NO_ID,
                command(
                    Icon::Close,
                    i18n.tr("dialog-cancel"),
                    false,
                    Some(spell(SpellingMessage::ConfirmApply(false))),
                ),
            ),
            with_id(
                SPELL_CONFIRM_YES_ID,
                command(
                    Icon::Save,
                    i18n.tr("spell-confirm-yes"),
                    true,
                    Some(spell(SpellingMessage::ConfirmApply(true))),
                ),
            ),
        ]
    } else {
        vec![
            with_id(
                SPELL_CLOSE_ID,
                command(
                    Icon::Close,
                    i18n.tr("ptools-close"),
                    false,
                    Some(spell(SpellingMessage::CloseCheck)),
                ),
            ),
            with_id(
                SPELL_STOP_ID,
                command(
                    Icon::Pause,
                    i18n.tr("ptools-stop"),
                    false,
                    v.scanning.map(|_| spell(SpellingMessage::CancelScan)),
                ),
            ),
            with_id(
                SPELL_APPLY_ID,
                command(
                    Icon::Save,
                    i18n.tr("spell-apply"),
                    true,
                    (v.writable && v.ticked > 0 && v.scanning.is_none() && v.writing.is_none())
                        .then(|| spell(SpellingMessage::Apply)),
                ),
            ),
        ]
    };
    modal(Card {
        id: SPELL_BOX_ID,
        title: i18n.tr_args("spell-title-kind", &title),
        body: body.into(),
        buttons,
        width: card_width(980.0, window),
    })
}

/// The dialog "Add Elo ratings".
pub fn add_elo_view<'a>(
    v: &mychess_app::AddEloView,
    i18n: &I18n,
    window: Size,
) -> Element<'a, Message> {
    let busy = v.planning.is_some() || v.writing.is_some();
    let free = !busy && v.result.is_none();
    let mut body = column![
        text(i18n.tr("spell-elo-description")).size(13),
        with_id(
            ELO_OVERWRITE_ID,
            check(
                i18n.tr("spell-elo-overwrite"),
                v.overwrite,
                free && v.planned.is_none(),
                |on| { spell(SpellingMessage::SetEloOverwrite(on)) }
            ),
        ),
        row![
            text(i18n.tr("spell-elo-scope")).size(13),
            with_id(
                ELO_ALL_ID,
                small(
                    i18n.tr("spell-elo-all"),
                    !v.only_filter,
                    (free && v.planned.is_none())
                        .then(|| spell(SpellingMessage::SetEloOnlyFilter(false))),
                ),
            ),
            with_id(
                ELO_FILTER_ID,
                small(
                    i18n.tr("spell-elo-filter"),
                    v.only_filter,
                    (free && v.planned.is_none())
                        .then(|| spell(SpellingMessage::SetEloOnlyFilter(true))),
                ),
            ),
        ]
        .spacing(10)
        .align_y(Alignment::Center),
    ]
    .spacing(12);
    if let Some(progress) = v.planning {
        body = body.push(bar(i18n, "spell-elo-planning", progress));
    }
    if let Some(progress) = v.writing {
        body = body.push(bar(i18n, "spell-elo-writing", progress));
    }
    if let Some((ratings, games)) = v.planned.filter(|_| v.result.is_none()) {
        let mut args = FluentArgs::new();
        args.set("ratings", i64::from(ratings));
        args.set("games", i64::from(games));
        body = body.push(text(i18n.tr_args("spell-elo-planned", &args)).size(13));
    }
    if !v.writable {
        body = body.push(
            text(i18n.tr("spell-read-only"))
                .size(13)
                .style(text::danger),
        );
    }
    if let Some(result) = &v.result {
        body = body.push(text(texts::spelling_notice_text(i18n, result)).size(13));
    }
    let mut buttons = vec![with_id(
        ELO_CLOSE_ID,
        command(
            Icon::Close,
            i18n.tr("ptools-close"),
            false,
            Some(spell(SpellingMessage::CloseAddElo)),
        ),
    )];
    if v.result.is_none() {
        buttons.push(with_id(
            ELO_PLAN_ID,
            command(
                Icon::Search,
                i18n.tr("spell-elo-plan"),
                v.planned.is_none(),
                (free && v.writable && v.planned.is_none())
                    .then(|| spell(SpellingMessage::PlanElo)),
            ),
        ));
        buttons.push(with_id(
            ELO_ADD_ID,
            command(
                Icon::Save,
                i18n.tr("spell-elo-add"),
                true,
                (!busy && v.writable && v.planned.is_some())
                    .then(|| spell(SpellingMessage::ConfirmElo)),
            ),
        ));
    }
    modal(Card {
        id: ADD_ELO_BOX_ID,
        title: i18n.tr("spell-add-elo"),
        body: body.into(),
        buttons,
        width: card_width(560.0, window),
    })
}

/// The height of a row, for the keys of the list.
pub const fn row_height() -> f32 {
    SPELL_ROW
}
