//! The title bar, the buttons, the progress and the report of the player
//! report window (spec M7-3c, M7-3/D4205-D4210).

use fluent_bundle::FluentArgs;
use iced::widget::{button, column, container, row, scrollable, space, text};
use iced::{Alignment, Border, Element, Length, Theme};
use mychess_app::opening_report::ReportSelector;
use mychess_app::player_report::data::{EndTheme, PlayerReport, natural_name};
use mychess_app::player_report::{
    PlayerFailure, PlayerPhase, PlayerReportMessage, PlayerReportView,
};
use mychess_core::Color;

use super::{PrUiMessage, prep, ui};
use crate::i18n::I18n;
use crate::shell::{Message, Shell};
use crate::theme::{self, ButtonLook, Tokens};
use crate::widgets::icon::{Icon, IconTone, icon};
use crate::windows::panel_kit::{BODY_TEXT, action, bold, danger, muted, progress};

/// The most game rows the window lists (the files hold them all).
pub const MAX_GAME_ROWS: usize = 200;
/// The most rows of the theory table the window lists.
pub const MAX_THEORY_ROWS: usize = 100;

/// Id of the scrollable body.
pub const PREPORT_BODY_ID: &str = "preport-body";

fn link_style(theme: &Theme, status: button::Status) -> button::Style {
    let t = Tokens::of(theme);
    let color = match status {
        button::Status::Hovered | button::Status::Pressed => {
            theme::tokens::mix(t.text, t.accent, 0.35)
        }
        button::Status::Disabled => t.muted,
        button::Status::Active => t.accent,
    };
    button::Style {
        background: None,
        text_color: color,
        border: Border::default(),
        ..button::Style::default()
    }
}

fn link<'a>(label: String, message: Message, id: String) -> Element<'a, Message> {
    container(
        button(text(label).size(BODY_TEXT))
            .padding([0, 2])
            .style(link_style)
            .on_press(message),
    )
    .id(id)
    .into()
}

fn tool<'a>(
    glyph: Icon,
    label: String,
    message: Option<Message>,
    id: &'static str,
) -> Element<'a, Message> {
    let tone = if message.is_some() {
        IconTone::Text
    } else {
        IconTone::Disabled
    };
    let body = button(
        row![icon(glyph, 16.0, tone), text(label).size(BODY_TEXT)]
            .spacing(6)
            .align_y(Alignment::Center),
    )
    .padding([4, 8])
    .style(theme::command_button(ButtonLook::Secondary, false))
    .on_press_maybe(message);
    container(body).id(id).into()
}

fn tr1(i18n: &I18n, key: &str, name: &str, value: &str) -> String {
    let mut args = FluentArgs::new();
    args.set(name, value);
    i18n.tr_args(key, &args)
}

/// The title and the button that closes the window.
pub(super) fn title_bar<'a>(shell: &'a Shell, view: &PlayerReportView<'_>) -> Element<'a, Message> {
    let i18n = &shell.i18n;
    let line = view
        .report
        .map(|r| natural_name(&r.name))
        .unwrap_or_default();
    row![
        text(i18n.tr("preport-title")).size(18),
        text(line).size(15).style(theme::accent_text),
        space::horizontal(),
        container(action(
            i18n.tr("preport-ui-close"),
            Some(ui(PrUiMessage::Close))
        ))
        .id("preport-close"),
    ]
    .spacing(12)
    .align_y(Alignment::Center)
    .into()
}

/// The buttons of the report.
pub(super) fn tools<'a>(shell: &'a Shell, view: &PlayerReportView<'_>) -> Element<'a, Message> {
    let i18n = &shell.i18n;
    let running = matches!(view.phase, PlayerPhase::Generating(_));
    let has_report = view.report.is_some() && !running;
    let fresh = has_report && !view.stale;
    let on = |ok: bool, m: Message| ok.then_some(m);
    row![
        tool(
            Icon::Manage,
            i18n.tr("preport-change"),
            on(!running, ui(PrUiMessage::Change)),
            "preport-change",
        ),
        tool(
            Icon::Rematch,
            i18n.tr("preport-ui-update"),
            on(!running && view.report.is_some(), ui(PrUiMessage::Refresh)),
            "preport-update",
        ),
        tool(
            Icon::AppendFile,
            i18n.tr("preport-ui-save"),
            on(fresh, ui(PrUiMessage::OpenSave)),
            "preport-save",
        ),
        tool(
            Icon::Report,
            i18n.tr("preport-ui-view-html"),
            on(fresh, prep(PlayerReportMessage::ViewHtml)),
            "preport-view-html",
        ),
    ]
    .spacing(6)
    .align_y(Alignment::Center)
    .wrap()
    .into()
}

