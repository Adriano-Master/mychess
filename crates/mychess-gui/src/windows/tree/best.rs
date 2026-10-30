//! The best games section of the tree panel (spec M5-2, D1600-D1612): the
//! list of the games that reach the position, with its choices, columns,
//! selection and the load, browse and copy buttons.

use iced::widget::{
    button, checkbox, column, container, mouse_area, pick_list, row, scrollable, text,
};
use iced::{Alignment, Element, Length};
use mychess_app::{
    BEST_MAX_CHOICES, BestColumn, BestMessage, BestOrder, BestResults, BestRow, BestView,
    ListColumn, Message as AppMessage, PickMode, TreeMessage,
};

use crate::i18n::I18n;
use crate::shell::Message;
use crate::theme::{self, ButtonLook, RowLook};
use crate::windows::game_list::Column;
use crate::windows::header_dialog::Labeled;

/// Text size of the list.
const LIST_TEXT: f32 = 12.0;
/// Space between the cells.
const SPACING: f32 = 8.0;

fn best_message(message: BestMessage) -> Message {
    Message::App(AppMessage::Tree(TreeMessage::Best(message)))
}

/// The width of a column, in logical pixels.
fn width_of(column: BestColumn) -> f32 {
    match column {
        BestColumn::Number => 52.0,
        BestColumn::White | BestColumn::Black => 130.0,
        BestColumn::WhiteElo | BestColumn::BlackElo | BestColumn::Length | BestColumn::Eco => 44.0,
        BestColumn::Result => 48.0,
        BestColumn::Date => 76.0,
        BestColumn::Event => 120.0,
        BestColumn::Moves => 220.0,
    }
}

fn title_key(column: BestColumn) -> &'static str {
    match column {
        BestColumn::Number => "games-col-number",
        BestColumn::White => "games-col-white",
        BestColumn::WhiteElo => "games-col-white-elo",
        BestColumn::Black => "games-col-black",
        BestColumn::BlackElo => "games-col-black-elo",
        BestColumn::Result => "games-col-result",
        BestColumn::Length => "games-col-length",
        BestColumn::Date => "games-col-date",
        BestColumn::Event => "games-col-event",
        BestColumn::Eco => "games-col-eco",
        BestColumn::Moves => "tree-best-col-moves",
    }
}

fn list_column(column: BestColumn) -> Option<ListColumn> {
    Some(match column {
        BestColumn::Number => ListColumn::Number,
        BestColumn::White => ListColumn::White,
        BestColumn::WhiteElo => ListColumn::WhiteElo,
        BestColumn::Black => ListColumn::Black,
        BestColumn::BlackElo => ListColumn::BlackElo,
        BestColumn::Result => ListColumn::Result,
        BestColumn::Length => ListColumn::Length,
        BestColumn::Date => ListColumn::Date,
        BestColumn::Event => ListColumn::Event,
        BestColumn::Eco => ListColumn::Eco,
        BestColumn::Moves => return None,
    })
}

/// The text of a cell.
pub(super) fn cell_text(column: BestColumn, row: &BestRow) -> String {
    match list_column(column) {
        Some(list) => Column::of(list).cell(&row.game),
        None => row.next_moves.join(" "),
    }
}

fn results_key(results: BestResults) -> &'static str {
    match results {
        BestResults::All => "tree-best-results-all",
        BestResults::White => "tree-best-results-white",
        BestResults::Black => "tree-best-results-black",
        BestResults::Decisive => "tree-best-results-decisive",
        BestResults::Draw => "tree-best-results-draw",
        BestResults::Unfinished => "tree-best-results-unfinished",
    }
}

fn order_key(order: BestOrder) -> &'static str {
    match order {
        BestOrder::Rating => "tree-best-order-rating",
        BestOrder::Database => "tree-best-order-database",
    }
}

fn cell<'a>(content: String, width: f32) -> Element<'a, Message> {
    text(content)
        .size(LIST_TEXT)
        .width(Length::Fixed(width))
        .wrapping(text::Wrapping::None)
        .into()
}

fn muted<'a>(content: String) -> Element<'a, Message> {
    text(content)
        .size(LIST_TEXT)
        .style(theme::muted_text)
        .into()
}

