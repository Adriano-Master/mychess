//! The ECO browser (spec M5-7b, D1700-D1709): a bar chart of the games by
//! ECO code with the results, the openings of the book under the node and a
//! search by name. The application keeps everything (`AppState::eco_browser_view`);
//! this module draws it and sends [`EcoMessage`]s.
//!
//! The chart follows `eco.tcl`: a bar for every child of the node (`A`-`E`,
//! `B0`-`B9`, `B90a`-`B90z`), clicking one goes down, the bars show White's
//! wins, the draws and Black's wins, and a grid line marks every
//! `bar_scale` games. The list is not virtualized:
//! the application cuts it at 500 rows.

use fluent_bundle::FluentArgs;
use iced::widget::{
    button, column, container, mouse_area, row, scrollable, space, stack, text, text_input,
};
use iced::{Alignment, Element, Length, Theme};
use mychess_app::{
    AppState, BAR_UNITS, BrowserScope, EcoBar, EcoBrowserView, EcoLevel, EcoMessage,
    Message as AppMessage,
};

use super::eco_text::slot_label;
use crate::i18n::I18n;
use crate::shell::Message;
use crate::theme::tree_colors::TreeBarColors;
use crate::theme::{self, ButtonLook, RowLook, Tokens};
use crate::widgets::modal::{Card, modal};

/// Id of the box.
pub const ECO_BROWSER_BOX_ID: &str = "eco-browser-box";
/// Id of the search field.
pub const ECO_FIND_ID: &str = "eco-browser-find";

/// Height of the chart, in logical pixels.
const CHART_HEIGHT: f32 = 170.0;
/// Height of the list of openings.
const LIST_HEIGHT: f32 = 190.0;
/// Width of the code column of the list.
const CODE_WIDTH: f32 = 64.0;

fn send(message: EcoMessage) -> Message {
    Message::App(AppMessage::Eco(message))
}

/// The portion of a fill that stands for `units` out of [`BAR_UNITS`].
fn portion(units: u16) -> Length {
    Length::FillPortion(units.max(1))
}

fn colored(fill: iced::Color, outline: iced::Color) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: Some(fill.into()),
        border: iced::Border {
            color: outline,
            width: 1.0,
            radius: 0.0.into(),
        },
        ..container::Style::default()
    }
}

/// The column of one bar: an empty top, then the Black part, the draws and
/// the White part from the top down.
fn bar_body<'a>(bar: &EcoBar) -> Element<'a, Message> {
    if bar.height == 0 {
        return space().height(Length::Fill).into();
    }
    let [white, draw, black] = bar.parts;
    let mut parts = column![].width(Length::Fill).height(portion(bar.height));
    for (units, pick) in [(black, 2), (draw, 1), (white, 0)] {
        if units == 0 {
            continue;
        }
        let part = container(space())
            .width(Length::Fill)
            .height(portion(units))
            .style(move |theme: &Theme| {
                let colors = TreeBarColors::of(&Tokens::of(theme));
                let fill = [colors.white, colors.grey, colors.black][pick];
                colored(fill, colors.outline)(theme)
            });
        parts = parts.push(part);
    }
    column![
        space().height(portion(BAR_UNITS.saturating_sub(bar.height))),
        parts
    ]
    .height(Length::Fill)
    .into()
}

/// The horizontal lines of the grid, behind the bars.
fn grid<'a>(view: &EcoBrowserView<'_>) -> Element<'a, Message> {
    let mut lines = column![].height(Length::Fill).width(Length::Fill);
    let mut above = BAR_UNITS;
    for line in view.grid.iter().rev() {
        lines = lines.push(space().height(portion(above.saturating_sub(line.height))));
        lines = lines.push(
            container(space())
                .width(Length::Fill)
                .height(Length::Fixed(1.0))
                .style(theme::divider),
        );
        above = line.height;
    }
    lines.push(space().height(portion(above))).into()
}

/// The bar itself, over the height of the chart.
fn bar_top<'a>(bar: &EcoBar) -> Element<'a, Message> {
    mouse_area(
        container(bar_body(bar))
            .height(Length::Fixed(CHART_HEIGHT))
            .width(Length::Fill),
    )
    .on_press(send(EcoMessage::BrowseTo(bar.prefix.clone())))
    .into()
}

