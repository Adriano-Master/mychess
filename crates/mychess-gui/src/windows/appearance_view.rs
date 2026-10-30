//! The appearance window: tabs on the left, the preview and the choices on
//! the right, Cancel and Apply at the bottom (spec M9-3).

use iced::widget::{
    Space, button, checkbox, column, container, opaque, responsive, row, scrollable, text,
};
use iced::{Alignment, Element, Length, Size, Theme};

use crate::app_api::MaterialMode;
use crate::i18n::I18n;
use crate::shell::Message;
use crate::theme::board_theme::{BoardTheme, BoardThemeId};
use crate::theme::{self, AccentId, ButtonLook, ThemeMode};
use crate::widgets::board::geometry::CoordsMode;
use crate::widgets::board::pieces::PieceSetId;
use crate::widgets::icon::{IconTone, icon};
use crate::windows::appearance::{AppearanceMessage, AppearanceTab, AppearanceUi, PlayerSwitch};
use crate::windows::appearance_preview as preview;
use crate::windows::dialog_ui::DialogMessage;

/// Id of the window (the focus scope and the handle for tests).
pub const APPEARANCE_BOX_ID: &str = "appearance-box";
/// Id of the preview area.
pub const PREVIEW_ID: &str = "appearance-preview";

/// Largest size of the window, in logical pixels.
const MAX_SIZE: Size = Size::new(880.0, 600.0);
/// The window takes at most this share of the application window.
const SHARE: f32 = 0.9;
/// Width of the column of tabs.
const SIDEBAR_WIDTH: f32 = 190.0;
const PAD: f32 = 16.0;
const FOOTER_HEIGHT: f32 = 48.0;
const BOARD_CARD: f32 = 96.0;
const PIECE_CARD: f32 = 128.0;
const CARD_GAP: f32 = 8.0;
const SWATCH: f32 = 26.0;

fn send(message: AppearanceMessage) -> Message {
    Message::Dialog(DialogMessage::Appearance(message))
}

/// The size of the window for an application window of `window`.
pub fn box_size(window: Size) -> Size {
    Size::new(
        (window.width * SHARE).min(MAX_SIZE.width),
        (window.height * SHARE).min(MAX_SIZE.height),
    )
}

/// The layer for the open window. `system` is what the operating system
/// reports (the "System" mode of the draft follows it).
pub fn view<'a>(
    ui: &'a AppearanceUi,
    system: iced::theme::Mode,
    i18n: &'a I18n,
) -> Element<'a, Message> {
    responsive(move |window| layer(ui, system, i18n, window)).into()
}

fn tagged<'a>(id: &'static str, content: Element<'a, Message>) -> Element<'a, Message> {
    container(content).id(id).into()
}

fn layer<'a>(
    ui: &'a AppearanceUi,
    system: iced::theme::Mode,
    i18n: &'a I18n,
    window: Size,
) -> Element<'a, Message> {
    let size = box_size(window);
    let content_width = size.width - SIDEBAR_WIDTH - 3.0 * PAD;
    let content_height = size.height - FOOTER_HEIGHT - 2.0 * PAD;
    let preview_height = (content_height * 0.42).clamp(120.0, 240.0);
    let body = column![
        tagged(
            PREVIEW_ID,
            preview_area(ui, system, i18n, content_width, preview_height)
        ),
        scrollable(
            container(choices(ui, system, i18n, content_width - 12.0)).padding(iced::Padding {
                right: 12.0,
                ..iced::Padding::ZERO
            })
        )
        .width(Length::Fill)
        .height(Length::Fill),
    ]
    .spacing(PAD)
    .height(Length::Fill);
    let footer = row![
        text(i18n.tr("appearance-footer-hint"))
            .size(12)
            .style(theme::muted_text)
            .width(Length::Fill),
        tagged(
            "appearance-cancel",
            button(text(i18n.tr("dialog-cancel")))
                .style(button::secondary)
                .on_press(send(AppearanceMessage::Close))
                .into()
        ),
        tagged(
            "appearance-apply",
            button(text(i18n.tr("appearance-apply")))
                .on_press(send(AppearanceMessage::Apply))
                .into()
        ),
    ]
    .spacing(8)
    .align_y(Alignment::Center)
    .height(Length::Fixed(FOOTER_HEIGHT - PAD));
    let right = column![body, footer]
        .spacing(PAD)
        .width(Length::Fill)
        .height(Length::Fill);
    let card = container(
        row![sidebar(ui, i18n), right]
            .spacing(PAD)
            .height(Length::Fill),
    )
    .id(APPEARANCE_BOX_ID)
    .padding(PAD)
    .width(Length::Fixed(size.width))
    .height(Length::Fixed(size.height))
    .style(theme::pane_body);
    opaque(container(card).center(Length::Fill).style(theme::scrim))
}

