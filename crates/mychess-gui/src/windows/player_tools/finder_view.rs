//! The player finder and the tournament finder (spec M6-3, sections 2 and
//! 4, rules 75 to 80 and 91 to 100).

use fluent_bundle::FluentArgs;
use iced::widget::{button, column, container, progress_bar, row, text, text_input, tooltip};
use iced::{Alignment, Element, Length, Size};
use mychess_app::{
    FinderMessage, PlayerField, PlayerFinderView, PlayerSortKey, TournamentAction, TournamentField,
    TournamentFinderView, TournamentRowView, TournamentSortKey,
};

use super::table::{self, ROW_HEIGHT};
use super::{
    ListId, PLAYER_FINDER_BOX_ID, PLAYER_PREFIX_ID, PlayerToolsMessage, PlayerToolsUi,
    TOURNAMENT_FINDER_BOX_ID, app, card_width, list_height, text as texts,
};
use crate::i18n::I18n;
use crate::shell::Message;
use crate::theme;
use crate::widgets::icon::{Icon, IconTone, icon};
use crate::widgets::modal::{Card, modal};

/// Id of the "Update" button of the player finder.
pub const PLAYER_UPDATE_ID: &str = "player-finder-update";
/// Id of the "Defaults" button of the player finder.
pub const PLAYER_DEFAULTS_ID: &str = "player-finder-defaults";
/// Id of the "Close" button of the player finder.
pub const PLAYER_CLOSE_ID: &str = "player-finder-close";
/// Id of the "Stop" button of the player finder.
pub const PLAYER_STOP_ID: &str = "player-finder-stop";
/// Id of the "Update" button of the tournament finder.
pub const TOURNAMENT_UPDATE_ID: &str = "tournament-finder-update";
/// Id of the "Defaults" button of the tournament finder.
pub const TOURNAMENT_DEFAULTS_ID: &str = "tournament-finder-defaults";
/// Id of the "Close" button of the tournament finder.
pub const TOURNAMENT_CLOSE_ID: &str = "tournament-finder-close";
/// Id of the "Stop" button of the tournament finder.
pub const TOURNAMENT_STOP_ID: &str = "tournament-finder-stop";
/// Id of the "Crosstable" button.
pub const TOURNAMENT_CROSSTABLE_ID: &str = "tournament-finder-crosstable";
/// Id of the "Show in filter" button.
pub const TOURNAMENT_FILTER_ID: &str = "tournament-finder-filter";
/// Id of the "Load first game" button.
pub const TOURNAMENT_LOAD_ID: &str = "tournament-finder-load";

/// What a list spends besides its rows: title, form, header, status and
/// buttons.
/// What the player finder spends besides the rows.
pub const PLAYER_CHROME: f32 = 330.0;
/// What the tournament finder spends besides the rows.
pub const TOURNAMENT_CHROME: f32 = 400.0;

/// The id of the header of a column of the player finder.
pub fn player_header_id(key: PlayerSortKey) -> &'static str {
    match key {
        PlayerSortKey::Name => "player-finder-sort-name",
        PlayerSortKey::Games => "player-finder-sort-games",
        PlayerSortKey::Oldest => "player-finder-sort-oldest",
        PlayerSortKey::Newest => "player-finder-sort-newest",
        PlayerSortKey::Elo => "player-finder-sort-elo",
        PlayerSortKey::Country => "player-finder-sort-country",
    }
}

/// The id of the header of a column of the tournament finder.
pub fn tournament_header_id(key: TournamentSortKey) -> &'static str {
    match key {
        TournamentSortKey::Date => "tournament-finder-sort-date",
        TournamentSortKey::Players => "tournament-finder-sort-players",
        TournamentSortKey::Games => "tournament-finder-sort-games",
        TournamentSortKey::Elo => "tournament-finder-sort-elo",
        TournamentSortKey::Event => "tournament-finder-sort-event",
        TournamentSortKey::Site => "tournament-finder-sort-site",
        TournamentSortKey::Winner => "tournament-finder-sort-winner",
    }
}

/// The id of a field of the player finder.
pub fn player_field_id(field: PlayerField) -> &'static str {
    match field {
        PlayerField::Prefix => PLAYER_PREFIX_ID,
        PlayerField::GamesMin => "player-finder-games-min",
        PlayerField::GamesMax => "player-finder-games-max",
        PlayerField::EloMin => "player-finder-elo-min",
        PlayerField::EloMax => "player-finder-elo-max",
        PlayerField::Limit => "player-finder-limit",
        PlayerField::Country => "player-finder-country",
    }
}