/// The count and the label under the bar.
fn bar_caption<'a>(bar: &EcoBar) -> Element<'a, Message> {
    let body = column![
        text(bar.stats.count.to_string()).size(11),
        text(bar.label.clone()).size(12),
    ]
    .align_x(Alignment::Center)
    .width(Length::Fill);
    mouse_area(body)
        .on_press(send(EcoMessage::BrowseTo(bar.prefix.clone())))
        .into()
}

fn chart<'a>(view: &EcoBrowserView<'_>, i18n: &I18n) -> Element<'a, Message> {
    if view.bars.is_empty() {
        return container(
            text(i18n.tr("eco-browser-no-bars"))
                .size(12)
                .style(theme::muted_text),
        )
        .height(Length::Fixed(CHART_HEIGHT))
        .center_y(Length::Fixed(CHART_HEIGHT))
        .width(Length::Fill)
        .into();
    }
    let bars = row(view.bars.iter().map(bar_top))
        .spacing(4)
        .width(Length::Fill);
    let captions = row(view.bars.iter().map(bar_caption))
        .spacing(4)
        .width(Length::Fill);
    // The grid sits behind the bars; only this stack has the fixed height,
    // so the counts and labels under it keep their own size (D1742).
    column![
        stack![
            container(grid(view)).height(Length::Fixed(CHART_HEIGHT)),
            bars
        ]
        .width(Length::Fill)
        .height(Length::Fixed(CHART_HEIGHT)),
        captions,
    ]
    .spacing(2)
    .width(Length::Fill)
    .into()
}

/// The title of the node: all sections, a section or a code.
fn node_title(view: &EcoBrowserView<'_>, i18n: &I18n) -> String {
    let mut args = FluentArgs::new();
    args.set("code", view.prefix);
    i18n.tr_args(
        match view.level {
            EcoLevel::All => "eco-browser-all",
            EcoLevel::Section => "eco-browser-section",
            EcoLevel::Code => "eco-browser-code",
        },
        &args,
    )
}

/// `1234 games: +500 =400 -334 (55.5%)`.
fn node_summary(view: &EcoBrowserView<'_>, i18n: &I18n) -> String {
    let stats = &view.stats;
    let mut args = FluentArgs::new();
    args.set("games", i64::from(stats.count));
    args.set("white", i64::from(stats.white));
    args.set("draw", i64::from(stats.draw + stats.none));
    args.set("black", i64::from(stats.black));
    let tenths = stats.score_tenths();
    args.set("whole", i64::from(tenths / 10));
    args.set("tenth", i64::from(tenths % 10));
    i18n.tr_args("eco-browser-summary", &args)
}

fn entry_row<'a>(
    index: usize,
    entry: &mychess_app::EcoEntryRow,
    zebra: bool,
) -> Element<'a, Message> {
    let cells = row![
        text(entry.code.clone())
            .size(12)
            .width(Length::Fixed(CODE_WIDTH)),
        text(entry.name.clone())
            .size(12)
            .width(Length::FillPortion(3)),
        text(entry.moves.clone())
            .size(12)
            .width(Length::FillPortion(4))
            .style(theme::muted_text),
    ]
    .spacing(8);
    mouse_area(
        container(cells)
            .padding([2, 8])
            .width(Length::Fill)
            .style(theme::list_row(RowLook {
                zebra,
                ..RowLook::default()
            })),
    )
    .on_press(send(EcoMessage::ImportLine(index)))
    .into()
}

