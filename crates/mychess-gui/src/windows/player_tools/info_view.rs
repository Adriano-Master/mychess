//! The player information window (spec M6-3, section 3): what the spelling
//! file says, the statistics of the database, the openings and the rating
//! history. The numbers are links that set the filter (section 3.4).
//!
//! The photo is not drawn: this build decodes no images (D3464), so the
//! window keeps the space empty.

use fluent_bundle::FluentArgs;
use iced::widget::{
    button, column, container, pick_list, progress_bar, row, scrollable, text, tooltip,
};
use iced::{Alignment, Element, Length, Size};
use mychess_app::{InfoHeader, InfoStats, PlayerInfoMessage, PlayerInfoView, SplitView, TallyView};

use super::{PLAYER_INFO_BOX_ID, app, card_width, list_height, text as texts};
use crate::i18n::I18n;
use crate::shell::Message;
use crate::theme;
use crate::widgets::icon::{Icon, IconTone, icon};
use crate::widgets::modal::{Card, modal};

/// Id of the "Update" button.
pub const INFO_UPDATE_ID: &str = "player-info-update";
/// Id of the "Close" button.
pub const INFO_CLOSE_ID: &str = "player-info-close";
/// Id of the "Lookup name" button.
pub const INFO_LOOKUP_ID: &str = "player-info-lookup";
/// Id of the "Tournaments" button.
pub const INFO_TOURNAMENTS_ID: &str = "player-info-tournaments";
/// Id of the "Player report" button (spec M7-3c).
pub const INFO_REPORT_ID: &str = "player-info-report";
/// Id of the FIDE link.
pub const INFO_FIDE_ID: &str = "player-info-fide";

const CHROME: f32 = 230.0;

fn info(message: PlayerInfoMessage) -> Message {
    app(mychess_app::Message::PlayerInfo(message))
}

fn link_message(code: &str) -> Message {
    info(PlayerInfoMessage::FilterLink(code.to_owned()))
}

fn with_id<'a>(id: &'static str, body: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    container(body).id(id).into()
}

/// A number that is a link: it sets the filter.
fn link<'a>(label: String, code: &str, width: Length) -> Element<'a, Message> {
    button(text(label).size(13).style(theme::accent_text))
        .padding([0, 2])
        .width(width)
        .style(button::text)
        .on_press(link_message(code))
        .into()
}

fn muted<'a>(content: String) -> Element<'a, Message> {
    text(content).size(12).style(theme::muted_text).into()
}

fn heading<'a>(content: String) -> Element<'a, Message> {
    text(content).size(13).style(theme::accent_text).into()
}

fn command<'a>(
    glyph: Icon,
    label: String,
    message: Option<Message>,
    primary: bool,
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

/// One line `label   value` of the header.
fn fact<'a>(label: String, value: String) -> Element<'a, Message> {
    row![
        text(label)
            .size(13)
            .width(Length::Fixed(110.0))
            .style(theme::muted_text),
        text(value).size(13),
    ]
    .spacing(8)
    .into()
}