fn failure_key(failure: PlayerFailure) -> &'static str {
    match failure {
        PlayerFailure::UnsupportedPosition => "preport-err-position",
        PlayerFailure::BadLine => "preport-err-line",
        _ => "preport-err-failed",
    }
}

/// The lines under the buttons: progress, out of date, failure.
pub(super) fn banners<'a>(shell: &'a Shell, view: &PlayerReportView<'_>) -> Element<'a, Message> {
    let i18n = &shell.i18n;
    let mut col = column![].spacing(6);
    match view.phase {
        PlayerPhase::Generating((done, total)) => {
            col = col.push(
                row![
                    text(i18n.tr("preport-ui-progress"))
                        .size(BODY_TEXT)
                        .width(Length::Fixed(300.0)),
                    container(progress(done, total)).width(Length::Fixed(260.0)),
                    container(action(
                        i18n.tr("preport-ui-stop"),
                        Some(ui(PrUiMessage::Stop))
                    ))
                    .id("preport-stop"),
                ]
                .spacing(8)
                .align_y(Alignment::Center),
            );
        }
        PlayerPhase::Failed(failure) => {
            col = col.push(danger(i18n.tr(failure_key(failure))));
        }
        _ => {}
    }
    if view.stale && !matches!(view.phase, PlayerPhase::Generating(_)) {
        col = col.push(
            row![
                danger(i18n.tr("preport-stale")),
                container(action(
                    i18n.tr("preport-ui-update"),
                    Some(ui(PrUiMessage::Refresh))
                ))
                .id("preport-stale-update"),
            ]
            .spacing(10)
            .align_y(Alignment::Center),
        );
    }
    col.into()
}

/// The last remark of the application about the report.
pub(super) fn footer(shell: &Shell) -> Element<'_, Message> {
    let line = match &shell.ui.status_notice {
        Some(mychess_app::AppNotice::PlayerReport(notice)) => shell.i18n.tr(notice.message_key()),
        _ => String::new(),
    };
    container(text(line).size(12).style(theme::muted_text))
        .id("preport-footer")
        .into()
}

fn section<'a>(title: String) -> Element<'a, Message> {
    text(title).size(15).font(bold()).into()
}

/// The report, or what stands in its place.
#[allow(clippy::too_many_lines)] // the sections in order
pub(super) fn content<'a>(shell: &'a Shell, view: &PlayerReportView<'_>) -> Element<'a, Message> {
    let Some(report) = view.report else {
        return container(muted(String::new())).into();
    };
    let options = view.options;
    let i18n = &shell.i18n;
    let name = natural_name(&report.name);
    let white = report.color == Color::White;
    let mut col = column![].spacing(6).padding([0, 12]);

    // Header.
    let color_text = i18n.tr(if white {
        "preport-label-with-white"
    } else {
        "preport-label-with-black"
    });
    let mut head = format!("{name} {color_text}");
    if report.position.is_some() && !report.stem.is_empty() {
        head.push(' ');
        head.push_str(&tr1(i18n, "preport-after", "moves", &report.stem.join(" ")));
    }
    col = col.push(text(head).size(17).style(theme::accent_text));
    col = col.push(muted(tr1(i18n, "preport-pov", "name", &name)));
    let mut args = FluentArgs::new();
    args.set("games", i64::from(report.game_count()));
    args.set("database", view.database_name.unwrap_or_default());
    col = col.push(text(i18n.tr_args("preport-ui-report-line", &args)).size(BODY_TEXT));
    if report.core.interrupted {
        col = col.push(danger(i18n.tr("preport-partial")));
    }
    if report.core.skipped > 0 {
        let mut args = FluentArgs::new();
        args.set("n", i64::from(report.core.skipped));
        col = col.push(muted(i18n.tr_args("oprep-label-skipped", &args)));
    }
    if report.game_count() == 0 {
        col = col.push(text(tr1(i18n, "preport-none", "name", &name)).size(BODY_TEXT));
        return scrollable(col)
            .id(PREPORT_BODY_ID)
            .height(Length::Fill)
            .into();
    }

    // Score.
    let score = &report.player_score;
    let mut args = FluentArgs::new();
    args.set("name", name.as_str());
    args.set("wins", i64::from(score.wins));
    args.set("draws", i64::from(score.draws));
    args.set("losses", i64::from(score.losses));
    args.set("percent", i64::from(score.score_percent()));
    args.set("games", i64::from(report.game_count()));
    col = col.push(link(
        i18n.tr_args("preport-ui-score", &args),
        prep(PlayerReportMessage::Select(ReportSelector::All)),
        "preport-select-all".to_owned(),
    ));

    // Ratings and performance.
    if options.is_none_or(|o| o.avg_perf) {
        let mine = report.core.elo_report(report.color);
        let theirs = report.core.elo_report(!report.color);
        col = col.push(section(i18n.tr("oprep-label-ratings-perf")));
        let line = |rating: &str, perf: &str, own: (u32, u32), other: (u32, u32, u32)| {
            let mut args = FluentArgs::new();
            args.set("rating", i18n.tr(rating));
            args.set("average", i64::from(own.0));
            args.set("games", i64::from(own.1));
            args.set("perf", i18n.tr(perf));
            args.set("performance", i64::from(other.0));
            args.set("percent", i64::from(other.1));
            args.set("against", i64::from(other.2));
            text(i18n.tr_args("preport-ui-rating-line", &args)).size(BODY_TEXT)
        };
        col = col.push(line(
            "preport-label-player-rating",
            "preport-label-player-perf",
            (mine.average, mine.games),
            (
                theirs.opponent_performance,
                theirs.opponent_percent,
                theirs.average,
            ),
        ));
        col = col.push(line(
            "preport-label-opponents-rating",
            "preport-label-opponents-perf",
            (theirs.average, theirs.games),
            (
                mine.opponent_performance,
                mine.opponent_percent,
                mine.average,
            ),
        ));
    }

    col = opponents(
        col,
        shell,
        report,
        options.map_or(10, |o| usize::from(o.opponents)),
    );
    col = openings(
        col,
        shell,
        report,
        options.map_or(10, |o| usize::from(o.eco_codes)),
    );
    if options.is_none_or(|o| o.themes) {
        col = themes(col, shell, report);
    }
    if options.is_none_or(|o| o.endgames) {
        col = endgames(col, shell, report);
    }
    if options.is_none_or(|o| o.max_table_games > 0) {
        col = theory(col, shell, report);
    }
    col = game_list(col, shell, report);
    scrollable(col)
        .id(PREPORT_BODY_ID)
        .height(Length::Fill)
        .into()
}