fn entries<'a>(view: &EcoBrowserView<'_>, i18n: &I18n) -> Element<'a, Message> {
    let mut list = column![].width(Length::Fill);
    if !view.book_loaded {
        list = list.push(
            container(
                column![
                    text(i18n.tr("eco-browser-no-book")).size(12),
                    button(text(i18n.tr("eco-browser-load-file")).size(13))
                        .style(button::secondary)
                        .on_press(send(EcoMessage::LoadBookFile)),
                ]
                .spacing(6),
            )
            .padding(8),
        );
    } else if view.entries.is_empty() {
        list = list.push(
            container(
                text(i18n.tr("eco-browser-no-entries"))
                    .size(12)
                    .style(theme::muted_text),
            )
            .padding(8),
        );
    }
    for (index, entry) in view.entries.iter().enumerate() {
        list = list.push(entry_row(index, entry, index % 2 == 1));
    }
    if view.truncated {
        let mut args = FluentArgs::new();
        args.set("count", i64::try_from(view.entries.len()).unwrap_or(0));
        list = list.push(
            container(
                text(i18n.tr_args("eco-browser-truncated", &args))
                    .size(12)
                    .style(theme::muted_text),
            )
            .padding(8),
        );
    }
    scrollable(list).height(Length::Fixed(LIST_HEIGHT)).into()
}

fn scope_buttons<'a>(view: &EcoBrowserView<'_>, i18n: &I18n) -> Element<'a, Message> {
    let one = |label: &str, scope: BrowserScope, enabled: bool| {
        button(text(i18n.tr(label)).size(13))
            .padding([3, 8])
            .style(theme::command_button(
                ButtonLook::Secondary,
                view.scope == scope,
            ))
            .on_press_maybe(enabled.then(|| send(EcoMessage::SetScope(scope))))
    };
    row![
        one("eco-browser-scope-all", BrowserScope::WholeBase, true),
        one(
            "eco-browser-scope-filter",
            BrowserScope::Filter,
            view.filter_active || view.scope == BrowserScope::Filter,
        ),
    ]
    .spacing(4)
    .into()
}

/// The box, when the application has the browser open.
pub fn view<'a>(app: &AppState, i18n: &I18n) -> Option<Element<'a, Message>> {
    let view = app.eco_browser_view()?;
    Some(browser_box(app, &view, i18n))
}

fn browser_box<'a>(app: &AppState, view: &EcoBrowserView<'_>, i18n: &I18n) -> Element<'a, Message> {
    let mut base = FluentArgs::new();
    base.set("name", slot_label(app, i18n, view.slot));
    base.set("classified", i64::from(view.classified));
    base.set("total", i64::from(view.total_games));
    let header = column![
        text(node_title(view, i18n)).size(15),
        text(if view.building {
            i18n.tr("eco-browser-counting")
        } else {
            node_summary(view, i18n)
        })
        .size(13),
        text(i18n.tr_args("eco-browser-base", &base))
            .size(12)
            .style(theme::muted_text),
    ]
    .spacing(2)
    .align_x(Alignment::Center)
    .width(Length::Fill);

    let list_title = if let Some(found) = view.finding {
        let mut args = FluentArgs::new();
        args.set("text", found);
        i18n.tr_args("eco-browser-found", &args)
    } else {
        i18n.tr(if view.prefix.is_empty() {
            "eco-browser-list-all"
        } else {
            "eco-browser-list-node"
        })
    };

    let find = text_input(&i18n.tr("eco-browser-search-placeholder"), view.find)
        .id(ECO_FIND_ID)
        .on_input(|t| send(EcoMessage::SetFind(t)))
        .on_submit(send(EcoMessage::SubmitFind))
        .padding(5)
        .width(Length::Fill);
    let tools = row![
        button(text(i18n.tr("eco-browser-up")).size(13))
            .style(button::secondary)
            .on_press_maybe((!view.prefix.is_empty()).then(|| send(EcoMessage::BrowseUp))),
        find,
        button(text(i18n.tr("eco-browser-search")).size(13))
            .style(button::secondary)
            .on_press(send(EcoMessage::SubmitFind)),
    ]
    .spacing(6)
    .align_y(Alignment::Center);

    let body = column![
        header,
        chart(view, i18n),
        text(list_title).size(12).style(theme::muted_text),
        entries(view, i18n),
        tools,
        scope_buttons(view, i18n),
    ]
    .spacing(8);
    modal(Card {
        id: ECO_BROWSER_BOX_ID,
        title: i18n.tr("eco-browser-title"),
        body: body.into(),
        buttons: vec![
            button(text(i18n.tr("eco-close")))
                .on_press(send(EcoMessage::CloseBrowser))
                .into(),
        ],
        width: 820.0,
    })
}