fn sidebar<'a>(ui: &AppearanceUi, i18n: &I18n) -> Element<'a, Message> {
    let mut tabs = column![
        container(text(i18n.tr("appearance-title")).size(18)).padding([4, 10]),
        Space::new().height(8)
    ]
    .spacing(2);
    for tab in AppearanceTab::ALL {
        let active = ui.tab == tab;
        let changed = ui.draft.differs_in(&ui.baseline, tab);
        let mut line = row![
            icon(
                tab.icon(),
                18.0,
                if active {
                    IconTone::OnAccent
                } else {
                    IconTone::Text
                }
            ),
            text(i18n.tr(tab.name_key())).width(Length::Fill),
        ]
        .spacing(10)
        .align_y(Alignment::Center);
        if changed {
            line = line.push(preview::tip(
                text("\u{25cf}").size(10).into(),
                i18n.tr("appearance-tab-changed"),
            ));
        }
        tabs = tabs.push(tagged(
            tab.button_id(),
            button(line)
                .width(Length::Fill)
                .padding([8, 10])
                .style(theme::command_button(ButtonLook::Secondary, active))
                .on_press(send(AppearanceMessage::Tab(tab)))
                .into(),
        ));
    }
    container(tabs)
        .padding(8)
        .width(Length::Fixed(SIDEBAR_WIDTH))
        .height(Length::Fill)
        .style(theme::modal_sidebar)
        .into()
}

fn preview_area<'a>(
    ui: &'a AppearanceUi,
    system: iced::theme::Mode,
    i18n: &'a I18n,
    width: f32,
    height: f32,
) -> Element<'a, Message> {
    let current_board = BoardTheme::of(ui.baseline.board_theme);
    let inner: Element<'a, Message> = match ui.tab {
        AppearanceTab::Theme => {
            preview::theme_miniature(&ui.draft, system, &current_board, i18n, height)
        }
        AppearanceTab::Board => {
            let side = height - 16.0;
            container(
                ui.board_widget(ui.board_pieces())
                    .into_element()
                    .map(|_| preview::noop()),
            )
            .width(Length::Fixed(side))
            .height(Length::Fixed(side))
            .into()
        }
        AppearanceTab::Pieces => {
            let side = height - 16.0;
            let cell = ((width - 2.0 * PAD - side) / 6.0).clamp(24.0, side / 2.0);
            row![
                preview::piece_grid(ui.draft_pieces(), &current_board, cell),
                container(
                    ui.pieces_widget(ui.draft_pieces())
                        .into_element()
                        .map(|_| preview::noop()),
                )
                .width(Length::Fixed(side))
                .height(Length::Fixed(side)),
            ]
            .spacing(PAD)
            .align_y(Alignment::Center)
            .into()
        }
        AppearanceTab::Players => preview::strips_sample(
            &ui.draft,
            ui.draft_pieces(),
            i18n,
            (width - 4.0 * PAD).clamp(200.0, 460.0),
        ),
    };
    // The miniature of the theme is the whole backdrop; the others sit on
    // the window color.
    if ui.tab == AppearanceTab::Theme {
        return container(inner)
            .width(Length::Fill)
            .height(Length::Fixed(height))
            .into();
    }
    container(inner)
        .center(Length::Fill)
        .width(Length::Fill)
        .height(Length::Fixed(height))
        .padding(8)
        .style(theme::modal_sidebar)
        .into()
}