/// The id of a field of the tournament finder.
pub fn tournament_field_id(field: TournamentField) -> &'static str {
    match field {
        TournamentField::GamesMin => "tournament-finder-games-min",
        TournamentField::GamesMax => "tournament-finder-games-max",
        TournamentField::PlayersMin => "tournament-finder-players-min",
        TournamentField::PlayersMax => "tournament-finder-players-max",
        TournamentField::EloMin => "tournament-finder-elo-min",
        TournamentField::EloMax => "tournament-finder-elo-max",
        TournamentField::Start => "tournament-finder-start",
        TournamentField::End => "tournament-finder-end",
        TournamentField::Event => "tournament-finder-event",
        TournamentField::Site => "tournament-finder-site",
        TournamentField::Player => "tournament-finder-player",
        TournamentField::Country => "tournament-finder-country",
        TournamentField::Limit => "tournament-finder-limit",
    }
}

/// The id of a row of the player finder.
pub fn player_row_id(index: usize) -> iced::widget::Id {
    iced::widget::Id::from(format!("player-finder-row-{index}"))
}

/// The id of a row of the tournament finder.
pub fn tournament_row_id(index: usize) -> iced::widget::Id {
    iced::widget::Id::from(format!("tournament-finder-row-{index}"))
}

fn finder(message: FinderMessage) -> Message {
    app(mychess_app::Message::Finder(message))
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

fn field<'a>(
    id: &'static str,
    value: &str,
    width: f32,
    on_input: impl Fn(String) -> Message + 'a,
    submit: Message,
) -> Element<'a, Message> {
    text_input("", value)
        .id(id)
        .size(13)
        .padding([2, 4])
        .width(Length::Fixed(width))
        .on_input(on_input)
        .on_submit(submit)
        .into()
}

fn label<'a>(content: String, width: f32) -> Element<'a, Message> {
    text(content)
        .size(13)
        .width(Length::Fixed(width))
        .style(theme::muted_text)
        .into()
}