/// The "result", "sort" and "max" labels with their pick lists.
fn pickers<'a>(best: &BestView, i18n: &I18n) -> Vec<Element<'a, Message>> {
    let results: Vec<Labeled<BestResults>> = BestResults::ALL
        .into_iter()
        .map(|value| Labeled {
            value,
            label: i18n.tr(results_key(value)),
        })
        .collect();
    let orders: Vec<Labeled<BestOrder>> = BestOrder::ALL
        .into_iter()
        .map(|value| Labeled {
            value,
            label: i18n.tr(order_key(value)),
        })
        .collect();
    let maxes: Vec<Labeled<u32>> = BEST_MAX_CHOICES
        .into_iter()
        .map(|value| Labeled {
            value,
            label: value.to_string(),
        })
        .collect();
    vec![
        muted(i18n.tr("tree-best-result")),
        pick_list(
            results,
            Some(Labeled {
                value: best.results,
                label: i18n.tr(results_key(best.results)),
            }),
            |c: Labeled<BestResults>| best_message(BestMessage::SetResults(c.value)),
        )
        .text_size(LIST_TEXT)
        .padding(3)
        .into(),
        muted(i18n.tr("tree-best-sort")),
        pick_list(
            orders,
            Some(Labeled {
                value: best.order,
                label: i18n.tr(order_key(best.order)),
            }),
            |c: Labeled<BestOrder>| best_message(BestMessage::SetOrder(c.value)),
        )
        .text_size(LIST_TEXT)
        .padding(3)
        .into(),
        muted(i18n.tr("tree-best-max")),
        pick_list(
            maxes,
            Some(Labeled {
                value: best.max,
                label: best.max.to_string(),
            }),
            |c: Labeled<u32>| best_message(BestMessage::SetMax(c.value)),
        )
        .text_size(LIST_TEXT)
        .padding(3)
        .into(),
    ]
}

fn controls<'a>(best: &BestView, i18n: &I18n, can_merge: bool) -> Element<'a, Message> {
    let some = !best.selected.is_empty();
    let action = |key: &str, message: BestMessage, enabled: bool| {
        button(text(i18n.tr(key)).size(LIST_TEXT))
            .padding([3, 8])
            .style(theme::command_button(ButtonLook::Secondary, false))
            .on_press_maybe(enabled.then(|| best_message(message)))
    };
    let first = best
        .rows
        .iter()
        .find(|r| best.selected.contains(&r.game.number));
    let load = action(
        "tree-best-load",
        BestMessage::Load(first.map_or(0, |r| r.game.number)),
        first.is_some(),
    );
    // Spec M7-6d: "Merge" takes the first selected game into the game of
    // the editor (the preview dialog opens).
    let merge_button = button(text(i18n.tr("merge-button")).size(LIST_TEXT))
        .padding([3, 8])
        .style(theme::command_button(ButtonLook::Secondary, false))
        .on_press_maybe(
            best.slot
                .filter(|_| can_merge)
                .zip(first)
                .map(|(slot, row)| {
                    Message::App(crate::windows::merge_dialog::open_message_app(
                        slot,
                        row.game.number,
                    ))
                }),
        );
    let mut items = pickers(best, i18n);
    items.extend([
        action("tree-best-columns", BestMessage::ToggleColumnMenu, true).into(),
        load.into(),
        merge_button.into(),
        action("tree-best-browse", BestMessage::Browse, some).into(),
        action(
            "tree-best-copy",
            BestMessage::Copy,
            some && !best.in_clipbase,
        )
        .into(),
    ]);
    row::Row::with_children(items)
        .spacing(SPACING)
        .align_y(Alignment::Center)
        .wrap()
        .into()
}

fn column_boxes<'a>(best: &BestView, i18n: &I18n) -> Element<'a, Message> {
    let items: Vec<Element<'a, Message>> = BestColumn::ALL
        .into_iter()
        .map(|c| {
            checkbox(best.columns.contains(&c))
                .label(i18n.tr(title_key(c)))
                .size(14)
                .text_size(LIST_TEXT)
                .on_toggle(move |_| best_message(BestMessage::ToggleColumn(c)))
                .into()
        })
        .collect();
    row::Row::with_children(items)
        .spacing(SPACING)
        .wrap()
        .into()
}

fn header<'a>(best: &BestView, i18n: &I18n) -> Element<'a, Message> {
    let cells: Vec<Element<'a, Message>> = best
        .columns
        .iter()
        .map(|c| cell(i18n.tr(title_key(*c)), width_of(*c)))
        .collect();
    container(row::Row::with_children(cells).spacing(SPACING))
        .padding([4, 8])
        .style(theme::list_header)
        .into()
}

fn data_row<'a>(
    best: &BestView,
    row_data: &BestRow,
    index: usize,
    dimmed: bool,
    mods: (bool, bool),
) -> Element<'a, Message> {
    let number = row_data.game.number;
    let cells: Vec<Element<'a, Message>> = best
        .columns
        .iter()
        .map(|c| cell(cell_text(*c, row_data), width_of(*c)))
        .collect();
    let body = container(row::Row::with_children(cells).spacing(SPACING))
        .id(format!("tree-best-row-{number}"))
        .padding([2, 8])
        .style(theme::list_row(RowLook {
            zebra: index % 2 == 1,
            selected: best.selected.contains(&number),
            current: false,
            dimmed,
        }));
    let (shift, command) = mods;
    let mode = if shift {
        PickMode::Range
    } else if command {
        PickMode::Toggle
    } else {
        PickMode::Only
    };
    mouse_area(body)
        .on_press(best_message(BestMessage::Pick { number, mode }))
        .on_double_click(best_message(BestMessage::Load(number)))
        .into()
}