fn header_block<'a>(h: &InfoHeader, i18n: &I18n) -> Element<'a, Message> {
    let mut block = column![].spacing(3);
    if h.approximate {
        block = block.push(
            text(i18n.tr("pinfo-approximate"))
                .size(13)
                .style(text::danger),
        );
        if !h.candidates.is_empty() {
            let mut args = FluentArgs::new();
            args.set("names", h.candidates.join("; "));
            block = block.push(muted(i18n.tr_args("pinfo-candidates", &args)));
        }
    }
    if !h.title_names.is_empty() {
        block = block.push(fact(
            i18n.tr("pinfo-title-label"),
            h.title_names.join(",  "),
        ));
    }
    if h.peak != 0 {
        let value = if h.peak_is_estimate {
            let mut args = FluentArgs::new();
            args.set("peak", i64::from(h.peak));
            i18n.tr_args("pinfo-peak-estimate", &args)
        } else {
            h.peak.to_string()
        };
        block = block.push(fact(i18n.tr("pinfo-peak"), value));
    }
    if !h.country_names.is_empty() {
        block = block.push(fact(
            i18n.tr("pinfo-country-label"),
            h.country_names.join(",  "),
        ));
    }
    if !h.born.is_empty() {
        block = block.push(fact(i18n.tr("pinfo-born"), h.born.clone()));
    }
    if !h.bio.is_empty() {
        block = block.push(heading(i18n.tr("pinfo-bio")));
        for note in &h.bio {
            block = block.push(text(format!("    {note}")).size(13));
        }
    }
    if h.fide_url.is_some() {
        block = block.push(with_id(
            INFO_FIDE_ID,
            button(
                text(i18n.tr("pinfo-fide"))
                    .size(13)
                    .style(theme::accent_text),
            )
            .padding([0, 2])
            .style(button::text)
            .on_press(info(PlayerInfoMessage::OpenFide)),
        ));
    }
    block.into()
}

const LABEL_W: f32 = 80.0;
const NUM_W: f32 = 64.0;

/// The line `Total   87.50%   +3  -0  =1   3.5 / 4`.
fn tally_line<'a>(
    i18n: &I18n,
    label: String,
    t: &TallyView,
    codes: Option<[&str; 4]>,
) -> Element<'a, Message> {
    let number = |value: String, code: Option<&str>| -> Element<'a, Message> {
        match code {
            Some(code) => link(value, code, Length::Fixed(NUM_W)),
            None => text(value).size(13).width(Length::Fixed(NUM_W)).into(),
        }
    };
    row![
        text(label).size(13).width(Length::Fixed(LABEL_W)),
        text(texts::percent(i18n, t.percent_hundredths))
            .size(13)
            .width(Length::Fixed(NUM_W + 16.0)),
        number(format!("+{}", t.wins), codes.map(|c| c[0])),
        number(format!("-{}", t.losses), codes.map(|c| c[1])),
        number(format!("={}", t.draws), codes.map(|c| c[2])),
        number(format!("{} / {}", t.points, t.games), codes.map(|c| c[3])),
    ]
    .spacing(6)
    .align_y(Alignment::Center)
    .into()
}

/// The three lines of a section. `prefix` is `-f` for the links of "All
/// games", `-o` for those of "Against"; `None` draws no links.
fn split_block<'a>(
    title: String,
    s: &SplitView,
    prefix: Option<&str>,
    i18n: &I18n,
) -> Element<'a, Message> {
    let code = |letters: &str| prefix.map(|p| format!("{p}{letters}"));
    // The codes are owned strings; leak-free: built per line below.
    let lines = [
        ("pinfo-total", &s.total, ["wW", "lL", "dD", "aA"]),
        ("pinfo-white", &s.white, ["w", "l", "d", "a"]),
        ("pinfo-black", &s.black, ["W", "L", "D", "A"]),
    ];
    let mut block = column![heading(title)].spacing(2);
    for (key, tally, letters) in lines {
        let owned: Option<Vec<String>> = prefix.map(|_| {
            letters
                .iter()
                .map(|l| code(l).unwrap_or_default())
                .collect()
        });
        let line: Element<'a, Message> = match owned {
            Some(codes) => {
                let refs = [
                    codes[0].as_str(),
                    codes[1].as_str(),
                    codes[2].as_str(),
                    codes[3].as_str(),
                ];
                tally_line(i18n, i18n.tr(key), tally, Some(refs))
            }
            None => tally_line(i18n, i18n.tr(key), tally, None),
        };
        block = block.push(line);
    }
    block.into()
}

fn openings_block<'a>(stats: &InfoStats, i18n: &I18n) -> Element<'a, Message> {
    let side = |key: &str, list: &[mychess_app::OpeningView]| -> Element<'a, Message> {
        let mut col = column![heading(i18n.tr(key))].spacing(2);
        for o in list {
            col = col.push(
                text(format!(
                    "{:>4}:  {} ({}%)",
                    o.label, o.games, o.score_percent
                ))
                .size(13),
            );
        }
        col.width(Length::FillPortion(1)).into()
    };
    row![
        side("pinfo-openings-white", &stats.openings[0]),
        side("pinfo-openings-black", &stats.openings[1]),
    ]
    .spacing(16)
    .into()
}