fn progress<'a>(i18n: &I18n, key: &str, done: u64, total: u64) -> Element<'a, Message> {
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

fn tip(body: Element<'_, Message>, tip: String) -> Element<'_, Message> {
    tooltip(body, text(tip).size(12), tooltip::Position::Top)
        .style(theme::tooltip_box)
        .into()
}

// ---- the player finder ----

const RANK_W: f32 = 56.0;
const NUM_W: f32 = 74.0;

fn player_columns(country: bool) -> Vec<(PlayerSortKey, &'static str, Length)> {
    let mut columns = vec![
        (
            PlayerSortKey::Games,
            "pfinder-col-games",
            Length::Fixed(NUM_W),
        ),
        (
            PlayerSortKey::Oldest,
            "pfinder-col-oldest",
            Length::Fixed(NUM_W + 20.0),
        ),
        (
            PlayerSortKey::Newest,
            "pfinder-col-newest",
            Length::Fixed(NUM_W + 20.0),
        ),
    ];
    if country {
        columns.push((
            PlayerSortKey::Country,
            "pfinder-col-country",
            Length::Fixed(NUM_W),
        ));
    }
    columns.push((
        PlayerSortKey::Elo,
        "pfinder-col-elo",
        Length::Fixed(NUM_W - 14.0),
    ));
    columns
}

fn player_form<'a>(v: &PlayerFinderView, i18n: &I18n) -> Element<'a, Message> {
    let update = finder(FinderMessage::UpdatePlayers);
    let set = |f: PlayerField| move |t: String| finder(FinderMessage::SetPlayerField(f, t));
    let num = |f: PlayerField, value: &str| {
        field(player_field_id(f), value, 64.0, set(f), update.clone())
    };
    let form = &v.form;
    let line1 = row![
        label(i18n.tr("pfinder-games"), 90.0),
        num(PlayerField::GamesMin, &form.games_min),
        text("-").size(13),
        num(PlayerField::GamesMax, &form.games_max),
        label(i18n.tr("pfinder-player"), 80.0),
        field(
            PLAYER_PREFIX_ID,
            &form.prefix,
            220.0,
            set(PlayerField::Prefix),
            update.clone()
        ),
    ]
    .spacing(6)
    .align_y(Alignment::Center);
    let line2 = row![
        label(i18n.tr("pfinder-elo"), 90.0),
        num(PlayerField::EloMin, &form.elo_min),
        text("-").size(13),
        num(PlayerField::EloMax, &form.elo_max),
        label(i18n.tr("pfinder-list-size"), 80.0),
        num(PlayerField::Limit, &form.limit),
        label(i18n.tr("pfinder-country"), 60.0),
        tip(
            num(PlayerField::Country, &form.country),
            i18n.tr("pfinder-country-tip")
        ),
    ]
    .spacing(6)
    .align_y(Alignment::Center);
    column![line1, line2].spacing(6).into()
}

fn player_header<'a>(v: &PlayerFinderView, i18n: &I18n) -> Element<'a, Message> {
    let mut cells = vec![table::header(
        i18n.tr("pfinder-col-rank"),
        Length::Fixed(RANK_W),
        None,
        None,
    )];
    for (key, text_key, width) in player_columns(v.country_column) {
        cells.push(with_id(
            player_header_id(key),
            table::header(
                i18n.tr(text_key),
                width,
                (v.sort == key).then_some(v.descending),
                Some(finder(FinderMessage::SortPlayers(key))),
            ),
        ));
    }
    let name = with_id(
        player_header_id(PlayerSortKey::Name),
        table::header(
            i18n.tr("pfinder-col-name"),
            Length::Fill,
            (v.sort == PlayerSortKey::Name).then_some(v.descending),
            Some(finder(FinderMessage::SortPlayers(PlayerSortKey::Name))),
        ),
    );
    cells.push(name);
    container(row(cells).spacing(8))
        .padding([0, 6])
        .style(theme::list_header)
        .into()
}

/// The player finder.
#[allow(clippy::too_many_lines)] // one card: form, header, list, buttons
pub fn players<'a>(
    ui: &PlayerToolsUi,
    v: &PlayerFinderView,
    i18n: &I18n,
    window: Size,
) -> Element<'a, Message> {
    let height = list_height(window, PLAYER_CHROME);
    let country = v.country_column;
    let rows = v.rows.clone();
    let cursor = ui.players.cursor;
    let list = table::rows(
        ListId::Players,
        rows.len(),
        ui.players.scroll,
        height,
        ROW_HEIGHT,
        move |index| {
            let Some(r) = rows.get(index) else {
                return text("").into();
            };
            let mut cells = vec![
                table::cell(r.rank.to_string(), Length::Fixed(RANK_W)),
                table::cell(r.games.to_string(), Length::Fixed(NUM_W)),
                table::cell(texts::year(r.first_year), Length::Fixed(NUM_W + 20.0)),
                table::cell(texts::year(r.last_year), Length::Fixed(NUM_W + 20.0)),
            ];
            if country {
                cells.push(table::cell(
                    r.country.clone().unwrap_or_default(),
                    Length::Fixed(NUM_W),
                ));
            }
            cells.push(table::cell(texts::elo(r.elo), Length::Fixed(NUM_W - 14.0)));
            cells.push(table::cell(r.name.clone(), Length::Fill));
            container(table::row_box(
                cells,
                index,
                cursor == Some(index),
                Message::PlayerTools(PlayerToolsMessage::ClickPlayer(index)),
                None,
            ))
            .id(player_row_id(index))
            .into()
        },
    );
    let status: Element<'_, Message> = if v.no_base {
        text(i18n.tr("pfinder-no-base")).size(13).into()
    } else if let Some((done, total)) = v.building {
        progress(i18n, "pfinder-building", done, total)
    } else if v.rows.is_empty() {
        text(i18n.tr("pfinder-empty")).size(13).into()
    } else {
        let mut args = FluentArgs::new();
        args.set("count", i64::try_from(v.rows.len()).unwrap_or(i64::MAX));
        let mut line = i18n.tr_args("pfinder-shown", &args);
        if v.estimated {
            line = format!("{line}  -  {}", i18n.tr("pfinder-elo-tip"));
        }
        text(line).size(12).style(theme::muted_text).into()
    };
    let body = column![player_form(v, i18n), player_header(v, i18n), list, status].spacing(8);
    let title = match &v.file_name {
        Some(file) => {
            let mut args = FluentArgs::new();
            args.set("file", file.as_str());
            args.set("count", i64::try_from(v.players).unwrap_or(i64::MAX));
            i18n.tr_args("pfinder-title-count", &args)
        }
        None => i18n.tr("pfinder-title"),
    };
    let title = if v.file_name.is_some() {
        format!("{}: {title}", i18n.tr("pfinder-title"))
    } else {
        title
    };
    modal(Card {
        id: PLAYER_FINDER_BOX_ID,
        title,
        body: body.into(),
        buttons: vec![
            with_id(
                PLAYER_CLOSE_ID,
                command(
                    Icon::Close,
                    i18n.tr("ptools-close"),
                    false,
                    Some(finder(FinderMessage::ClosePlayers)),
                ),
            ),
            with_id(
                PLAYER_STOP_ID,
                command(
                    Icon::Pause,
                    i18n.tr("ptools-stop"),
                    false,
                    v.building.map(|_| finder(FinderMessage::CancelPlayers)),
                ),
            ),
            with_id(
                PLAYER_DEFAULTS_ID,
                command(
                    Icon::Undo,
                    i18n.tr("ptools-defaults"),
                    false,
                    Some(finder(FinderMessage::DefaultsPlayers)),
                ),
            ),
            with_id(
                PLAYER_UPDATE_ID,
                command(
                    Icon::Search,
                    i18n.tr("ptools-update"),
                    true,
                    Some(finder(FinderMessage::UpdatePlayers)),
                ),
            ),
        ],
        width: card_width(820.0, window),
    })
}