fn section(title: String, content: Element<'_, Message>) -> Element<'_, Message> {
    column![text(title).size(13).style(theme::muted_text), content]
        .spacing(8)
        .into()
}

/// A row of exclusive choices drawn as buttons (the chosen one filled with
/// the accent).
fn segmented<'a, T: Copy + PartialEq + 'a>(
    items: &[(&'static str, &'static str, T)],
    chosen: T,
    i18n: &I18n,
    message: impl Fn(T) -> AppearanceMessage,
) -> Element<'a, Message> {
    let mut line = row![].spacing(6);
    for &(id, key, value) in items {
        let selected = chosen == value;
        let label = if selected {
            format!("\u{2713} {}", i18n.tr(key))
        } else {
            i18n.tr(key)
        };
        line = line.push(tagged(
            id,
            button(text(label))
                .padding([6, 12])
                .style(theme::command_button(ButtonLook::Primary, selected))
                .on_press(send(message(value)))
                .into(),
        ));
    }
    line.wrap().into()
}

fn grid(cards: Vec<Element<'_, Message>>, per_row: usize) -> Element<'_, Message> {
    let mut rows = column![].spacing(CARD_GAP);
    let mut cards = cards.into_iter().peekable();
    while cards.peek().is_some() {
        let mut line = row![].spacing(CARD_GAP);
        for card in cards.by_ref().take(per_row.max(1)) {
            line = line.push(card);
        }
        rows = rows.push(line);
    }
    rows.into()
}

fn per_row(width: f32, card: f32) -> usize {
    // Truncation intended: a whole number of cards per line.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let fit = ((width + CARD_GAP) / (card + CARD_GAP)).floor().max(1.0) as usize;
    fit
}

fn card<'a>(
    id: &'static str,
    selected: bool,
    width: f32,
    thumbnail: Element<'a, Message>,
    name: String,
    on_press: AppearanceMessage,
) -> Element<'a, Message> {
    let label = if selected {
        format!("\u{2713} {name}")
    } else {
        name
    };
    tagged(
        id,
        button(
            column![
                thumbnail,
                container(text(label).size(11).align_x(Alignment::Center))
                    .width(Length::Fill)
                    .height(Length::Fixed(30.0))
                    .center_x(Length::Fill),
            ]
            .spacing(4)
            .align_x(Alignment::Center),
        )
        .width(Length::Fixed(width))
        .padding(6)
        .style(theme::choice_card(selected))
        .on_press(send(on_press))
        .into(),
    )
}

const MODES: [(&str, &str, ThemeMode); 3] = [
    (
        "appearance-mode-system",
        "menu-view-theme-system",
        ThemeMode::System,
    ),
    (
        "appearance-mode-light",
        "menu-view-theme-light",
        ThemeMode::Light,
    ),
    (
        "appearance-mode-dark",
        "menu-view-theme-dark",
        ThemeMode::Dark,
    ),
];

const COORDS: [(&str, &str, CoordsMode); 4] = [
    (
        "appearance-coords-none",
        "menu-view-coords-none",
        CoordsMode::None,
    ),
    (
        "appearance-coords-outside",
        "menu-view-coords-outside",
        CoordsMode::Outside,
    ),
    (
        "appearance-coords-both",
        "menu-view-coords-both",
        CoordsMode::OutsideBothSides,
    ),
    (
        "appearance-coords-inside",
        "menu-view-coords-inside",
        CoordsMode::Inside,
    ),
];

const MATERIAL: [(&str, &str, MaterialMode); 3] = [
    (
        "appearance-material-off",
        "menu-view-material-off",
        MaterialMode::Off,
    ),
    (
        "appearance-material-difference",
        "menu-view-material-difference",
        MaterialMode::Difference,
    ),
    (
        "appearance-material-captured",
        "menu-view-material-captured",
        MaterialMode::Captured,
    ),
];