fn percent_args(games: u32, percent: u32) -> FluentArgs<'static> {
    let mut args = FluentArgs::new();
    args.set("games", i64::from(games));
    args.set("percent", i64::from(percent));
    args
}

fn opponents<'a>(
    mut col: iced::widget::Column<'a, Message>,
    shell: &'a Shell,
    report: &PlayerReport,
    count: usize,
) -> iced::widget::Column<'a, Message> {
    let i18n = &shell.i18n;
    if count == 0 || report.opponents.is_empty() {
        return col;
    }
    col = col.push(section(i18n.tr("preport-label-most-frequent-opponents")));
    col = col.push(muted(i18n.tr("preport-ui-opponents-hint")));
    for p in report.opponents.iter().take(count) {
        let mut row_args = percent_args(p.games, p.score_percent);
        row_args.set("first", i64::from(p.first_year));
        row_args.set("last", i64::from(p.last_year));
        let row_key = match (p.first_year, p.last_year) {
            (0, _) => "preport-ui-opponent-row",
            (a, b) if a == b => "preport-ui-opponent-row-year",
            _ => "preport-ui-opponent-row-years",
        };
        col = col.push(
            row![
                text(format!("{}.", p.rank))
                    .size(BODY_TEXT)
                    .width(Length::Fixed(30.0)),
                link(
                    p.name.clone(),
                    prep(PlayerReportMessage::OpenOpponent(p.name.clone())),
                    format!("preport-opponent-{}", p.rank),
                ),
                muted(i18n.tr_args(row_key, &row_args)),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        );
    }
    col
}

fn openings<'a>(
    mut col: iced::widget::Column<'a, Message>,
    shell: &'a Shell,
    report: &PlayerReport,
    count: usize,
) -> iced::widget::Column<'a, Message> {
    let i18n = &shell.i18n;
    if count == 0 || report.eco_groups.is_empty() {
        return col;
    }
    col = col.push(section(i18n.tr("preport-openings")));
    for g in report.eco_groups.iter().take(count) {
        col = col.push(
            row![
                text(format!("{}-{}", g.first, g.last))
                    .size(BODY_TEXT)
                    .width(Length::Fixed(90.0)),
                muted(i18n.tr_args(
                    "preport-ui-eco-row",
                    &percent_args(g.games, g.score_percent)
                )),
            ]
            .spacing(8),
        );
    }
    col
}