fn ratings_block<'a>(stats: &InfoStats, i18n: &I18n) -> Element<'a, Message> {
    let mut grid = column![heading(i18n.tr("pinfo-rating-history"))].spacing(2);
    for pair in stats.ratings.chunks(4) {
        let mut line = row![].spacing(18);
        for (year, month, elo) in pair {
            line = line.push(text(texts::rating_entry(*year, *month, *elo)).size(13));
        }
        grid = grid.push(line);
    }
    grid.into()
}

fn stats_block<'a>(stats: &InfoStats, i18n: &I18n) -> Element<'a, Message> {
    let mut args = FluentArgs::new();
    args.set("count", i64::from(stats.games));
    let games_text = i18n.tr_args("pinfo-games", &args);
    let mut first_line = row![if stats.games > 0 {
        link(games_text, "-faA", Length::Shrink)
    } else {
        text(i18n.tr("pinfo-no-games")).size(13).into()
    }]
    .spacing(12)
    .align_y(Alignment::Center);
    if stats.games > 0 {
        let first = stats.first.short();
        let dates = if stats.last_shown {
            let mut args = FluentArgs::new();
            args.set("first", first);
            args.set("last", stats.last.short());
            i18n.tr_args("pinfo-dates", &args)
        } else {
            first
        };
        first_line = first_line.push(muted(dates));
    }
    let mut block = column![first_line].spacing(8);
    if let Some(in_filter) = stats.in_filter {
        let mut args = FluentArgs::new();
        args.set("count", i64::from(in_filter));
        block = block.push(link(
            i18n.tr_args("pinfo-filter-games", &args),
            "-F",
            Length::Shrink,
        ));
    }
    if stats.games == 0 {
        return block.into();
    }
    block = block.push(muted(i18n.tr("pinfo-legend")));
    block = block.push(split_block(
        i18n.tr("pinfo-all"),
        &stats.all,
        Some("-f"),
        i18n,
    ));
    if let Some(filter) = &stats.filter {
        block = block.push(split_block(i18n.tr("pinfo-filter"), filter, None, i18n));
    }
    if let Some((name, split)) = &stats.versus {
        let mut args = FluentArgs::new();
        args.set("name", name.as_str());
        block = block.push(split_block(
            i18n.tr_args("pinfo-against", &args),
            split,
            Some("-o"),
            i18n,
        ));
    }
    if stats.openings.iter().any(|o| !o.is_empty()) {
        block = block.push(openings_block(stats, i18n));
    }
    if !stats.ratings.is_empty() {
        block = block.push(ratings_block(stats, i18n));
    }
    block.into()
}

fn body<'a>(v: &PlayerInfoView, i18n: &I18n, height: f32) -> Element<'a, Message> {
    let mut content = column![].spacing(10).width(Length::Fill).padding([0, 8]);
    if v.name.is_empty() {
        content = content.push(text(i18n.tr("pinfo-empty")).size(13));
    } else {
        content = content.push(text(v.name.clone()).size(20));
    }
    if v.no_base {
        content = content.push(text(i18n.tr("pinfo-no-base")).size(13));
    }
    if let Some(header) = &v.header {
        content = content.push(header_block(header, i18n));
    } else if !v.spelling_loaded && !v.name.is_empty() {
        content = content.push(muted(i18n.tr("pinfo-no-spelling")));
    }
    if let Some(stats) = &v.stats {
        content = content.push(stats_block(stats, i18n));
    } else if v.progress.is_none() && !v.name.is_empty() && !v.no_base && !v.found {
        content = content.push(text(i18n.tr("pinfo-not-in-base")).size(13));
    }
    scrollable(content).height(Length::Fixed(height)).into()
}