fn accent_swatches<'a>(
    ui: &AppearanceUi,
    system: iced::theme::Mode,
    i18n: &I18n,
) -> Element<'a, Message> {
    let dark = ui.draft.is_dark(system);
    let mut line = row![].spacing(6);
    for id in AccentId::ALL {
        let selected = ui.draft.accent == id;
        let color = id.color(dark);
        let on = id.on_color(dark);
        let dot = container(text(if selected { "\u{2713}" } else { "" }).size(15))
            .center(Length::Fixed(SWATCH))
            .style(move |theme: &Theme| container::Style {
                text_color: Some(on),
                ..theme::swatch(theme, color, selected)
            });
        let name = i18n.tr(id.name_key());
        let tip = if selected {
            format!("{name} ({})", i18n.tr("accent-selected"))
        } else {
            name
        };
        line = line.push(tagged(
            id.name_key(),
            preview::tip(
                button(dot)
                    .padding(3)
                    .style(theme::command_button(ButtonLook::Secondary, false))
                    .on_press(send(AppearanceMessage::SetAccent(id)))
                    .into(),
                tip,
            ),
        ));
    }
    line.wrap().into()
}

fn board_cards<'a>(ui: &AppearanceUi, i18n: &I18n, width: f32) -> Element<'a, Message> {
    let cards = BoardThemeId::ALL
        .into_iter()
        .map(|id| {
            card(
                id.name_key(),
                ui.draft.board_theme == id,
                BOARD_CARD,
                preview::mini_board(&BoardTheme::of(id), 64.0),
                i18n.tr(id.name_key()),
                AppearanceMessage::SetBoardTheme(id),
            )
        })
        .collect();
    grid(cards, per_row(width, BOARD_CARD))
}

fn piece_cards<'a>(ui: &AppearanceUi, i18n: &I18n, width: f32) -> Element<'a, Message> {
    let cards = PieceSetId::ALL
        .into_iter()
        .map(|id| {
            card(
                id.name_key(),
                ui.draft.piece_set == id,
                PIECE_CARD,
                preview::piece_sample(ui.set(id), 17.0),
                i18n.tr(id.name_key()),
                AppearanceMessage::SetPieceSet(id),
            )
        })
        .collect();
    grid(cards, per_row(width, PIECE_CARD))
}

fn switches<'a>(ui: &AppearanceUi, i18n: &I18n) -> Element<'a, Message> {
    let mut list = column![].spacing(10);
    for which in PlayerSwitch::ALL {
        let on = which.is_on(ui.draft.players);
        list = list.push(
            checkbox(on)
                .label(i18n.tr(which.label_key()))
                .on_toggle(move |value| send(AppearanceMessage::SetSwitch(which, value))),
        );
    }
    list.into()
}

fn choices<'a>(
    ui: &'a AppearanceUi,
    system: iced::theme::Mode,
    i18n: &'a I18n,
    width: f32,
) -> Element<'a, Message> {
    match ui.tab {
        AppearanceTab::Theme => column![
            section(
                i18n.tr("appearance-section-mode"),
                segmented(&MODES, ui.draft.mode, i18n, AppearanceMessage::SetMode)
            ),
            section(
                i18n.tr("menu-view-accent"),
                accent_swatches(ui, system, i18n)
            ),
        ]
        .spacing(PAD)
        .into(),
        AppearanceTab::Board => column![
            section(
                i18n.tr("menu-view-coords"),
                segmented(&COORDS, ui.draft.coords, i18n, AppearanceMessage::SetCoords)
            ),
            section(
                i18n.tr("menu-view-board-theme"),
                board_cards(ui, i18n, width)
            ),
        ]
        .spacing(PAD)
        .into(),
        AppearanceTab::Pieces => column![
            section(i18n.tr("menu-view-pieces"), piece_cards(ui, i18n, width)),
            section(
                i18n.tr("menu-view-material"),
                column![
                    segmented(
                        &MATERIAL,
                        ui.draft.players.material,
                        i18n,
                        AppearanceMessage::SetMaterial
                    ),
                    preview::strips_sample(
                        &ui.draft,
                        ui.draft_pieces(),
                        i18n,
                        (width - 2.0 * PAD).clamp(200.0, 420.0)
                    ),
                ]
                .spacing(10)
                .into()
            ),
        ]
        .spacing(PAD)
        .into(),
        AppearanceTab::Players => section(i18n.tr("menu-view-player-strips"), switches(ui, i18n)),
    }
}