// ---- the tournament finder ----

const T_RANK_W: f32 = 44.0;
const T_DATE_W: f32 = 84.0;
const T_NUM_W: f32 = 62.0;

fn t_columns() -> [(TournamentSortKey, &'static str, Length); 6] {
    [
        (
            TournamentSortKey::Date,
            "tmt-col-date",
            Length::Fixed(T_DATE_W),
        ),
        (
            TournamentSortKey::Players,
            "tmt-col-players",
            Length::Fixed(T_NUM_W + 14.0),
        ),
        (
            TournamentSortKey::Games,
            "tmt-col-games",
            Length::Fixed(T_NUM_W),
        ),
        (
            TournamentSortKey::Elo,
            "tmt-col-elo",
            Length::Fixed(T_NUM_W - 14.0),
        ),
        (
            TournamentSortKey::Event,
            "tmt-col-event",
            Length::FillPortion(3),
        ),
        (
            TournamentSortKey::Site,
            "tmt-col-site",
            Length::FillPortion(2),
        ),
    ]
}

fn tournament_form<'a>(v: &TournamentFinderView, i18n: &I18n) -> Element<'a, Message> {
    let update = finder(FinderMessage::UpdateTournaments);
    let set = |f: TournamentField| move |t: String| finder(FinderMessage::SetTournamentField(f, t));
    let f = &v.form;
    let input = |field_: TournamentField, value: &str, width: f32| {
        field(
            tournament_field_id(field_),
            value,
            width,
            set(field_),
            update.clone(),
        )
    };
    let pair = |key: &str, a: (TournamentField, &str), b: (TournamentField, &str), w: f32| {
        row![
            label(i18n.tr(key), 80.0),
            input(a.0, a.1, w),
            text("-").size(13),
            input(b.0, b.1, w),
        ]
        .spacing(6)
        .align_y(Alignment::Center)
    };
    let line1 = row![
        pair(
            "tmt-games",
            (TournamentField::GamesMin, &f.games_min),
            (TournamentField::GamesMax, &f.games_max),
            56.0
        ),
        pair(
            "tmt-players",
            (TournamentField::PlayersMin, &f.players_min),
            (TournamentField::PlayersMax, &f.players_max),
            56.0
        ),
        pair(
            "tmt-mean-elo",
            (TournamentField::EloMin, &f.elo_min),
            (TournamentField::EloMax, &f.elo_max),
            56.0
        ),
    ]
    .spacing(14);
    let line2 = row![
        pair(
            "tmt-date",
            (TournamentField::Start, &f.start),
            (TournamentField::End, &f.end),
            96.0
        ),
        label(i18n.tr("tmt-event"), 50.0),
        input(TournamentField::Event, &f.event, 150.0),
        label(i18n.tr("tmt-site"), 40.0),
        input(TournamentField::Site, &f.site, 150.0),
    ]
    .spacing(6)
    .align_y(Alignment::Center);
    let line3 = row![
        label(i18n.tr("tmt-player"), 80.0),
        input(TournamentField::Player, &f.player, 200.0),
        label(i18n.tr("tmt-country"), 60.0),
        input(TournamentField::Country, &f.country, 56.0),
        label(i18n.tr("tmt-list-size"), 80.0),
        input(TournamentField::Limit, &f.limit, 64.0),
    ]
    .spacing(6)
    .align_y(Alignment::Center);
    column![line1, line2, line3].spacing(6).into()
}

fn tournament_header<'a>(v: &TournamentFinderView, i18n: &I18n) -> Element<'a, Message> {
    let mut cells = vec![table::header(
        i18n.tr("tmt-col-rank"),
        Length::Fixed(T_RANK_W),
        None,
        None,
    )];
    let sorted = |key: TournamentSortKey| (v.sort == key).then_some(false);
    for (key, text_key, width) in t_columns() {
        cells.push(with_id(
            tournament_header_id(key),
            table::header(
                i18n.tr(text_key),
                width,
                sorted(key),
                Some(finder(FinderMessage::SortTournaments(key))),
            ),
        ));
    }
    cells.push(with_id(
        tournament_header_id(TournamentSortKey::Winner),
        table::header(
            i18n.tr("tmt-col-winner"),
            Length::FillPortion(3),
            sorted(TournamentSortKey::Winner),
            Some(finder(FinderMessage::SortTournaments(
                TournamentSortKey::Winner,
            ))),
        ),
    ));
    cells.push(table::header(
        i18n.tr("tmt-col-second"),
        Length::FillPortion(3),
        None,
        None,
    ));
    container(row(cells).spacing(8))
        .padding([0, 6])
        .style(theme::list_header)
        .into()
}

/// The first 25 characters of a name (`Event` and `Site` of the list).
fn clip(name: &str) -> String {
    name.chars().take(25).collect()
}

/// `1. Alfa (2510)`, `1= Alfa`.
fn standing_text(rank: &str, standing: Option<&mychess_app::StandingView>) -> String {
    match standing {
        Some(s) => match s.elo {
            Some(elo) => format!("{rank} {} ({elo})", s.surname),
            None => format!("{rank} {}", s.surname),
        },
        None => String::new(),
    }
}

/// The text of the winner and of the runner-up of a row.
pub fn winner_texts(r: &TournamentRowView) -> (String, String) {
    let (a, b) = if r.tie { ("1=", "1=") } else { ("1.", "2.") };
    (
        standing_text(a, r.winner.as_ref()),
        standing_text(b, r.runner_up.as_ref()),
    )
}

fn tournament_row<'a>(r: &TournamentRowView, index: usize, selected: bool) -> Element<'a, Message> {
    let (winner, second) = winner_texts(r);
    let cells = vec![
        table::cell(r.rank.to_string(), Length::Fixed(T_RANK_W)),
        table::cell(r.date.short(), Length::Fixed(T_DATE_W)),
        table::cell(r.players.to_string(), Length::Fixed(T_NUM_W + 14.0)),
        table::cell(r.games.to_string(), Length::Fixed(T_NUM_W)),
        table::cell(texts::elo(r.mean_elo), Length::Fixed(T_NUM_W - 14.0)),
        table::cell(clip(&r.event), Length::FillPortion(3)),
        table::cell(clip(&r.site), Length::FillPortion(2)),
        table::cell(winner, Length::FillPortion(3)),
        table::cell(second, Length::FillPortion(3)),
    ];
    container(table::row_box(
        cells,
        index,
        selected,
        Message::PlayerTools(PlayerToolsMessage::SelectTournament(index)),
        Some(finder(FinderMessage::ClickTournament {
            index,
            action: TournamentAction::OpenCrosstable,
        })),
    ))
    .id(tournament_row_id(index))
    .into()
}

/// The tournament finder.
#[allow(clippy::too_many_lines)] // one card: form, header, list, buttons
pub fn tournaments<'a>(
    ui: &PlayerToolsUi,
    v: &TournamentFinderView,
    i18n: &I18n,
    window: Size,
) -> Element<'a, Message> {
    let height = list_height(window, TOURNAMENT_CHROME);
    let cursor = ui.tournaments.cursor;
    let rows = v.rows.clone();
    let total = rows.as_ref().map_or(0, |r| r.len());
    let list = {
        let rows = rows.clone();
        table::rows(
            ListId::Tournaments,
            total,
            ui.tournaments.scroll,
            height,
            ROW_HEIGHT,
            move |index| match rows.as_ref().and_then(|r| r.get(index)) {
                Some(r) => tournament_row(r, index, cursor == Some(index)),
                None => text("").into(),
            },
        )
    };
    let status: Element<'_, Message> = if v.no_base {
        text(i18n.tr("tmt-no-base")).size(13).into()
    } else if let Some((done, total)) = v.running {
        progress(i18n, "tmt-running", done, total)
    } else if rows.as_ref().is_some_and(|r| r.is_empty()) {
        text(i18n.tr("tmt-none")).size(13).into()
    } else {
        let mut args = FluentArgs::new();
        args.set("count", i64::try_from(total).unwrap_or(i64::MAX));
        text(format!(
            "{}.  {}",
            i18n.tr_args("tmt-shown", &args),
            i18n.tr("tmt-select-tip")
        ))
        .size(12)
        .style(theme::muted_text)
        .into()
    };
    let body = column![
        tournament_form(v, i18n),
        tournament_header(v, i18n),
        list,
        status
    ]
    .spacing(8);
    let title = match (&v.player, &v.file_name) {
        (Some(player), file) => {
            let mut args = FluentArgs::new();
            args.set("player", player.as_str());
            args.set("file", file.clone().unwrap_or_default());
            i18n.tr_args("tmt-title-player", &args)
        }
        (None, Some(file)) => {
            let mut args = FluentArgs::new();
            args.set("file", file.as_str());
            i18n.tr_args("tmt-title-file", &args)
        }
        (None, None) => i18n.tr("tmt-title"),
    };
    let selected = cursor.filter(|i| *i < total);
    let act = |action: TournamentAction| {
        selected.map(|index| finder(FinderMessage::ClickTournament { index, action }))
    };
    modal(Card {
        id: TOURNAMENT_FINDER_BOX_ID,
        title,
        body: body.into(),
        buttons: vec![
            with_id(
                TOURNAMENT_CLOSE_ID,
                command(
                    Icon::Close,
                    i18n.tr("ptools-close"),
                    false,
                    Some(finder(FinderMessage::CloseTournaments)),
                ),
            ),
            with_id(
                TOURNAMENT_STOP_ID,
                command(
                    Icon::Pause,
                    i18n.tr("ptools-stop"),
                    false,
                    v.running.map(|_| finder(FinderMessage::CancelTournaments)),
                ),
            ),
            with_id(
                TOURNAMENT_LOAD_ID,
                command(
                    Icon::Load,
                    i18n.tr("tmt-load-first"),
                    false,
                    act(TournamentAction::LoadFirstGame),
                ),
            ),
            with_id(
                TOURNAMENT_FILTER_ID,
                command(
                    Icon::FilterList,
                    i18n.tr("tmt-show-in-filter"),
                    false,
                    act(TournamentAction::ShowInFilter),
                ),
            ),
            with_id(
                TOURNAMENT_CROSSTABLE_ID,
                command(
                    Icon::Trophy,
                    i18n.tr("tmt-crosstable"),
                    false,
                    act(TournamentAction::OpenCrosstable),
                ),
            ),
            with_id(
                TOURNAMENT_DEFAULTS_ID,
                command(
                    Icon::Undo,
                    i18n.tr("ptools-defaults"),
                    false,
                    Some(finder(FinderMessage::DefaultsTournaments)),
                ),
            ),
            with_id(
                TOURNAMENT_UPDATE_ID,
                command(
                    Icon::Search,
                    i18n.tr("ptools-update"),
                    true,
                    Some(finder(FinderMessage::UpdateTournaments)),
                ),
            ),
        ],
        width: card_width(1120.0, window),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_cut_at_twenty_five_characters() {
        assert_eq!(
            clip("Torneio Internacional de Xadrez do Rio")
                .chars()
                .count(),
            25
        );
        assert_eq!(clip("Aberto"), "Aberto");
    }
}
