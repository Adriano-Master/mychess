//! The grid of small boards (spec M3-4e).

use fluent_bundle::FluentArgs;
use iced::widget::{column, container, mouse_area, row, scrollable, text};
use iced::{Alignment, Border, Element, Length};
use mychess_app::{BrowseMessage, BrowserGame, BrowserState};

use super::{
    BrowserMessage, CELL_MAX, CELL_MIN, Layer, act, board, loading_line, text_button, tool, window,
};
use crate::i18n::I18n;
use crate::shell::{Message, Shell};
use crate::theme::{self, Tokens};
use crate::widgets::icon::Icon;

pub(super) fn grid<'a>(shell: &'a Shell, browser: &'a BrowserState) -> Layer<'a> {
    let i18n = &shell.i18n;
    let win = window(shell);
    let cell = shell.ui.browser.cell;
    let columns = browser.columns();
    let mut lines = column![].spacing(10);
    for (line, chunk) in browser.games.chunks(columns).enumerate() {
        let mut cells = row![].spacing(10);
        for (offset, game) in chunk.iter().enumerate() {
            cells = cells.push(grid_cell(
                shell,
                browser,
                line * columns + offset,
                game,
                cell,
            ));
        }
        lines = lines.push(cells);
    }
    let cell_width = cell + 12.0 + 10.0;
    let mut width = f32::from(u16::try_from(columns).unwrap_or(7)) * cell_width + 56.0;
    width = width.clamp(480.0, (win.width - 40.0).max(480.0));
    let height = (win.height - 230.0).max(200.0);
    let mut body = column![].spacing(8);
    if browser.is_loading() {
        body = body.push(loading_line(shell, browser));
    }
    if browser.left_out > 0 {
        body = body.push(
            text(browser_limit_text(i18n, browser))
                .size(12)
                .style(theme::muted_text),
        );
    }
    body = body.push(
        scrollable(container(lines).padding([0, 8]))
            .width(Length::Fill)
            .height(Length::Fixed(height)),
    );
    let can_grow = cell < CELL_MAX;
    let can_shrink = cell > CELL_MIN;
    let buttons = vec![
        tool(
            i18n,
            Icon::ZoomOut,
            "browser-tip-smaller",
            can_shrink.then(|| Message::Browser(BrowserMessage::Resize(-1))),
            false,
        ),
        tool(
            i18n,
            Icon::ZoomIn,
            "browser-tip-larger",
            can_grow.then(|| Message::Browser(BrowserMessage::Resize(1))),
            false,
        ),
        text_button(
            i18n.tr("browser-close"),
            Some(act(BrowseMessage::Close)),
            true,
        ),
    ];
    (body.into(), buttons, width)
}

fn browser_limit_text(i18n: &I18n, browser: &BrowserState) -> String {
    let mut args = FluentArgs::new();
    let shown = browser.games.len();
    args.set("shown", i64::try_from(shown).unwrap_or(i64::MAX));
    args.set(
        "asked",
        i64::try_from(shown + browser.left_out).unwrap_or(i64::MAX),
    );
    i18n.tr_args("base-notice-browser-limit", &args)
}

fn grid_cell<'a>(
    shell: &'a Shell,
    browser: &'a BrowserState,
    index: usize,
    game: &'a BrowserGame,
    side: f32,
) -> Element<'a, Message> {
    let i18n = &shell.i18n;
    let focused = browser.focus == index;
    let mut label_args = FluentArgs::new();
    let label = if let Some(ready) = game.ready() {
        label_args.set("white", ready.header.white.as_str());
        label_args.set("black", ready.header.black.as_str());
        i18n.tr_args("browser-players", &label_args)
    } else {
        label_args.set("number", i64::from(game.number));
        i18n.tr_args("browser-game-line", &label_args)
    };
    let mut info = String::new();
    if let Some(ready) = game.ready() {
        info.push_str(ready.header.result.as_str());
    }
    if game.deleted {
        if !info.is_empty() {
            info.push_str("  ");
        }
        info.push_str(&i18n.tr("browser-deleted"));
    }
    let ok = game.ready().is_some();
    let tools = row![
        text(info)
            .size(11)
            .style(theme::muted_text)
            .width(Length::Fill),
        tool(
            i18n,
            Icon::Flip,
            "browser-tip-flip",
            Some(act(BrowseMessage::Flip { game: Some(index) })),
            false
        ),
        tool(
            i18n,
            Icon::Load,
            "browser-tip-load",
            ok.then(|| Message::Browser(BrowserMessage::Load {
                game: index,
                keep_open: None,
            })),
            false
        ),
        // Spec M7-6d: merge this game into the game of the editor.
        tool(
            i18n,
            Icon::Merge,
            "merge-action",
            (ok && crate::windows::merge_dialog::can_merge(&shell.app)).then(|| {
                Message::App(crate::windows::merge_dialog::open_message_app(
                    browser.slot,
                    game.number,
                ))
            }),
            false
        ),
    ]
    .align_y(Alignment::Center);
    let content = column![
        board(shell, index, game, side),
        text(label)
            .size(12)
            .width(Length::Fixed(side))
            .wrapping(text::Wrapping::None),
        container(tools).width(Length::Fixed(side)),
    ]
    .spacing(3);
    let framed = container(content)
        .padding(5)
        .style(move |theme: &iced::Theme| {
            let tokens = Tokens::of(theme);
            container::Style {
                border: Border {
                    color: if focused {
                        tokens.accent
                    } else {
                        iced::Color::TRANSPARENT
                    },
                    width: 2.0,
                    radius: tokens.radius.card.into(),
                },
                ..container::Style::default()
            }
        });
    mouse_area(framed)
        .on_enter(act(BrowseMessage::Focus(index)))
        .on_double_click(Message::Browser(BrowserMessage::Load {
            game: index,
            keep_open: None,
        }))
        .on_scroll(move |delta| Message::Browser(BrowserMessage::Wheel { game: index, delta }))
        .into()
}