fn themes<'a>(
    mut col: iced::widget::Column<'a, Message>,
    shell: &'a Shell,
    report: &PlayerReport,
) -> iced::widget::Column<'a, Message> {
    let i18n = &shell.i18n;
    if report.core.themes.iter().all(|n| *n == 0) {
        return col;
    }
    col = col.push(section(i18n.tr("preport-ui-themes")));
    for (i, count) in report.core.themes.iter().enumerate() {
        let label = format!("{}: {count}", i18n.tr(&format!("oprep-label-theme-{i}")));
        let item: Element<'a, Message> = if *count > 0 {
            link(
                label,
                prep(PlayerReportMessage::Select(ReportSelector::Theme(i))),
                format!("preport-theme-{i}"),
            )
        } else {
            muted(label)
        };
        col = col.push(item);
    }
    col
}

fn endgames<'a>(
    mut col: iced::widget::Column<'a, Message>,
    shell: &'a Shell,
    report: &PlayerReport,
) -> iced::widget::Column<'a, Message> {
    let i18n = &shell.i18n;
    if report.core.endgames.report.iter().all(|n| *n == 0) {
        return col;
    }
    col = col.push(section(i18n.tr("preport-ui-endgames")));
    for (i, class) in EndTheme::ALL.iter().enumerate() {
        let count = report.core.endgames.report.get(i).copied().unwrap_or(0);
        let label = format!("{}: {count}", class.short_name());
        let item: Element<'a, Message> = if count > 0 {
            link(
                label,
                prep(PlayerReportMessage::Select(ReportSelector::Endgame(i))),
                format!("preport-endgame-{i}"),
            )
        } else {
            muted(label)
        };
        col = col.push(item);
    }
    col
}

fn theory<'a>(
    mut col: iced::widget::Column<'a, Message>,
    shell: &'a Shell,
    report: &PlayerReport,
) -> iced::widget::Column<'a, Message> {
    let i18n = &shell.i18n;
    if report.core.rows.is_empty() {
        return col;
    }
    col = col.push(section(i18n.tr("preport-ui-theory")));
    // With Black to move the first move of a line is the last of the stem.
    let skip = usize::from(
        report
            .position
            .as_ref()
            .is_some_and(|p| p.turn() == Color::Black),
    );
    for row_data in report.core.rows.iter().take(MAX_THEORY_ROWS) {
        let Some(line) = report.core.lines.get(row_data.line) else {
            continue;
        };
        let moves = line
            .moves
            .iter()
            .skip(skip)
            .cloned()
            .collect::<Vec<_>>()
            .join(" ");
        let white_percent = row_data.score_percent();
        let percent = if report.color == Color::White {
            white_percent
        } else {
            100_u32.saturating_sub(white_percent)
        };
        let mut args = FluentArgs::new();
        args.set("count", i64::from(row_data.count));
        args.set("percent", i64::from(percent));
        col = col.push(
            row![
                text(moves).size(BODY_TEXT).width(Length::FillPortion(3)),
                muted(i18n.tr_args("preport-ui-theory-row", &args)),
            ]
            .spacing(8),
        );
    }
    col
}

fn game_list<'a>(
    mut col: iced::widget::Column<'a, Message>,
    shell: &'a Shell,
    report: &PlayerReport,
) -> iced::widget::Column<'a, Message> {
    let i18n = &shell.i18n;
    let lines = &report.core.lines;
    if lines.is_empty() {
        return col;
    }
    col = col.push(section(i18n.tr("preport-ui-games")));
    if lines.len() > MAX_GAME_ROWS {
        let mut args = FluentArgs::new();
        args.set("shown", i64::try_from(MAX_GAME_ROWS).unwrap_or(0));
        args.set("total", i64::try_from(lines.len()).unwrap_or(0));
        col = col.push(muted(i18n.tr_args("preport-ui-games-shown", &args)));
    }
    for line in lines.iter().take(MAX_GAME_ROWS) {
        let elo = |e: u16| {
            if e == 0 {
                String::new()
            } else {
                // The key has no leading blank (Fluent trims it).
                format!(
                    " {}",
                    tr1(i18n, "preport-ui-game-elo", "elo", &e.to_string())
                )
            }
        };
        let eco = line.eco.map(|e| format!(" {e}")).unwrap_or_default();
        let label = format!(
            "{}{} - {}{}  {}  {}  {}{eco}",
            line.white,
            elo(line.white_elo),
            line.black,
            elo(line.black_elo),
            line.result,
            line.date,
            line.site,
        );
        col = col.push(
            row![
                link(
                    tr1(i18n, "preport-ui-game-ref", "n", &line.game.to_string()),
                    prep(PlayerReportMessage::LoadGame(line.game)),
                    format!("preport-game-{}", line.game),
                ),
                text(label).size(BODY_TEXT),
            ]
            .spacing(8),
        );
    }
    col
}