/// The section for `best`; `mods` are the `(shift, command)` keys held,
/// `searching` greys the list while a new one is on its way.
pub(super) fn section<'a>(
    best: &BestView,
    i18n: &I18n,
    searching: bool,
    mods: (bool, bool),
    can_merge: bool,
) -> Element<'a, Message> {
    let mut page = column![controls(best, i18n, can_merge)].spacing(4);
    if best.columns_open {
        page = page.push(column_boxes(best, i18n));
    }
    if best.rows.is_empty() {
        let key = if searching {
            "tree-loading"
        } else {
            "tree-best-empty"
        };
        page = page.push(muted(i18n.tr(key)));
    } else {
        let rows = column(
            best.rows
                .iter()
                .enumerate()
                .map(|(i, r)| data_row(best, r, i, searching, mods)),
        );
        page = page.push(header(best, i18n)).push(
            scrollable(scrollable(rows).width(Length::Shrink))
                .direction(scrollable::Direction::Horizontal(
                    scrollable::Scrollbar::default(),
                ))
                .width(Length::Fill)
                .height(Length::Fill),
        );
    }
    page.height(Length::Fill).into()
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use mychess_app::GameRow;

    use super::*;

    fn row() -> BestRow {
        BestRow {
            game: {
                let mut g = GameRow::unreadable(7);
                g.unreadable = false;
                g
            },
            avg_elo: 2500,
            ply: 4,
            next_moves: vec!["Nf3".to_owned(), "Nc6".to_owned()],
        }
    }

    #[test]
    fn the_moves_column_joins_the_san() {
        assert_eq!(cell_text(BestColumn::Moves, &row()), "Nf3 Nc6");
        assert_eq!(cell_text(BestColumn::Number, &row()), "7");
    }

    #[test]
    #[allow(clippy::assert_is_empty)] // `title_key` returns a `&str`
    fn every_column_has_a_width_and_a_title() {
        for c in BestColumn::ALL {
            assert!(width_of(c) > 0.0);
            assert!(!title_key(c).is_empty());
        }
    }

    #[test]
    fn the_section_builds_for_an_empty_and_a_full_list() {
        let i18n = I18n::new(crate::i18n::Language::EnUs);
        let mut view = BestView {
            columns: BestColumn::defaults(),
            columns_open: true,
            results: BestResults::All,
            order: BestOrder::Rating,
            max: 100,
            rows: Arc::new(Vec::new()),
            selected: std::collections::BTreeSet::new(),
            in_clipbase: false,
            slot: None,
        };
        drop(section(&view, &i18n, false, (false, false), false));
        view.rows = Arc::new(vec![row()]);
        drop(section(&view, &i18n, true, (true, false), false));
    }

    #[test]
    fn merge_takes_the_game_from_the_database_of_the_rows() {
        let i18n = I18n::new(crate::i18n::Language::EnUs);
        // The tree is locked on a database other than the current one.
        let locked = mychess_app::SlotId::CLIPBASE;
        let view = BestView {
            columns: BestColumn::defaults(),
            columns_open: false,
            results: BestResults::All,
            order: BestOrder::Rating,
            max: 100,
            rows: Arc::new(vec![row()]),
            selected: std::collections::BTreeSet::from([7]),
            in_clipbase: true,
            slot: Some(locked),
        };
        let element = section(&view, &i18n, false, (false, false), true);
        let mut sim = iced_test::Simulator::with_size(
            iced::Settings::default(),
            iced::Size::new(1200.0, 600.0),
            element,
        );
        sim.click(i18n.tr("merge-button").as_str())
            .expect("the button");
        let sent = format!("{:?}", sim.into_messages().collect::<Vec<_>>());
        assert!(
            sent.contains("Merge") && sent.contains(&format!("slot: {locked:?}")),
            "{sent}"
        );
        // Without the permission of the editor the button does nothing.
        let element = section(&view, &i18n, false, (false, false), false);
        let mut sim = iced_test::Simulator::with_size(
            iced::Settings::default(),
            iced::Size::new(1200.0, 600.0),
            element,
        );
        sim.click(i18n.tr("merge-button").as_str())
            .expect("the button");
        assert_eq!(sim.into_messages().count(), 0);
    }
}