fn status<'a>(v: &PlayerInfoView, i18n: &I18n) -> Element<'a, Message> {
    if let Some((done, total)) = v.progress {
        let ratio = if total == 0 {
            0.0
        } else {
            // A fraction to draw: the precision of an `f32` is plenty.
            #[allow(clippy::cast_precision_loss)]
            let ratio = (done as f32 / total as f32).clamp(0.0, 1.0);
            ratio
        };
        return row![
            progress_bar(0.0..=1.0, ratio).girth(8.0),
            text(i18n.tr("pinfo-working")).size(12)
        ]
        .spacing(8)
        .align_y(Alignment::Center)
        .into();
    }
    if v.stale {
        return muted(i18n.tr("pinfo-stale"));
    }
    text("").size(12).into()
}

fn recent<'a>(v: &PlayerInfoView, i18n: &I18n) -> Element<'a, Message> {
    pick_list(v.history.clone(), None::<String>, |name| {
        info(PlayerInfoMessage::Open { name })
    })
    .placeholder(i18n.tr("pinfo-recent"))
    .text_size(12)
    .padding(3)
    .width(Length::Fixed(220.0))
    .into()
}

/// The player information window.
pub fn view<'a>(v: &PlayerInfoView, i18n: &I18n, window: Size) -> Element<'a, Message> {
    let height = list_height(window, CHROME);
    let title = if v.name.is_empty() {
        i18n.tr("pinfo-title")
    } else {
        let mut args = FluentArgs::new();
        args.set("name", v.name.as_str());
        i18n.tr_args("pinfo-title-name", &args)
    };
    let later = |label: String, glyph: Icon| -> Element<'a, Message> {
        tooltip(
            command(glyph, label, None, false),
            text(i18n.tr("pinfo-later-tip")).size(12),
            tooltip::Position::Top,
        )
        .style(theme::tooltip_box)
        .into()
    };
    let has_name = !v.name.is_empty();
    let content = column![recent(v, i18n), body(v, i18n, height), status(v, i18n)].spacing(8);
    modal(Card {
        id: PLAYER_INFO_BOX_ID,
        title,
        body: content.into(),
        buttons: vec![
            with_id(
                INFO_CLOSE_ID,
                command(
                    Icon::Close,
                    i18n.tr("ptools-close"),
                    Some(info(PlayerInfoMessage::Close)),
                    false,
                ),
            ),
            later(i18n.tr("pinfo-edit-name"), Icon::Manage),
            later(i18n.tr("pinfo-edit-ratings"), Icon::Gauge),
            with_id(
                INFO_TOURNAMENTS_ID,
                command(
                    Icon::Trophy,
                    i18n.tr("pinfo-tournaments"),
                    has_name.then(|| info(PlayerInfoMessage::OpenTournaments)),
                    false,
                ),
            ),
            // Spec M7-3c: the report of the games of this player.
            with_id(
                INFO_REPORT_ID,
                tooltip(
                    command(
                        Icon::Report,
                        i18n.tr("preport-ui-info"),
                        has_name.then(|| {
                            Message::PlayerReportUi(
                                crate::windows::player_report::PrUiMessage::Open(Some(
                                    v.name.clone(),
                                )),
                            )
                        }),
                        false,
                    ),
                    text(i18n.tr("preport-ui-info-tip")).size(12),
                    tooltip::Position::Top,
                )
                .style(theme::tooltip_box),
            ),
            with_id(
                INFO_LOOKUP_ID,
                command(
                    Icon::Users,
                    i18n.tr("pinfo-lookup"),
                    has_name.then(|| info(PlayerInfoMessage::LookupName)),
                    false,
                ),
            ),
            with_id(
                INFO_UPDATE_ID,
                command(
                    Icon::Search,
                    i18n.tr("ptools-update"),
                    Some(info(PlayerInfoMessage::Refresh)),
                    true,
                ),
            ),
        ],
        width: card_width(780.0, window),
    })
}
